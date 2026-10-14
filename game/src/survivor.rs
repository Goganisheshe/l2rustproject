//! Vampire Survivors style loop: move, auto-attack, collect XP, pick upgrades,
//! survive escalating waves.

use std::f32::consts::{FRAC_PI_2, TAU};

use bevy::math::Affine2;
use bevy::prelude::*;
use rand::Rng;

use crate::input::PlayerInput;
use std::sync::Arc;

use bevy::mesh::skinning::SkinnedMeshInverseBindposes;

use crate::character::{find_class, part_spec, CLASSES};
use crate::l2assets::{L2Library, L2Textures};
use crate::character::weapon_spec;
use crate::effects::{spawn_effect, EffectAsset, EffectCache};

/// Skill effects from the client: (cast for fighters, cast for mystics,
/// projectile flight, impact).
const FX_CAST_FIGHTER: &str = "soul_N_stick";
const FX_CAST_MYSTIC: &str = "el_wind_strike_ca";
const FX_FLIGHT: &str = "el_wind_strike_fl";
const FX_HIT: &str = "el_wind_strike_ta";

#[derive(Resource, Default)]
struct Fx {
    cast_fighter: Option<Arc<EffectAsset>>,
    cast_mystic: Option<Arc<EffectAsset>>,
    flight: Option<Arc<EffectAsset>>,
    hit: Option<Arc<EffectAsset>>,
}
use crate::skinned::{spawn_model, Animated, Attachment, Model, ModelCache};


/// Enemy roster: (package, mesh, hp multiplier, speed, unlock minute).
const ENEMY_KINDS: &[(&str, &str, f32, f32, f32)] = &[
    ("lineagemonsters", "gremlin_m00", 0.7, 3.0, 0.0),
    ("lineagemonsters", "Fox_m00", 0.5, 3.8, 0.0),
    ("lineagemonsters", "hobgoblin_m00", 1.3, 2.6, 0.5),
    ("lineagemonsters", "orc_m00", 2.0, 2.4, 1.0),
    ("lineagemonsters", "orc_fighter_m00", 3.0, 2.6, 2.0),
];

#[derive(Resource, Default)]
struct Roster {
    player: Option<Arc<Model>>,
    /// Weapon geometry for the chosen class, hung on `Weapon_R_Bone`.
    weapon: Vec<crate::l2assets::MeshPart>,
    enemies: Vec<(Arc<Model>, f32, f32, f32)>,
}

impl Roster {
    fn attachments(&self) -> Vec<Attachment> {
        if self.weapon.is_empty() {
            return Vec::new();
        }
        vec![Attachment { bone: "Weapon_R_Bone", parts: self.weapon.clone() }]
    }
}

/// Links a gameplay entity to its animated model instance.
#[derive(Component)]
struct ModelLink(Entity);

/// Enemy playing its death animation before being removed.
#[derive(Component)]
struct Dying(f32);

/// Scenery scattered around the arena, straight from the client's static
/// meshes: (package, mesh, count, min distance from start, max distance, scale).
const SCENERY: &[(&str, &str, u32, f32, f32, f32)] = &[
    ("elmo_fielddeco_s", "Rune_grass001", 120, 2.0, 120.0, 1.0),
    ("elmo_fielddeco_s", "Rune_grass002", 120, 2.0, 120.0, 1.0),
    ("elmo_fielddeco_s", "godard_V_gr0001", 80, 2.0, 120.0, 1.0),
    ("aden_fairy_valley_s", "fairy_rock01", 25, 8.0, 120.0, 0.25),
    ("aden_fairy_valley_s", "fairy_rock03", 25, 8.0, 120.0, 0.25),
    ("aden_tree_s", "Laketree01_A", 10, 30.0, 150.0, 1.0),
    ("aden_tree_s", "aden_tree1A", 10, 30.0, 150.0, 1.0),
    ("aden_tree_s", "orenGtree1_A", 10, 30.0, 150.0, 1.0),
];

const ARENA_TILE: f32 = 8.0;
const SPAWN_RADIUS: f32 = 22.0;
const DESPAWN_RADIUS: f32 = 40.0;
const PLAYER_RADIUS: f32 = 0.5;
const ENEMY_RADIUS: f32 = 0.45;
const GEM_PICKUP_RADIUS: f32 = 0.6;

#[derive(States, Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum GameState {
    /// Race / class selection before a run.
    #[default]
    Select,
    Playing,
    Paused,
    LevelUp,
    GameOver,
}

#[derive(Component)]
struct Player;

