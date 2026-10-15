//! Background loading of map tiles.
//!
//! A worker thread owns its own `ue2::Library`, decodes terrain, bakes
//! terrain textures and prepares static meshes (CPU-side `Mesh`/`Image`
//! data). The main thread only turns the results into GPU assets and
//! entities, so walking into a new tile does not freeze the game.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Mutex;

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use ue2::library::{Library, MaterialInfo};
use ue2::world::{read_heightmap, read_map, Terrain};
use ue2::ObjectRef;

use crate::l2assets::{to_image, MeshPart, UU_TO_M};

const DEFAULT_CLIENT_DIR: &str = "G:/visery/l2-client";
/// Baked terrain texture resolution per tile.
const TERRAIN_TEXELS: usize = 2048;
/// Unreal units per map tile.
pub const TILE_UU: f32 = 32768.0;

/// Tile name for an Unreal world position (`X_Y`, as in `maps/20_21.unr`).
pub fn tile_of(ue_x: f32, ue_y: f32) -> (i32, i32) {
    ((ue_x / TILE_UU).floor() as i32 + 20, (ue_y / TILE_UU).floor() as i32 + 18)
}

pub fn ue_to_bevy(p: [f32; 3]) -> Vec3 {
    Vec3::new(p[0], p[2], p[1]) * UU_TO_M
}

pub fn bevy_to_ue(p: Vec3) -> [f32; 3] {
    let p = p / UU_TO_M;
    [p.x, p.z, p.y]
}

/// Unreal rotator -> Bevy rotation (see FRotationMatrix; axes mirrored Y<->Z).
pub fn ue_rotator(r: [i32; 3]) -> Quat {
    let k = std::f32::consts::TAU / 65536.0;
    let (p, y, rl) = (r[0] as f32 * k, r[1] as f32 * k, r[2] as f32 * k);
    let (sp, cp) = p.sin_cos();
    let (sy, cy) = y.sin_cos();
    let (sr, cr) = rl.sin_cos();
    let x = Vec3::new(cp * cy, cp * sy, sp);
    let yv = Vec3::new(sr * sp * cy - cr * sy, sr * sp * sy + cr * cy, -sr * cp);
    let z = Vec3::new(-(cr * sp * cy + sr * sy), cy * sr - cr * sp * sy, cr * cp);
    let swap = |v: Vec3| Vec3::new(v.x, v.z, v.y);
    Quat::from_mat3(&Mat3::from_cols(swap(x), swap(z), swap(yv))).normalize()
}

/// Terrain heights of one tile in Bevy space, for walking.
#[derive(Clone)]
pub struct HeightField {
    /// Bevy position of vertex (0, 0).
    origin: Vec3,
    /// Meters between vertices along Bevy X and Z.
    step: Vec2,
    width: usize,
    height: usize,
    /// Bevy Y per vertex.
    y: Vec<f32>,
    /// Quads cut out of the terrain (cave mouths): no ground there.
    holes: Vec<bool>,
}

impl HeightField {
    /// Ground height at a Bevy (x, z), bilinear; `None` outside this tile.
    pub fn sample(&self, x: f32, z: f32) -> Option<f32> {
        let fx = (x - self.origin.x) / self.step.x;
        let fz = (z - self.origin.z) / self.step.y;
        if fx < 0.0 || fz < 0.0 || fx > (self.width - 1) as f32 || fz > (self.height - 1) as f32 {
            return None;
        }
        let (i, j) = (fx.floor() as usize, fz.floor() as usize);
        if self.holes.get(j * self.width + i).copied().unwrap_or(false) {
            return None;
        }
        let (i1, j1) = ((i + 1).min(self.width - 1), (j + 1).min(self.height - 1));
        let (tx, tz) = (fx - i as f32, fz - j as f32);
        let h = |a: usize, b: usize| self.y[b * self.width + a];
        let top = h(i, j) * (1.0 - tx) + h(i1, j) * tx;
        let bottom = h(i, j1) * (1.0 - tx) + h(i1, j1) * tx;
        Some(top * (1.0 - tz) + bottom * tz)
    }
}

struct PreparedSection {
    mesh: Mesh,
    material_key: String,
    material: Option<MaterialInfo>,
}

