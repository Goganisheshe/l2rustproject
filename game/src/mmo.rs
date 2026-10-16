//! Offline Lineage II: character creation, starting village, walking the
//! streamed world with a third-person camera.
//!
//! Controls: WASD / left stick move (camera-relative), Q/E or right stick
//! turn the camera, mouse wheel / triggers zoom, Shift walks.

use std::sync::Arc;

use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll};
use bevy::mesh::skinning::SkinnedMeshInverseBindposes;
use bevy::prelude::*;

use crate::character::{part_spec, weapon_spec, CharClass, CLASSES};
use crate::collision::CollisionWorld;
use crate::datapack::Datapack;
use crate::rpg::{spawn_hud, template_for, HeroStats};
use crate::input::PlayerInput;
use crate::l2assets::L2Library;
use crate::skinned::{spawn_model, Animated, Attachment, Model, ModelCache};
use crate::streaming::{bevy_to_ue, tile_of, ue_to_bevy, TileStreamer};

/// Run speed in meters per second (L2 base run speed ~120 UU/s).
const RUN_SPEED: f32 = 120.0 / 27.0;
const WALK_SPEED: f32 = 60.0 / 27.0;

/// Starting point: the class template's creation point from the datapack,
/// else the race's newbie village. `L2_START="x,y,z"` overrides.
/// Start position chosen by a loaded save game.
pub static START_OVERRIDE: std::sync::Mutex<Option<[f32; 3]>> = std::sync::Mutex::new(None);

fn start_location(class: &CharClass, dp: &Datapack) -> [f32; 3] {
    if let Some(p) = *START_OVERRIDE.lock().unwrap() {
        return p;
    }
    if let Some(p) = std::env::var("L2_START").ok().and_then(|s| {
        let v: Vec<f32> = s.split(',').filter_map(|x| x.trim().parse().ok()).collect();
        (v.len() == 3).then(|| [v[0], v[1], v[2]])
    }) {
        return p;
    }
    if let Some(p) = dp.classes.get(template_for(class)).and_then(|t| t.creation_points.first()) {
        return *p;
    }
    race_village(class)
}

/// The classic newbie village of each race (Unreal units).
fn race_village(class: &CharClass) -> [f32; 3] {
    let p = class.prefix;
    if p.contains("DarkElf") {
        [28295.0, 11063.0, -4224.0] // Dark Elf Village
    } else if p.contains("Elf") {
        [46045.0, 41251.0, -3440.0] // Elven Village
    } else if p.contains("Orc") || p.contains("Shaman") {
        [-56693.0, -113610.0, -690.0] // Orc Village
    } else if p.contains("Dwarf") {
        [108512.0, -174026.0, -400.0] // Dwarven Village
    } else if p.contains("Kamael") {
        [-125464.0, 37776.0, 1176.0] // Kamael Village
    } else {
        [-71338.0, 258271.0, -3104.0] // Talking Island Village
    }
}

#[derive(States, Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Phase {
    /// Login screen (lobby.rs).
    #[default]
    Login,
    /// Character select.
    Lobby,
    /// Character creation.
    Create,
    Loading,
    InWorld,
}

#[derive(Resource, Default)]
pub struct Creation {
    /// Index into `CLASSES`.
    pub index: usize,
    /// Character name.
    pub name: String,
    pub model: Option<Arc<Model>>,
    pub weapon: Vec<crate::l2assets::MeshPart>,
}

impl Creation {
    pub fn attachments(&self) -> Vec<Attachment> {
        if self.weapon.is_empty() {
            return Vec::new();
        }
        vec![Attachment { bone: "Weapon_R_Bone", parts: self.weapon.clone() }]
    }
}

#[derive(Component)]
pub struct Hero {
    pub actor: Entity,
    /// Index into `CLASSES`.
    pub class: usize,
    /// Weapon stance suffix of animation clips: `1HS`, `2HS`, `Bow`...
    pub stance: &'static str,
}

impl Hero {
    /// `<clip>_<stance>` followed by fallbacks, for `Animated::play`.
    pub fn clips(&self, clip: &str, fallback: &str) -> [String; 3] {
        [format!("{clip}_{}", self.stance), format!("{clip}_1HS"), fallback.to_string()]
    }
}

#[derive(Component)]
struct HudText;

#[derive(Component)]
struct LoadingUi;

/// Click-to-move destination, as in the original client.
#[derive(Resource, Default)]
pub struct MoveTarget(pub Option<Vec3>);