#[derive(Component)]
struct Enemy {
    hp: f32,
    speed: f32,
    damage: f32,
}

#[derive(Component)]
struct Projectile {
    velocity: Vec3,
    damage: f32,
    pierce: u32,
    life: f32,
}

#[derive(Component)]
struct Gem(u32);

#[derive(Component)]
struct Aura;

#[derive(Component)]
struct Hud;

#[derive(Component)]
struct Menu;

/// Everything spawned for one run; despawned on restart.
#[derive(Component)]
struct RunEntity;

#[derive(Resource)]
struct Run {
    hp: f32,
    max_hp: f32,
    speed: f32,
    level: u32,
    xp: u32,
    xp_next: u32,
    kills: u32,
    time: f32,
    magnet: f32,
    bolt_damage: f32,
    bolt_cooldown: f32,
    bolt_count: u32,
    bolt_pierce: u32,
    bolt_timer: f32,
    /// Set when a volley fires; the player swings toward it if standing still.
    swing: Option<Vec3>,
    aura_radius: f32,
    aura_dps: f32,
    spawn_timer: f32,
    contact_cooldown: f32,
}

impl Default for Run {
    fn default() -> Self {
        Self {
            hp: 100.0,
            max_hp: 100.0,
            speed: 5.0,
            level: 1,
            xp: 0,
            xp_next: 5,
            kills: 0,
            time: 0.0,
            magnet: 2.5,
            bolt_damage: 10.0,
            bolt_cooldown: 0.9,
            bolt_count: 1,
            bolt_pierce: 0,
            bolt_timer: 0.0,
            swing: None,
            aura_radius: 0.0,
            aura_dps: 0.0,
            spawn_timer: 0.0,
            contact_cooldown: 0.0,
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum Upgrade {
    Damage,
    FireRate,
    Multishot,
    Pierce,
    Aura,
    Speed,
    Magnet,
    Vitality,
}

impl Upgrade {
    const ALL: [Upgrade; 8] = [
        Upgrade::Damage,
        Upgrade::FireRate,
        Upgrade::Multishot,
        Upgrade::Pierce,
        Upgrade::Aura,
        Upgrade::Speed,
        Upgrade::Magnet,
        Upgrade::Vitality,
    ];

    fn label(self) -> &'static str {
        match self {
            Upgrade::Damage => "Wind Strike: +30% damage",
            Upgrade::FireRate => "Haste: -15% cooldown",
            Upgrade::Multishot => "Soulshot: +1 bolt",
            Upgrade::Pierce => "Piercing: bolts pass +1 enemy",
            Upgrade::Aura => "Aura of Hate: damage nearby",
            Upgrade::Speed => "Wind Walk: +12% move speed",
            Upgrade::Magnet => "Greed: +40% pickup radius",
            Upgrade::Vitality => "Vitality: +20 max HP, heal",
        }
    }

    fn apply(self, run: &mut Run) {
        match self {
            Upgrade::Damage => run.bolt_damage *= 1.3,
            Upgrade::FireRate => run.bolt_cooldown *= 0.85,
            Upgrade::Multishot => run.bolt_count += 1,
            Upgrade::Pierce => run.bolt_pierce += 1,
            Upgrade::Aura => {
                run.aura_radius = if run.aura_radius == 0.0 { 2.0 } else { run.aura_radius * 1.2 };
                run.aura_dps += 12.0;
            }
            Upgrade::Speed => run.speed *= 1.12,
            Upgrade::Magnet => run.magnet *= 1.4,
            Upgrade::Vitality => {
                run.max_hp += 20.0;
                run.hp = run.max_hp;
            }
        }
    }
}

#[derive(Resource, Default)]
struct LevelUpChoice {
    options: Vec<Upgrade>,
    selected: usize,
}

#[derive(Resource)]
struct Palette {
    enemy_mesh: Handle<Mesh>,
    enemy_mat: Handle<StandardMaterial>,
    bolt_mesh: Handle<Mesh>,
    bolt_mat: Handle<StandardMaterial>,
    gem_mesh: Handle<Mesh>,
    gem_mat: Handle<StandardMaterial>,
}

pub struct SurvivorPlugin;

impl Plugin for SurvivorPlugin {
    fn build(&self, app: &mut App) {
        app.init_state::<GameState>()
            .init_resource::<Run>()
            .init_resource::<LevelUpChoice>()
            .init_resource::<SelectState>()
            .add_systems(Startup, setup_world)
            .add_systems(OnEnter(GameState::Select), open_select)
            .add_systems(Update, select_menu.run_if(in_state(GameState::Select)))
            .add_systems(OnExit(GameState::Select), (close_menu, clear_preview, start_run).chain())
            .add_systems(
                Update,
                (
                    move_player,
                    animate_player,
                    spawn_enemies,
                    move_enemies,
                    fire_bolts,
                    move_bolts,
                    aura_damage,
                    collect_gems,
                    enemy_contact,
                    tick_run,
                    dying,
                )
                    .chain()
                    .run_if(in_state(GameState::Playing)),
            )
            .add_systems(Update, (follow_camera, update_hud, pause_toggle))
            .add_systems(OnEnter(GameState::LevelUp), open_level_up)
            .add_systems(Update, level_up_menu.run_if(in_state(GameState::LevelUp)))
            .add_systems(OnExit(GameState::LevelUp), close_menu)
            .add_systems(OnEnter(GameState::GameOver), open_game_over)
            .add_systems(Update, game_over_menu.run_if(in_state(GameState::GameOver)))
            .add_systems(OnExit(GameState::GameOver), (close_menu, start_run).chain());
    }
}

fn setup_world(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    textures: Res<L2Textures>,
    mut lib: ResMut<L2Library>,
    mut images: ResMut<Assets<Image>>,
    mut cache: ResMut<ModelCache>,
    mut bindposes: ResMut<Assets<SkinnedMeshInverseBindposes>>,
    mut fx_cache: ResMut<EffectCache>,
) {
    let mut load_fx = |name: &str| fx_cache.load(&mut lib, name, &mut meshes, &mut materials, &mut images);
    let fx = Fx {
        cast_fighter: load_fx(FX_CAST_FIGHTER),
        cast_mystic: load_fx(FX_CAST_MYSTIC),
        flight: load_fx(FX_FLIGHT),
        hit: load_fx(FX_HIT),
    };
    commands.insert_resource(fx);

    // Characters from the client's skeletal meshes.
    let mut roster = Roster::default();
    for &(p, m, hp, speed, unlock) in ENEMY_KINDS {
        if let Some(model) = cache.load(&mut lib, p, m, None, &mut meshes, &mut materials, &mut images, &mut bindposes) {
            roster.enemies.push((model, hp, speed, unlock));
        }
    }
    commands.insert_resource(roster);

    // Huge tiled ground using a texture straight from the original client.
    let ground_size = 400.0;
    let ground_mat = match &textures.ground {
        Some(img) => StandardMaterial {
            base_color_texture: Some(img.clone()),
            uv_transform: Affine2::from_scale(Vec2::splat(ground_size / ARENA_TILE)),
            perceptual_roughness: 1.0,
            ..default()
        },
        None => StandardMaterial::from(Color::srgb(0.25, 0.35, 0.2)),
    };
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(ground_size, ground_size))),
        MeshMaterial3d(materials.add(ground_mat)),
    ));

    // L2_VIEW=<package>:<mesh> puts one static mesh right next to the start point.
    let mut placements: Vec<((String, String), Vec3, f32, f32)> = Vec::new();
    if let Ok(spec) = std::env::var("L2_VIEW") {
        if let Some((p, o)) = spec.split_once(':') {
            placements.push(((p.to_string(), o.to_string()), Vec3::new(0.0, 0.0, -6.0), 0.0, 1.0));
        }
    }
    let mut rng = rand::rng();
    for &(p, o, count, near, far, scale) in SCENERY {
        for _ in 0..count {
            let a = rng.random_range(0.0..TAU);
            let d = rng.random_range(near..far);
            let pos = Vec3::new(a.cos() * d, 0.0, a.sin() * d);
            placements.push(((p.to_string(), o.to_string()), pos, rng.random_range(0.0..TAU), scale));
        }
    }
    let mut cache: std::collections::HashMap<(String, String), Vec<crate::l2assets::MeshPart>> = Default::default();
    for (key, pos, yaw, scale) in placements {
        let parts = cache
            .entry(key.clone())
            .or_insert_with(|| lib.static_mesh(&key.0, &key.1, &mut meshes, &mut materials, &mut images).unwrap_or_default());
        if parts.is_empty() {
            continue;
        }
        commands
            .spawn((
                Transform::from_translation(pos).with_rotation(Quat::from_rotation_y(yaw)).with_scale(Vec3::splat(scale)),
                Visibility::default(),
            ))
            .with_children(|c| {
                for part in parts.iter() {
                    c.spawn((Mesh3d(part.mesh.clone()), MeshMaterial3d(part.material.clone())));
                }
            });
    }

    commands.spawn((
        DirectionalLight { illuminance: 9000.0, shadow_maps_enabled: true, ..default() },
        Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -1.0, 0.4, 0.0)),
    ));
    commands.spawn((Camera3d::default(), Transform::from_xyz(0.0, 18.0, 12.0).looking_at(Vec3::ZERO, Vec3::Y)));

    commands.insert_resource(Palette {
        enemy_mesh: meshes.add(Capsule3d::new(ENEMY_RADIUS, 0.8)),
        enemy_mat: materials.add(Color::srgb(0.6, 0.15, 0.15)),
        bolt_mesh: meshes.add(Sphere::new(0.1)),
        bolt_mat: materials.add(StandardMaterial {
            base_color: Color::srgb(0.6, 0.85, 1.0),
            emissive: LinearRgba::rgb(2.0, 4.0, 8.0),
            ..default()
        }),
        gem_mesh: meshes.add(Cuboid::new(0.25, 0.25, 0.25)),
        gem_mat: materials.add(StandardMaterial {
            base_color: Color::srgb(0.3, 1.0, 0.5),
            emissive: LinearRgba::rgb(0.5, 3.0, 1.0),
            ..default()
        }),
    });

    commands.spawn((
        Hud,
        Text::new(""),
        TextFont { font_size: FontSize::Px(22.0), ..default() },
        Node { position_type: PositionType::Absolute, top: Val::Px(12.0), left: Val::Px(12.0), ..default() },
    ));
}

