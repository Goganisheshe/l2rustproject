//! Offline Lineage II: character creation, starting village, walking the
//! streamed world with a third-person camera.
//!
//! Controls: WASD / left stick move (camera-relative), Q/E or right stick
//! turn the camera, mouse wheel / triggers zoom, Shift walks.

use std::f32::consts::FRAC_PI_2;
use std::sync::Arc;

use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll};
use bevy::mesh::skinning::SkinnedMeshInverseBindposes;
use bevy::prelude::*;

use crate::character::{find_class, part_spec, weapon_spec, CharClass, CLASSES};
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

/// Starting point. The datapack's creation points are the remade Talking
/// Island, but this client ships the classic island geometry (16_25.unr is
/// identical to 16_25_Classic.unr), so everyone starts in a village whose
/// geometry matches the datapack spawns.
fn start_location(class: &CharClass, _dp: &Datapack) -> [f32; 3] {
    // L2_START="x,y,z" overrides (Unreal units), e.g. a hunting ground for tests.
    if let Some(p) = std::env::var("L2_START").ok().and_then(|s| {
        let v: Vec<f32> = s.split(',').filter_map(|x| x.trim().parse().ok()).collect();
        (v.len() == 3).then(|| [v[0], v[1], v[2]])
    }) {
        return p;
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
        [-80826.0, 149775.0, -3043.0] // Gludin Village
    }
}

#[derive(States, Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Phase {
    #[default]
    Create,
    Loading,
    InWorld,
}

#[derive(Resource, Default)]
struct Creation {
    index: usize,
    shown: Option<usize>,
    model: Option<Arc<Model>>,
    weapon: Vec<crate::l2assets::MeshPart>,
}

impl Creation {
    fn attachments(&self) -> Vec<Attachment> {
        if self.weapon.is_empty() {
            return Vec::new();
        }
        vec![Attachment { bone: "Weapon_R_Bone", parts: self.weapon.clone() }]
    }
}

#[derive(Component)]
struct Preview;

#[derive(Component)]
struct CreateUi;

#[derive(Component)]
struct ClassLine(usize);

#[derive(Component)]
pub struct Hero {
    pub actor: Entity,
}

#[derive(Component)]
struct HudText;

#[derive(Component)]
struct LoadingUi;

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
            .insert_resource(Orbit { yaw: 0.0, pitch: -0.35, distance: 7.0 })
            .insert_resource(ClearColor(Color::srgb(0.55, 0.68, 0.85)))
            .add_systems(Startup, setup)
            .add_systems(OnEnter(Phase::Create), open_create)
            .add_systems(Update, create_menu.run_if(in_state(Phase::Create)))
            .add_systems(OnExit(Phase::Create), close_create)
            .add_systems(OnEnter(Phase::Loading), start_loading)
            .add_systems(Update, wait_loading.run_if(in_state(Phase::Loading)))
            .add_systems(OnEnter(Phase::InWorld), enter_world)
            .add_systems(Update, (move_hero, orbit_camera, stream_around, hud).chain().run_if(in_state(Phase::InWorld)));
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

fn open_create(mut commands: Commands, mut c: ResMut<Creation>) {
    if let Some(i) = std::env::var("L2_CLASS").ok().and_then(|k| find_class(&k)) {
        c.index = i;
    }
    c.shown = None;
    commands
        .spawn((
            CreateUi,
            Node {
                position_type: PositionType::Absolute,
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::Center,
                padding: UiRect::left(Val::Px(48.0)),
                row_gap: Val::Px(4.0),
                ..default()
            },
        ))
        .with_children(|p| {
            p.spawn((Text::new("Create character"), TextFont { font_size: FontSize::Px(32.0), ..default() }));
            for (i, class) in CLASSES.iter().enumerate() {
                p.spawn((ClassLine(i), Text::new(class.name), TextFont { font_size: FontSize::Px(20.0), ..default() }));
            }
            p.spawn((Text::new("Enter / A: start"), TextFont { font_size: FontSize::Px(16.0), ..default() }));
        });
}