/// Ring on the ground at the click-to-move destination.
#[derive(Component)]
struct MoveMarker;

/// Where a camera ray first meets the world: building geometry or terrain.
pub fn ground_point(origin: Vec3, dir: Vec3, streamer: &TileStreamer, world: &CollisionWorld) -> Option<Vec3> {
    const MAX: f32 = 300.0;
    let solid = world.raycast(origin, dir, MAX).map(|h| origin + dir * h.t);
    let mut terrain = None;
    let mut t = 0.0;
    while t < MAX {
        let p = origin + dir * t;
        if let Some(y) = streamer.ground_y(p) {
            if p.y <= y {
                terrain = Some(p.with_y(y));
                break;
            }
        }
        t += 0.5;
    }
    match (solid, terrain) {
        (Some(a), Some(b)) => Some(if a.distance(origin) < b.distance(origin) { a } else { b }),
        (a, b) => a.or(b),
    }
}

/// Camera turn per pixel of mouse movement with the right button held.
const MOUSE_RADIANS_PER_PIXEL: f32 = 0.005;
/// Camera pitch limits: from nearly straight down to a little below the hero.
const MIN_PITCH: f32 = -1.45;
const MAX_PITCH: f32 = 0.3;

/// Third-person orbit camera.
#[derive(Resource)]
struct Orbit {
    yaw: f32,
    pitch: f32,
    distance: f32,
}

pub struct MmoPlugin;

impl Plugin for MmoPlugin {
    fn build(&self, app: &mut App) {
        app.init_state::<Phase>()
            .init_resource::<Creation>()
            .init_resource::<MoveTarget>()
            .insert_resource(Orbit { yaw: 0.0, pitch: -0.35, distance: 7.0 })
            .insert_resource(ClearColor(Color::srgb(0.55, 0.68, 0.85)))
            .add_systems(Startup, setup)
            .add_systems(OnEnter(Phase::Loading), start_loading)
            .add_systems(Update, wait_loading.run_if(in_state(Phase::Loading)))
            .add_systems(OnEnter(Phase::InWorld), enter_world)
            .add_systems(
                Update,
                (move_hero, move_marker, orbit_camera, stream_around, hud).chain().run_if(in_state(Phase::InWorld)),
            );
    }
}

fn setup(mut commands: Commands) {
    commands.spawn((
        DirectionalLight { illuminance: 9000.0, shadow_maps_enabled: true, ..default() },
        Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -0.9, 0.5, 0.0)),
    ));
    commands.insert_resource(GlobalAmbientLight { brightness: 600.0, ..default() });
    commands.spawn((
        Camera3d::default(),
        Projection::Perspective(PerspectiveProjection { far: 2500.0, ..default() }),
        Transform::from_xyz(0.0, 1.6, 4.2).looking_at(Vec3::new(0.0, 1.0, 0.0), Vec3::Y),
    ));
}

/// Loads a class's model and starting weapon into `c` (character screens).
#[allow(clippy::too_many_arguments)]
pub fn load_class_model(
    c: &mut Creation,
    lib: &mut L2Library,
    cache: &mut ModelCache,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
    bindposes: &mut Assets<SkinnedMeshInverseBindposes>,
) {
    let class = &CLASSES[c.index];
    let spec = part_spec(lib, class);
    c.model = cache.load(lib, class.package, &spec, None, meshes, materials, images, bindposes);
    c.weapon = lib.rigid_mesh("lineageweapons", &weapon_spec(class), meshes, materials, images).unwrap_or_default();
}

fn start_loading(mut commands: Commands, c: Res<Creation>, dp: Res<Datapack>, mut streamer: ResMut<TileStreamer>) {
    let start = ue_to_bevy(start_location(&CLASSES[c.index], &dp));
    streamer.request_around(start);
    commands.spawn((
        LoadingUi,
        Text::new("Loading world..."),
        TextFont { font_size: FontSize::Px(28.0), ..default() },
        Node { position_type: PositionType::Absolute, bottom: Val::Px(40.0), left: Val::Px(40.0), ..default() },
    ));
}

fn wait_loading(
    mut commands: Commands,
    c: Res<Creation>,
    dp: Res<Datapack>,
    streamer: Res<TileStreamer>,
    ui: Query<Entity, With<LoadingUi>>,
    mut next: ResMut<NextState<Phase>>,
) {
    let ue = start_location(&CLASSES[c.index], &dp);
    if streamer.is_loaded(tile_of(ue[0], ue[1])) {
        for e in &ui {
            commands.entity(e).despawn();
        }
        next.set(Phase::InWorld);
    }
}