fn start_run(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    old: Query<Entity, With<RunEntity>>,
    roster: Res<Roster>,
) {
    for e in &old {
        commands.entity(e).despawn();
    }
    commands.insert_resource(Run::default());
    let player = commands.spawn((Player, RunEntity, Transform::default(), Visibility::default())).id();
    match &roster.player {
        Some(model) => {
            let actor = spawn_model(&mut commands, model, player, "wait_1HS", &roster.attachments());
            commands.entity(player).insert(ModelLink(actor));
        }
        None => {
            commands.spawn((
                Mesh3d(meshes.add(Capsule3d::new(PLAYER_RADIUS, 1.0))),
                MeshMaterial3d(materials.add(Color::srgb(0.85, 0.8, 0.6))),
                Transform::from_xyz(0.0, 1.0, 0.0),
                ChildOf(player),
            ));
        }
    }
    commands
        .entity(player)
        .with_child((
            Aura,
            Mesh3d(meshes.add(Annulus::new(0.95, 1.0))),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: Color::srgba(1.0, 0.3, 0.2, 0.5),
                emissive: LinearRgba::rgb(3.0, 0.6, 0.3),
                alpha_mode: AlphaMode::Blend,
                unlit: true,
                ..default()
            })),
            Transform::from_xyz(0.0, 0.05, 0.0).with_rotation(Quat::from_rotation_x(-FRAC_PI_2)),
            Visibility::Hidden,
        ));
}