#[allow(clippy::too_many_arguments)]
fn create_menu(
    mut commands: Commands,
    input: Res<PlayerInput>,
    mut c: ResMut<Creation>,
    mut next: ResMut<NextState<Phase>>,
    mut lines: Query<(&ClassLine, &mut TextColor)>,
    previews: Query<Entity, With<Preview>>,
    mut lib: ResMut<L2Library>,
    mut cache: ResMut<ModelCache>,
    mut assets: (
        ResMut<Assets<Mesh>>,
        ResMut<Assets<StandardMaterial>>,
        ResMut<Assets<Image>>,
        ResMut<Assets<SkinnedMeshInverseBindposes>>,
    ),
) {
    let n = CLASSES.len() as i32;
    if input.nav != 0 {
        c.index = (c.index as i32 + input.nav).rem_euclid(n) as usize;
    }
    for (line, mut color) in &mut lines {
        color.0 = if line.0 == c.index { Color::srgb(1.0, 0.85, 0.3) } else { Color::WHITE };
    }
    if c.shown != Some(c.index) {
        c.shown = Some(c.index);
        for e in &previews {
            commands.entity(e).despawn();
        }
        let class = &CLASSES[c.index];
        let spec = part_spec(&mut lib, class);
        let (meshes, materials, images, bindposes) = &mut assets;
        c.model = cache.load(&mut lib, class.package, &spec, None, meshes, materials, images, bindposes);
        c.weapon = lib.rigid_mesh("lineageweapons", &weapon_spec(class), meshes, materials, images).unwrap_or_default();
        let root = commands
            .spawn((Preview, Transform::from_xyz(1.2, 0.0, 0.0).with_rotation(Quat::from_rotation_y(-FRAC_PI_2)), Visibility::default()))
            .id();
        if let Some(model) = &c.model {
            spawn_model(&mut commands, model, root, "wait_1HS", &c.attachments());
        }
    }
    if input.confirm || std::env::var_os("L2_CLASS").is_some() {
        next.set(Phase::Loading);
    }
}

fn close_create(mut commands: Commands, ui: Query<Entity, Or<(With<CreateUi>, With<Preview>)>>) {
    for e in &ui {
        commands.entity(e).despawn();
    }
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
        (None, None) => streamer.ground_y(pos).unwrap_or(pos.y),
    }
}

fn enter_world(
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
    let template = dp.classes.get(template_for(class.prefix)).cloned().unwrap_or_default();
    commands.entity(hero).insert((Hero { actor }, HeroStats::new(template)));
    spawn_hud(&mut commands);
    commands.spawn((
        HudText,
        Text::new(""),
        TextFont { font_size: FontSize::Px(18.0), ..default() },
        Node { position_type: PositionType::Absolute, top: Val::Px(10.0), left: Val::Px(10.0), ..default() },
    ));
}


fn move_hero(
    time: Res<Time>,
    input: Res<PlayerInput>,
    keys: Res<ButtonInput<KeyCode>>,
    orbit: Res<Orbit>,
    streamer: Res<TileStreamer>,
    world: Res<CollisionWorld>,
    hero: Single<(&Hero, &mut Transform, &HeroStats)>,
    mut anims: Query<&mut Animated>,
) {
    let (hero, mut tr, stats) = hero.into_inner();
    if stats.hp <= 0.0 {
        return;
    }
    let d = input.move_dir;
    let walking = keys.pressed(KeyCode::ShiftLeft);
    // Camera-relative: stick up = away from the camera.
    let forward = Quat::from_rotation_y(orbit.yaw) * Vec3::NEG_Z;
    let right = Quat::from_rotation_y(orbit.yaw) * Vec3::X;
    let dir = (forward * d.y + right * d.x).with_y(0.0);
    let moving = dir.length() > 0.1;
    if moving {
        let speed = if walking { WALK_SPEED } else { RUN_SPEED };
        let step = dir.normalize() * speed * d.length().min(1.0) * time.delta_secs();
        let moved = slide(&world, tr.translation, step);
        tr.translation += moved;
        // Lineage models face +X.
        tr.rotation = Quat::from_rotation_y((-dir.z).atan2(dir.x));
    }
    // Stand on the highest surface within a step of the feet: building
    // floors and stairs, else the terrain.
    let terrain = streamer.ground_y(tr.translation);
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
    if let Ok(mut anim) = anims.get_mut(hero.actor) {
        if moving && walking {
            anim.play(&["walk_1HS", "walk"], true);
        } else if moving {
            anim.play(&["run_1HS", "run"], true);
        } else {
            anim.play(&["wait_1HS", "Wait"], true);
        }
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
    gamepads: Query<&Gamepad>,
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
    if mouse.pressed(MouseButton::Right) {
        turn -= motion.delta.x * 0.15;
        tilt -= motion.delta.y * 0.15;
    }
    orbit.yaw += turn * 2.0 * dt;
    orbit.pitch = (orbit.pitch + tilt * 1.5 * dt).clamp(-1.3, 0.3);
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

fn hud(c: Res<Creation>, hero: Single<&Transform, With<Hero>>, mut text: Single<&mut Text, With<HudText>>) {
    let ue = bevy_to_ue(hero.translation);
    let (tx, ty) = tile_of(ue[0], ue[1]);
    text.0 = format!(
        "{}   x {:.0}  y {:.0}  z {:.0}   tile {tx}_{ty}",
        CLASSES[c.index].name, ue[0], ue[1], ue[2]
    );
}