/// Walkable height closest to a server position (whose z comes from the
/// server's geodata): a building floor or bridge near it, else the terrain.
pub fn settle(pos: Vec3, streamer: &TileStreamer, world: &CollisionWorld) -> f32 {
    let floor = world.ground(pos, 3.0, 6.0);
    let terrain = streamer.ground_y(pos).filter(|y| (*y - pos.y).abs() < 6.0);
    match (floor, terrain) {
        (Some(f), Some(t)) => {
            if (f - pos.y).abs() <= (t - pos.y).abs() {
                f
            } else {
                t
            }
        }
        (Some(f), None) => f,
        (None, Some(t)) => t,
        // Nothing near the server height: trust it unless the terrain is
        // below (a guessed start point). Terrain far above is the hill over
        // a cave (the Kamael village).
        (None, None) => match streamer.ground_y(pos) {
            Some(t) if t < pos.y => t,
            _ => pos.y,
        },
    }
}

/// Shows the destination ring while walking to a clicked point.
fn move_marker(
    mut commands: Commands,
    time: Res<Time>,
    target: Res<MoveTarget>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut marker: Query<(&mut Transform, &mut Visibility), With<MoveMarker>>,
) {
    let Ok((mut tr, mut vis)) = marker.single_mut() else {
        commands.spawn((
            MoveMarker,
            Mesh3d(meshes.add(Annulus::new(0.28, 0.4))),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: Color::srgba(1.0, 0.85, 0.3, 0.8),
                emissive: LinearRgba::rgb(1.0, 0.8, 0.2),
                unlit: true,
                alpha_mode: AlphaMode::Blend,
                ..default()
            })),
            bevy::light::NotShadowCaster,
            Transform::from_rotation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)),
            Visibility::Hidden,
        ));
        return;
    };
    match target.0 {
        Some(p) => {
            tr.translation = p + Vec3::Y * 0.05;
            let pulse = 1.0 + 0.15 * (time.elapsed_secs() * 6.0).sin();
            tr.scale = Vec3::splat(pulse);
            *vis = Visibility::Visible;
        }
        None => *vis = Visibility::Hidden,
    }
}

fn enter_world(
    iface: Option<Res<crate::l2ui::Interface>>,
    mut commands: Commands,
    c: Res<Creation>,
    dp: Res<Datapack>,
    streamer: Res<TileStreamer>,
    world: Res<CollisionWorld>,
) {
    let class = &CLASSES[c.index];
    let mut pos = ue_to_bevy(start_location(class, &dp));
    pos.y = settle(pos, &streamer, &world);
    let hero = commands.spawn((Transform::from_translation(pos), Visibility::default())).id();
    let actor = match &c.model {
        Some(model) => spawn_model(&mut commands, model, hero, "wait_1HS", &c.attachments()),
        None => hero,
    };
    let template = dp.classes.get(template_for(class)).cloned().unwrap_or_default();
    let mut stats = HeroStats::new(template);
    stats.respawn = pos;
    stats.sitting = std::env::var_os("L2_SIT").is_some();
    stats.class_key = template_for(class).to_string();
    // L2_LEVEL=<n> starts at that level (tests).
    if let Some(l) = std::env::var("L2_LEVEL").ok().and_then(|v| v.parse::<u32>().ok()) {
        stats.exp = dp.exp.get(l as usize).copied().unwrap_or(0);
        stats.level = dp.level_for(stats.exp);
        stats.sp = 1_000_000;
        stats.recalc();
        stats.hp = stats.max_hp;
        stats.mp = stats.max_mp;
    }
    // L2_MOVE_TO="dx,dz": click-to-move that many meters away (tests).
    if let Some(v) = std::env::var("L2_MOVE_TO").ok().map(|s| s.split(',').filter_map(|x| x.trim().parse::<f32>().ok()).collect::<Vec<_>>()) {
        if v.len() == 2 {
            commands.insert_resource(MoveTarget(Some(pos + Vec3::new(v[0], 0.0, v[1]))));
        }
    }
    commands.entity(hero).insert((Hero { actor, class: c.index, stance: "1HS" }, stats));
    // The original client windows (l2ui) replace the simple HUD when the
    // client's interface.xdat is available.
    if iface.is_none() {
        spawn_hud(&mut commands);
    }
    commands.spawn((
        HudText,
        if iface.is_some() && std::env::var_os("L2_COORDS").is_none() { Visibility::Hidden } else { Visibility::Inherited },
        Text::new(""),
        TextFont { font_size: FontSize::Px(18.0), ..default() },
        Node { position_type: PositionType::Absolute, top: Val::Px(10.0), left: Val::Px(10.0), ..default() },
    ));
}