fn move_player(
    time: Res<Time>,
    input: Res<PlayerInput>,
    run: Res<Run>,
    mut player: Single<&mut Transform, With<Player>>,
) {
    // Stick/keyboard Y maps to "into the screen" (-Z).
    let d = input.move_dir;
    let dir = Vec3::new(d.x, 0.0, -d.y);
    player.translation += dir * run.speed * time.delta_secs();
    face(&mut player, dir);
}

/// Turns a ground entity toward `dir`. Lineage models face +X.
fn face(t: &mut Transform, dir: Vec3) {
    if dir.length_squared() > 1e-4 {
        t.rotation = Quat::from_rotation_y((-dir.z).atan2(dir.x));
    }
}

/// Picks run / idle clips for the player from movement input.
/// Run while moving; when standing, swing the weapon on each volley and
/// fall back to the combat stance once the swing ends.
fn animate_player(
    input: Res<PlayerInput>,
    mut run: ResMut<Run>,
    player: Single<(&ModelLink, &mut Transform), With<Player>>,
    mut anims: Query<&mut Animated>,
) {
    let swing = run.swing.take();
    let (link, mut transform) = player.into_inner();
    let Ok(mut anim) = anims.get_mut(link.0) else { return };
    if input.move_dir.length() > 0.1 {
        anim.play(&["Run_1HS", "run"], true);
    } else if let Some(aim) = swing {
        face(&mut transform, aim);
        anim.restart(&["Atk01_1HS", "atk01"], false);
    } else if anim.looping || anim.finished() {
        anim.play(&["AtkWait_1HS", "wait_1HS", "Wait"], true);
    }
}

