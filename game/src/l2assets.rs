//! Bridge from the original client's UE2 packages to Bevy assets.
//!
//! The client folder is read-only input. Set `L2_CLIENT_DIR` to point at it;
//! when it is missing the game still runs with flat-colored placeholders.

use std::collections::HashMap;
use std::path::PathBuf;

use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use ue2::library::Library;

const DEFAULT_CLIENT_DIR: &str = "G:/visery/l2-client";
const DEFAULT_GROUND: &str = "ertheia_v_t:ertheia_a_ground00";

/// Unreal units to Bevy meters. L2 humans are roughly 60-70 units tall.
pub const UU_TO_M: f32 = 1.0 / 50.0;

/// Converts an Unreal position (left-handed, Z up) into Bevy space
/// (right-handed, Y up). Swapping Y and Z mirrors the handedness, which also
/// turns Unreal's clockwise front faces into Bevy's counter-clockwise ones.
pub fn ue_to_bevy(p: [f32; 3]) -> [f32; 3] {
    [p[0], p[2], p[1]]
}

#[derive(Resource)]
pub struct L2Library {
    pub lib: Option<Library>,
    /// Material cache keyed by "package:export".
    materials: HashMap<String, Handle<StandardMaterial>>,
}

/// Handles to textures pulled from the client; `None` when unavailable.
#[derive(Resource, Default)]
pub struct L2Textures {
    pub ground: Option<Handle<Image>>,
}

/// One drawable piece of a static mesh (a section with its material).
#[derive(Clone)]
pub struct MeshPart {
    pub mesh: Handle<Mesh>,
    pub material: Handle<StandardMaterial>,
}

pub struct L2AssetsPlugin;

impl Plugin for L2AssetsPlugin {
    fn build(&self, app: &mut App) {
        let root = std::env::var_os("L2_CLIENT_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(DEFAULT_CLIENT_DIR));
        let lib = match Library::new(&root) {
            Ok(l) => Some(l),
            Err(e) => {
                warn!("Lineage II client not found at {}: {e}", root.display());
                None
            }
        };
        app.insert_resource(L2Library { lib, materials: HashMap::new() })
            .init_resource::<L2Textures>()
            .add_systems(PreStartup, load_textures);
    }
}

fn load_textures(mut lib: ResMut<L2Library>, mut images: ResMut<Assets<Image>>, mut tex: ResMut<L2Textures>) {
    // Override with L2_GROUND=<package>:<texture>.
    let spec = std::env::var("L2_GROUND").unwrap_or_else(|_| DEFAULT_GROUND.to_string());
    let (package, object) = spec.split_once(':').unwrap_or((spec.as_str(), ""));
    tex.ground = lib.texture(package, object, true).map(|img| images.add(img));
}

pub fn to_image(t: ue2::texture::Texture, tiled: bool) -> Image {
    let mut img = Image::new(
        Extent3d { width: t.width, height: t.height, depth_or_array_layers: 1 },
        TextureDimension::D2,
        t.rgba,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    img.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: if tiled { ImageAddressMode::Repeat } else { ImageAddressMode::ClampToEdge },
        address_mode_v: if tiled { ImageAddressMode::Repeat } else { ImageAddressMode::ClampToEdge },
        ..ImageSamplerDescriptor::linear()
    });
    img
}

impl L2Library {
    pub fn texture(&mut self, package: &str, object: &str, tiled: bool) -> Option<Image> {
        let lib = self.lib.as_mut()?;
        let Some(h) = lib.find(package, object) else {
            warn!("texture {package}:{object} not found");
            return None;
        };
        match ue2::texture::read_texture(&h.package, h.export) {
            Ok(t) => {
                info!("loaded {} ({}x{}, {:?})", t.name, t.width, t.height, t.format);
                Some(to_image(t, tiled))
            }
            Err(e) => {
                warn!("texture {package}:{object}: {e}");
                None
            }
        }
    }

    /// Loads a static mesh as one Bevy mesh per section, already converted to
    /// Bevy axes and meters.
    pub fn static_mesh(
        &mut self,
        package: &str,
        object: &str,
        meshes: &mut Assets<Mesh>,
        materials: &mut Assets<StandardMaterial>,
        images: &mut Assets<Image>,
    ) -> Option<Vec<MeshPart>> {
        let lib = self.lib.as_mut()?;
        let h = lib.find(package, object)?;
        let sm = match ue2::mesh::read_static_mesh(&h.package, h.export) {
            Ok(m) => m,
            Err(e) => {
                warn!("static mesh {package}:{object}: {e}");
                return None;
            }
        };
        let positions: Vec<[f32; 3]> =
            sm.positions.iter().map(|&p| ue_to_bevy(p).map(|c| c * UU_TO_M)).collect();
        let normals: Vec<[f32; 3]> = sm.normals.iter().map(|&n| ue_to_bevy(n)).collect();

        let mut parts = Vec::new();
        for s in &sm.sections {
            if s.num_faces == 0 {
                continue;
            }
            let range = s.first_index as usize..(s.first_index + s.num_faces * 3) as usize;
            let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD);
            mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions.clone());
            mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals.clone());
            mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, sm.uvs.clone());
            mesh.insert_indices(Indices::U32(sm.indices[range].to_vec()));

            let key = format!("{}:{:?}", h.package.name, s.material);
            let material = match self.materials.get(&key) {
                Some(m) => m.clone(),
                None => {
                    let lib = self.lib.as_mut()?;
                    let tex = lib.resolve(&h.package, s.material).and_then(|m| lib.diffuse_texture(&m));
                    let mat = match tex {
                        Some(t) => StandardMaterial {
                            base_color_texture: Some(images.add(to_image(t, true))),
                            alpha_mode: AlphaMode::Mask(0.5),
                            double_sided: true,
                            cull_mode: None,
                            perceptual_roughness: 0.9,
                            ..default()
                        },
                        None => StandardMaterial::from(Color::srgb(0.6, 0.6, 0.6)),
                    };
                    let handle = materials.add(mat);
                    self.materials.insert(key, handle.clone());
                    handle
                }
            };
            parts.push(MeshPart { mesh: meshes.add(mesh), material });
        }
        Some(parts)
    }
}
