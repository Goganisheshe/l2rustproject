//! World viewer: `L2_WORLD=20_21[,20_22,...]` loads map tiles (terrain and
//! static meshes) and lets you fly around with keyboard or gamepad.
//!
//! Controls: WASD / left stick move, Q/E or triggers down/up, arrow keys /
//! right stick look, Shift / South button for speed.

use std::collections::HashMap;

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use ue2::world::{read_heightmap, read_map, Terrain};
use ue2::ObjectRef;

use crate::l2assets::{to_image, L2Library, MeshPart, UU_TO_M};

/// Baked terrain texture resolution per tile.
const TERRAIN_TEXELS: usize = 2048;

pub struct WorldPlugin {
    pub tiles: Vec<String>,
}

#[derive(Resource)]
struct Tiles(Vec<String>);

#[derive(Component)]
struct FlyCam {
    yaw: f32,
    pitch: f32,
}

impl Plugin for WorldPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(Tiles(self.tiles.clone()))
            .insert_resource(ClearColor(Color::srgb(0.55, 0.68, 0.85)))
            .add_systems(Startup, load_world)
            .add_systems(Update, fly);
    }
}

fn ue(p: [f32; 3]) -> Vec3 {
    Vec3::new(p[0], p[2], p[1]) * UU_TO_M
}

/// Unreal rotator -> Bevy rotation (see FRotationMatrix; axes mirrored Y<->Z).
pub fn ue_rotator(r: [i32; 3]) -> Quat {
    let k = std::f32::consts::TAU / 65536.0;
    let (p, y, rl) = (r[0] as f32 * k, r[1] as f32 * k, r[2] as f32 * k);
    let (sp, cp) = p.sin_cos();
    let (sy, cy) = y.sin_cos();
    let (sr, cr) = rl.sin_cos();
    // Unreal axes of the rotated frame, in Unreal coordinates.
    let x = Vec3::new(cp * cy, cp * sy, sp);
    let yv = Vec3::new(sr * sp * cy - cr * sy, sr * sp * sy + cr * cy, -sr * cp);
    let z = Vec3::new(-(cr * sp * cy + sr * sy), cy * sr - cr * sp * sy, cr * cp);
    let swap = |v: Vec3| Vec3::new(v.x, v.z, v.y);
    // Bevy local x = Unreal x, local y = Unreal z, local z = Unreal y.
    Quat::from_mat3(&Mat3::from_cols(swap(x), swap(z), swap(yv))).normalize()
}

#[allow(clippy::too_many_arguments)]
fn load_world(
    mut commands: Commands,
    tiles: Res<Tiles>,
    mut lib: ResMut<L2Library>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    let mut first_center = None;
    let mut cache: HashMap<String, Vec<MeshPart>> = HashMap::new();
    for name in &tiles.0 {
        let Some(pkg) = lib.lib.as_mut().and_then(|l| l.package(name)) else {
            warn!("map {name} not found");
            continue;
        };
        let tile = match read_map(&pkg) {
            Ok(t) => t,
            Err(e) => {
                warn!("map {name}: {e}");
                continue;
            }
        };
        for t in &tile.terrains {
            if let Some(center) = spawn_terrain(&mut commands, &mut lib, &pkg, t, &mut meshes, &mut materials, &mut images)
            {
                first_center.get_or_insert(center);
            }
        }
        let mut placed = 0;
        for a in &tile.actors {
            let Some(h) = lib.lib.as_mut().and_then(|l| l.resolve(&pkg, a.mesh)) else { continue };
            let key = format!("{}.{}", h.package.name, h.name());
            let parts = cache.entry(key).or_insert_with(|| {
                let (p, n) = (h.package.name.clone(), h.name().to_string());
                lib.static_mesh(&p, &n, &mut meshes, &mut materials, &mut images).unwrap_or_default()
            });
            if parts.is_empty() {
                continue;
            }
            placed += 1;
            commands
                .spawn((
                    Transform {
                        translation: ue(a.location),
                        rotation: ue_rotator(a.rotation),
                        scale: Vec3::new(a.scale[0], a.scale[2], a.scale[1]),
                    },
                    Visibility::default(),
                ))
                .with_children(|c| {
                    for part in parts.iter() {
                        c.spawn((Mesh3d(part.mesh.clone()), MeshMaterial3d(part.material.clone())));
                    }
                });
        }
        info!("map {name}: {placed}/{} static meshes, {} terrains", tile.actors.len(), tile.terrains.len());
    }

    commands.spawn((
        DirectionalLight { illuminance: 9000.0, shadow_maps_enabled: true, ..default() },
        Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -0.9, 0.5, 0.0)),
    ));
    commands.insert_resource(GlobalAmbientLight { brightness: 600.0, ..default() });
    let start = first_center.unwrap_or(Vec3::ZERO) + Vec3::new(0.0, 60.0, 0.0);
    commands.spawn((
        Camera3d::default(),
        Projection::Perspective(PerspectiveProjection { far: 3000.0, ..default() }),
        Transform::from_translation(start),
        FlyCam { yaw: 0.0, pitch: -0.5 },
    ));
}