struct PreparedTile {
    name: String,
    terrain: Option<(Mesh, Image, HeightField)>,
    /// World-space collision triangles of the tile's static meshes.
    collision: Vec<[Vec3; 3]>,
    /// Newly decoded static meshes (not sent before), by key.
    meshes: Vec<(String, Vec<PreparedSection>)>,
    placements: Vec<(String, Transform)>,
}

pub struct TileStreamPlugin;

impl Plugin for TileStreamPlugin {
    fn build(&self, app: &mut App) {
        let root = std::env::var_os("L2_CLIENT_DIR").map(PathBuf::from).unwrap_or_else(|| DEFAULT_CLIENT_DIR.into());
        let (req_tx, req_rx) = channel::<String>();
        let (res_tx, res_rx) = channel::<PreparedTile>();
        std::thread::Builder::new()
            .name("tile-loader".into())
            .spawn(move || worker(root, req_rx, res_tx))
            .expect("spawn tile loader");
        app.insert_resource(TileStreamer {
            requests: req_tx,
            results: Mutex::new(res_rx),
            requested: HashSet::new(),
            loaded: HashSet::new(),
            meshes: HashMap::new(),
            materials: HashMap::new(),
            ground: HashMap::new(),
        })
        .init_resource::<crate::collision::CollisionWorld>()
        .add_systems(Update, receive_tiles);
    }
}

#[derive(Resource)]
pub struct TileStreamer {
    requests: Sender<String>,
    results: Mutex<Receiver<PreparedTile>>,
    requested: HashSet<String>,
    loaded: HashSet<String>,
    meshes: HashMap<String, Vec<MeshPart>>,
    materials: HashMap<String, Handle<StandardMaterial>>,
    ground: HashMap<String, HeightField>,
}

/// Marks entities belonging to a loaded tile.
#[derive(Component)]
pub struct TileEntity(pub String);

impl TileStreamer {
    /// Requests a tile (no-op if already requested or missing).
    pub fn request(&mut self, tile: (i32, i32)) {
        let name = format!("{}_{}", tile.0, tile.1);
        if self.requested.insert(name.clone()) {
            let _ = self.requests.send(name);
        }
    }

    /// Requests a map package by name (the lobby scene `Lobby01`).
    pub fn request_named(&mut self, name: &str) {
        if self.requested.insert(name.to_string()) {
            let _ = self.requests.send(name.to_string());
        }
    }

    /// Forgets a loaded map (its entities are despawned by the caller), so it
    /// can be requested again.
    pub fn forget(&mut self, name: &str) {
        self.requested.remove(name);
        self.loaded.remove(name);
        self.ground.remove(name);
    }

    pub fn is_loaded_named(&self, name: &str) -> bool {
        self.loaded.contains(name)
    }

    /// Requests the tile at a Bevy position and its 8 neighbours.
    pub fn request_around(&mut self, pos: Vec3) {
        let ue = bevy_to_ue(pos);
        let (tx, ty) = tile_of(ue[0], ue[1]);
        self.request((tx, ty));
        for dy in -1..=1 {
            for dx in -1..=1 {
                self.request((tx + dx, ty + dy));
            }
        }
    }

    pub fn loaded_tiles(&self) -> impl Iterator<Item = &String> {
        self.loaded.iter()
    }

    pub fn is_loaded(&self, tile: (i32, i32)) -> bool {
        self.loaded.contains(&format!("{}_{}", tile.0, tile.1))
    }

    /// Ground height under a Bevy position, from any loaded tile.
    pub fn ground_y(&self, pos: Vec3) -> Option<f32> {
        let ue = bevy_to_ue(pos);
        let (tx, ty) = tile_of(ue[0], ue[1]);
        self.ground
            .get(&format!("{tx}_{ty}"))
            .and_then(|h| h.sample(pos.x, pos.z))
            .or_else(|| self.ground.values().find_map(|h| h.sample(pos.x, pos.z)))
    }
}

fn worker(root: PathBuf, requests: Receiver<String>, results: Sender<PreparedTile>) {
    let mut lib = match Library::new(&root) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("tile loader: client not found at {}: {e}", root.display());
            return;
        }
    };
    let mut sent: HashSet<String> = HashSet::new();
    let mut colliders: HashMap<String, Vec<[Vec3; 3]>> = HashMap::new();
    while let Ok(name) = requests.recv() {
        let tile = prepare_tile(&mut lib, &name, &mut sent, &mut colliders);
        if results.send(tile).is_err() {
            return;
        }
    }
}

