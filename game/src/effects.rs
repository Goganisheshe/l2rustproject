//! A small UE2-style particle system driving Lineage effect classes
//! (`el_wind_strike_fl`, `soul_N_stick`, ...): sprite and mesh emitters with
//! lifetimes, velocity, acceleration, size/color curves, fades, spin and
//! texture atlases. Particles live in world space, like the engine default.

use std::collections::HashMap;
use std::sync::Arc;

use bevy::light::NotShadowCaster;
use bevy::math::Affine2;
use bevy::prelude::*;
use rand::Rng;
use ue2::effect::{EmitterDef, EmitterKind};

use crate::l2assets::{to_image, L2Library, MeshPart, UU_TO_M};

/// Script packages that hold effect classes, searched in order.
const EFFECT_PACKAGES: &[&str] = &[
    "lineageeffect",
    "lineageeffect2",
    "lineageeffect_br",
    "lineageskilleffect",
    "la2worldeffect",
    "la2world_effect",
    "lineagemonster",
    "lineagemonster2",
    "lineagemonster3",
    "lineagemonster4",
    "lineagemonster5",
    "lineagemonster6",
    "lineagemonster7",
    "lineagemonster8",
    "lineagemonster9",
    "lineagemonster10",
    "lineagemonster11",
    "lineagemonster12",
    "lineagemonster13",
    "lineagemonster99",
    "lineagenpc",
];

/// Hard cap per emitter instance, to keep hordes of projectiles affordable.
const MAX_PARTICLES_PER_EMITTER: u32 = 40;

pub struct EffectsPlugin;

impl Plugin for EffectsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<EffectCache>().add_systems(Update, (emit, update_particles).chain());
    }
}

enum Visual {
    Sprite { image: Handle<Image> },
    Mesh { parts: Vec<MeshPart> },
}

struct Emitter {
    def: EmitterDef,
    visual: Visual,
}

pub struct EffectAsset {
    emitters: Vec<Emitter>,
}

#[derive(Resource, Default)]
pub struct EffectCache {
    effects: HashMap<String, Option<Arc<EffectAsset>>>,
    quad: Option<Handle<Mesh>>,
}

impl EffectCache {
    /// Loads an effect class by name from the client's effect packages.
    pub fn load(
        &mut self,
        lib: &mut L2Library,
        class: &str,
        meshes: &mut Assets<Mesh>,
        materials: &mut Assets<StandardMaterial>,
        images: &mut Assets<Image>,
    ) -> Option<Arc<EffectAsset>> {
        if self.quad.is_none() {
            self.quad = Some(meshes.add(Rectangle::new(1.0, 1.0)));
        }
        if let Some(e) = self.effects.get(class) {
            return e.clone();
        }
        let asset = build(lib, class, meshes, materials, images).map(Arc::new);
        if asset.is_none() {
            warn!("effect {class} not found");
        }
        self.effects.insert(class.to_string(), asset.clone());
        asset
    }
}

fn build(
    lib: &mut L2Library,
    class: &str,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
) -> Option<EffectAsset> {
    let l = lib.lib.as_mut()?;
    let (pkg, defs) = EFFECT_PACKAGES.iter().find_map(|p| {
        let pkg = l.package(p)?;
        let defs = ue2::effect::read_effect(&pkg, class)?;
        Some((pkg, defs))
    })?;
    let mut emitters = Vec::new();
    for def in defs {
        let visual = match def.kind {
            EmitterKind::Sprite => {
                let l = lib.lib.as_mut()?;
                let Some(mut tex) = l.resolve(&pkg, def.texture).and_then(|h| l.diffuse_texture(&h)) else { continue };
                // UE2 "Darken" is dst * (1 - src): a black texture whose alpha is
                // the source brightness gives exactly that with alpha blending.
                // "Modulated" (dst * src * 2) is approximated by multiplying with
                // a brightened texture.
                match def.draw_style {
                    5 => tex.rgba.chunks_exact_mut(4).for_each(|p| {
                        let luma = (p[0] as u32 * 3 + p[1] as u32 * 6 + p[2] as u32) / 10;
                        p[3] = (luma * p[3] as u32 / 255) as u8;
                        p[0] = 0;
                        p[1] = 0;
                        p[2] = 0;
                    }),
                    2 => tex.rgba.chunks_exact_mut(4).for_each(|p| {
                        p[0] = p[0].saturating_mul(2);
                        p[1] = p[1].saturating_mul(2);
                        p[2] = p[2].saturating_mul(2);
                    }),
                    _ => {}
                }
                Visual::Sprite { image: images.add(to_image(tex, false)) }
            }
            EmitterKind::Mesh => {
                let l = lib.lib.as_mut()?;
                let Some(h) = l.resolve(&pkg, def.static_mesh) else { continue };
                let name = h.name().to_string();
                let Some(parts) = lib.static_mesh(&h.package.name, &name, meshes, materials, images) else { continue };
                Visual::Mesh { parts }
            }
            EmitterKind::Other(_) => continue,
        };
        emitters.push(Emitter { def, visual });
    }
    Some(EffectAsset { emitters })
}

