//! Vampire Survivors style loop: move, auto-attack, collect XP, pick upgrades,
//! survive escalating waves.

use std::f32::consts::{FRAC_PI_2, TAU};

use bevy::math::Affine2;
use bevy::prelude::*;
use rand::Rng;

use crate::input::PlayerInput;
use crate::l2assets::L2Textures;

const ARENA_TILE: f32 = 8.0;
const SPAWN_RADIUS: f32 = 22.0;
const DESPAWN_RADIUS: f32 = 40.0;
const PLAYER_RADIUS: f32 = 0.5;
const ENEMY_RADIUS: f32 = 0.45;
const GEM_PICKUP_RADIUS: f32 = 0.6;

#[derive(States, Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum GameState {
    #[default]
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
            .add_systems(Startup, (setup_world, start_run).chain())
            .add_systems(
                Update,
                (
                    move_player,
                    spawn_enemies,
                    move_enemies,
                    fire_bolts,
                    move_bolts,
                    aura_damage,
                    collect_gems,
                    enemy_contact,
                    tick_run,
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
) {
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

    commands.spawn((
        DirectionalLight { illuminance: 9000.0, shadow_maps_enabled: true, ..default() },
        Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -1.0, 0.4, 0.0)),
    ));
    commands.spawn((Camera3d::default(), Transform::from_xyz(0.0, 18.0, 12.0).looking_at(Vec3::ZERO, Vec3::Y)));

    commands.insert_resource(Palette {
        enemy_mesh: meshes.add(Capsule3d::new(ENEMY_RADIUS, 0.8)),
        enemy_mat: materials.add(Color::srgb(0.6, 0.15, 0.15)),
        bolt_mesh: meshes.add(Sphere::new(0.18)),
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
) {
    for e in &old {
        commands.entity(e).despawn();
    }
    commands.insert_resource(Run::default());
    commands
        .spawn((
            Player,
            RunEntity,
            Mesh3d(meshes.add(Capsule3d::new(PLAYER_RADIUS, 1.0))),
            MeshMaterial3d(materials.add(Color::srgb(0.85, 0.8, 0.6))),
            Transform::from_xyz(0.0, 1.0, 0.0),
        ))
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
            Transform::from_xyz(0.0, -0.95, 0.0).with_rotation(Quat::from_rotation_x(-FRAC_PI_2)),
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
    player.translation += Vec3::new(d.x, 0.0, -d.y) * run.speed * time.delta_secs();
}

fn spawn_enemies(
    mut commands: Commands,
    time: Res<Time>,
    mut run: ResMut<Run>,
    palette: Res<Palette>,
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
    for _ in 0..batch {
        let a = rng.random_range(0.0..TAU);
        let pos = player.translation + Vec3::new(a.cos(), 0.0, a.sin()) * SPAWN_RADIUS;
        commands.spawn((
            RunEntity,
            Enemy { hp: 15.0 * (1.0 + minutes), speed: rng.random_range(2.0..3.2), damage: 8.0 },
            Mesh3d(palette.enemy_mesh.clone()),
            MeshMaterial3d(palette.enemy_mat.clone()),
            Transform::from_xyz(pos.x, 0.9, pos.z),
        ));
    }
}

fn move_enemies(
    mut commands: Commands,
    time: Res<Time>,
    player: Single<&Transform, (With<Player>, Without<Enemy>)>,
    mut enemies: Query<(Entity, &Enemy, &mut Transform)>,
) {
    let target = player.translation.with_y(0.9);
    for (e, enemy, mut t) in &mut enemies {
        let to = target - t.translation;
        if to.length() > DESPAWN_RADIUS {
            commands.entity(e).despawn();
            continue;
        }
        t.translation += to.normalize_or_zero() * enemy.speed * time.delta_secs();
    }
}

fn fire_bolts(
    mut commands: Commands,
    time: Res<Time>,
    mut run: ResMut<Run>,
    palette: Res<Palette>,
    player: Single<&Transform, With<Player>>,
    enemies: Query<&Transform, With<Enemy>>,
) {
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
    let n = run.bolt_count;
    for i in 0..n {
        let spread = (i as f32 - (n - 1) as f32 / 2.0) * 0.18;
        let dir = Quat::from_rotation_y(spread) * aim;
        commands.spawn((
            RunEntity,
            Projectile { velocity: dir * 14.0, damage: run.bolt_damage, pierce: run.bolt_pierce, life: 1.6 },
            Mesh3d(palette.bolt_mesh.clone()),
            MeshMaterial3d(palette.bolt_mat.clone()),
            Transform::from_translation(origin),
        ));
    }
}

fn move_bolts(
    mut commands: Commands,
    time: Res<Time>,
    mut run: ResMut<Run>,
    palette: Res<Palette>,
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
    commands.entity(e).despawn();
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

fn follow_camera(
    time: Res<Time>,
    player: Option<Single<&Transform, With<Player>>>,
    mut cam: Single<&mut Transform, (With<Camera3d>, Without<Player>)>,
) {
    let Some(player) = player else { return };
    let target = player.translation + Vec3::new(0.0, 17.0, 11.0);
    let k = 1.0 - (-8.0 * time.delta_secs()).exp();
    cam.translation = cam.translation.lerp(target, k);
    let eye = cam.translation;
    cam.look_at(eye - Vec3::new(0.0, 17.0, 11.0), Vec3::Y);
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
    commands
        .spawn((
            Menu,
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                row_gap: Val::Px(10.0),
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.6)),
        ))
        .with_children(|p| {
            p.spawn((Text::new(title), TextFont { font_size: FontSize::Px(40.0), ..default() }));
            for (i, line) in lines.iter().enumerate() {
                p.spawn((MenuLine(i), Text::new(line.clone()), TextFont { font_size: FontSize::Px(26.0), ..default() }));
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
