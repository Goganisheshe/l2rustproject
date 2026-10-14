//! Vampire Survivors style loop: move, auto-cast Lineage skills, collect XP,
//! pick new skills or upgrades, survive escalating waves of client monsters.

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
use crate::gamedata::{FxSpec, GameData, MonsterKind, SkillDef};

use crate::skinned::{spawn_model, Animated, Attachment, Model, ModelCache};


#[derive(Resource, Default)]
struct Roster {
    player: Option<Arc<Model>>,
    /// Weapon geometry for the chosen class, hung on `Weapon_R_Bone`.
    weapon: Vec<crate::l2assets::MeshPart>,
    /// Monster kinds in the current wave rotation.
    enemies: Vec<(Arc<Model>, MonsterKind)>,
}

/// Starting skills: Wind Strike for mystics, Power Strike for fighters.
const START_MAGIC: u32 = 1177;
const START_PHYSICAL: u32 = 3;
/// Monster kinds active at once; a new one rotates in every `WAVE_SECONDS`.
const ACTIVE_KINDS: usize = 6;
const WAVE_SECONDS: f32 = 40.0;

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
    radius: f32,
}

#[derive(Component)]
struct Projectile {
    velocity: Vec3,
    damage: f32,
    pierce: u32,
    life: f32,
    /// Explosion effects of the skill, played on each hit.
    explosion: Vec<(FxSpec, Arc<EffectAsset>)>,
}

#[derive(Component)]
struct Gem(u32);

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
    damage_mult: f32,
    cooldown_mult: f32,
    cast_speed: f32,
    pierce: u32,
    spawn_timer: f32,
    wave_timer: f32,
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
            damage_mult: 1.0,
            cooldown_mult: 1.0,
            cast_speed: 1.25,
            pierce: 0,
            spawn_timer: 0.0,
            wave_timer: 0.0,
            contact_cooldown: 0.0,
        }
    }
}

/// Passive upgrades offered next to new skills.
#[derive(Clone, Copy, Debug)]
enum Passive {
    Might,
    Haste,
    Acumen,
    Piercing,
    Speed,
    Magnet,
    Vitality,
}

impl Passive {
    const ALL: [Passive; 7] = [
        Passive::Might,
        Passive::Haste,
        Passive::Acumen,
        Passive::Piercing,
        Passive::Speed,
        Passive::Magnet,
        Passive::Vitality,
    ];

    fn label(self) -> &'static str {
        match self {
            Passive::Might => "Might: +20% skill damage",
            Passive::Haste => "Haste: -12% skill reuse",
            Passive::Acumen => "Acumen: +20% casting speed",
            Passive::Piercing => "Piercing: projectiles pass +1 enemy",
            Passive::Speed => "Wind Walk: +12% move speed",
            Passive::Magnet => "Greed: +40% pickup radius",
            Passive::Vitality => "Vitality: +20 max HP, heal",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Passive::Might => "icon.skill1068",
            Passive::Haste => "icon.skill1086",
            Passive::Acumen => "icon.skill1085",
            Passive::Piercing => "icon.skill0056",
            Passive::Speed => "icon.skill1204",
            Passive::Magnet => "icon.skill1040",
            Passive::Vitality => "icon.skill1045",
        }
    }

    fn apply(self, run: &mut Run) {
        match self {
            Passive::Might => run.damage_mult *= 1.2,
            Passive::Haste => run.cooldown_mult *= 0.88,
            Passive::Acumen => run.cast_speed *= 1.2,
            Passive::Piercing => run.pierce += 1,
            Passive::Speed => run.speed *= 1.12,
            Passive::Magnet => run.magnet *= 1.4,
            Passive::Vitality => {
                run.max_hp += 20.0;
                run.hp = run.max_hp;
            }
        }
    }
}

#[derive(Clone)]
enum Choice {
    NewSkill(Arc<SkillDef>),
    SkillUp(usize),
    Passive(Passive),
}