/// A running effect. Follows `follow` while it exists; once it is gone the
/// effect stops emitting and despawns after its last particle.
#[derive(Component)]
pub struct Effect {
    asset: Arc<EffectAsset>,
    follow: Option<Entity>,
    age: f32,
    state: Vec<EmitterState>,
    stopped: bool,
}

#[derive(Default, Clone)]
struct EmitterState {
    spawned: u32,
    budget: f32,
    delay: f32,
    alive: u32,
}

impl Effect {
    pub fn new(asset: Arc<EffectAsset>, follow: Option<Entity>) -> Self {
        let mut rng = rand::rng();
        let state = asset
            .emitters
            .iter()
            .map(|e| EmitterState { delay: rand_range(&mut rng, e.def.initial_delay), ..default() })
            .collect();
        Self { asset, follow, age: 0.0, state, stopped: false }
    }
}

/// Spawns `effect` at `transform` (X axis = effect forward), optionally
/// following an entity.
pub fn spawn_effect(commands: &mut Commands, asset: &Arc<EffectAsset>, transform: Transform, follow: Option<Entity>) {
    commands.spawn((Effect::new(asset.clone(), follow), transform, Visibility::default()));
}

#[derive(Component)]
struct Particle {
    effect: Entity,
    emitter: usize,
    asset: Arc<EffectAsset>,
    age: f32,
    life: f32,
    velocity: Vec3,
    acceleration: Vec3,
    base_scale: Vec3,
    color: [f32; 3],
    spin: f32,
    spin_rate: f32,
    base_rotation: Quat,
    material: Option<Handle<StandardMaterial>>,
}

fn rand_range(rng: &mut impl Rng, r: [f32; 2]) -> f32 {
    let (a, b) = if r[0] <= r[1] { (r[0], r[1]) } else { (r[1], r[0]) };
    if b > a {
        rng.random_range(a..=b)
    } else {
        a
    }
}

fn rand_vec(rng: &mut impl Rng, r: [[f32; 2]; 3]) -> [f32; 3] {
    [rand_range(rng, r[0]), rand_range(rng, r[1]), rand_range(rng, r[2])]
}

/// Unreal vector -> Bevy local vector (meters), see `l2assets::ue_to_bevy`.
fn uu(v: [f32; 3]) -> Vec3 {
    Vec3::new(v[0], v[2], v[1]) * UU_TO_M
}