/// Plays the death clip, then removes the corpse.
fn dying(
    mut commands: Commands,
    time: Res<Time>,
    mut corpses: Query<(Entity, &mut Dying, Option<&ModelLink>)>,
    mut anims: Query<&mut Animated>,
) {
    for (e, mut d, link) in &mut corpses {
        match link.and_then(|l| anims.get_mut(l.0).ok()) {
            Some(mut anim) => anim.play(&["death"], false),
            None => d.0 = d.0.min(0.0),
        }
        d.0 -= time.delta_secs();
        if d.0 <= 0.0 {
            commands.entity(e).despawn();
        }
    }
}

fn spawn_enemies(
    mut commands: Commands,
    time: Res<Time>,
    mut run: ResMut<Run>,
    palette: Res<Palette>,
    roster: Res<Roster>,
    player: Single<&Transform, With<Player>>,
) {
    run.spawn_timer -= time.delta_secs();
    if run.spawn_timer > 0.0 {
        return;
    }
    // Waves get denser and tougher every minute.
    let minutes = run.time / 60.0;
    run.spawn_timer = (0.8 / (1.0 + minutes * 1.5)).max(0.08);
    let batch = 1 + (minutes * 2.0) as u32;
    let mut rng = rand::rng();
    let unlocked: Vec<_> = roster.enemies.iter().filter(|e| e.3 <= minutes).collect();
    for _ in 0..batch {
        let a = rng.random_range(0.0..TAU);
        let pos = player.translation + Vec3::new(a.cos(), 0.0, a.sin()) * SPAWN_RADIUS;
        let kind = (!unlocked.is_empty()).then(|| unlocked[rng.random_range(0..unlocked.len())]);
        let (hp_mul, speed) = kind.map(|k| (k.1, k.2)).unwrap_or((1.0, rng.random_range(2.0..3.2)));
        let enemy = commands
            .spawn((
                RunEntity,
                Enemy { hp: 15.0 * hp_mul * (1.0 + minutes), speed: speed * rng.random_range(0.9..1.1), damage: 8.0 },
                Transform::from_xyz(pos.x, 0.0, pos.z),
                Visibility::default(),
            ))
            .id();
        match kind {
            Some((model, ..)) => {
                let actor = spawn_model(&mut commands, model, enemy, "run", &[]);
                commands.entity(enemy).insert(ModelLink(actor));
            }
            None => {
                commands.spawn((
                    Mesh3d(palette.enemy_mesh.clone()),
                    MeshMaterial3d(palette.enemy_mat.clone()),
                    Transform::from_xyz(0.0, 0.9, 0.0),
                    ChildOf(enemy),
                ));
            }
        }
    }
}

fn move_enemies(
    mut commands: Commands,
    time: Res<Time>,
    player: Single<&Transform, (With<Player>, Without<Enemy>)>,
    mut enemies: Query<(Entity, &Enemy, &mut Transform)>,
) {
    let target = player.translation.with_y(0.0);
    for (e, enemy, mut t) in &mut enemies {
        let to = target - t.translation;
        if to.length() > DESPAWN_RADIUS {
            commands.entity(e).despawn();
            continue;
        }
        let dir = to.normalize_or_zero();
        t.translation += dir * enemy.speed * time.delta_secs();
        face(&mut t, dir);
    }
}

fn fire_bolts(
    mut commands: Commands,
    time: Res<Time>,
    mut run: ResMut<Run>,
    palette: Res<Palette>,
    fx: Res<Fx>,
    select: Res<SelectState>,
    player: Single<(Entity, &Transform), With<Player>>,
    enemies: Query<&Transform, With<Enemy>>,
) {
    let (player_entity, player) = player.into_inner();
    run.bolt_timer -= time.delta_secs();
    if run.bolt_timer > 0.0 {
        return;
    }
    let origin = player.translation;
    let Some(nearest) = enemies
        .iter()
        .min_by(|a, b| a.translation.distance_squared(origin).total_cmp(&b.translation.distance_squared(origin)))
    else {
        return;
    };
    run.bolt_timer = run.bolt_cooldown;
    let aim = (nearest.translation - origin).with_y(0.0).normalize_or_zero();
    run.swing = Some(aim);
    let mystic = CLASSES.get(select.index).is_some_and(|c| c.prefix.contains("Magic") || c.prefix.contains("Shaman"));
    let cast = if mystic { &fx.cast_mystic } else { &fx.cast_fighter };
    if let Some(cast) = cast {
        spawn_effect(&mut commands, cast, Transform::default(), Some(player_entity));
    }
    let n = run.bolt_count;
    for i in 0..n {
        let spread = (i as f32 - (n - 1) as f32 / 2.0) * 0.18;
        let dir = Quat::from_rotation_y(spread) * aim;
        let mut transform = Transform::from_translation(origin + Vec3::Y * 1.0);
        face(&mut transform, dir);
        let bolt = commands
            .spawn((
                RunEntity,
                Projectile { velocity: dir * 14.0, damage: run.bolt_damage, pierce: run.bolt_pierce, life: 1.6 },
                transform,
                Visibility::default(),
            ))
            .id();
        // A small glowing core keeps bolts readable over bright ground; the
        // client's Wind Strike trail flies around it.
        commands.entity(bolt).insert((Mesh3d(palette.bolt_mesh.clone()), MeshMaterial3d(palette.bolt_mat.clone())));
        if let Some(flight) = &fx.flight {
            spawn_effect(&mut commands, flight, transform, Some(bolt));
        }
    }
}