#[derive(Resource, Default)]
struct LevelUpChoice {
    options: Vec<Choice>,
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
            .init_resource::<Loadout>()
            .init_resource::<CastState>()
            .init_resource::<FxLoader>()
            .init_resource::<IconCache>()
            .add_systems(Startup, setup_world)
            .add_systems(OnEnter(GameState::Select), open_select)
            .add_systems(Update, select_menu.run_if(in_state(GameState::Select)))
            .add_systems(OnExit(GameState::Select), (close_menu, clear_preview, start_run).chain())
            .add_systems(
                Update,
                (
                    move_player,
                    cast_skills,
                    animate_player,
                    grow_roster,
                    spawn_enemies,
                    move_enemies,
                    move_bolts,
                    collect_gems,
                    enemy_contact,
                    tick_run,
                    dying,
                )
                    .chain()
                    .run_if(in_state(GameState::Playing)),
            )
            .add_systems(Update, (follow_camera, update_hud, update_skill_bar, pause_toggle))
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
    mut bindposes: ResMut<Assets<SkinnedMeshInverseBindposes>>,
) {
    let _ = &mut bindposes;
    commands.insert_resource(Roster::default());

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
    commands.spawn((
        SkillBar,
        Node {
            position_type: PositionType::Absolute,
            bottom: Val::Px(16.0),
            width: Val::Percent(100.0),
            justify_content: JustifyContent::Center,
            column_gap: Val::Px(6.0),
            ..default()
        },
    ));
}

fn start_run(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    old: Query<Entity, With<RunEntity>>,
    mut roster: ResMut<Roster>,
    data: Res<GameData>,
    select: Res<SelectState>,
) {
    for e in &old {
        commands.entity(e).despawn();
    }
    commands.insert_resource(Run::default());
    commands.insert_resource(CastState::default());
    roster.enemies.clear();
    let mystic = is_mystic(select.index);
    let start = data
        .skill(if mystic { START_MAGIC } else { START_PHYSICAL })
        .or_else(|| data.skills.iter().find(|s| s.magic == mystic))
        .cloned();
    // L2_SKILLS=<id,id,...> overrides the starting skills (for testing).
    let forced: Vec<Arc<SkillDef>> = std::env::var("L2_SKILLS")
        .unwrap_or_default()
        .split(',')
        .filter_map(|id| data.skill(id.trim().parse().ok()?).cloned())
        .collect();
    let start: Vec<Arc<SkillDef>> = if forced.is_empty() { start.into_iter().collect() } else { forced };
    commands.insert_resource(Loadout {
        skills: start.into_iter().map(|def| Owned { def, level: 1, cooldown: 0.0 }).collect(),
    });
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
    let _ = (&mut meshes, &mut materials);
}

fn is_mystic(index: usize) -> bool {
    CLASSES.get(index).is_some_and(|c| c.prefix.contains("Magic") || c.prefix.contains("Shaman"))
}

fn move_player(
    time: Res<Time>,
    input: Res<PlayerInput>,
    run: Res<Run>,
    cast: Res<CastState>,
    mut player: Single<&mut Transform, With<Player>>,
) {
    // Stick/keyboard Y maps to "into the screen" (-Z). Casting slows you down
    // and keeps you facing the target.
    let d = input.move_dir;
    let dir = Vec3::new(d.x, 0.0, -d.y);
    let speed = if cast.active.is_some() { run.speed * 0.35 } else { run.speed };
    player.translation += dir * speed * time.delta_secs();
    if cast.active.is_none() {
        face(&mut player, dir);
    }
}

/// Turns a ground entity toward `dir`. Lineage models face +X.
fn face(t: &mut Transform, dir: Vec3) {
    if dir.length_squared() > 1e-4 {
        t.rotation = Quat::from_rotation_y((-dir.z).atan2(dir.x));
    }
}