fn prepare_tile(
    lib: &mut Library,
    name: &str,
    sent: &mut HashSet<String>,
    colliders: &mut HashMap<String, Vec<[Vec3; 3]>>,
) -> PreparedTile {
    let mut out = PreparedTile {
        name: name.to_string(),
        terrain: None,
        collision: Vec::new(),
        meshes: Vec::new(),
        placements: Vec::new(),
    };
    // The Classic world: prefer `X_Y_Classic.unr` where the client has one.
    // Regions Classic leaves out (the Kamael island, Gracia, Hellbound...) are
    // stubs there: flat terrain and no actors. Those use the full tile.
    let classic = lib.package(&format!("{name}_Classic")).and_then(|p| read_map(&p).ok().map(|t| (p, t)));
    let picked = match classic {
        Some((p, t)) if !t.actors.is_empty() => Some((p, t)),
        stub => lib.package(name).and_then(|p| read_map(&p).ok().map(|t| (p, t))).or(stub),
    };
    let Some((pkg, tile)) = picked else { return out };
    if let Some(t) = tile.terrains.first() {
        out.terrain = prepare_terrain(lib, &pkg, t);
    }
    // Brush (BSP) geometry: town walls, floors, dungeons.
    if let Some(model) = ue2::bsp::level_model(&pkg) {
        match ue2::bsp::read_bsp(&pkg, model) {
            Ok(polys) => {
                let (sections, tris) = prepare_bsp(lib, &pkg, &polys);
                out.collision.extend(tris);
                let key = format!("{name}#bsp");
                out.meshes.push((key.clone(), sections));
                out.placements.push((key, Transform::IDENTITY));
            }
            Err(e) => eprintln!("bsp {name}: {e}"),
        }
    }
    for a in &tile.actors {
        let Some(h) = lib.resolve(&pkg, a.mesh) else { continue };
        let key = format!("{}.{}", h.package.name, h.name());
        if sent.insert(key.clone()) {
            let (sections, tris) = prepare_static_mesh(lib, &h).unwrap_or_default();
            colliders.insert(key.clone(), tris);
            out.meshes.push((key.clone(), sections));
        }
        let transform = Transform {
            translation: ue_to_bevy(a.location),
            rotation: ue_rotator(a.rotation),
            scale: Vec3::new(a.scale[0], a.scale[2], a.scale[1]),
        };
        if let Some(tris) = colliders.get(&key) {
            let m = transform.to_matrix();
            out.collision.extend(tris.iter().map(|t| t.map(|v| m.transform_point3(v))));
        }
        out.placements.push((key, transform));
    }
    out
}

/// BSP polygons grouped by material into meshes, plus solid triangles.
fn prepare_bsp(
    lib: &mut Library,
    pkg: &std::sync::Arc<ue2::Package>,
    polys: &[ue2::bsp::BspPoly],
) -> (Vec<PreparedSection>, Vec<[Vec3; 3]>) {
    use ue2::bsp::{PF_MASKED, PF_TRANSLUCENT, PF_TWO_SIDED};
    let mut tris = Vec::new();
    let mut groups: HashMap<(ObjectRef, u32), Vec<usize>> = HashMap::new();
    for (i, p) in polys.iter().enumerate() {
        if p.solid() {
            let v: Vec<Vec3> = p.verts.iter().map(|&v| ue_to_bevy(v)).collect();
            for k in 1..v.len() - 1 {
                tris.push([v[0], v[k], v[k + 1]]);
            }
        }
        if p.visible() {
            let style = p.flags & (PF_MASKED | PF_TRANSLUCENT | PF_TWO_SIDED);
            groups.entry((p.material, style)).or_default().push(i);
        }
    }
    let mut sections = Vec::new();
    for ((material, style), list) in groups {
        let mut info = lib.resolve(pkg, material).and_then(|m| lib.material_info(&m));
        let (tw, th) = info.as_ref().map_or((256.0, 256.0), |m| (m.texture.width as f32, m.texture.height as f32));
        if let Some(m) = info.as_mut() {
            m.cutout |= style & PF_MASKED != 0;
            // Winding of BSP polygons varies after the CSG build; draw both sides.
            m.two_sided = true;
        }
        let _ = PF_TWO_SIDED;
        let (mut pos, mut nrm, mut uv, mut idx) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        for &i in &list {
            let p = &polys[i];
            let base = pos.len() as u32;
            let n = ue_to_bevy(p.normal) / UU_TO_M;
            for &v in &p.verts {
                pos.push(ue_to_bevy(v).to_array());
                nrm.push(n.to_array());
                let t = p.texel(v);
                uv.push([t[0] / tw, t[1] / th]);
            }
            // Unreal polygons are clockwise; the Y/Z mirror flips them, so a
            // plain fan comes out counter-clockwise for Bevy.
            for k in 1..p.verts.len() as u32 - 1 {
                idx.extend_from_slice(&[base, base + k, base + k + 1]);
            }
        }
        let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD);
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, nrm);
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uv);
        mesh.insert_indices(Indices::U32(idx));
        let material_key = format!("{}:bsp:{:?}:{style}", pkg.name, material);
        sections.push(PreparedSection { mesh, material_key, material: info });
    }
    (sections, tris)
}

