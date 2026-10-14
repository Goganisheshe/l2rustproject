//! Cross-package object resolution over a whole client folder.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::props::{Properties, Value};
use crate::texture::{read_texture, Texture};
use crate::{Error, ObjectRef, Package, Reader};

const EXTENSIONS: &[&str] = &["u", "ukx", "utx", "usx", "unr", "uax", "uix", "ugx"];

/// A resolved object: the package that holds it and its export index.
#[derive(Clone)]
pub struct ObjectHandle {
    pub package: Arc<Package>,
    pub export: usize,
}

impl ObjectHandle {
    pub fn class(&self) -> &str {
        self.package.class_name(&self.package.exports[self.export])
    }
    pub fn name(&self) -> &str {
        self.package.object_name(ObjectRef::Export(self.export))
    }
}

pub struct Library {
    files: HashMap<String, PathBuf>,
    open: HashMap<String, Option<Arc<Package>>>,
}

impl Library {
    /// Indexes every UE package under `root` (one level of subfolders deep).
    pub fn new(root: impl AsRef<Path>) -> Result<Self, Error> {
        let mut files = HashMap::new();
        for dir in std::fs::read_dir(root)? {
            let dir = dir?.path();
            if !dir.is_dir() {
                continue;
            }
            for f in std::fs::read_dir(&dir)? {
                let f = f?.path();
                let ext = f.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
                if !EXTENSIONS.contains(&ext.as_str()) {
                    continue;
                }
                if let Some(stem) = f.file_stem().and_then(|s| s.to_str()) {
                    // Prefer the localized-neutral "system" copy when duplicates exist.
                    let key = stem.to_ascii_lowercase();
                    let in_system = dir.file_name().is_some_and(|d| d == "system");
                    if in_system || !files.contains_key(&key) {
                        files.insert(key, f);
                    }
                }
            }
        }
        Ok(Self { files, open: HashMap::new() })
    }

    pub fn package(&mut self, name: &str) -> Option<Arc<Package>> {
        let key = name.to_ascii_lowercase();
        if let Some(p) = self.open.get(&key) {
            return p.clone();
        }
        let pkg = self.files.get(&key).and_then(|path| Package::open(path).ok()).map(Arc::new);
        self.open.insert(key, pkg.clone());
        pkg
    }

    pub fn find(&mut self, package: &str, object: &str) -> Option<ObjectHandle> {
        let pkg = self.package(package)?;
        let export = pkg.find_export(object)?;
        Some(ObjectHandle { package: pkg, export })
    }

    /// Follows an object reference made inside `pkg` to the export that defines it.
    pub fn resolve(&mut self, pkg: &Arc<Package>, r: ObjectRef) -> Option<ObjectHandle> {
        match r {
            ObjectRef::Null => None,
            ObjectRef::Export(i) => (i < pkg.exports.len()).then(|| ObjectHandle { package: pkg.clone(), export: i }),
            ObjectRef::Import(i) => {
                let imp = pkg.imports.get(i)?;
                let class = pkg.name(imp.class_name).to_string();
                let object = pkg.name(imp.object_name).to_string();
                // Walk to the outermost package; collect group path on the way.
                let mut outer = imp.package;
                let mut top = None;
                while let ObjectRef::Import(j) = outer {
                    let parent = pkg.imports.get(j)?;
                    top = Some(pkg.name(parent.object_name).to_string());
                    outer = parent.package;
                }
                let target = self.package(&top?)?;
                let export = (0..target.exports.len()).find(|&e| {
                    let ex = &target.exports[e];
                    target.name(ex.object_name).eq_ignore_ascii_case(&object)
                        && target.class_name(ex).eq_ignore_ascii_case(&class)
                })?;
                Some(ObjectHandle { package: target, export })
            }
        }
    }

    /// Finds the base color texture behind a material (Texture, Shader,
    /// FinalBlend, Combiner, Tex* modifiers...).
    pub fn diffuse_texture(&mut self, material: &ObjectHandle) -> Option<Texture> {
        self.material_info(material).map(|m| m.texture)
    }