#[allow(clippy::too_many_arguments)]
fn move_hero(
    time: Res<Time>,
    input: Res<PlayerInput>,
    keys: Res<ButtonInput<KeyCode>>,
    orbit: Res<Orbit>,
    streamer: Res<TileStreamer>,
    world: Res<CollisionWorld>,
    mut target: ResMut<MoveTarget>,
    hero: Single<(&Hero, &mut Transform, &mut HeroStats)>,
    mut anims: Query<&mut Animated>,
) {
    let (hero, mut tr, mut stats) = hero.into_inner();
    if stats.hp <= 0.0 {
        target.0 = None;
        stats.sitting = false;
        return;
    }
    // V sits down / stands up; moving or fighting stands up.
    if keys.just_pressed(KeyCode::KeyV) {
        stats.sitting = !stats.sitting;
        if stats.sitting {
            target.0 = None;
            stats.attacking = false;
            if let Ok(mut a) = anims.get_mut(hero.actor) {
                a.restart(&["Sit"], false);
            }
        } else if let Ok(mut a) = anims.get_mut(hero.actor) {
            a.restart(&["Stand"], false);
        }
    }
    if stats.sitting {
        if input.move_dir.length() > 0.1 || stats.attacking || target.0.is_some() {
            stats.sitting = false;
            if let Ok(mut a) = anims.get_mut(hero.actor) {
                a.restart(&["Stand"], false);
            }
        } else {
            if let Ok(mut a) = anims.get_mut(hero.actor) {
                if a.finished() || a.looping {
                    a.play(&["SitWait"], true);
                }
            }
            return;
        }
    }
    let mut d = input.move_dir;
    let walking = keys.pressed(KeyCode::ShiftLeft);
    // Camera-relative: stick up = away from the camera.
    let forward = Quat::from_rotation_y(orbit.yaw) * Vec3::NEG_Z;
    let right = Quat::from_rotation_y(orbit.yaw) * Vec3::X;
    let mut dir = (forward * d.y + right * d.x).with_y(0.0);
    if dir.length() > 0.1 || stats.attacking {
        // Keys, sticks and fights cancel a click-to-move.
        target.0 = None;
    } else if let Some(goal) = target.0 {
        let to = (goal - tr.translation).with_y(0.0);
        if to.length() < 0.2 {
            target.0 = None;
        } else {
            dir = to;
            // Arrive without overshooting the destination.
            let speed = stats.run_speed * crate::l2assets::UU_TO_M * time.delta_secs();
            d = Vec2::splat((to.length() / speed.max(1e-4)).min(1.0) / std::f32::consts::SQRT_2);
        }
    }
    let moving = dir.length() > 0.01;
    if moving {
        let speed = if walking { WALK_SPEED } else { stats.run_speed * crate::l2assets::UU_TO_M };
        let step = dir.normalize() * speed * d.length().min(1.0) * time.delta_secs();
        if target.0.is_some() && step.length() < 1e-4 {
            target.0 = None;
        }
        let moved = slide(&world, tr.translation, step);
        tr.translation += moved;
        // Blocked by a wall: give up on the destination like the client does.
        if target.0.is_some() && moved.length() < step.length() * 0.05 {
            target.0 = None;
        }
        // Lineage models face +X.
        tr.rotation = Quat::from_rotation_y((-dir.z).atan2(dir.x));
    }
    // Stand on the highest surface within a step of the feet: building
    // floors and stairs, else the terrain.
    // Terrain well above the feet is the hill over a cave (the Kamael
    // village): not something to step onto.
    let terrain = streamer.ground_y(tr.translation).filter(|t| *t <= tr.translation.y + 1.0);
    let floor = world.ground(tr.translation, 0.6, 3.0);
    let ground = match (terrain, floor) {
        (Some(t), Some(f)) => Some(t.max(f)),
        (a, b) => a.or(b),
    };
    if let Some(y) = ground {
        tr.translation.y = y;
    }
    if stats.attacking && !moving {
        return; // combat drives the animation
    }
    // Let the stand-up clip finish before idling.
    if !moving {
        if let Ok(a) = anims.get(hero.actor) {
            if !a.looping && !a.finished() {
                return;
            }
        }
    }
    if let Ok(mut anim) = anims.get_mut(hero.actor) {
        let names = if moving && walking {
            hero.clips("walk", "walk")
        } else if moving {
            hero.clips("run", "run")
        } else {
            hero.clips("wait", "Wait")
        };
        anim.play(&names.each_ref().map(String::as_str), true);
    }
}