#[allow(clippy::too_many_arguments)]
fn emit(
    mut commands: Commands,
    time: Res<Time>,
    cache: Res<EffectCache>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut effects: Query<(Entity, &mut Effect, &mut Transform)>,
    followed: Query<&GlobalTransform, Without<Effect>>,
) {
    let dt = time.delta_secs();
    let mut rng = rand::rng();
    let Some(quad) = cache.quad.clone() else { return };
    for (entity, mut fx, mut tr) in &mut effects {
        fx.age += dt;
        if let Some(f) = fx.follow {
            match followed.get(f) {
                Ok(g) => {
                    let (_, r, t) = g.to_scale_rotation_translation();
                    tr.translation = t;
                    tr.rotation = r;
                }
                Err(_) => fx.stopped = true,
            }
        }
        let asset = fx.asset.clone();
        let stopped = fx.stopped;
        let mut finished = true;
        for (i, em) in asset.emitters.iter().enumerate() {
            let d = &em.def;
            let st = &mut fx.state[i];
            let cap = d.max_particles.min(MAX_PARTICLES_PER_EMITTER);
            let exhausted = !d.respawn && st.spawned >= cap;
            if st.alive > 0 || (!stopped && !exhausted) {
                finished = false;
            }
            if stopped || exhausted {
                continue;
            }
            if st.delay > 0.0 {
                st.delay -= dt;
                continue;
            }
            st.budget += d.rate.max(0.0) * dt;
            if d.rate <= 0.0 && st.spawned == 0 {
                st.budget = cap as f32;
            }
            while st.budget >= 1.0 && (d.respawn && st.alive < cap || !d.respawn && st.spawned < cap) {
                st.budget -= 1.0;
                st.spawned += 1;
                st.alive += 1;
                let offset = uu(d.location_offset) + uu(rand_vec(&mut rng, d.location_range));
                let pos = tr.translation + tr.rotation * offset;
                let velocity = tr.rotation * uu(rand_vec(&mut rng, d.velocity));
                let acceleration = tr.rotation * uu(d.acceleration);
                let size = rand_vec(&mut rng, d.start_size);
                let cm = rand_vec(&mut rng, d.color_multiplier);
                let spin = if d.spin { spin_radians(rand_range(&mut rng, d.start_spin[0])) } else { 0.0 };
                let spin_rate = if d.spin { spin_radians(rand_range(&mut rng, d.spin_rate[0])) } else { 0.0 };
                let mut particle = Particle {
                    effect: entity,
                    emitter: i,
                    asset: asset.clone(),
                    age: 0.0,
                    life: rand_range(&mut rng, d.lifetime).max(0.02),
                    velocity,
                    acceleration,
                    base_scale: Vec3::ONE,
                    color: cm,
                    spin,
                    spin_rate,
                    base_rotation: tr.rotation,
                    material: None,
                };
                let mut transform = Transform::from_translation(pos);
                match &em.visual {
                    Visual::Sprite { image } => {
                        let s = if d.uniform_size { size[0] } else { size[0].max(size[1]) } * UU_TO_M * 2.0;
                        particle.base_scale = Vec3::splat(s);
                        let mat = materials.add(sprite_material(d, image.clone(), &mut rng));
                        particle.material = Some(mat.clone());
                        commands.spawn((particle, transform, Mesh3d(quad.clone()), MeshMaterial3d(mat), NotShadowCaster));
                    }
                    Visual::Mesh { parts } => {
                        let s = if d.uniform_size { Vec3::splat(size[0]) } else { Vec3::new(size[0], size[2], size[1]) };
                        particle.base_scale = s;
                        if d.rotation_from_actor {
                            transform.rotation = tr.rotation;
                        }
                        let mut mats = Vec::new();
                        for part in parts {
                            let base = materials.get(&part.material).cloned().unwrap_or_default();
                            mats.push(materials.add(effect_material(d, base)));
                        }
                        particle.material = mats.first().cloned();
                        commands.spawn((particle, transform, Visibility::default())).with_children(|c| {
                            for (part, mat) in parts.iter().zip(mats) {
                                c.spawn((Mesh3d(part.mesh.clone()), MeshMaterial3d(mat), NotShadowCaster));
                            }
                        });
                    }
                }
            }
        }
        if finished && fx.age > 0.1 {
            commands.entity(entity).despawn();
        }
    }
}

/// Spin values are revolutions in UE2; some Lineage effects store degrees.
fn spin_radians(v: f32) -> f32 {
    if v.abs() > 4.0 {
        v.to_radians()
    } else {
        v * std::f32::consts::TAU
    }
}

fn blend(d: &EmitterDef) -> AlphaMode {
    match d.draw_style {
        1 | 4 | 5 => AlphaMode::Blend,
        2 => AlphaMode::Multiply,
        _ => AlphaMode::Add,
    }
}

fn sprite_material(d: &EmitterDef, image: Handle<Image>, rng: &mut impl Rng) -> StandardMaterial {
    let (u, v) = d.subdivisions;
    let uv_transform = if u > 1 || v > 1 {
        let (u, v) = (u.max(1), v.max(1));
        let (start, end) = d.subdivision_range;
        let count = u * v;
        let cell = if end > start { rng.random_range(start..=end.min(count - 1)) } else { start.min(count - 1) };
        let (cx, cy) = (cell % u, cell / u);
        Affine2::from_scale_angle_translation(
            Vec2::new(1.0 / u as f32, 1.0 / v as f32),
            0.0,
            Vec2::new(cx as f32 / u as f32, cy as f32 / v as f32),
        )
    } else {
        Affine2::IDENTITY
    };
    StandardMaterial {
        base_color_texture: Some(image),
        unlit: true,
        alpha_mode: blend(d),
        double_sided: true,
        cull_mode: None,
        uv_transform,
        ..default()
    }
}

