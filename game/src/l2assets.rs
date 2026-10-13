//! Bridge from the original client's UE2 packages to Bevy assets.
//!
//! The client folder is read-only input. Set `L2_CLIENT_DIR` to point at it;
//! when it is missing the game still runs with flat-colored placeholders.

use std::path::PathBuf;

use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

const DEFAULT_CLIENT_DIR: &str = "G:/visery/l2-client";
const DEFAULT_GROUND: &str = "textures/ertheia_v_t.utx:ertheia_a_ground00";

#[derive(Resource, Clone)]
pub struct L2Client {
    pub root: PathBuf,
}

/// Handles to textures pulled from the client; `None` when unavailable.
#[derive(Resource, Default)]
pub struct L2Textures {
    pub ground: Option<Handle<Image>>,
}

pub struct L2AssetsPlugin;

impl Plugin for L2AssetsPlugin {
    fn build(&self, app: &mut App) {
        let root = std::env::var_os("L2_CLIENT_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(DEFAULT_CLIENT_DIR));
        app.insert_resource(L2Client { root })
            .init_resource::<L2Textures>()
            .add_systems(PreStartup, load_textures);
    }
}

fn load_textures(client: Res<L2Client>, mut images: ResMut<Assets<Image>>, mut tex: ResMut<L2Textures>) {
    // Override with L2_GROUND=<package path>:<texture name>.
    let spec = std::env::var("L2_GROUND").unwrap_or_else(|_| DEFAULT_GROUND.to_string());
    let (package, object) = spec.rsplit_once(':').unwrap_or((spec.as_str(), ""));
    tex.ground = load_texture(&client, package, object, true).map(|img| images.add(img));
}

/// Loads mip 0 of `object` from `package` (relative to the client root).
pub fn load_texture(client: &L2Client, package: &str, object: &str, tiled: bool) -> Option<Image> {
    let path = client.root.join(package);
    let result = ue2::Package::open(&path).map_err(|e| e.to_string()).and_then(|pkg| {
        let idx = pkg.find_export(object).ok_or_else(|| format!("no export {object}"))?;
        ue2::texture::read_texture(&pkg, idx).map_err(|e| e.to_string())
    });
    match result {
        Ok(t) => {
            info!("loaded {} ({}x{}, {:?}) from {}", t.name, t.width, t.height, t.format, path.display());
            let mut img = Image::new(
                Extent3d { width: t.width, height: t.height, depth_or_array_layers: 1 },
                TextureDimension::D2,
                t.rgba,
                TextureFormat::Rgba8UnormSrgb,
                RenderAssetUsages::RENDER_WORLD,
            );
            if tiled {
                img.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
                    address_mode_u: ImageAddressMode::Repeat,
                    address_mode_v: ImageAddressMode::Repeat,
                    ..ImageSamplerDescriptor::linear()
                });
            }
            Some(img)
        }
        Err(e) => {
            warn!("could not load {object} from {}: {e}", path.display());
            None
        }
    }
}