fn move_bolts(
    mut commands: Commands,
    time: Res<Time>,
    mut run: ResMut<Run>,
    palette: Res<Palette>,
    fx: Res<Fx>,
    mut bolts: Query<(Entity, &mut Projectile, &mut Transform), Without<Enemy>>,
    mut enemies: Query<(Entity, &mut Enemy, &Transform)>,
) {
    let dt = time.delta_secs();
    for (b, mut bolt, mut t) in &mut bolts {
        t.translation += bolt.velocity * dt;
        bolt.life -= dt;
        let mut gone = bolt.life <= 0.0;
        if !gone {
            for (e, mut enemy, et) in &mut enemies {
                if enemy.hp <= 0.0 || et.translation.with_y(0.0).distance(t.translation.with_y(0.0)) > ENEMY_RADIUS + 0.2 {
                    continue;
                }
                enemy.hp -= bolt.damage;
                if let Some(hit) = &fx.hit {
                    let mut at = Transform::from_translation(et.translation + Vec3::Y * 0.9);
                    face(&mut at, -bolt.velocity);
                    spawn_effect(&mut commands, hit, at, None);
                }
                if enemy.hp <= 0.0 {
                    kill(&mut commands, &mut run, &palette, e, et.translation);
                }
                if bolt.pierce == 0 {
                    gone = true;
                    break;
                }
                bolt.pierce -= 1;
            }
        }
        if gone {
            commands.entity(b).despawn();
        }
    }
}

fn aura_damage(
    mut commands: Commands,
    time: Res<Time>,
    mut run: ResMut<Run>,
    palette: Res<Palette>,
    player: Single<&Transform, With<Player>>,
    mut aura: Single<(&mut Transform, &mut Visibility), (With<Aura>, Without<Player>, Without<Enemy>)>,
    mut enemies: Query<(Entity, &mut Enemy, &Transform), Without<Player>>,
) {
    let (ref mut at, ref mut vis) = *aura;
    if run.aura_radius <= 0.0 {
        **vis = Visibility::Hidden;
        return;
    }
    **vis = Visibility::Inherited;
    at.scale = Vec3::splat(run.aura_radius);
    let center = player.translation.with_y(0.0);
    let dmg = run.aura_dps * time.delta_secs();
    for (e, mut enemy, et) in &mut enemies {
        if enemy.hp > 0.0 && et.translation.with_y(0.0).distance(center) < run.aura_radius + ENEMY_RADIUS {
            enemy.hp -= dmg;
            if enemy.hp <= 0.0 {
                kill(&mut commands, &mut run, &palette, e, et.translation);
            }
        }
    }
}

fn kill(commands: &mut Commands, run: &mut Run, palette: &Palette, e: Entity, at: Vec3) {
    commands.entity(e).remove::<Enemy>().insert(Dying(1.6));
    run.kills += 1;
    commands.spawn((
        RunEntity,
        Gem(1),
        Mesh3d(palette.gem_mesh.clone()),
        MeshMaterial3d(palette.gem_mat.clone()),
        Transform::from_xyz(at.x, 0.3, at.z).with_rotation(Quat::from_rotation_y(0.7)),
    ));
}

