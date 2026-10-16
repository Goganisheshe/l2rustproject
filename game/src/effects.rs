//! A small UE2-style particle system driving Lineage effect classes
//! (`el_wind_strike_fl`, `soul_N_stick`, ...): sprite and mesh emitters with
//! lifetimes, velocity, acceleration, size/color curves, fades, spin and
//! texture atlases. Particles live in world space, like the engine default.
//! Beam and trail emitters draw ribbons (`ribbon.rs`) rebuilt every frame.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::light::NotShadowCaster;
use bevy::math::Affine2;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use rand::Rng;
use ue2::effect::{BeamDef, BeamEnd, EmitterDef, EmitterKind, TrailDef, TrailShade};

use crate::l2assets::{to_image, L2Library, MeshPart, UU_TO_M};
use crate::ribbon::{self, Facing, RibbonPoint};

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
/// Hard caps for ribbon geometry.
const MAX_TRAIL_POINTS: usize = 64;
const MAX_BEAM_POINTS: usize = 48;
/// Beam end when nothing better is known (UU).
const DEFAULT_BEAM_LENGTH: f32 = 500.0;

pub struct EffectsPlugin;

impl Plugin for EffectsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<EffectCache>().add_systems(Update, (emit, update_particles).chain());
    }
}

enum Visual {
    Sprite { image: Handle<Image> },
    Mesh { parts: Vec<MeshPart> },
    Beam { image: Handle<Image>, beam: BeamDef },
    Trail { image: Handle<Image>, trail: TrailDef },
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
    // Emitter index in the class -> index in `emitters` (some are skipped).
    let mut index_map: Vec<Option<usize>> = vec![None; defs.len()];
    for (def_index, def) in defs.into_iter().enumerate() {
        if def.disabled {
            continue;
        }
        let visual = match def.kind {
            EmitterKind::Sprite | EmitterKind::Beam | EmitterKind::Trail => {
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
                // Ribbons repeat their texture along their length.
                let ribbon = def.beam.is_some() || def.trail.is_some();
                let image = images.add(to_image(tex, ribbon));
                match (&def.beam, &def.trail) {
                    (Some(beam), _) => Visual::Beam { image, beam: beam.clone() },
                    (_, Some(trail)) => Visual::Trail { image, trail: trail.clone() },
                    _ => Visual::Sprite { image },
                }
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
        index_map[def_index] = Some(emitters.len());
        emitters.push(Emitter { def, visual });
    }
    for em in &mut emitters {
        if let Visual::Trail { trail, .. } = &mut em.visual {
            trail.attach = trail.attach.and_then(|a| index_map.get(a).copied().flatten());
        }
    }
    Some(EffectAsset { emitters })
}

/// A running effect. Follows `follow` while it exists; once it is gone the
/// effect stops emitting and despawns after its last particle.
#[derive(Component)]
pub struct Effect {
    asset: Arc<EffectAsset>,
    follow: Option<Entity>,
    /// Where `PTEP_Actor` beams end.
    target: Option<Entity>,
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
        Self { asset, follow, target: None, age: 0.0, state, stopped: false }
    }
}

/// Spawns `effect` at `transform` (X axis = effect forward), optionally
/// following an entity.
pub fn spawn_effect(commands: &mut Commands, asset: &Arc<EffectAsset>, transform: Transform, follow: Option<Entity>) {
    spawn_effect_targeted(commands, asset, transform, follow, None);
}

/// Like `spawn_effect`; beams that end on an actor end on `target`.
pub fn spawn_effect_targeted(
    commands: &mut Commands,
    asset: &Arc<EffectAsset>,
    transform: Transform,
    follow: Option<Entity>,
    target: Option<Entity>,
) {
    let mut fx = Effect::new(asset.clone(), follow);
    fx.target = target;
    commands.spawn((fx, transform, Visibility::default()));
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
    /// Ribbons keep their entity at the origin; this is the particle's own
    /// position (the head of a trail).
    pos: Vec3,
    /// The effect's last transform, for ribbons outliving their effect.
    emitter_tr: Transform,
    ribbon: Option<Ribbon>,
}

struct Ribbon {
    mesh: Handle<Mesh>,
    /// Meters.
    half_width: f32,
    kind: RibbonKind,
}

enum RibbonKind {
    Beam(BeamState),
    Trail(TrailState),
}

struct BeamState {
    /// Start in the effect's local space (meters, Bevy axes).
    start: Vec3,
    end: BeamTarget,
    /// Small-bend offset per polyline point and large-bend offset per control
    /// point, in the effect's local space.
    hf: Vec<Vec3>,
    lf: Vec<Vec3>,
}

enum BeamTarget {
    /// End in the effect's local space.
    Local(Vec3),
    /// An actor; the local end is used once it is gone.
    Entity(Entity, Vec3),
}

struct TrailState {
    points: VecDeque<TrailPoint>,
    /// Spawn offset in effect space when the head stays on the emitter.
    on_emitter: Option<Vec3>,
    /// The head follows the newest particle of this emitter.
    attach: Option<usize>,
}

struct TrailPoint {
    pos: Vec3,
    age: f32,
    shade: f32,
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
    mut meshes: ResMut<Assets<Mesh>>,
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
        let target = fx.target;
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
                    pos,
                    emitter_tr: *tr,
                    ribbon: None,
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
                    Visual::Beam { image, beam } => {
                        let local_velocity = tr.rotation.inverse() * velocity;
                        let state = new_beam(beam, d, offset, local_velocity, particle.life, target, &mut rng);
                        particle.velocity = Vec3::ZERO;
                        particle.acceleration = Vec3::ZERO;
                        let kind = RibbonKind::Beam(state);
                        spawn_ribbon(&mut commands, &mut meshes, &mut materials, particle, kind, d, image, size[0], &mut rng);
                    }
                    Visual::Trail { image, trail } => {
                        let on_emitter = trail.follow_emitter.then_some(offset);
                        let kind = RibbonKind::Trail(TrailState { points: VecDeque::new(), on_emitter, attach: trail.attach });
                        spawn_ribbon(&mut commands, &mut meshes, &mut materials, particle, kind, d, image, size[0], &mut rng);
                    }
                }
            }
        }
        if finished && fx.age > 0.1 {
            commands.entity(entity).despawn();
        }
    }
}

