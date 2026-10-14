//! Map tiles (`maps/X_Y.unr`): terrain and static mesh actors.
//!
//! Each tile is a UE2 level. We read actors through their tagged properties
//! (after the script state frame); the terrain comes from `TerrainInfo`:
//! a 16-bit heightmap texture, a scale and up to 32 texture layers with
//! alpha maps.

use crate::props::{read_object, Properties};
use crate::texture::{read_texture, Texture};
use crate::{Error, ObjectRef, Package};

#[derive(Debug, Clone)]
pub struct StaticActor {
    pub mesh: ObjectRef,
    /// Unreal units.
    pub location: [f32; 3],
    /// Pitch, yaw, roll (65536 = full turn).
    pub rotation: [i32; 3],
    /// DrawScale * DrawScale3D.
    pub scale: [f32; 3],
}

#[derive(Debug, Clone)]
pub struct TerrainLayer {
    pub texture: ObjectRef,
    pub alpha_map: ObjectRef,
    pub uscale: f32,
    pub vscale: f32,
}

#[derive(Debug, Clone)]
pub struct Terrain {
    pub heightmap: ObjectRef,
    pub location: [f32; 3],
    pub scale: [f32; 3],
    pub layers: Vec<TerrainLayer>,
}

#[derive(Debug, Clone, Default)]
pub struct MapTile {
    pub actors: Vec<StaticActor>,
    pub terrains: Vec<Terrain>,
}

pub fn read_map(pkg: &Package) -> Result<MapTile, Error> {
    let mut tile = MapTile::default();
    for i in 0..pkg.exports.len() {
        let class = pkg.class_name(&pkg.exports[i]);
        match class {
            "StaticMeshActor" | "MovableStaticMeshActor" => {
                let Ok((p, _)) = read_object(pkg, i) else { continue };
                let Some(mesh) = p.object(pkg, "StaticMesh").map(ObjectRef::from_raw) else { continue };
                if p.bool(pkg, "bHidden") == Some(true) {
                    continue;
                }
                tile.actors.push(StaticActor {
                    mesh,
                    location: p.vector(pkg, "Location").unwrap_or([0.0; 3]),
                    rotation: p.rotator(pkg, "Rotation").unwrap_or([0; 3]),
                    scale: actor_scale(pkg, &p),
                });
            }
            "TerrainInfo" => {
                let Ok((p, _)) = read_object(pkg, i) else { continue };
                let Some(heightmap) = p.object(pkg, "TerrainMap").map(ObjectRef::from_raw) else { continue };
                let layers = p
                    .0
                    .iter()
                    .filter(|x| pkg.name(x.name).eq_ignore_ascii_case("Layers"))
                    .filter_map(|x| match &x.value {
                        crate::props::Value::Raw { data, .. } => {
                            let l = Properties::read(pkg, &mut crate::Reader::new(data)).ok()?;
                            Some(TerrainLayer {
                                texture: l.object(pkg, "Texture").map(ObjectRef::from_raw).unwrap_or_default(),
                                alpha_map: l.object(pkg, "AlphaMap").map(ObjectRef::from_raw).unwrap_or_default(),
                                uscale: l.float(pkg, "UScale").unwrap_or(1.0),
                                vscale: l.float(pkg, "VScale").unwrap_or(1.0),
                            })
                        }
                        _ => None,
                    })
                    .filter(|l| l.texture != ObjectRef::Null)
                    .collect();
                tile.terrains.push(Terrain {
                    heightmap,
                    location: p.vector(pkg, "Location").unwrap_or([0.0; 3]),
                    scale: p.vector(pkg, "TerrainScale").unwrap_or([64.0, 64.0, 64.0]),
                    layers,
                });
            }
            _ => {}
        }
    }
    Ok(tile)
}

fn actor_scale(pkg: &Package, p: &Properties) -> [f32; 3] {
    let s = p.float(pkg, "DrawScale").unwrap_or(1.0);
    let s3 = p.vector(pkg, "DrawScale3D").unwrap_or([1.0; 3]);
    [s * s3[0], s * s3[1], s * s3[2]]
}

/// Raw 16-bit heights of a terrain heightmap (G16 texture).
pub struct Heightmap {
    pub width: usize,
    pub height: usize,
    pub data: Vec<u16>,
}

/// Reads a G16 heightmap's mip 0 as u16 values.
pub fn read_heightmap(pkg: &Package, export: usize) -> Result<Heightmap, Error> {
    // read_texture expands G16 to 8-bit gray; re-read the raw mip for precision.
    let t: Texture = read_texture(pkg, export)?;
    let (w, h) = (t.width as usize, t.height as usize);
    let raw = crate::texture::raw_mip0(pkg, export)?;
    let data = if raw.len() >= w * h * 2 {
        raw.chunks_exact(2).take(w * h).map(|c| u16::from_le_bytes([c[0], c[1]])).collect()
    } else {
        t.rgba.chunks_exact(4).map(|p| (p[0] as u16) << 8).collect()
    };
    Ok(Heightmap { width: w, height: h, data })
}