fn collect_gems(
    mut commands: Commands,
    time: Res<Time>,
    mut run: ResMut<Run>,
    mut next: ResMut<NextState<GameState>>,
    player: Single<&Transform, (With<Player>, Without<Gem>)>,
    mut gems: Query<(Entity, &Gem, &mut Transform)>,
) {
    let p = player.translation.with_y(0.3);
    for (e, gem, mut t) in &mut gems {
        let d = p.distance(t.translation);
        if d < GEM_PICKUP_RADIUS {
            commands.entity(e).despawn();
            run.xp += gem.0;
        } else if d < run.magnet {
            let step = (p - t.translation).normalize() * 12.0 * time.delta_secs();
            t.translation += step;
        }
        t.rotate_y(3.0 * time.delta_secs());
    }
    if run.xp >= run.xp_next {
        run.xp -= run.xp_next;
        run.level += 1;
        run.xp_next = (run.xp_next as f32 * 1.35).ceil() as u32 + 2;
        next.set(GameState::LevelUp);
    }
}

fn enemy_contact(
    time: Res<Time>,
    mut run: ResMut<Run>,
    mut next: ResMut<NextState<GameState>>,
    player: Single<&Transform, With<Player>>,
    enemies: Query<(&Enemy, &Transform), Without<Player>>,
) {
    run.contact_cooldown -= time.delta_secs();
    if run.contact_cooldown > 0.0 {
        return;
    }
    let p = player.translation.with_y(0.0);
    let hit: f32 = enemies
        .iter()
        .filter(|(_, t)| t.translation.with_y(0.0).distance(p) < PLAYER_RADIUS + ENEMY_RADIUS)
        .map(|(e, _)| e.damage)
        .sum();
    if hit > 0.0 {
        run.hp -= hit;
        run.contact_cooldown = 0.4;
        if run.hp <= 0.0 {
            next.set(GameState::GameOver);
        }
    }
}

fn tick_run(time: Res<Time>, mut run: ResMut<Run>) {
    run.time += time.delta_secs();
}

#[derive(Resource, Default)]
struct SelectState {
    index: usize,
    shown: Option<usize>,
}

/// Character shown on the selection screen.
#[derive(Component)]
struct Preview;

fn open_select(mut commands: Commands, mut state: ResMut<SelectState>) {
    // L2_CLASS=<prefix or name> skips the menu (handy for unattended runs).
    if let Some(i) = std::env::var("L2_CLASS").ok().and_then(|k| find_class(&k)) {
        state.index = i;
    }
    state.shown = None;
    let lines: Vec<String> = CLASSES.iter().map(|c| c.name.to_string()).collect();
    spawn_menu_styled(&mut commands, "Choose your character", &lines, true);
}

#[allow(clippy::too_many_arguments)]
fn select_menu(
    mut commands: Commands,
    input: Res<PlayerInput>,
    mut state: ResMut<SelectState>,
    mut roster: ResMut<Roster>,
    mut next: ResMut<NextState<GameState>>,
    mut lines: Query<(&MenuLine, &mut TextColor)>,
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
    let n = CLASSES.len();
    if input.nav != 0 {
        state.index = (state.index as i32 + input.nav).rem_euclid(n as i32) as usize;
    }
    for (line, mut color) in &mut lines {
        color.0 = if line.0 == state.index { Color::srgb(1.0, 0.85, 0.3) } else { Color::WHITE };
    }
    if state.shown != Some(state.index) {
        state.shown = Some(state.index);
        for e in &previews {
            commands.entity(e).despawn();
        }
        let class = &CLASSES[state.index];
        let spec = part_spec(&mut lib, class);
        let (meshes, materials, images, bindposes) = &mut assets;
        roster.player = cache.load(&mut lib, class.package, &spec, None, meshes, materials, images, bindposes);
        roster.weapon =
            lib.rigid_mesh("lineageweapons", &weapon_spec(class), meshes, materials, images).unwrap_or_default();
        // Face the camera (+Z); models look along +X.
        let root = commands
            .spawn((Preview, Transform::from_xyz(1.2, 0.0, 0.0).with_rotation(Quat::from_rotation_y(-FRAC_PI_2)), Visibility::default()))
            .id();
        if let Some(model) = &roster.player {
            spawn_model(&mut commands, model, root, "wait_1HS", &roster.attachments());
        }
    }
    if input.confirm || std::env::var_os("L2_CLASS").is_some() {
        next.set(GameState::Playing);
    }
}

fn clear_preview(mut commands: Commands, previews: Query<Entity, With<Preview>>) {
    for e in &previews {
        commands.entity(e).despawn();
    }
}