/// Picks the beam's end and noise for one particle (effect-local space).
fn new_beam(
    beam: &BeamDef,
    d: &EmitterDef,
    start: Vec3,
    velocity: Vec3,
    life: f32,
    target: Option<Entity>,
    rng: &mut impl Rng,
) -> BeamState {
    let forward = if velocity.length_squared() > 1e-8 { velocity.normalize() } else { Vec3::X };
    let end = match beam.end {
        BeamEnd::Velocity => {
            let end = start + velocity * life + uu(d.acceleration) * (0.5 * life * life);
            if (end - start).length_squared() > 1e-6 {
                end
            } else {
                beam_end_by_distance(beam, start, forward, rng)
            }
        }
        BeamEnd::Distance | BeamEnd::DynamicDistance => beam_end_by_distance(beam, start, forward, rng),
        // Absolute offsets are world coordinates of one level; effects play
        // anywhere, so they are taken as relative like the others.
        BeamEnd::Offset | BeamEnd::TraceOffset | BeamEnd::OffsetAsAbsolute | BeamEnd::Actor => {
            match beam_end_by_offset(beam, rng) {
                Some(end) => end,
                None => beam_end_by_distance(beam, start, forward, rng),
            }
        }
    };
    let end = match (beam.end, target) {
        (BeamEnd::Actor, Some(e)) => BeamTarget::Entity(e, end),
        _ => BeamTarget::Local(end),
    };
    let points = (beam.hf_points as usize).clamp(2, MAX_BEAM_POINTS);
    let controls = (beam.lf_points as usize).clamp(2, MAX_BEAM_POINTS);
    BeamState {
        start,
        end,
        hf: beam_noise(points, beam.hf_noise, beam.noise_moves_end, rng),
        lf: beam_noise(controls, beam.lf_noise, beam.noise_moves_end, rng),
    }
}

fn beam_end_by_distance(beam: &BeamDef, start: Vec3, forward: Vec3, rng: &mut impl Rng) -> Vec3 {
    let len = if beam.distance[0].max(beam.distance[1]) > 0.0 { rand_range(rng, beam.distance) } else { DEFAULT_BEAM_LENGTH };
    start + forward * len * UU_TO_M
}

