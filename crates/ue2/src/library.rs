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
            ObjectRef::Export(i) => Some(ObjectHandle { package: pkg.clone(), export: i }),
            ObjectRef::Import(i) => {
                let imp = &pkg.imports[i];
                let class = pkg.name(imp.class_name).to_string();
                let object = pkg.name(imp.object_name).to_string();
                // Walk to the outermost package; collect group path on the way.
                let mut outer = imp.package;
                let mut top = None;
                while let ObjectRef::Import(j) = outer {
                    top = Some(pkg.name(pkg.imports[j].object_name).to_string());
                    outer = pkg.imports[j].package;
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
        let (mut cutout, mut two_sided) = (false, false);
        for _ in 0..8 {
            let class = cur.class().to_ascii_lowercase();
            if class == "texture" {
                let texture = read_texture(&cur.package, cur.export).ok()?;
                cutout |= texture.masked || texture.alpha;
                return Some(MaterialInfo { texture, cutout, two_sided });
            }
            let field = match class.as_str() {
                "shader" => "Diffuse",
                "combiner" => "Material1",
                "materialswitch" => "Current",
                _ => "Material", // FinalBlend, TexPanner, TexEnvMap, TexOscillator, ...
            };
            let pkg = cur.package.clone();
            let mut r = Reader::new(pkg.export_data(cur.export));
            let props = Properties::read(&pkg, &mut r).ok()?;
            let flag = |n: &str| matches!(props.get(&pkg, n), Some(Value::Bool(true)));
            // Shader with an opacity map, or FinalBlend with alpha test: foliage, fences...
            cutout |= props.object(&pkg, "Opacity").is_some_and(|o| o != 0) || flag("AlphaTest");
            two_sided |= flag("TwoSided");
            let next = props.object(&pkg, field).map(ObjectRef::from_raw)?;
            cur = self.resolve(&pkg, next)?;
        }
        None
    }
}

pub struct MaterialInfo {
    pub texture: Texture,
    /// Alpha channel should be treated as a cutout mask.
    pub cutout: bool,
    pub two_sided: bool,
}