fn follow_camera(
    time: Res<Time>,
    player: Option<Single<&Transform, With<Player>>>,
    mut cam: Single<&mut Transform, (With<Camera3d>, Without<Player>)>,
) {
    let k = 1.0 - (-8.0 * time.delta_secs()).exp();
    let Some(player) = player else {
        // Selection screen: close-up on the preview character.
        let eye = Vec3::new(0.0, 1.6, 4.2);
        cam.translation = cam.translation.lerp(eye, k);
        cam.look_at(Vec3::new(0.0, 1.0, 0.0), Vec3::Y);
        return;
    };
    let target = player.translation + Vec3::new(0.0, 11.0, 7.5);
    cam.translation = cam.translation.lerp(target, k);
    let eye = cam.translation;
    cam.look_at(eye - Vec3::new(0.0, 11.0, 7.5), Vec3::Y);
}

fn update_hud(run: Res<Run>, state: Res<State<GameState>>, mut hud: Single<&mut Text, With<Hud>>) {
    let t = run.time as u32;
    let paused = if *state.get() == GameState::Paused { "   [PAUSED]" } else { "" };
    hud.0 = format!(
        "HP {:.0}/{:.0}   Lv {}  XP {}/{}   Kills {}   {:02}:{:02}{paused}",
        run.hp.max(0.0),
        run.max_hp,
        run.level,
        run.xp,
        run.xp_next,
        run.kills,
        t / 60,
        t % 60
    );
}

fn pause_toggle(input: Res<PlayerInput>, state: Res<State<GameState>>, mut next: ResMut<NextState<GameState>>) {
    if !input.pause {
        return;
    }
    match state.get() {
        GameState::Playing => next.set(GameState::Paused),
        GameState::Paused => next.set(GameState::Playing),
        _ => {}
    }
}

fn spawn_menu(commands: &mut Commands, title: &str, lines: &[String]) {
    spawn_menu_styled(commands, title, lines, false);
}

/// `side`: a compact list on the left that leaves the scene visible.
fn spawn_menu_styled(commands: &mut Commands, title: &str, lines: &[String], side: bool) {
    let (title_px, line_px, gap) = if side { (32.0, 20.0, 4.0) } else { (40.0, 26.0, 10.0) };
    commands
        .spawn((
            Menu,
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::Center,
                align_items: if side { AlignItems::FlexStart } else { AlignItems::Center },
                padding: UiRect::left(Val::Px(if side { 48.0 } else { 0.0 })),
                row_gap: Val::Px(gap),
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, if side { 0.0 } else { 0.6 })),
        ))
        .with_children(|p| {
            p.spawn((Text::new(title), TextFont { font_size: FontSize::Px(title_px), ..default() }));
            for (i, line) in lines.iter().enumerate() {
                p.spawn((MenuLine(i), Text::new(line.clone()), TextFont { font_size: FontSize::Px(line_px), ..default() }));
            }
        });
}

#[derive(Component)]
struct MenuLine(usize);

fn open_level_up(mut commands: Commands, mut choice: ResMut<LevelUpChoice>) {
    let mut rng = rand::rng();
    let mut pool = Upgrade::ALL.to_vec();
    choice.options = (0..3).map(|_| pool.swap_remove(rng.random_range(0..pool.len()))).collect();
    choice.selected = 0;
    let lines: Vec<String> = choice.options.iter().map(|u| u.label().to_string()).collect();
    spawn_menu(&mut commands, "Level up! Choose a skill", &lines);
}

fn level_up_menu(
    input: Res<PlayerInput>,
    mut choice: ResMut<LevelUpChoice>,
    mut run: ResMut<Run>,
    mut next: ResMut<NextState<GameState>>,
    mut lines: Query<(&MenuLine, &mut TextColor)>,
) {
    let n = choice.options.len();
    if input.nav != 0 {
        choice.selected = (choice.selected as i32 + input.nav).rem_euclid(n as i32) as usize;
    }
    for (line, mut color) in &mut lines {
        color.0 = if line.0 == choice.selected { Color::srgb(1.0, 0.85, 0.3) } else { Color::WHITE };
    }
    if input.confirm {
        choice.options[choice.selected].apply(&mut run);
        next.set(GameState::Playing);
    }
}

fn open_game_over(mut commands: Commands, run: Res<Run>) {
    let t = run.time as u32;
    let lines = vec![
        format!("Survived {:02}:{:02}, level {}, {} kills", t / 60, t % 60, run.level, run.kills),
        "Press Enter / A to try again".to_string(),
    ];
    spawn_menu(&mut commands, "You died", &lines);
}

fn game_over_menu(input: Res<PlayerInput>, mut next: ResMut<NextState<GameState>>) {
    if input.confirm {
        next.set(GameState::Playing);
    }
}

fn close_menu(mut commands: Commands, menus: Query<Entity, With<Menu>>) {
    for e in &menus {
        commands.entity(e).despawn();
    }
}