/// Moves by `step` but stops at walls, sliding along them. Rays are cast at
/// knee and chest height so low steps don't block but walls do.
fn slide(world: &CollisionWorld, pos: Vec3, step: Vec3) -> Vec3 {
    const RADIUS: f32 = 0.35;
    let mut step = step;
    for _ in 0..2 {
        let len = step.length();
        if len < 1e-5 {
            return Vec3::ZERO;
        }
        let dir = step / len;
        let wall = [0.7, 1.4]
            .iter()
            .filter_map(|h| world.raycast(pos + Vec3::Y * *h, dir, len + RADIUS))
            .filter(|hit| hit.normal.y.abs() < 0.6)
            .min_by(|a, b| a.t.total_cmp(&b.t));
        let Some(hit) = wall else { return step };
        let n = hit.normal.with_y(0.0).normalize_or_zero();
        step -= n * step.dot(n);
    }
    step
}

#[allow(clippy::too_many_arguments)]
fn orbit_camera(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    scroll: Res<AccumulatedMouseScroll>,
    gamepads: Query<&Gamepad, With<crate::input::ActivePad>>,
    mut orbit: ResMut<Orbit>,
    world: Res<CollisionWorld>,
    hero: Single<&Transform, With<Hero>>,
    mut cam: Single<&mut Transform, (With<Camera3d>, Without<Hero>)>,
) {
    let dt = time.delta_secs();
    let mut turn = 0.0;
    let mut tilt = 0.0;
    if keys.pressed(KeyCode::KeyQ) {
        turn += 1.0;
    }
    if keys.pressed(KeyCode::KeyE) {
        turn -= 1.0;
    }
    let mut zoom = -scroll.delta.y * 0.8;
    for pad in &gamepads {
        let r = pad.right_stick();
        if r.length() > 0.15 {
            turn -= r.x;
            tilt += r.y;
        }
        zoom += (pad.get(GamepadButton::LeftTrigger2).unwrap_or(0.0) - pad.get(GamepadButton::RightTrigger2).unwrap_or(0.0)) * dt * 8.0;
    }
    // Keys and sticks turn at a rate (per second); the mouse turns by a fixed
    // angle per pixel moved, whatever the frame rate.
    let mut mouse_turn = Vec2::ZERO;
    if mouse.pressed(MouseButton::Right) {
        mouse_turn = -motion.delta * MOUSE_RADIANS_PER_PIXEL;
    }
    orbit.yaw += turn * 2.0 * dt + mouse_turn.x;
    orbit.pitch = (orbit.pitch + tilt * 1.5 * dt + mouse_turn.y).clamp(MIN_PITCH, MAX_PITCH);
    orbit.distance = (orbit.distance + zoom).clamp(2.0, 30.0);
    let focus = hero.translation + Vec3::Y * 1.5;
    let back = Quat::from_euler(EulerRot::YXZ, orbit.yaw, orbit.pitch, 0.0) * Vec3::Z;
    // Pull the camera in front of walls between it and the hero.
    let dist = world.raycast(focus, back, orbit.distance).map_or(orbit.distance, |h| (h.t - 0.3).max(0.8));
    cam.translation = focus + back * dist;
    cam.look_at(focus, Vec3::Y);
}

fn stream_around(mut streamer: ResMut<TileStreamer>, hero: Single<&Transform, With<Hero>>) {
    streamer.request_around(hero.translation);
}

fn hud(
    dp: Res<Datapack>,
    hero: Single<(&Transform, &crate::rpg::HeroStats), With<Hero>>,
    mut text: Single<&mut Text, With<HudText>>,
) {
    let (tr, stats) = hero.into_inner();
    let ue = bevy_to_ue(tr.translation);
    let (tx, ty) = tile_of(ue[0], ue[1]);
    text.0 = format!(
        "{}   x {:.0}  y {:.0}  z {:.0}   tile {tx}_{ty}",
        dp.class_name(stats.template.class_id),
        ue[0],
        ue[1],
        ue[2]
    );
}