/// Player animation: the current skill's combo stage while casting (e.g.
/// CastMid -> CastEnd -> MagicShot for Wind Strike, SpAtk01 for Power
/// Strike), otherwise run or combat stance.
fn animate_player(
    input: Res<PlayerInput>,
    cast: Res<CastState>,
    loadout: Res<Loadout>,
    player: Single<&ModelLink, With<Player>>,
    mut anims: Query<&mut Animated>,
    mut last: Local<Option<(usize, usize)>>,
) {
    let Ok(mut anim) = anims.get_mut(player.0) else { return };
    if let Some(c) = &cast.active {
        let def = &loadout.skills[c.skill].def;
        let key = (c.skill, c.stage);
        let looped = c.stage == 0 && def.looped && def.anims.len() > 1;
        if *last != Some(key) {
            *last = Some(key);
            let names = clip_candidates(&def.anims[c.stage]);
            let names: Vec<&str> = names.iter().map(String::as_str).chain(["Atk01_1HS"]).collect();
            anim.restart(&names, looped);
            // Fit the clip into the stage time.
            if let Some(len) = loadout_clip_len(&anim, &names) {
                anim.speed = if looped { 1.0 } else { (len / c.stage_len.max(0.05)).clamp(0.5, 3.0) };
            }
        }
        return;
    }
    *last = None;
    anim.speed = 1.0;
    if input.move_dir.length() > 0.1 {
        anim.play(&["Run_1HS", "run"], true);
    } else if anim.looping || anim.finished() {
        anim.play(&["AtkWait_1HS", "wait_1HS", "Wait"], true);
    }
}