/// Builds the terrain mesh from the heightmap and bakes its texture layers
/// into one image. Returns the terrain center in Bevy space.
#[allow(clippy::too_many_arguments)]
fn spawn_terrain(
    commands: &mut Commands,
    lib: &mut L2Library,
    pkg: &std::sync::Arc<ue2::Package>,
    t: &Terrain,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
) -> Option<Vec3> {
    let l = lib.lib.as_mut()?;
    let hm_handle = l.resolve(pkg, t.heightmap)?;
    let hm = read_heightmap(&hm_handle.package, hm_handle.export).ok()?;
    let (w, h) = (hm.width, hm.height);
    let [sx, sy, sz] = t.scale;
    let loc = t.location;

    // Vertex (i, j) sits at Location + ((i - w/2) * sx, (j - h/2) * sy, (height - 32768) * sz / 256).
    let mut positions = Vec::with_capacity(w * h);
    let mut uvs = Vec::with_capacity(w * h);
    for j in 0..h {
        for i in 0..w {
            let z = (hm.data[j * w + i] as f32 - 32768.0) * sz / 256.0;
            let p = [loc[0] + (i as f32 - w as f32 / 2.0) * sx, loc[1] + (j as f32 - h as f32 / 2.0) * sy, loc[2] + z];
            positions.push(ue(p).to_array());
            uvs.push([i as f32 / (w - 1) as f32, j as f32 / (h - 1) as f32]);
        }
    }
    let mut indices = Vec::with_capacity((w - 1) * (h - 1) * 6);
    for j in 0..h - 1 {
        for i in 0..w - 1 {
            let a = (j * w + i) as u32;
            let (b, c, d) = (a + 1, a + w as u32, a + w as u32 + 1);
            // Counter-clockwise seen from above in Bevy space (Y/Z mirrored).
            indices.extend_from_slice(&[a, c, b, b, c, d]);
        }
    }
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD);
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(Indices::U32(indices));
    mesh.compute_smooth_normals();

    let baked = bake_layers(lib, pkg, t, w, h);
    let material = materials.add(StandardMaterial {
        base_color_texture: baked.map(|i| images.add(i)),
        perceptual_roughness: 1.0,
        ..default()
    });
    commands.spawn((Mesh3d(meshes.add(mesh)), MeshMaterial3d(material)));
    Some(ue(loc))
}

/// Blends the terrain layers (each a tiled texture weighted by its alpha
/// map) into one texture covering the whole tile.
fn bake_layers(lib: &mut L2Library, pkg: &std::sync::Arc<ue2::Package>, t: &Terrain, w: usize, _h: usize) -> Option<Image> {
    let l = lib.lib.as_mut()?;
    let n = TERRAIN_TEXELS;
    let mut out = vec![0f32; n * n * 3];
    let tex_of = |l: &mut ue2::library::Library, r: ObjectRef| l.resolve(pkg, r).and_then(|h| l.diffuse_texture(&h));
    for (li, layer) in t.layers.iter().enumerate() {
        let Some(tex) = tex_of(l, layer.texture) else { continue };
        let alpha = if layer.alpha_map == ObjectRef::Null { None } else { tex_of(l, layer.alpha_map) };
        // Average the layer texture over one baked texel to avoid aliasing.
        let world_per_texel = (w as f32 - 1.0) * t.scale[0] / n as f32;
        let tex_px_per_world = 1.0 / layer.uscale.max(0.01);
        let footprint = (world_per_texel * tex_px_per_world).max(1.0) as usize;
        let small = downsample(&tex, footprint.min(tex.width as usize));
        let (tw, th) = (small.0, small.1);
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
                let wx = x as f32 * world_per_texel;
                let wy = y as f32 * world_per_texel;
                let tx = ((wx * tex_px_per_world) as usize / footprint.max(1)) % tw;
                let ty = ((wy * tex_px_per_world) as usize / footprint.max(1)) % th;
                let c = &small.2[(ty * tw + tx) * 3..];
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

fn fly(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    gamepads: Query<&Gamepad>,
    mut cam: Single<(&mut Transform, &mut FlyCam)>,
) {
    let dt = time.delta_secs();
    let (ref mut tr, ref mut fc) = *cam;
    let mut mv = Vec3::ZERO;
    let mut look = Vec2::ZERO;
    let mut fast = keys.pressed(KeyCode::ShiftLeft);
    if keys.pressed(KeyCode::KeyW) {
        mv.z -= 1.0;
    }
    if keys.pressed(KeyCode::KeyS) {
        mv.z += 1.0;
    }
    if keys.pressed(KeyCode::KeyA) {
        mv.x -= 1.0;
    }
    if keys.pressed(KeyCode::KeyD) {
        mv.x += 1.0;
    }
    if keys.pressed(KeyCode::KeyE) {
        mv.y += 1.0;
    }
    if keys.pressed(KeyCode::KeyQ) {
        mv.y -= 1.0;
    }
    if keys.pressed(KeyCode::ArrowLeft) {
        look.x -= 1.0;
    }
    if keys.pressed(KeyCode::ArrowRight) {
        look.x += 1.0;
    }
    if keys.pressed(KeyCode::ArrowUp) {
        look.y += 1.0;
    }
    if keys.pressed(KeyCode::ArrowDown) {
        look.y -= 1.0;
    }
    for pad in &gamepads {
        let l = pad.left_stick();
        if l.length() > 0.15 {
            mv.x += l.x;
            mv.z -= l.y;
        }
        let r = pad.right_stick();
        if r.length() > 0.15 {
            look += r;
        }
        mv.y += pad.get(GamepadButton::RightTrigger2).unwrap_or(0.0) - pad.get(GamepadButton::LeftTrigger2).unwrap_or(0.0);
        fast |= pad.pressed(GamepadButton::South);
    }
    fc.yaw -= look.x * 1.6 * dt;
    fc.pitch = (fc.pitch + look.y * 1.2 * dt).clamp(-1.5, 1.5);
    tr.rotation = Quat::from_euler(EulerRot::YXZ, fc.yaw, fc.pitch, 0.0);
    let speed = if fast { 120.0 } else { 30.0 };
    let dir = tr.rotation * Vec3::new(mv.x, 0.0, mv.z) + Vec3::Y * mv.y;
    tr.translation += dir * speed * dt;
}