/// A `BeamEndPoints` entry picked by weight, its offset randomized.
fn beam_end_by_offset(beam: &BeamDef, rng: &mut impl Rng) -> Option<Vec3> {
    if beam.end_points.is_empty() {
        return None;
    }
    let total: f32 = beam.end_points.iter().map(|p| p.weight.max(0.0)).sum();
    // Lineage leaves Weight at 0 on most single-point beams: pick evenly then.
    if total <= 0.0 {
        let point = &beam.end_points[rng.random_range(0..beam.end_points.len())];
        return Some(uu(rand_vec(rng, point.offset)));
    }
    let mut pick = rng.random_range(0.0..total);
    let point = beam.end_points.iter().find(|p| {
        pick -= p.weight.max(0.0);
        pick < 0.0
    })?;
    Some(uu(rand_vec(rng, point.offset)))
}

/// Random offsets for `n` points; the start is pinned, the end too unless
/// the noise may move it.
fn beam_noise(n: usize, range: [[f32; 2]; 3], moves_end: bool, rng: &mut impl Rng) -> Vec<Vec3> {
    (0..n)
        .map(|i| if i == 0 || (i == n - 1 && !moves_end) { Vec3::ZERO } else { uu(rand_vec(rng, range)) })
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn spawn_ribbon(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    mut particle: Particle,
    kind: RibbonKind,
    d: &EmitterDef,
    image: &Handle<Image>,
    size: f32,
    rng: &mut impl Rng,
) {
    // A degenerate quad until the first update builds the real strip.
    let p = particle.pos.to_array();
    let first = ribbon::RibbonMesh {
        positions: vec![p; 4],
        uvs: vec![[0.0; 2]; 4],
        colors: vec![[0.0; 4]; 4],
        indices: vec![0, 2, 1, 1, 2, 3],
    };
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    write_mesh(&mut mesh, first);
    let mesh = meshes.add(mesh);
    let mat = materials.add(sprite_material(d, image.clone(), rng));
    particle.material = Some(mat.clone());
    particle.ribbon = Some(Ribbon { mesh: mesh.clone(), half_width: size.abs() * UU_TO_M, kind });
    commands.spawn((
        particle,
        Transform::IDENTITY,
        Mesh3d(mesh),
        MeshMaterial3d(mat),
        NotShadowCaster,
        NoFrustumCulling,
    ));
}

fn write_mesh(mesh: &mut Mesh, m: ribbon::RibbonMesh) {
    let n = m.positions.len();
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, m.positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0f32, 1.0, 0.0]; n]);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, m.uvs);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, m.colors);
    mesh.insert_indices(Indices::U32(m.indices));
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