/// GPU-ready sections plus mesh-space collision triangles.
fn prepare_static_mesh(
    lib: &mut Library,
    h: &ue2::library::ObjectHandle,
) -> Option<(Vec<PreparedSection>, Vec<[Vec3; 3]>)> {
    let sm = ue2::mesh::read_static_mesh(&h.package, h.export).ok()?;
    let positions: Vec<[f32; 3]> = sm.positions.iter().map(|&p| ue_to_bevy(p).to_array()).collect();
    let normals: Vec<[f32; 3]> = sm.normals.iter().map(|&n| [n[0], n[2], n[1]]).collect();
    let mut out = Vec::new();
    let mut tris = Vec::new();
    for s in &sm.sections {
        if s.num_faces == 0 {
            continue;
        }
        let range = s.first_index as usize..(s.first_index + s.num_faces * 3) as usize;
        if s.collision {
            for t in sm.indices[range.clone()].chunks_exact(3) {
                tris.push([0, 1, 2].map(|k| Vec3::from_array(positions[t[k] as usize])));
            }
        }
        // Keep only the vertices this section uses.
        let mut remap: HashMap<u32, u32> = HashMap::new();
        let mut idx = Vec::with_capacity(range.len());
        let (mut p, mut n, mut uv) = (Vec::new(), Vec::new(), Vec::new());
        for &i in &sm.indices[range] {
            let new = *remap.entry(i).or_insert_with(|| {
                p.push(positions[i as usize]);
                n.push(normals[i as usize]);
                uv.push(sm.uvs[i as usize]);
                (p.len() - 1) as u32
            });
            idx.push(new);
        }
        let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD);
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, p);
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, n);
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uv);
        mesh.insert_indices(Indices::U32(idx));
        let material_key = format!("{}:{:?}", h.package.name, s.material);
        let material = lib.resolve(&h.package, s.material).and_then(|m| lib.material_info(&m));
        out.push(PreparedSection { mesh, material_key, material });
    }
    Some((out, tris))
}