fn effect_material(d: &EmitterDef, base: StandardMaterial) -> StandardMaterial {
    StandardMaterial {
        unlit: true,
        alpha_mode: blend(d),
        double_sided: true,
        cull_mode: None,
        base_color: Color::WHITE,
        ..base
    }
}

fn curve<T: Copy>(points: &[(f32, T)], t: f32, lerp: impl Fn(T, T, f32) -> T) -> Option<T> {
    let first = points.first()?;
    if t <= first.0 {
        return Some(first.1);
    }
    for w in points.windows(2) {
        let ((t0, a), (t1, b)) = (w[0], w[1]);
        if t <= t1 {
            let k = if t1 > t0 { (t - t0) / (t1 - t0) } else { 1.0 };
            return Some(lerp(a, b, k));
        }
    }
    points.last().map(|p| p.1)
}

fn update_particles(
    mut commands: Commands,
    time: Res<Time>,
    camera: Single<&GlobalTransform, With<Camera3d>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut particles: Query<(Entity, &mut Particle, &mut Transform)>,
    mut effects: Query<&mut Effect>,
) {
    let dt = time.delta_secs();
    let cam_rot = camera.to_scale_rotation_translation().1;
    for (e, mut p, mut tr) in &mut particles {
        p.age += dt;
        if p.age >= p.life {
            if let Ok(mut fx) = effects.get_mut(p.effect) {
                if let Some(st) = fx.state.get_mut(p.emitter) {
                    st.alive = st.alive.saturating_sub(1);
                }
            }
            commands.entity(e).despawn();
            continue;
        }
        let accel = p.acceleration;
        p.velocity += accel * dt;
        tr.translation += p.velocity * dt;
        p.spin += p.spin_rate * dt;

        let d = &p.asset.emitters[p.emitter].def;
        let t = p.age / p.life;
        let size = curve(&d.size_scale, t, |a, b, k| a + (b - a) * k).unwrap_or(1.0);
        tr.scale = p.base_scale * size;
        match p.asset.emitters[p.emitter].visual {
            Visual::Sprite { .. } => tr.rotation = cam_rot * Quat::from_rotation_z(p.spin),
            Visual::Mesh { .. } => {
                if !d.rotation_from_actor {
                    tr.rotation = p.base_rotation;
                }
                tr.rotation *= Quat::from_rotation_x(p.spin_rate * dt);
            }
        }

        // Color: scale curve times multiplier, faded in/out over the lifetime.
        let rgba = curve(&d.color_scale, t, |a: [f32; 4], b: [f32; 4], k| {
            [0, 1, 2, 3].map(|i| a[i] + (b[i] - a[i]) * k)
        })
        .unwrap_or([1.0; 4]);
        let mut alpha = rgba[3];
        if let Some(end) = d.fade_in_end.filter(|&x| x > 0.0) {
            alpha *= (p.age / end).min(1.0);
        }
        if let Some(start) = d.fade_out_start {
            if p.age > start && p.life > start {
                alpha *= 1.0 - (p.age - start) / (p.life - start);
            }
        }
        let c = [rgba[0] * p.color[0], rgba[1] * p.color[1], rgba[2] * p.color[2]];
        if let Some(mut m) = p.material.as_ref().and_then(|h| materials.get_mut(h)) {
            m.base_color = match m.alpha_mode {
                // Additive: fading means darkening.
                AlphaMode::Add => Color::srgb(c[0] * alpha, c[1] * alpha, c[2] * alpha),
                // Multiply: fading means going toward white (no effect).
                AlphaMode::Multiply => {
                    let f = |x: f32| 1.0 - (1.0 - x) * alpha;
                    Color::srgb(f(c[0]), f(c[1]), f(c[2]))
                }
                _ => Color::srgba(c[0], c[1], c[2], alpha),
            };
        }
    }
}