fn loadout_clip_len(anim: &Animated, names: &[&str]) -> Option<f32> {
    names.iter().find_map(|n| {
        let i = anim.model.clip(n)?;
        let c = &anim.model.clips[i];
        Some(c.frames / if c.rate > 0.0 { c.rate } else { 30.0 })
    })
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
    for _ in 0..batch {
        let a = rng.random_range(0.0..TAU);
        let pos = player.translation + Vec3::new(a.cos(), 0.0, a.sin()) * SPAWN_RADIUS;
        let kind = (!roster.enemies.is_empty()).then(|| &roster.enemies[rng.random_range(0..roster.enemies.len())]);
        // Bigger monsters are tougher and slower.
        let radius = kind.map(|k| k.1.radius).unwrap_or(ENEMY_RADIUS);
        let size = (radius / 0.45).clamp(0.7, 2.5);
        let enemy = commands
            .spawn((
                RunEntity,
                Enemy {
                    hp: 10.0 * size * (1.0 + minutes),
                    speed: (3.2 / size.sqrt()) * rng.random_range(0.85..1.1),
                    damage: 3.0 + 1.5 * size,
                    radius,
                },
                Transform::from_xyz(pos.x, 0.0, pos.z),
                Visibility::default(),
            ))
            .id();
        match kind {
            Some((model, _)) => {
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

// ---------------------------------------------------------------------------
// Skills: cast with the character's own animations, client effects, damage.
// ---------------------------------------------------------------------------

/// Skill owned by the player in this run.
struct Owned {
    def: Arc<SkillDef>,
    level: u32,
    cooldown: f32,
}

#[derive(Resource, Default)]
struct Loadout {
    skills: Vec<Owned>,
}

const MAX_SKILLS: usize = 6;

/// The cast in progress: one animation stage after another, firing the
/// skill at the start of the last stage (or mid-way for single clips).
struct ActiveCast {
    skill: usize,
    target: Entity,
    target_pos: Vec3,
    stage: usize,
    stage_time: f32,
    stage_len: f32,
    fired: bool,
    started: bool,
}

#[derive(Resource, Default)]
struct CastState {
    active: Option<ActiveCast>,
}

impl ActiveCast {
    fn fire_stage(&self, def: &SkillDef) -> usize {
        def.anims.len().saturating_sub(1)
    }
}

/// Clip names to try for a combo animation, most specific first: the
/// weapon-type variant (we hold one-handed weapons), then the plain one.
fn clip_candidates(base: &str) -> Vec<String> {
    vec![format!("{base}_1HS"), base.to_string(), format!("{base}_Hand")]
}

fn clip_len(model: &Model, base: &str) -> Option<f32> {
    clip_candidates(base).iter().find_map(|c| {
        let i = model.clip(c)?;
        let clip = &model.clips[i];
        Some(clip.frames / if clip.rate > 0.0 { clip.rate } else { 30.0 })
    })
}

/// Duration of a cast stage. Cast loops are shortened from the client's
/// hit times (several seconds) to keep the survivor pace.
fn stage_duration(def: &SkillDef, stage: usize, model: Option<&Model>, cast_speed: f32) -> f32 {
    let natural = model.and_then(|m| clip_len(m, &def.anims[stage])).unwrap_or(0.6);
    let len = if stage == 0 && def.looped && def.anims.len() > 1 {
        (def.hit_time * 0.1).clamp(0.15, 0.45)
    } else {
        natural.clamp(0.15, 0.55)
    };
    len / cast_speed
}

fn skill_damage(def: &SkillDef, level: u32, run: &Run) -> f32 {
    (def.power * 0.35).max(12.0) * (1.0 + 0.35 * (level as f32 - 1.0)) * run.damage_mult
}

#[allow(clippy::too_many_arguments)]
fn cast_skills(
    mut commands: Commands,
    time: Res<Time>,
    run: Res<Run>,
    mut loadout: ResMut<Loadout>,
    mut cast: ResMut<CastState>,
    mut fx: ResMut<FxLoader>,
    mut lib: ResMut<L2Library>,
    mut cache: ResMut<EffectCache>,
    mut assets: (ResMut<Assets<Mesh>>, ResMut<Assets<StandardMaterial>>, ResMut<Assets<Image>>),
    palette: Res<Palette>,
    roster: Res<Roster>,
    player: Single<(Entity, &mut Transform), With<Player>>,
    mut enemies: Query<(Entity, &mut Enemy, &Transform), Without<Player>>,
) {
    let dt = time.delta_secs();
    for s in &mut loadout.skills {
        s.cooldown -= dt;
    }
    let (player_entity, mut player_tr) = player.into_inner();
    let origin = player_tr.translation;
    let (meshes, materials, images) = &mut assets;
    let mut load = |name: &str| fx.get(name, &mut cache, &mut lib, meshes, materials, images);

    if cast.active.is_none() {
        // Start the first ready skill that has an enemy in range.
        let mut pick = None;
        for (i, s) in loadout.skills.iter().enumerate() {
            if s.cooldown > 0.0 {
                continue;
            }
            let reach = s.def.range.clamp(2.2, 14.0);
            let nearest = enemies
                .iter()
                .map(|(e, _, t)| (e, t.translation, t.translation.distance(origin)))
                .filter(|x| x.2 <= reach)
                .min_by(|a, b| a.2.total_cmp(&b.2));
            if let Some((e, pos, _)) = nearest {
                pick = Some((i, e, pos));
                break;
            }
        }
        if let Some((i, target, pos)) = pick {
            cast.active = Some(ActiveCast {
                skill: i,
                target,
                target_pos: pos,
                stage: 0,
                stage_time: 0.0,
                stage_len: stage_duration(&loadout.skills[i].def, 0, roster.player.as_deref(), run.cast_speed),
                fired: false,
                started: false,
            });
        }
    }

    let Some(active) = cast.active.as_mut() else { return };
    let def = loadout.skills[active.skill].def.clone();
    let level = loadout.skills[active.skill].level;
    if let Ok((_, _, t)) = enemies.get(active.target) {
        active.target_pos = t.translation;
    }
    face(&mut player_tr, (active.target_pos - origin).with_y(0.0));

    if !active.started {
        active.started = true;
        for spec in &def.casting {
            if let Some(asset) = load(&spec.class) {
                spawn_fx(&mut commands, &asset, spec, &player_tr, player_entity, active.target_pos);
            }
        }
    }

    active.stage_time += dt;
    let fire_stage = active.fire_stage(&def);
    let fire_now = !active.fired
        && active.stage == fire_stage
        && (def.anims.len() > 1 || active.stage_time >= active.stage_len * 0.45);
    if fire_now {
        active.fired = true;
        let damage = skill_damage(&def, level, &run);
        let aim = (active.target_pos - origin).with_y(0.0).normalize_or_zero();
        let mut flyer = None;
        for spec in &def.shot {
            let Some(asset) = load(&spec.class) else { continue };
            if def.flying_time > 0.0 && !spec.attached && !spec.on_target && flyer.is_none() {
                flyer = Some(asset);
            } else {
                spawn_fx(&mut commands, &asset, spec, &player_tr, player_entity, active.target_pos);
            }
        }
        let explosion: Vec<(FxSpec, Arc<EffectAsset>)> =
            def.explosion.iter().filter_map(|s| Some((s.clone(), load(&s.class)?))).collect();
        if def.flying_time > 0.0 {
            let dist = origin.distance(active.target_pos).max(1.0);
            let speed = (dist / def.flying_time).clamp(10.0, 28.0);
            let mut tr = Transform::from_translation(origin + Vec3::Y * 1.1);
            face(&mut tr, aim);
            let bolt = commands
                .spawn((
                    RunEntity,
                    Projectile { velocity: aim * speed, damage, pierce: run.pierce, life: 1.8, explosion },
                    tr,
                    Visibility::default(),
                    Mesh3d(palette.bolt_mesh.clone()),
                    MeshMaterial3d(palette.bolt_mat.clone()),
                ))
                .id();
            if let Some(asset) = flyer {
                spawn_effect(&mut commands, &asset, tr, Some(bolt));
            }
        } else {
            // Instant skills strike the target and anything right next to it.
            let center = active.target_pos.with_y(0.0);
            let radius = if def.range < 3.0 { 1.6 } else { 1.2 };
            for (e, mut enemy, t) in &mut enemies {
                if enemy.hp <= 0.0 || t.translation.with_y(0.0).distance(center) > radius + enemy.radius {
                    continue;
                }
                enemy.hp -= damage;
                if enemy.hp <= 0.0 {
                    commands.queue(Kill { enemy: e, at: t.translation });
                }
            }
            for (spec, asset) in &explosion {
                let mut at = Transform::from_translation(active.target_pos + Vec3::Y * 0.9 + spec.offset);
                face(&mut at, -aim);
                spawn_effect(&mut commands, asset, at, None);
            }
        }
    }

    let mut done = false;
    if active.stage_time >= active.stage_len {
        active.stage += 1;
        active.stage_time = 0.0;
        if active.stage >= def.anims.len() {
            done = true;
        } else {
            active.stage_len = stage_duration(&def, active.stage, roster.player.as_deref(), run.cast_speed);
        }
    }
    if done {
        let skill = active.skill;
        // Client reuse delays, compressed for a horde game.
        loadout.skills[skill].cooldown = (def.reuse * 0.6).clamp(0.4, 6.0) * run.cooldown_mult;
        cast.active = None;
    }
}

/// Spawns one skill effect relative to the caster or its target.
fn spawn_fx(
    commands: &mut Commands,
    asset: &Arc<EffectAsset>,
    spec: &FxSpec,
    caster: &Transform,
    caster_entity: Entity,
    target: Vec3,
) {
    if spec.on_target {
        let at = Transform::from_translation(target + Vec3::Y * 0.9 + spec.offset).with_rotation(caster.rotation);
        spawn_effect(commands, asset, at, None);
    } else if spec.attached {
        let at = Transform::from_translation(caster.translation + Vec3::Y * 1.0 + caster.rotation * spec.offset)
            .with_rotation(caster.rotation);
        spawn_effect(commands, asset, at, Some(caster_entity));
    } else {
        let at = Transform::from_translation(caster.translation + Vec3::Y * 1.0 + caster.rotation * spec.offset)
            .with_rotation(caster.rotation);
        spawn_effect(commands, asset, at, None);
    }
}

/// Lazily loaded effect classes for skills (some skills use dozens).
#[derive(Resource, Default)]
struct FxLoader {
    loaded: std::collections::HashMap<String, Option<Arc<EffectAsset>>>,
}

impl FxLoader {
    fn get(
        &mut self,
        name: &str,
        cache: &mut EffectCache,
        lib: &mut L2Library,
        meshes: &mut Assets<Mesh>,
        materials: &mut Assets<StandardMaterial>,
        images: &mut Assets<Image>,
    ) -> Option<Arc<EffectAsset>> {
        self.loaded
            .entry(name.to_string())
            .or_insert_with(|| cache.load(lib, name, meshes, materials, images))
            .clone()
    }
}

/// Deferred kill so damage code can run inside queries.
struct Kill {
    enemy: Entity,
    at: Vec3,
}

impl Command for Kill {
    type Out = ();

    fn apply(self, world: &mut World) {
        let Ok(mut e) = world.get_entity_mut(self.enemy) else { return };
        if e.take::<Enemy>().is_none() {
            return; // already dead this frame
        }
        e.insert(Dying(1.6));
        world.resource_mut::<Run>().kills += 1;
        let (mesh, mat) = {
            let p = world.resource::<Palette>();
            (p.gem_mesh.clone(), p.gem_mat.clone())
        };
        world.spawn((
            RunEntity,
            Gem(1),
            Mesh3d(mesh),
            MeshMaterial3d(mat),
            Transform::from_xyz(self.at.x, 0.3, self.at.z).with_rotation(Quat::from_rotation_y(0.7)),
        ));
    }
}

fn move_bolts(
    mut commands: Commands,
    time: Res<Time>,
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
                if enemy.hp <= 0.0 || et.translation.with_y(0.0).distance(t.translation.with_y(0.0)) > enemy.radius + 0.2 {
                    continue;
                }
                enemy.hp -= bolt.damage;
                for (spec, asset) in &bolt.explosion {
                    let mut at = Transform::from_translation(et.translation + Vec3::Y * 0.9 + spec.offset);
                    face(&mut at, -bolt.velocity);
                    spawn_effect(&mut commands, asset, at, None);
                }
                if enemy.hp <= 0.0 {
                    commands.queue(Kill { enemy: e, at: et.translation });
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

/// Rotates client monsters into the waves: starts with a few kinds and swaps
/// in a new random one every `WAVE_SECONDS`.
#[allow(clippy::too_many_arguments)]
fn grow_roster(
    time: Res<Time>,
    mut run: ResMut<Run>,
    mut roster: ResMut<Roster>,
    data: Res<GameData>,
    mut lib: ResMut<L2Library>,
    mut cache: ResMut<ModelCache>,
    mut assets: (
        ResMut<Assets<Mesh>>,
        ResMut<Assets<StandardMaterial>>,
        ResMut<Assets<Image>>,
        ResMut<Assets<SkinnedMeshInverseBindposes>>,
    ),
) {
    run.wave_timer -= time.delta_secs();
    let want_more = roster.enemies.len() < 3;
    if (run.wave_timer > 0.0 && !want_more) || data.monsters.is_empty() {
        return;
    }
    if run.wave_timer <= 0.0 {
        run.wave_timer = WAVE_SECONDS;
    }
    let (meshes, materials, images, bindposes) = &mut assets;
    let mut rng = rand::rng();
    for _ in 0..12 {
        let kind = data.monsters[rng.random_range(0..data.monsters.len())].clone();
        if roster.enemies.iter().any(|(_, k)| k.mesh == kind.mesh) {
            continue;
        }
        let Some(model) = cache.load(&mut lib, &kind.package, &kind.spec, None, meshes, materials, images, bindposes)
        else {
            continue;
        };
        // Needs at least a run cycle to chase the player, and early waves stay
        // small: bigger beasts join as the run goes on.
        let max_height = (2.2 + run.time / 60.0 * 0.6).min(4.5);
        let bad_size = !model.height.is_finite() || model.height < 0.4 || model.height > max_height;
        if (model.clip("run").is_none() && model.clip("walk").is_none()) || bad_size {
            continue;
        }
        let mut kind = kind;
        kind.radius = (model.height * 0.22).clamp(0.3, 1.0);
        info!("wave: {} ({}), {:.1} m", kind.mesh, kind.npc_id, model.height);
        roster.enemies.push((model, kind));
        if roster.enemies.len() > ACTIVE_KINDS {
            roster.enemies.remove(0);
        }
        break;
    }
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
        .filter(|(e, t)| t.translation.with_y(0.0).distance(p) < PLAYER_RADIUS + e.radius)
        .map(|(e, _)| e.damage)
        .sum();
    if hit > 0.0 {
        run.hp -= hit;
        run.contact_cooldown = 0.5;
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
    // L2_ZOOM < 1 brings the camera closer (for inspecting animations).
    let zoom: f32 = std::env::var("L2_ZOOM").ok().and_then(|v| v.parse().ok()).unwrap_or(1.0);
    let offset = Vec3::new(0.0, 11.0, 7.5) * zoom;
    let target = player.translation + offset;
    cam.translation = cam.translation.lerp(target, k);
    let eye = cam.translation;
    cam.look_at(eye - offset + Vec3::Y * (1.0 - zoom).max(0.0), Vec3::Y);
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

/// Skill icons from the client (`icon.skill1177` -> package icon, object skill1177).
#[derive(Resource, Default)]
struct IconCache(std::collections::HashMap<String, Option<Handle<Image>>>);

impl IconCache {
    fn get(&mut self, name: &str, lib: &mut L2Library, images: &mut Assets<Image>) -> Option<Handle<Image>> {
        self.0
            .entry(name.to_string())
            .or_insert_with(|| {
                let (p, o) = name.split_once('.')?;
                lib.texture(p, o, false).map(|i| images.add(i))
            })
            .clone()
    }
}

fn choice_text(c: &Choice, loadout: &Loadout) -> (String, String) {
    match c {
        Choice::NewSkill(d) => (
            format!("New skill: {}{}", d.name, if d.flying_time > 0.0 { " (ranged)" } else { "" }),
            d.icon.clone(),
        ),
        Choice::SkillUp(i) => {
            let o = &loadout.skills[*i];
            (format!("{} Lv {} -> {}: +35% damage", o.def.name, o.level, o.level + 1), o.def.icon.clone())
        }
        Choice::Passive(p) => (p.label().to_string(), p.icon().to_string()),
    }
}

#[allow(clippy::too_many_arguments)]
fn open_level_up(
    mut commands: Commands,
    mut choice: ResMut<LevelUpChoice>,
    loadout: Res<Loadout>,
    data: Res<GameData>,
    select: Res<SelectState>,
    mut icons: ResMut<IconCache>,
    mut lib: ResMut<L2Library>,
    mut images: ResMut<Assets<Image>>,
) {
    let mut rng = rand::rng();
    let mystic = is_mystic(select.index);
    let mut pool: Vec<Choice> = Vec::new();
    if loadout.skills.len() < MAX_SKILLS {
        let fresh: Vec<&Arc<SkillDef>> = data
            .skills
            .iter()
            .filter(|s| s.magic == mystic && !loadout.skills.iter().any(|o| o.def.id == s.id))
            .collect();
        for _ in 0..2 {
            if !fresh.is_empty() {
                pool.push(Choice::NewSkill(fresh[rng.random_range(0..fresh.len())].clone()));
            }
        }
    }
    for i in 0..loadout.skills.len() {
        pool.push(Choice::SkillUp(i));
    }
    for p in Passive::ALL {
        pool.push(Choice::Passive(p));
    }
    // Always offer at least one skill-related pick.
    let mut options = Vec::new();
    if let Some(i) = pool.iter().position(|c| !matches!(c, Choice::Passive(_))) {
        let first = rng.random_range(0..=i.min(pool.len() - 1));
        options.push(pool.remove(if matches!(pool[first], Choice::Passive(_)) { i } else { first }));
    }
    while options.len() < 3 && !pool.is_empty() {
        options.push(pool.swap_remove(rng.random_range(0..pool.len())));
    }
    choice.options = options;
    choice.selected = 0;

    let entries: Vec<(String, Option<Handle<Image>>)> = choice
        .options
        .iter()
        .map(|c| {
            let (text, icon) = choice_text(c, &loadout);
            (text, icons.get(&icon, &mut lib, &mut images))
        })
        .collect();
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
                row_gap: Val::Px(12.0),
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.6)),
        ))
        .with_children(|p| {
            p.spawn((Text::new("Level up! Choose"), TextFont { font_size: FontSize::Px(40.0), ..default() }));
            for (i, (text, icon)) in entries.into_iter().enumerate() {
                p.spawn(Node { align_items: AlignItems::Center, column_gap: Val::Px(12.0), width: Val::Px(620.0), ..default() })
                    .with_children(|row| {
                        if let Some(img) = icon {
                            row.spawn((ImageNode::new(img), Node { width: Val::Px(40.0), height: Val::Px(40.0), ..default() }));
                        }
                        row.spawn((MenuLine(i), Text::new(text), TextFont { font_size: FontSize::Px(24.0), ..default() }));
                    });
            }
        });
}

fn level_up_menu(
    input: Res<PlayerInput>,
    mut choice: ResMut<LevelUpChoice>,
    mut run: ResMut<Run>,
    mut loadout: ResMut<Loadout>,
    mut next: ResMut<NextState<GameState>>,
    mut lines: Query<(&MenuLine, &mut TextColor)>,
) {
    let n = choice.options.len().max(1);
    if input.nav != 0 {
        choice.selected = (choice.selected as i32 + input.nav).rem_euclid(n as i32) as usize;
    }
    for (line, mut color) in &mut lines {
        color.0 = if line.0 == choice.selected { Color::srgb(1.0, 0.85, 0.3) } else { Color::WHITE };
    }
    if input.confirm {
        match choice.options.get(choice.selected).cloned() {
            Some(Choice::NewSkill(def)) => loadout.skills.push(Owned { def, level: 1, cooldown: 0.0 }),
            Some(Choice::SkillUp(i)) => loadout.skills[i].level += 1,
            Some(Choice::Passive(p)) => p.apply(&mut run),
            None => {}
        }
        next.set(GameState::Playing);
    }
}

/// Bottom bar: owned skills with their icons, levels and cooldowns.
#[derive(Component)]
struct SkillBar;

#[derive(Component)]
struct SkillSlot;

#[allow(clippy::too_many_arguments)]
fn update_skill_bar(
    mut commands: Commands,
    loadout: Res<Loadout>,
    bar: Single<Entity, With<SkillBar>>,
    slots: Query<Entity, With<SkillSlot>>,
    mut icons: ResMut<IconCache>,
    mut lib: ResMut<L2Library>,
    mut images: ResMut<Assets<Image>>,
    mut shown: Local<Vec<(u32, u32, bool)>>,
) {
    let now: Vec<(u32, u32, bool)> = loadout.skills.iter().map(|s| (s.def.id, s.level, s.cooldown > 0.0)).collect();
    if *shown == now {
        return;
    }
    *shown = now;
    for e in &slots {
        commands.entity(e).despawn();
    }
    for s in &loadout.skills {
        let icon = icons.get(&s.def.icon, &mut lib, &mut images);
        let tint = if s.cooldown > 0.0 { Color::srgb(0.45, 0.45, 0.45) } else { Color::WHITE };
        commands.entity(*bar).with_children(|b| {
            b.spawn((SkillSlot, Node { flex_direction: FlexDirection::Column, align_items: AlignItems::Center, ..default() }))
                .with_children(|slot| {
                    if let Some(img) = icon {
                        slot.spawn((
                            ImageNode::new(img).with_color(tint),
                            Node { width: Val::Px(44.0), height: Val::Px(44.0), ..default() },
                        ));
                    }
                    slot.spawn((Text::new(format!("Lv{}", s.level)), TextFont { font_size: FontSize::Px(14.0), ..default() }));
                });
        });
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