fn prepare_terrain(lib: &mut Library, pkg: &std::sync::Arc<ue2::Package>, t: &Terrain) -> Option<(Mesh, Image, HeightField)> {
    let hm_handle = lib.resolve(pkg, t.heightmap)?;
    let hm = read_heightmap(&hm_handle.package, hm_handle.export).ok()?;
    let (w, h) = (hm.width, hm.height);
    let [sx, sy, sz] = t.scale;
    let loc = t.location;
    let mut positions = Vec::with_capacity(w * h);
    let mut uvs = Vec::with_capacity(w * h);
    let mut ys = Vec::with_capacity(w * h);
    for j in 0..h {
        for i in 0..w {
            let z = (hm.data[j * w + i] as f32 - 32768.0) * sz / 256.0;
            let p = [loc[0] + (i as f32 - w as f32 / 2.0) * sx, loc[1] + (j as f32 - h as f32 / 2.0) * sy, loc[2] + z];
            let b = ue_to_bevy(p);
            ys.push(b.y);
            positions.push(b.to_array());
            uvs.push([i as f32 / (w - 1) as f32, j as f32 / (h - 1) as f32]);
        }
    }
    let mut indices = Vec::with_capacity((w - 1) * (h - 1) * 6);
    let mut holes = vec![false; w * h];
    for j in 0..h - 1 {
        for i in 0..w - 1 {
            let quad = j * w + i;
            if !t.quad_visible(quad) {
                holes[quad] = true;
                continue;
            }
            let a = quad as u32;
            let (b, c, d) = (a + 1, a + w as u32, a + w as u32 + 1);
            if t.quad_turned(quad) {
                indices.extend_from_slice(&[a, c, d, a, d, b]);
            } else {
                indices.extend_from_slice(&[a, c, b, b, c, d]);
            }
        }
    }
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD);
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(Indices::U32(indices));
    mesh.compute_smooth_normals();
    debug!("terrain: {} hole quads, {} turned bitmap words", holes.iter().filter(|h| **h).count(), t.edge_turn.len());
    let origin = ue_to_bevy([loc[0] - w as f32 / 2.0 * sx, loc[1] - h as f32 / 2.0 * sy, 0.0]);
    let field = HeightField {
        origin: Vec3::new(origin.x, 0.0, origin.z),
        step: Vec2::new(sx * UU_TO_M, sy * UU_TO_M),
        width: w,
        height: h,
        y: ys,
        holes,
    };
    let image = bake_layers(lib, pkg, t, w)?;
    Some((mesh, image, field))
}

/// Blends the terrain layers (tiled texture weighted by alpha map) into one
/// texture covering the tile.
fn bake_layers(l: &mut Library, pkg: &std::sync::Arc<ue2::Package>, t: &Terrain, w: usize) -> Option<Image> {
    let n = TERRAIN_TEXELS;
    let mut out = vec![0f32; n * n * 3];
    let tex_of = |l: &mut Library, r: ObjectRef| l.resolve(pkg, r).and_then(|h| l.diffuse_texture(&h));
    for (li, layer) in t.layers.iter().enumerate() {
        let Some(tex) = tex_of(l, layer.texture) else { continue };
        let alpha = if layer.alpha_map == ObjectRef::Null { None } else { tex_of(l, layer.alpha_map) };
        let world_per_texel = (w as f32 - 1.0) * t.scale[0] / n as f32;
        let tex_px_per_world = 1.0 / layer.uscale.max(0.01);
        let footprint = (world_per_texel * tex_px_per_world).max(1.0) as usize;
        let (tw, th, small) = downsample(&tex, footprint.min(tex.width as usize));
        let alpha_uses_red = alpha.as_ref().is_some_and(|a| a.rgba.chunks_exact(4).all(|p| p[3] == 255));
        for y in 0..n {
            for x in 0..n {
                let weight = match &alpha {
                    None => {
                        if li == 0 {
                            1.0
                        } else {
                            0.0
                        }
                    }
                    Some(a) => {
                        let ax = (x * a.width as usize / n).min(a.width as usize - 1);
                        let ay = (y * a.height as usize / n).min(a.height as usize - 1);
                        let p = &a.rgba[(ay * a.width as usize + ax) * 4..];
                        (if alpha_uses_red { p[0] } else { p[3] }) as f32 / 255.0
                    }
                };
                if weight <= 0.0 {
                    continue;
                }
                let tx = ((x as f32 * world_per_texel * tex_px_per_world) as usize / footprint) % tw;
                let ty = ((y as f32 * world_per_texel * tex_px_per_world) as usize / footprint) % th;
                let c = &small[(ty * tw + tx) * 3..];
                let o = &mut out[(y * n + x) * 3..(y * n + x) * 3 + 3];
                for k in 0..3 {
                    o[k] = o[k] * (1.0 - weight) + c[k] * weight;
                }
            }
        }
    }
    let rgba: Vec<u8> = out.chunks_exact(3).flat_map(|c| [c[0] as u8, c[1] as u8, c[2] as u8, 255]).collect();
    Some(to_image(
        ue2::texture::Texture {
            name: "terrain".into(),
            width: n as u32,
            height: n as u32,
            format: ue2::texture::Format::Rgba8,
            rgba,
            masked: false,
            alpha: false,
            split9: None,
        },
        false,
    ))
}