#[allow(clippy::too_many_arguments)]
fn update_particles(
    mut commands: Commands,
    time: Res<Time>,
    camera: Single<&GlobalTransform, With<Camera3d>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut particles: Query<(Entity, &mut Particle, &mut Transform), Without<Effect>>,
    mut effects: Query<(&mut Effect, &Transform), Without<Particle>>,
    targets: Query<&GlobalTransform, (Without<Effect>, Without<Particle>)>,
) {
    let dt = time.delta_secs();
    let now = time.elapsed_secs();
    let (_, cam_rot, cam_pos) = camera.to_scale_rotation_translation();
    let mut rng = rand::rng();
    // Newest live particle of each emitter, for trails attached to one.
    let mut newest: HashMap<(Entity, usize), (f32, Vec3)> = HashMap::new();
    for (_, p, tr) in particles.iter() {
        let pos = if p.ribbon.is_some() { p.pos } else { tr.translation };
        let slot = newest.entry((p.effect, p.emitter)).or_insert((f32::MAX, pos));
        if p.age < slot.0 {
            *slot = (p.age, pos);
        }
    }
    for (e, mut p, mut tr) in &mut particles {
        p.age += dt;
        if p.age >= p.life {
            if let Ok((mut fx, _)) = effects.get_mut(p.effect) {
                if let Some(st) = fx.state.get_mut(p.emitter) {
                    st.alive = st.alive.saturating_sub(1);
                }
            }
            commands.entity(e).despawn();
            continue;
        }
        if let Ok((_, fx_tr)) = effects.get(p.effect) {
            p.emitter_tr = *fx_tr;
        }
        let accel = p.acceleration;
        p.velocity += accel * dt;
        p.spin += p.spin_rate * dt;

        let asset = p.asset.clone();
        let d = &asset.emitters[p.emitter].def;
        let t = p.age / p.life;
        let size = curve(&d.size_scale, t, |a, b, k| a + (b - a) * k).unwrap_or(1.0);
        if p.ribbon.is_some() {
            let step = p.velocity * dt;
            p.pos += step;
            let attached = match &p.ribbon {
                Some(Ribbon { kind: RibbonKind::Trail(t), .. }) => {
                    t.attach.and_then(|a| newest.get(&(p.effect, a))).map(|x| x.1)
                }
                _ => None,
            };
            update_ribbon(&mut p, size, dt, cam_pos, now, attached, &targets, &mut meshes, &mut rng);
        } else {
            tr.translation += p.velocity * dt;
            tr.scale = p.base_scale * size;
        }
        match asset.emitters[p.emitter].visual {
            Visual::Sprite { .. } => tr.rotation = cam_rot * Quat::from_rotation_z(p.spin),
            Visual::Mesh { .. } => {
                if !d.rotation_from_actor {
                    tr.rotation = p.base_rotation;
                }
                tr.rotation *= Quat::from_rotation_x(p.spin_rate * dt);
            }
            Visual::Beam { .. } | Visual::Trail { .. } => {}
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

/// Moves a beam or trail and rebuilds its strip.
/// Bezier control polygon of a Lineage curved beam: start, each control
/// point at its place along the beam swinging over time, end. Offsets are in
/// a frame along the beam (Unreal X forward, Y side, Z up).
fn beam_curve(beam: &BeamDef, start: Vec3, end: Vec3, rotation: Quat, time: f32) -> Vec<[f32; 3]> {
    let (f, side, up) = ribbon::line_frame((end - start).to_array(), [0.0, 1.0, 0.0], (rotation * Vec3::X).to_array());
    let (f, side, up) = (Vec3::from_array(f), Vec3::from_array(side), Vec3::from_array(up));
    let mut points = vec![start.to_array()];
    let mut controls: Vec<_> = beam.control_points.iter().collect();
    controls.sort_by(|a, b| a.ratio.total_cmp(&b.ratio));
    for c in controls {
        let swing = (std::f32::consts::TAU * c.frequency * time).sin();
        let o = [0, 1, 2].map(|k| c.pivot[k] + c.oscillation[k] * swing);
        let offset = (f * o[0] + side * o[1] + up * o[2]) * UU_TO_M;
        points.push((start.lerp(end, c.ratio.clamp(0.0, 1.0)) + offset).to_array());
    }
    points.push(end.to_array());
    points
}

#[allow(clippy::too_many_arguments)]
fn update_ribbon(
    p: &mut Particle,
    size: f32,
    dt: f32,
    cam: Vec3,
    time: f32,
    attached: Option<Vec3>,
    targets: &Query<&GlobalTransform, (Without<Effect>, Without<Particle>)>,
    meshes: &mut Assets<Mesh>,
    rng: &mut impl Rng,
) {
    let asset = p.asset.clone();
    let visual = &asset.emitters[p.emitter].visual;
    let etr = p.emitter_tr;
    let head = p.pos;
    let Some(r) = p.ribbon.as_mut() else { return };
    let half = r.half_width * size;
    let (points, facing, v_scale): (Vec<RibbonPoint>, Facing, f32) = match (&mut r.kind, visual) {
        (RibbonKind::Beam(b), Visual::Beam { beam, .. }) => {
            let start = etr.transform_point(b.start);
            let end = match b.end {
                BeamTarget::Local(v) => etr.transform_point(v),
                BeamTarget::Entity(e, v) => {
                    targets.get(e).map(|g| g.translation() + Vec3::Y).unwrap_or_else(|_| etr.transform_point(v))
                }
            };
            let dir = (end - start).normalize_or_zero().to_array();
            let lf: Vec<[f32; 3]> = b.lf.iter().map(|v| v.to_array()).collect();
            let hf: Vec<[f32; 3]> = b.hf.iter().map(|v| v.to_array()).collect();
            let curve = beam.bezier.then(|| beam_curve(beam, start, end, etr.rotation, time));
            // A curve is sampled densely; a straight beam has one point per HF point.
            let n = match &curve {
                Some(_) => (beam.density as usize).clamp(8, MAX_BEAM_POINTS),
                None => b.hf.len().max(2),
            };
            let pos: Vec<[f32; 3]> = (0..n)
                .map(|i| {
                    let t = i as f32 / (n - 1) as f32;
                    let base = match &curve {
                        Some(c) => Vec3::from_array(ribbon::bezier(c, t)),
                        None => start.lerp(end, t),
                    };
                    let hf_noise = match &curve {
                        Some(_) => Vec3::from_array(ribbon::sample_lf(&hf, t)),
                        None => b.hf.get(i).copied().unwrap_or(Vec3::ZERO),
                    };
                    let local = hf_noise + Vec3::from_array(ribbon::sample_lf(&lf, t));
                    // Noise bends the beam sideways, never along it.
                    let noise = Vec3::from_array(ribbon::reject((etr.rotation * local).to_array(), dir));
                    (base + noise).to_array()
                })
                .collect();
            let u = ribbon::u_by_length(&pos, beam.u_scale);
            let pts = pos.iter().zip(u).map(|(q, u)| RibbonPoint { pos: *q, half_width: half, u, shade: 1.0 }).collect();
            let facing = if beam.sheets > 1 { Facing::Sheets(beam.sheets) } else { Facing::Camera(cam.to_array()) };
            (pts, facing, beam.v_scale)
        }
        (RibbonKind::Trail(tr), Visual::Trail { trail, .. }) => {
            // An attached trail stops growing once its particle is gone.
            let head = match (tr.attach, tr.on_emitter) {
                (Some(_), _) => attached,
                (None, Some(offset)) => Some(etr.transform_point(offset)),
                (None, None) => Some(head),
            };
            for pt in tr.points.iter_mut() {
                pt.age += dt;
            }
            if trail.point_life > 0.0 {
                tr.points.retain(|pt| pt.age < trail.point_life);
            }
            let threshold = trail.distance_threshold * UU_TO_M;
            if let Some(head) = head {
                if tr.points.back().is_none_or(|last| last.pos.distance(head) > threshold) {
                    tr.points.push_back(TrailPoint { pos: head, age: 0.0, shade: rng.random_range(0.0..=1.0) });
                }
            }
            let cap = (trail.max_points as usize).clamp(2, MAX_TRAIL_POINTS);
            while tr.points.len() > cap {
                tr.points.pop_front();
            }
            // Oldest point first; the live head closes the strip.
            let mut line: Vec<(Vec3, f32, f32)> = tr.points.iter().map(|pt| (pt.pos, pt.age, pt.shade)).collect();
            if let Some(head) = head.filter(|h| line.last().is_some_and(|l| l.0.distance(*h) > 1e-4)) {
                line.push((head, 0.0, 1.0));
            }
            let n = line.len();
            let pos: Vec<[f32; 3]> = line.iter().map(|l| l.0.to_array()).collect();
            let u = ribbon::u_by_length(&pos, 1.0);
            let pts = line
                .iter()
                .zip(u)
                .enumerate()
                .map(|(i, (l, u))| {
                    let shade = match trail.shade {
                        TrailShade::None => 1.0,
                        TrailShade::RandomStatic => l.2,
                        TrailShade::RandomDynamic => rng.random_range(0.0..=1.0),
                        TrailShade::Linear if n > 1 => i as f32 / (n - 1) as f32,
                        TrailShade::Linear => 1.0,
                        TrailShade::PointLife if trail.point_life > 0.0 => (1.0 - l.1 / trail.point_life).clamp(0.0, 1.0),
                        TrailShade::PointLife => 1.0,
                    };
                    RibbonPoint { pos: l.0.to_array(), half_width: half, u, shade }
                })
                .collect();
            let facing = if trail.crossed_sheets { Facing::Sheets(2) } else { Facing::Camera(cam.to_array()) };
            (pts, facing, 1.0)
        }
        _ => return,
    };
    let mut built = ribbon::build(&points, facing, v_scale);
    // Fewer than two points (a trail just born or fully faded): nothing to draw.
    if built.indices.is_empty() {
        let at = points.first().map(|q| q.pos).unwrap_or(head.to_array());
        built = ribbon::RibbonMesh {
            positions: vec![at; 4],
            uvs: vec![[0.0; 2]; 4],
            colors: vec![[0.0; 4]; 4],
            indices: vec![0, 2, 1, 1, 2, 3],
        };
    }
    if let Some(mut mesh) = meshes.get_mut(&r.mesh) {
        write_mesh(&mut *mesh, built);
    }
}