    /// Like `diffuse_texture`, plus blending hints gathered along the chain.
    pub fn material_info(&mut self, material: &ObjectHandle) -> Option<MaterialInfo> {
        let mut cur = material.clone();
        let (mut cutout, mut two_sided, mut shader) = (false, false, false);
        let mut additive = false;
        let mut alpha_ref: Option<u8> = None;
        let mut opacity: Option<ObjectHandle> = None;
        for _ in 0..8 {
            let class = cur.class().to_ascii_lowercase();
            if class == "texture" {
                let mut texture = read_texture(&cur.package, cur.export).ok()?;
                // Under a Shader the diffuse alpha is the specular mask unless the
                // shader names an opacity map; bare textures trust their flags.
                if !shader {
                    cutout |= texture.masked || texture.alpha;
                }
                if let Some(op) = opacity {
                    if let Some(mask) = self.diffuse_texture(&op) {
                        apply_alpha(&mut texture, &mask);
                    }
                }
                let alpha_cutoff = alpha_ref.map(|r| (r as f32 / 255.0).max(0.02)).unwrap_or(0.5);
                return Some(MaterialInfo { texture, cutout, two_sided, alpha_cutoff, additive });
            }
            let field = match class.as_str() {
                "shader" => "Diffuse",
                "combiner" => "Material1",
                "materialswitch" => "Current",
                _ => "Material", // FinalBlend, TexPanner, TexEnvMap, TexOscillator, ...
            };
            shader |= class == "shader";
            let pkg = cur.package.clone();
            let mut r = Reader::new(pkg.export_data(cur.export));
            let props = Properties::read(&pkg, &mut r).ok()?;
            let flag = |n: &str| matches!(props.get(&pkg, n), Some(Value::Bool(true)));
            // Shader with an opacity map, or FinalBlend with alpha test: foliage, fences...
            if let Some(o) = props.object(&pkg, "Opacity").filter(|&o| o != 0) {
                cutout = true;
                if opacity.is_none() {
                    opacity = self.resolve(&pkg, ObjectRef::from_raw(o));
                }
            }
            if flag("AlphaTest") {
                cutout = true;
                alpha_ref = alpha_ref.or(props.byte(&pkg, "AlphaRef"));
            }
            two_sided |= flag("TwoSided");
            // FinalBlend FB_Translucent / FB_Brighten and Shader OB_Translucent /
            // OB_Brighten draw additively (fire, glows, light shafts).
            if class == "finalblend" && matches!(props.byte(&pkg, "FrameBufferBlending"), Some(4 | 6)) {
                additive = true;
            }
            if class == "shader" && matches!(props.byte(&pkg, "OutputBlending"), Some(3 | 5)) {
                additive = true;
            }
            let next = props.object(&pkg, field).map(ObjectRef::from_raw)?;
            cur = self.resolve(&pkg, next)?;
        }
        None
    }
}

/// Copies `mask`'s alpha into `tex` (nearest sampling if sizes differ).
fn apply_alpha(tex: &mut Texture, mask: &Texture) {
    let (w, h) = (tex.width as usize, tex.height as usize);
    let (mw, mh) = (mask.width as usize, mask.height as usize);
    if mw == 0 || mh == 0 {
        return;
    }
    for y in 0..h {
        for x in 0..w {
            let (mx, my) = (x * mw / w, y * mh / h);
            tex.rgba[(y * w + x) * 4 + 3] = mask.rgba[(my * mw + mx) * 4 + 3];
        }
    }
}

pub struct MaterialInfo {
    pub texture: Texture,
    /// Alpha channel should be treated as a cutout mask.
    pub cutout: bool,
    pub two_sided: bool,
    /// Alpha below this is discarded when `cutout` (from `AlphaRef`).
    pub alpha_cutoff: f32,
    /// Additive blending (FinalBlend translucent/brighten).
    pub additive: bool,
}