/// Box-filters a texture by `f` and returns (width, height, rgb floats).
fn downsample(t: &ue2::texture::Texture, f: usize) -> (usize, usize, Vec<f32>) {
    let (w, h) = (t.width as usize, t.height as usize);
    let f = f.max(1);
    let (nw, nh) = ((w / f).max(1), (h / f).max(1));
    let mut out = vec![0f32; nw * nh * 3];
    for y in 0..nh {
        for x in 0..nw {
            let mut acc = [0f32; 3];
            for dy in 0..f.min(h) {
                for dx in 0..f.min(w) {
                    let p = &t.rgba[(((y * f + dy) % h) * w + (x * f + dx) % w) * 4..];
                    for k in 0..3 {
                        acc[k] += p[k] as f32;
                    }
                }
            }
            let cnt = (f.min(h) * f.min(w)) as f32;
            for k in 0..3 {
                out[(y * nw + x) * 3 + k] = acc[k] / cnt;
            }
        }
    }
    (nw, nh, out)
}

fn material_from_info(info: Option<MaterialInfo>, images: &mut Assets<Image>) -> StandardMaterial {
    match info {
        Some(m) => {
            let (cutout, two_sided) = (m.cutout, m.cutout || m.two_sided);
            StandardMaterial {
                base_color_texture: Some(images.add(to_image(m.texture, true))),
                alpha_mode: if m.additive {
                    AlphaMode::Add
                } else if cutout {
                    AlphaMode::Mask(m.alpha_cutoff)
                } else {
                    AlphaMode::Opaque
                },
                unlit: m.additive,
                double_sided: two_sided,
                cull_mode: if two_sided { None } else { Some(bevy::render::render_resource::Face::Back) },
                perceptual_roughness: 0.9,
                ..default()
            }
        }
        None => StandardMaterial::from(Color::srgb(0.6, 0.6, 0.6)),
    }
}

/// Turns finished tiles into assets and entities (at most one per frame).
fn receive_tiles(
    mut commands: Commands,
    mut streamer: ResMut<TileStreamer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut collision: ResMut<crate::collision::CollisionWorld>,
) {
    let Ok(tile) = streamer.results.lock().unwrap().try_recv() else { return };
    // Dropped while it was loading (the lobby scene after leaving it).
    if !streamer.requested.contains(&tile.name) {
        return;
    }
    // The lobby scene sits on top of real world coordinates: it is only a
    // backdrop, nothing walks on it.
    let backdrop = tile.name.starts_with("Lobby");
    if !backdrop {
        collision.add(tile.collision.iter().copied());
    }
    let streamer = streamer.as_mut();
    if let Some((mesh, image, field)) = tile.terrain {
        let material = materials.add(StandardMaterial {
            base_color_texture: Some(images.add(image)),
            perceptual_roughness: 1.0,
            ..default()
        });
        commands.spawn((TileEntity(tile.name.clone()), Mesh3d(meshes.add(mesh)), MeshMaterial3d(material)));
        if !backdrop {
            streamer.ground.insert(tile.name.clone(), field);
        }
    }
    for (key, sections) in tile.meshes {
        let parts = sections
            .into_iter()
            .map(|s| {
                let material = streamer
                    .materials
                    .entry(s.material_key)
                    .or_insert_with(|| materials.add(material_from_info(s.material, &mut images)))
                    .clone();
                MeshPart { mesh: meshes.add(s.mesh), material }
            })
            .collect();
        streamer.meshes.insert(key, parts);
    }
    let count = tile.placements.len();
    for (key, transform) in tile.placements {
        let Some(parts) = streamer.meshes.get(&key) else { continue };
        if parts.is_empty() {
            continue;
        }
        commands.spawn((TileEntity(tile.name.clone()), transform, Visibility::default())).with_children(|c| {
            for part in parts {
                c.spawn((Mesh3d(part.mesh.clone()), MeshMaterial3d(part.material.clone())));
            }
        });
    }
    info!("tile {} ready: {count} static meshes, {} collision triangles", tile.name, tile.collision.len());
    streamer.loaded.insert(tile.name);
}
