//! Offline gameplay on top of the streamed world: NPC spawns from the
//! datapack, monster AI, targeting, auto-attack, experience and levels,
//! death and respawn, and the HUD (HP/MP/EXP bars, target window, log).
//!
//! Formulas follow Lineage II in spirit: physical damage = 70 * P.Atk /
//! P.Def (+-10%), attack interval = 500 000 / attack speed ms, stats grow
//! with the level modifier (level + 89) / 100.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;

use bevy::mesh::skinning::SkinnedMeshInverseBindposes;
use bevy::prelude::*;
use rand::Rng;

use crate::collision::CollisionWorld;
use crate::datapack::{Datapack, NpcTemplate, PlayerTemplate};
use crate::gamedata::GameData;
use crate::l2assets::{L2Library, UU_TO_M};
use crate::mmo::{Hero, Phase};
use crate::skinned::{spawn_model, Animated, Model, ModelCache};
use crate::streaming::{ue_to_bevy, TileStreamer};

/// Collision radius of a player character (Unreal units; the datapack has
/// 6.5 to 9 depending on race and sex).
pub const HERO_RADIUS: f32 = 8.0;

/// Model kinds loaded per frame (each load decodes a skeletal mesh).
const MODEL_LOADS_PER_FRAME: usize = 2;

pub struct RpgPlugin;

impl Plugin for RpgPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<NpcSpawner>()
            .init_resource::<ReviveClicked>()
            .init_resource::<SpawnRequests>()
            .init_resource::<Target>()
            .init_resource::<Log>()
            .add_systems(
                Update,
                (
                    spawn_npcs,
                    spawn_requested,
                    attach_npc_models,
                    pick_target,
                    hero_attack,
                    npc_ai,
                    npc_respawn,
                    hero_regen_and_death,
                    update_hud,
                    debug_npcs,
                )
                    .chain()
                    .run_if(in_state(Phase::InWorld)),
            );
    }
}

// ---------------------------------------------------------------------------
// Hero stats
// ---------------------------------------------------------------------------

#[derive(Component)]
pub struct HeroStats {
    pub template: PlayerTemplate,
    /// Datapack template / skill tree name (`HumanFighter`...).
    pub class_key: String,
    pub level: u32,
    pub exp: u64,
    pub sp: u64,
    pub hp: f32,
    pub mp: f32,
    pub max_hp: f32,
    pub max_mp: f32,
    pub patk: f32,
    pub pdef: f32,
    pub matk: f32,
    pub mdef: f32,
    pub accuracy: f32,
    pub evasion: f32,
    /// Critical rate, per mille.
    pub crit: f32,
    /// Weapon damage spread, percent.
    pub random: f32,
    pub patk_speed: f32,
    pub matk_speed: f32,
    /// Run speed in Unreal units per second (buffs change it).
    pub run_speed: f32,
    pub atk_interval: f32,
    /// Meters.
    pub atk_range: f32,
    pub hp_regen: f32,
    pub mp_regen: f32,
    pub atk_timer: f32,
    pub attacking: bool,
    /// Sitting down (V): faster regeneration, no moving.
    pub sitting: bool,
    pub dead_timer: f32,
    /// Where the hero returns after death (the starting village).
    pub respawn: Vec3,
    /// Active buffs from skills.
    pub buffs: Vec<Buff>,
    pub equip: Equipment,
}

/// Equipped item stats, sorted for the stat formulas.
#[derive(Clone, Debug, Default)]
pub struct Equipment {
    /// Weapon stats (pAtk, mAtk, pAtkSpd, critRate, randomDamage,
    /// accCombat, pAtkRange); empty when unarmed.
    pub weapon: HashMap<String, f32>,
    /// P.Def of armor by the template slot it replaces (chest, legs...).
    pub armor_pdef: HashMap<String, f32>,
    /// M.Def of jewels by template slot (neck, rear, rfinger...).
    pub jewel_mdef: HashMap<String, f32>,
    /// Evasion from armor and shields.
    pub evasion: f32,
    pub max_mp: f32,
}

/// A timed stat modifier from a skill effect (PhysicalAttack, Speed...).
#[derive(Clone, Debug)]
pub struct Buff {
    pub skill: u32,
    pub stat: String,
    pub amount: f32,
    /// true = percent, false = flat.
    pub percent: bool,
    pub remaining: f32,
}

impl HeroStats {
    pub fn new(template: PlayerTemplate) -> Self {
        let mut s = Self {
            template,
            class_key: String::new(),
            level: 1,
            exp: 0,
            sp: 0,
            hp: 0.0,
            mp: 0.0,
            max_hp: 0.0,
            max_mp: 0.0,
            patk: 0.0,
            pdef: 0.0,
            matk: 0.0,
            mdef: 0.0,
            accuracy: 0.0,
            evasion: 0.0,
            crit: 0.0,
            random: 10.0,
            patk_speed: 300.0,
            matk_speed: 333.0,
            run_speed: 120.0,
            atk_interval: 1.0,
            atk_range: 1.5,
            hp_regen: 2.0,
            mp_regen: 1.0,
            atk_timer: 0.0,
            attacking: false,
            sitting: false,
            dead_timer: 0.0,
            respawn: Vec3::ZERO,
            buffs: Vec::new(),
            equip: Equipment::default(),
        };
        s.recalc();
        s.hp = s.max_hp;
        s.mp = s.max_mp;
        s
    }

    pub fn combat(&self) -> crate::formulas::Combat {
        crate::formulas::Combat {
            patk: self.patk,
            pdef: self.pdef,
            matk: self.matk,
            mdef: self.mdef,
            accuracy: self.accuracy,
            evasion: self.evasion,
            crit: self.crit,
            random: self.random,
        }
    }

    /// Recomputes derived stats with the server's stat functions.
    pub fn recalc(&mut self) {
        use crate::formulas as f;
        let t = &self.template;
        let lm = f::level_mod(self.level);
        let gain = t.levels.get(self.level as usize - 1).or(t.levels.last()).cloned().unwrap_or_default();
        let (con, men) = (f::con_bonus(t.con), f::men_bonus(t.men));
        self.max_hp = gain.hp * con;
        self.max_mp = gain.mp * men + self.equip.max_mp;
        self.hp_regen = gain.hp_regen * con;
        self.mp_regen = gain.mp_regen * men;
        let e = &self.equip;
        let w = |k: &str| e.weapon.get(k).copied();
        let int_b = f::int_bonus(t.int);
        self.patk = w("pAtk").unwrap_or(t.patk) * f::str_bonus(t.str_) * lm;
        self.matk = w("mAtk").unwrap_or(t.matk) * int_b * int_b * lm * lm;
        // Armor replaces the bare slot's base defence.
        let pdef: f32 = t.pdef_slots.iter().map(|(slot, base)| e.armor_pdef.get(slot).copied().unwrap_or(*base)).sum::<f32>()
            + e.armor_pdef.get("shield").copied().unwrap_or(0.0);
        let mdef: f32 = t.mdef_slots.iter().map(|(slot, base)| e.jewel_mdef.get(slot).copied().unwrap_or(*base)).sum();
        self.pdef = pdef * lm;
        self.mdef = mdef * men * lm;
        let dex = f::dex_bonus(t.dex);
        self.accuracy = f::dex_accuracy(t.dex, self.level) + w("accCombat").unwrap_or(0.0);
        self.evasion = f::dex_accuracy(t.dex, self.level) + e.evasion;
        self.crit = w("critRate").unwrap_or(t.crit) * dex * 10.0;
        self.random = w("randomDamage").unwrap_or(t.rnd_dmg);
        self.run_speed = t.run_speed;
        let mut atk_speed = w("pAtkSpd").unwrap_or(t.patk_speed) * dex;
        let mut cast_speed = t.matk_speed * f::wit_bonus(t.wit);
        for b in &self.buffs {
            let apply = |v: &mut f32| {
                if b.percent {
                    *v *= 1.0 + b.amount / 100.0
                } else {
                    *v += b.amount
                }
            };
            match b.stat.as_str() {
                "PhysicalAttack" => apply(&mut self.patk),
                "PhysicalDefence" => apply(&mut self.pdef),
                "MagicalAttack" => apply(&mut self.matk),
                "MagicalDefence" => apply(&mut self.mdef),
                "Speed" => apply(&mut self.run_speed),
                "PhysicalAttackSpeed" => apply(&mut atk_speed),
                "MagicalAttackSpeed" => apply(&mut cast_speed),
                "Accuracy" => apply(&mut self.accuracy),
                "PhysicalEvasion" => apply(&mut self.evasion),
                "CriticalRate" => apply(&mut self.crit),
                "MaxHp" => apply(&mut self.max_hp),
                "MaxMp" => apply(&mut self.max_mp),
                _ => {}
            }
        }
        self.patk_speed = atk_speed;
        self.matk_speed = cast_speed;
        // 500 000 / attack speed milliseconds between hits.
        self.atk_interval = 500.0 / atk_speed.max(1.0);
        // Lineage: a hit lands when the centres are within the weapon range
        // plus both collision radii; the target's radius is added at use.
        self.atk_range = (w("pAtkRange").unwrap_or(t.atk_range).max(20.0) + HERO_RADIUS) * UU_TO_M;
        self.hp = self.hp.min(self.max_hp);
        self.mp = self.mp.min(self.max_mp);
    }
}

pub fn template_for_stats(stats: &HeroStats) -> &str {
    &stats.class_key
}

/// Starting class template for a character-creation entry.
pub fn template_for(class: &crate::character::CharClass) -> &'static str {
    let mystic = class.name.contains("Mystic");
    match class.prefix {
        "MElf" | "FElf" if mystic => "ElvenMystic",
        "MDarkElf" | "FDarkElf" if mystic => "DarkMystic",
        "MFighter" | "FFighter" => "HumanFighter",
        "MMagic" | "FMagic" => "HumanMystic",
        "MElf" | "FElf" => "ElvenFighter",
        "MDarkElf" | "FDarkElf" => "DarkFighter",
        "MOrc" | "FOrc" => "OrcFighter",
        "MShaman" | "FShaman" => "OrcMystic",
        "MDwarf" | "FDwarf" => "DwarvenFighter",
        "MKamael" | "FKamael" => "KamaelSoldier",
        _ => "HumanFighter",
    }
}

// ---------------------------------------------------------------------------
// NPCs
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum NpcState {
    Idle,
    Chase,
    Return,
    Dead,
}

#[derive(Component)]
pub struct Npc {
    pub template: Arc<NpcTemplate>,
    spawn: usize,
    home: Vec3,
    pub hp: f32,
    pub state: NpcState,
    atk_timer: f32,
    dead_timer: f32,
    /// Hit by the hero: turns passive monsters hostile.
    pub provoked: bool,
    /// Drops already rolled.
    pub looted: bool,
    /// Death already reported to quests.
    pub quest_seen: bool,
    pub actor: Option<Entity>,
    /// The boss this minion escorts.
    pub leader: Option<Entity>,
    /// Seconds until the next skill.
    pub skill_timer: f32,
    /// Idle stroll around the spawn point.
    wander: Option<Vec3>,
    wander_timer: f32,
}

/// Bosses are balanced for parties; solo play scales their HP and attack
/// (`L2_BOSS_HP` / `L2_BOSS_ATK`, default 5% and 60%).
fn solo_scale(mut t: NpcTemplate, minion: bool) -> NpcTemplate {
    if t.is_boss() || minion {
        let env = |k: &str, d: f32| std::env::var(k).ok().and_then(|v| v.parse().ok()).unwrap_or(d);
        t.hp *= env("L2_BOSS_HP", 0.05);
        let atk = env("L2_BOSS_ATK", 0.6);
        t.patk *= atk;
        t.matk *= atk;
    }
    t
}

/// The death window's "To Village" button was clicked.
#[derive(Resource, Default)]
struct ReviveClicked(bool);

/// NPCs spawned by scripts (Baium waking up): id and position in Unreal units.
#[derive(Resource, Default)]
pub struct SpawnRequests(pub Vec<(u32, [f32; 3])>);

/// Spawn index of NPCs that never respawn on their own (minions, script spawns).
const NO_RESPAWN: usize = usize::MAX;

#[derive(Component)]
struct NeedsModel;

#[derive(Resource, Default)]
struct NpcSpawner {
    spawned_tiles: HashSet<String>,
    /// Spawn indices waiting for respawn, with remaining seconds.
    respawns: Vec<(usize, f32)>,
    models: HashMap<u32, Option<Arc<Model>>>,
    templates: HashMap<u32, Arc<NpcTemplate>>,
}

fn spawn_npc(
    commands: &mut Commands,
    spawner: &mut NpcSpawner,
    dp: &Datapack,
    streamer: &TileStreamer,
    world: &CollisionWorld,
    idx: usize,
) {
    let s = &dp.spawns[idx];
    spawn_at(commands, spawner, dp, streamer, world, s.npc, s.pos, s.heading, idx, None);
}

#[allow(clippy::too_many_arguments)]
fn spawn_at(
    commands: &mut Commands,
    spawner: &mut NpcSpawner,
    dp: &Datapack,
    streamer: &TileStreamer,
    world: &CollisionWorld,
    npc: u32,
    ue: [f32; 3],
    heading: i32,
    idx: usize,
    leader: Option<Entity>,
) -> Option<Entity> {
    let t = dp.npcs.get(&npc)?;
    // Raid escorts are party content like their boss; cached apart from
    // ordinary spawns of the same NPC.
    let key = if leader.is_some() { npc | 0x8000_0000 } else { npc };
    let template = spawner.templates.entry(key).or_insert_with(|| Arc::new(solo_scale(t.clone(), leader.is_some()))).clone();
    let mut pos = ue_to_bevy(ue);
    pos.y = crate::mmo::settle(pos, streamer, world);
    let yaw = -(heading as f32) * std::f32::consts::TAU / 65536.0;
    let minions = template.minions.clone();
    let e = commands
        .spawn((
            Npc {
                hp: template.hp,
                template,
                spawn: idx,
                home: pos,
                state: NpcState::Idle,
                atk_timer: 0.0,
                dead_timer: 0.0,
                provoked: false,
                looted: false,
                quest_seen: false,
                actor: None,
                leader,
                skill_timer: 3.0,
                wander: None,
                wander_timer: rand::rng().random_range(2.0..10.0),
            },
            NeedsModel,
            Transform::from_translation(pos).with_rotation(Quat::from_rotation_y(yaw)),
            Visibility::default(),
        ))
        .id();
    // The escort stands in a ring around its boss.
    let total: u32 = minions.iter().map(|m| m.1).sum();
    let mut k = 0;
    for (id, count) in minions {
        for _ in 0..count {
            let a = k as f32 / total.max(1) as f32 * std::f32::consts::TAU;
            k += 1;
            let p = [ue[0] + 120.0 * a.cos(), ue[1] + 120.0 * a.sin(), ue[2]];
            spawn_at(commands, spawner, dp, streamer, world, id, p, heading, NO_RESPAWN, Some(e));
        }
    }
    Some(e)
}

/// Script spawns (`SpawnRequests`).
fn spawn_requested(
    mut commands: Commands,
    mut req: ResMut<SpawnRequests>,
    mut spawner: ResMut<NpcSpawner>,
    dp: Res<Datapack>,
    streamer: Res<TileStreamer>,
    world: Res<CollisionWorld>,
) {
    for (npc, pos) in std::mem::take(&mut req.0) {
        spawn_at(&mut commands, &mut spawner, &dp, &streamer, &world, npc, pos, 0, NO_RESPAWN, None);
    }
}

fn spawn_npcs(
    mut commands: Commands,
    mut spawner: ResMut<NpcSpawner>,
    dp: Res<Datapack>,
    streamer: Res<TileStreamer>,
    world: Res<CollisionWorld>,
) {
    for tile in streamer.loaded_tiles() {
        if spawner.spawned_tiles.contains(tile) {
            continue;
        }
        spawner.spawned_tiles.insert(tile.clone());
        let Some((x, y)) = tile.split_once('_').and_then(|(a, b)| Some((a.parse().ok()?, b.parse().ok()?))) else {
            continue;
        };
        let list = dp.spawns_by_tile.get(&(x, y)).cloned().unwrap_or_default();
        for idx in &list {
            spawn_npc(&mut commands, &mut spawner, &dp, &streamer, &world, *idx);
        }
        info!("tile {tile}: {} npcs spawned", list.len());
    }
}

#[allow(clippy::too_many_arguments)]
fn attach_npc_models(
    mut commands: Commands,
    mut spawner: ResMut<NpcSpawner>,
    data: Res<GameData>,
    mut lib: ResMut<L2Library>,
    mut cache: ResMut<ModelCache>,
    mut assets: (
        ResMut<Assets<Mesh>>,
        ResMut<Assets<StandardMaterial>>,
        ResMut<Assets<Image>>,
        ResMut<Assets<SkinnedMeshInverseBindposes>>,
    ),
    mut pending: Query<(Entity, &mut Npc), With<NeedsModel>>,
) {
    let (meshes, materials, images, bindposes) = &mut assets;
    let mut loads = 0;
    for (e, mut npc) in &mut pending {
        let id = npc.template.id;
        if !spawner.models.contains_key(&id) {
            if loads >= MODEL_LOADS_PER_FRAME {
                continue;
            }
            loads += 1;
            let model = data
                .npc_models
                .get(&id)
                .and_then(|(pkg, spec)| cache.load(&mut lib, pkg, spec, None, meshes, materials, images, bindposes));
            spawner.models.insert(id, model);
        }
        commands.entity(e).remove::<NeedsModel>();
        if let Some(Some(model)) = spawner.models.get(&id) {
            npc.actor = Some(spawn_model(&mut commands, model, e, "Wait", &[]));
        }
    }
}

// ---------------------------------------------------------------------------
// Targeting and combat
// ---------------------------------------------------------------------------

#[derive(Resource, Default)]
pub struct Target(pub Option<Entity>);

#[derive(Resource, Default)]
pub struct Log(pub VecDeque<String>);

impl Log {
    pub fn push(&mut self, s: impl Into<String>) {
        self.0.push_back(s.into());
        while self.0.len() > 7 {
            self.0.pop_front();
        }
    }
}

/// Tab / gamepad RB: next monster in front of the camera; left click picks
/// the NPC under the cursor; F / gamepad X attacks.
#[allow(clippy::too_many_arguments)]
pub fn pick_target(
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    gamepads: Query<&Gamepad, With<crate::input::ActivePad>>,
    windows: Query<&Window>,
    camera: Single<(&Camera, &GlobalTransform)>,
    mut target: ResMut<Target>,
    hero: Single<(&Transform, &mut HeroStats), With<Hero>>,
    npcs: Query<(Entity, &Npc, &Transform), Without<Hero>>,
    mut talk: ResMut<crate::dialog::TalkRequest>,
    mut move_to: ResMut<crate::mmo::MoveTarget>,
    hover: Res<bevy::picking::hover::HoverMap>,
    ui_nodes: Query<(), With<Node>>,
    streamer: Res<crate::streaming::TileStreamer>,
    world: Res<crate::collision::CollisionWorld>,
) {
    let (hero_tr, mut stats) = hero.into_inner();
    // L2_AUTOFIGHT: keep targeting and attacking the nearest monster (tests).
    let auto = std::env::var_os("L2_AUTOFIGHT").is_some() && stats.hp > 0.0;
    let target_dead = target.0.is_none_or(|e| npcs.get(e).map_or(true, |(_, n, _)| n.state == NpcState::Dead));
    let cycle = keys.just_pressed(KeyCode::Tab)
        || gamepads.iter().any(|g| g.just_pressed(GamepadButton::RightTrigger))
        || (auto && target_dead);
    if auto && !target_dead {
        stats.attacking = true;
    }
    if cycle {
        let mut cands: Vec<(Entity, f32)> = npcs
            .iter()
            .filter(|(_, n, t)| n.state != NpcState::Dead && n.template.is_monster() && t.translation.distance(hero_tr.translation) < 60.0)
            // The auto-fighter only picks fights it can win.
            .filter(|(_, n, _)| !auto || n.template.level <= stats.level + 3)
            .map(|(e, _, t)| (e, t.translation.distance(hero_tr.translation)))
            .collect();
        cands.sort_by(|a, b| a.1.total_cmp(&b.1));
        let next = match target.0.and_then(|cur| cands.iter().position(|c| c.0 == cur)) {
            Some(i) => cands.get(i + 1).or(cands.first()).map(|c| c.0),
            None => cands.first().map(|c| c.0),
        };
        target.0 = next;
        stats.attacking = false;
    }
    // Clicks on windows and buttons belong to the UI.
    let over_ui = hover.values().any(|m| m.keys().any(|e| ui_nodes.contains(*e)));
    if mouse.just_pressed(MouseButton::Left) && !over_ui {
        let (cam, cam_tr) = camera.into_inner();
        let cursor = windows.iter().next().and_then(|w| w.cursor_position());
        if let Some(ray) = cursor.and_then(|c| cam.viewport_to_world(cam_tr, c).ok()) {
            let mut best = None;
            for (e, n, t) in &npcs {
                if n.state == NpcState::Dead {
                    continue;
                }
                let r = (n.template.radius * UU_TO_M).max(0.4);
                let center = t.translation + Vec3::Y * (n.template.height * UU_TO_M).max(0.8);
                let to = center - ray.origin;
                let along = to.dot(*ray.direction);
                if along > 0.0 && (to - *ray.direction * along).length() < r + 0.4 {
                    if best.is_none_or(|(_, d)| along < d) {
                        best = Some((e, along));
                    }
                }
            }
            if let Some((e, _)) = best {
                if target.0 == Some(e) {
                    // Second click attacks a monster or talks to an NPC.
                    match npcs.get(e) {
                        Ok((_, n, _)) if !n.template.is_monster() => talk.0 = Some(e),
                        _ => stats.attacking = true,
                    }
                    move_to.0 = None;
                }
                target.0 = Some(e);
            } else if stats.hp > 0.0 {
                // A click on the ground walks there; the target stays selected.
                if let Some(p) = crate::mmo::ground_point(ray.origin, *ray.direction, &streamer, &world) {
                    move_to.0 = Some(p);
                    stats.attacking = false;
                    talk.0 = None;
                }
            }
        }
    }
    let attack = keys.just_pressed(KeyCode::KeyF) || gamepads.iter().any(|g| g.just_pressed(GamepadButton::West));
    if attack {
        if let Some((_, n, _)) = target.0.and_then(|e| npcs.get(e).ok()) {
            if n.template.is_monster() {
                stats.attacking = true;
            } else {
                talk.0 = target.0;
            }
        }
    }
    if keys.just_pressed(KeyCode::Escape) {
        target.0 = None;
        stats.attacking = false;
    }
}

/// Runs to the target and auto-attacks it, awarding experience on kills.
#[allow(clippy::too_many_arguments)]
fn hero_attack(
    time: Res<Time>,
    input: Res<crate::input::PlayerInput>,
    dp: Res<Datapack>,
    mut target: ResMut<Target>,
    mut log: ResMut<Log>,
    items: Res<crate::items::ItemData>,
    hero: Single<(&Hero, &mut Transform, &mut HeroStats, Option<&mut crate::items::Inventory>)>,
    mut npcs: Query<(&mut Npc, &Transform), Without<Hero>>,
    mut anims: Query<&mut Animated>,
) {
    let (hero, mut tr, mut stats, inv) = hero.into_inner();
    let dt = time.delta_secs();
    stats.atk_timer -= dt;
    if stats.hp <= 0.0 || !stats.attacking || input.move_dir.length() > 0.1 {
        if input.move_dir.length() > 0.1 {
            stats.attacking = false;
        }
        return;
    }
    let Some(te) = target.0 else {
        stats.attacking = false;
        return;
    };
    let Ok((mut npc, ntr)) = npcs.get_mut(te) else {
        target.0 = None;
        stats.attacking = false;
        return;
    };
    if npc.state == NpcState::Dead {
        stats.attacking = false;
        return;
    }
    let to = (ntr.translation - tr.translation).with_y(0.0);
    let reach = stats.atk_range + npc.template.radius * UU_TO_M;
    tr.rotation = Quat::from_rotation_y((-to.z).atan2(to.x));
    let mut anim = anims.get_mut(hero.actor).ok();
    if to.length() > reach {
        let speed = stats.run_speed * UU_TO_M;
        tr.translation += to.normalize() * speed * dt;
        if let Some(a) = anim.as_mut() {
            a.play(&hero.clips("run", "run").each_ref().map(String::as_str), true);
        }
        return;
    }
    if stats.atk_timer > 0.0 {
        if let Some(a) = anim.as_mut() {
            if a.finished() || a.looping {
                a.play(&hero.clips("AtkWait", "wait_1HS").each_ref().map(String::as_str), true);
            }
        }
        return;
    }
    stats.atk_timer = stats.atk_interval;
    if let Some(a) = anim.as_mut() {
        a.restart(&hero.clips("Atk01", "atk01").each_ref().map(String::as_str), false);
    }
    let mut rng = rand::rng();
    let ss = inv.map_or(1.0, |mut i| crate::items::use_shot(&mut i, &items, false));
    let Some((dmg, crit)) = crate::formulas::physical(&stats.combat(), &npc.template.combat(), 0.0, ss, false, &mut rng) else {
        log.push("You missed.");
        return;
    };
    npc.hp -= dmg;
    npc.provoked = true;
    log.push(format!("You hit {} for {:.0}{}", npc.template.name, dmg, if crit { " (critical)" } else { "" }));
    if npc.hp <= 0.0 {
        award_kill(&mut npc, &mut stats, &dp, &mut log);
    }
}

/// Marks an NPC dead and gives the hero its experience and SP.
pub fn award_kill(npc: &mut Npc, stats: &mut HeroStats, dp: &Datapack, log: &mut Log) {
    if npc.state == NpcState::Dead {
        return;
    }
    npc.hp = 0.0;
    npc.state = NpcState::Dead;
    npc.dead_timer = 0.0;
    stats.attacking = false;
    let gained = npc.template.exp;
    stats.exp += gained;
    stats.sp += npc.template.sp;
    log.push(format!("{} died. +{} XP, +{} SP", npc.template.name, gained, npc.template.sp));
    let new_level = dp.level_for(stats.exp);
    if new_level > stats.level {
        stats.level = new_level;
        stats.recalc();
        stats.hp = stats.max_hp;
        stats.mp = stats.max_mp;
        log.push(format!("Level up! You are now level {new_level}"));
    }
}

/// Monster AI: aggro, chase, attack, leash back home.
#[allow(clippy::too_many_arguments)]
fn npc_ai(
    time: Res<Time>,
    mut log: ResMut<Log>,
    hero: Single<(&Transform, &mut HeroStats), With<Hero>>,
    mut npcs: Query<(Entity, &mut Npc, &mut Transform), Without<Hero>>,
    mut anims: Query<&mut Animated>,
    streamer: Res<TileStreamer>,
    world: Res<CollisionWorld>,
) {
    let dt = time.delta_secs();
    let (hero_tr, mut stats) = hero.into_inner();
    // Minions join the fight when their boss does.
    let fighting: HashSet<Entity> = npcs.iter().filter(|(_, n, _)| n.state == NpcState::Chase).map(|(e, _, _)| e).collect();
    let hero_pos = hero_tr.translation;
    let hero_alive = stats.hp > 0.0;
    let mut rng = rand::rng();
    for (_, mut npc, mut tr) in &mut npcs {
        let anim = npc.actor.and_then(|a| anims.get_mut(a).ok());
        if npc.state == NpcState::Dead {
            // Fall, then lie still (`deathwait`) like the client.
            if let Some(mut a) = anim {
                let lying = a.model.clip("DeathWait").is_some_and(|c| c == a.clip);
                if !lying {
                    a.play(&["Death"], false);
                    if a.finished() {
                        a.play(&["DeathWait"], true);
                    }
                }
            }
            continue;
        }
        if !npc.template.is_monster() {
            continue;
        }
        let dist = tr.translation.distance(hero_pos);
        let aggro = npc.template.aggro_range * UU_TO_M;
        let from_home = tr.translation.distance(npc.home);
        match npc.state {
            NpcState::Idle => {
                let assist = npc.leader.is_some_and(|l| fighting.contains(&l));
                if hero_alive && (npc.provoked || assist || (aggro > 0.0 && dist < aggro)) {
                    npc.state = NpcState::Chase;
                    npc.wander = None;
                }
            }
            NpcState::Chase => {
                if !hero_alive || from_home > 25.0 {
                    npc.state = NpcState::Return;
                    npc.provoked = false;
                }
            }
            NpcState::Return => {
                if from_home < 0.5 {
                    npc.state = NpcState::Idle;
                    npc.hp = npc.template.hp;
                }
            }
            NpcState::Dead => {}
        }
        let mut anim = anim;
        match npc.state {
            NpcState::Chase => {
                let to = (hero_pos - tr.translation).with_y(0.0);
                tr.rotation = Quat::from_rotation_y((-to.z).atan2(to.x));
                let reach = (npc.template.atk_range + npc.template.radius + HERO_RADIUS) * UU_TO_M;
                if to.length() > reach {
                    tr.translation += to.normalize() * npc.template.run_speed * UU_TO_M * dt;
                    tr.translation.y = crate::mmo::settle(tr.translation, &streamer, &world);
                    if let Some(a) = anim.as_mut() {
                        a.play(&["Run", "Walk"], true);
                    }
                } else {
                    npc.atk_timer -= dt;
                    if npc.atk_timer <= 0.0 {
                        npc.atk_timer = 500.0 / npc.template.atk_speed.max(1.0);
                        if let Some(a) = anim.as_mut() {
                            a.restart(&["atk01", "Atk01"], false);
                        }
                        let Some((dmg, crit)) =
                            crate::formulas::physical(&npc.template.combat(), &stats.combat(), 0.0, 1.0, false, &mut rng)
                        else {
                            log.push(format!("{} misses you.", npc.template.name));
                            continue;
                        };
                        stats.hp -= dmg;
                        log.push(format!("{} hits you for {:.0}{}", npc.template.name, dmg, if crit { " (critical)" } else { "" }));
                    } else if let Some(a) = anim.as_mut() {
                        if a.finished() || a.looping {
                            a.play(&["AtkWait", "Wait"], true);
                        }
                    }
                }
            }
            NpcState::Return => {
                let to = (npc.home - tr.translation).with_y(0.0);
                tr.rotation = Quat::from_rotation_y((-to.z).atan2(to.x));
                tr.translation += to.normalize_or_zero() * (npc.template.walk_speed * 2.0 * UU_TO_M * dt).min(to.length());
                tr.translation.y = crate::mmo::settle(tr.translation, &streamer, &world);
                if let Some(a) = anim.as_mut() {
                    a.play(&["Walk", "Run"], true);
                }
            }
            _ => {
                // Monsters stroll around their spawn point; bosses and
                // their escorts hold position.
                if npc.template.is_boss() || npc.leader.is_some() {
                    npc.wander_timer = f32::MAX;
                }
                npc.wander_timer -= dt;
                if npc.wander.is_none() && npc.wander_timer <= 0.0 {
                    let a = rng.random_range(0.0..std::f32::consts::TAU);
                    let r = rng.random_range(1.5..5.0);
                    npc.wander = Some(npc.home + Vec3::new(a.cos() * r, 0.0, a.sin() * r));
                }
                if let Some(goal) = npc.wander {
                    let to = (goal - tr.translation).with_y(0.0);
                    if to.length() < 0.2 {
                        npc.wander = None;
                        npc.wander_timer = rng.random_range(5.0..15.0);
                    } else {
                        tr.rotation = Quat::from_rotation_y((-to.z).atan2(to.x));
                        tr.translation += to.normalize() * (npc.template.walk_speed * UU_TO_M * dt).min(to.length());
                        tr.translation.y = crate::mmo::settle(tr.translation, &streamer, &world);
                        if let Some(a) = anim.as_mut() {
                            a.play(&["Walk", "Run"], true);
                        }
                        continue;
                    }
                }
                if let Some(a) = anim.as_mut() {
                    a.play(&["Wait"], true);
                }
            }
        }
    }
}

/// Corpses vanish after a few seconds and come back after the datapack
/// respawn time.
fn npc_respawn(
    mut commands: Commands,
    time: Res<Time>,
    mut spawner: ResMut<NpcSpawner>,
    dp: Res<Datapack>,
    streamer: Res<TileStreamer>,
    world: Res<CollisionWorld>,
    mut target: ResMut<Target>,
    mut npcs: Query<(Entity, &mut Npc)>,
) {
    let dt = time.delta_secs();
    for (e, mut npc) in &mut npcs {
        if npc.state != NpcState::Dead {
            continue;
        }
        npc.dead_timer += dt;
        if npc.dead_timer > 6.0 {
            if npc.spawn != NO_RESPAWN {
                let respawn = dp.spawns.get(npc.spawn).map_or(60.0, |s| s.respawn).clamp(10.0, 600.0);
                spawner.respawns.push((npc.spawn, respawn));
            }
            if target.0 == Some(e) {
                target.0 = None;
            }
            commands.entity(e).despawn();
        }
    }
    let mut due = Vec::new();
    spawner.respawns.retain_mut(|(idx, t)| {
        *t -= dt;
        if *t <= 0.0 {
            due.push(*idx);
            false
        } else {
            true
        }
    });
    for idx in due {
        spawn_npc(&mut commands, &mut spawner, &dp, &streamer, &world, idx);
    }
}

#[derive(Component)]
struct DeathWindow;

/// Death as in the client: the hero lies until "To Village" (Enter / A or
/// the button) restarts at the nearest town.
#[allow(clippy::too_many_arguments)]
fn hero_regen_and_death(
    mut commands: Commands,
    time: Res<Time>,
    dp: Res<Datapack>,
    input: Res<crate::input::PlayerInput>,
    mut log: ResMut<Log>,
    mut streamer: ResMut<TileStreamer>,
    mut revive: ResMut<ReviveClicked>,
    windows: Query<Entity, With<DeathWindow>>,
    hero: Single<(&Hero, &mut Transform, &mut HeroStats)>,
    mut anims: Query<&mut Animated>,
    mut ua: crate::l2ui::UiAssets,
    mut clicks: ResMut<crate::l2ui::UiClicks>,
    mut died: Local<bool>,
) {
    let dt = time.delta_secs();
    let (h, mut tr, mut stats) = hero.into_inner();
    // Tests: `L2_DIE` kills the hero once, a few seconds in.
    if !*died && std::env::var_os("L2_DIE").is_some() && time.elapsed_secs() > 8.0 {
        *died = true;
        stats.hp = 0.0;
    }
    if stats.hp <= 0.0 {
        if stats.dead_timer == 0.0 {
            log.push("You have died.");
            stats.attacking = false;
            revive.0 = false;
            // The client's restart menu, with only "To Village" offered.
            let screen = ua.screen();
            let framed = ua.iface.as_deref().and_then(|i| {
                let keep = |n: &str| n.starts_with("btn") && n != "btnVillage" || n.contains("Penalty");
                crate::l2ui::spawn_window(&mut commands, i, &i.0, "RestartMenuWnd", screen, &keep, &mut ua.tex, &mut ua.lib, &mut ua.images)
            });
            if let Some(root) = framed {
                commands.entity(root).insert(DeathWindow).entry::<Node>().and_modify(|mut n| n.height = Val::Px(43.0));
            }
            if framed.is_none() {
            commands
                .spawn((
                    DeathWindow,
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Percent(40.0),
                        top: Val::Percent(35.0),
                        width: Val::Px(300.0),
                        flex_direction: FlexDirection::Column,
                        align_items: AlignItems::Center,
                        padding: UiRect::all(Val::Px(12.0)),
                        row_gap: Val::Px(10.0),
                        ..default()
                    },
                    BackgroundColor(Color::srgba(0.05, 0.04, 0.07, 0.92)),
                ))
                .with_children(|p| {
                    p.spawn((Text::new("You have died."), TextFont { font_size: FontSize::Px(20.0), ..default() }));
                    p.spawn((
                        Node { padding: UiRect::axes(Val::Px(16.0), Val::Px(6.0)), ..default() },
                        BackgroundColor(Color::srgba(0.35, 0.28, 0.1, 0.9)),
                    ))
                    .with_children(|b| {
                        b.spawn((Text::new("To Village"), TextFont { font_size: FontSize::Px(18.0), ..default() }));
                    })
                    .observe(|_: On<bevy::picking::events::PointerClick>, mut flag: ResMut<ReviveClicked>| flag.0 = true);
                });
            }
        }
        if clicks.take("RestartMenuWnd", "btnVillage") {
            revive.0 = true;
        }
        stats.dead_timer += dt;
        if let Ok(mut a) = anims.get_mut(h.actor) {
            a.play(&["Death"], false);
        }
        let auto = std::env::var_os("L2_AUTOFIGHT").is_some() && stats.dead_timer > 4.0;
        if (input.confirm || auto || revive.0) && stats.dead_timer > 0.5 {
            revive.0 = false;
            stats.dead_timer = 0.0;
            stats.hp = stats.max_hp * 0.7;
            stats.mp = stats.max_mp * 0.7;
            // Nearest town restart point, else where the hero started.
            let here = crate::streaming::bevy_to_ue(tr.translation);
            let d2 = |p: &[f32; 3]| (p[0] - here[0]).powi(2) + (p[1] - here[1]).powi(2);
            let town = dp.restart_points.iter().min_by(|a, b| d2(&a.1).total_cmp(&d2(&b.1)));
            let pos = town.map_or(stats.respawn, |(_, p)| ue_to_bevy(*p));
            tr.translation = pos;
            streamer.request_around(pos);
            if let Some((name, _)) = town {
                log.push(format!("You have been returned to {name}."));
            }
            for e in &windows {
                commands.entity(e).despawn();
            }
        }
        return;
    }
    // Sitting regenerates 1.5x faster, as in the client.
    let rate = if stats.sitting { 0.75 } else { 0.5 };
    stats.hp = (stats.hp + stats.hp_regen * dt * rate).min(stats.max_hp);
    stats.mp = (stats.mp + stats.mp_regen * dt * rate).min(stats.max_mp);
}

// ---------------------------------------------------------------------------
// HUD
// ---------------------------------------------------------------------------

#[derive(Component)]
struct Bar(BarKind);

#[derive(Clone, Copy, PartialEq)]
enum BarKind {
    Hp,
    Mp,
    Exp,
    TargetHp,
}

#[derive(Component)]
struct BarLabel(BarKind);

#[derive(Component)]
struct TargetWindow;

#[derive(Component)]
struct LogText;

pub fn spawn_hud(commands: &mut Commands) {
    let bar = |p: &mut ChildSpawnerCommands, kind: BarKind, color: Color, width: f32| {
        p.spawn((Node { width: Val::Px(width), height: Val::Px(16.0), ..default() }, BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.6))))
            .with_children(|b| {
                b.spawn((Bar(kind), Node { width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() }, BackgroundColor(color)));
                b.spawn((
                    BarLabel(kind),
                    Text::new(""),
                    TextFont { font_size: FontSize::Px(12.0), ..default() },
                    Node { position_type: PositionType::Absolute, left: Val::Px(6.0), ..default() },
                ));
            });
    };
    // Status window (top-left).
    commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            top: Val::Px(36.0),
            left: Val::Px(10.0),
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(3.0),
            ..default()
        })
        .with_children(|p| {
            bar(p, BarKind::Hp, Color::srgb(0.75, 0.12, 0.1), 240.0);
            bar(p, BarKind::Mp, Color::srgb(0.12, 0.3, 0.8), 240.0);
            bar(p, BarKind::Exp, Color::srgb(0.85, 0.75, 0.2), 240.0);
        });
    // Target window (top-center).
    commands
        .spawn((
            TargetWindow,
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(10.0),
                width: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                ..default()
            },
            Visibility::Hidden,
        ))
        .with_children(|p| {
            bar(p, BarKind::TargetHp, Color::srgb(0.75, 0.12, 0.1), 280.0);
        });
    // System messages (bottom-left).
    commands.spawn((
        LogText,
        Text::new(""),
        TextFont { font_size: FontSize::Px(15.0), ..default() },
        Node { position_type: PositionType::Absolute, bottom: Val::Px(12.0), left: Val::Px(12.0), ..default() },
    ));
}

#[allow(clippy::type_complexity)]
fn update_hud(
    target: Res<Target>,
    log: Res<Log>,
    dp: Res<Datapack>,
    hero: Single<&HeroStats>,
    npcs: Query<&Npc>,
    mut bars: Query<(&Bar, &mut Node)>,
    mut labels: Query<(&BarLabel, &mut Text), Without<LogText>>,
    mut window: Single<&mut Visibility, With<TargetWindow>>,
    mut log_text: Single<&mut Text, With<LogText>>,
) {
    let s = *hero;
    let cur = dp.exp.get(s.level as usize).copied().unwrap_or(0);
    let next = dp.exp.get(s.level as usize + 1).copied().unwrap_or(cur + 1);
    let exp_frac = (s.exp.saturating_sub(cur)) as f32 / (next.saturating_sub(cur)).max(1) as f32;
    let tgt = target.0.and_then(|e| npcs.get(e).ok());
    for (bar, mut node) in &mut bars {
        let frac = match bar.0 {
            BarKind::Hp => s.hp / s.max_hp.max(1.0),
            BarKind::Mp => s.mp / s.max_mp.max(1.0),
            BarKind::Exp => exp_frac,
            BarKind::TargetHp => tgt.map_or(0.0, |n| n.hp / n.template.hp.max(1.0)),
        };
        node.width = Val::Percent(frac.clamp(0.0, 1.0) * 100.0);
    }
    for (label, mut text) in &mut labels {
        text.0 = match label.0 {
            BarKind::Hp => format!("HP {:.0} / {:.0}", s.hp.max(0.0), s.max_hp),
            BarKind::Mp => format!("MP {:.0} / {:.0}", s.mp, s.max_mp),
            BarKind::Exp => format!("Lv {}  EXP {:.2}%  SP {}", s.level, exp_frac * 100.0, s.sp),
            BarKind::TargetHp => tgt.map_or(String::new(), |n| {
                format!("{} (Lv {})  {:.0} / {:.0}", n.template.name, n.template.level, n.hp.max(0.0), n.template.hp)
            }),
        };
    }
    **window = if tgt.is_some() { Visibility::Inherited } else { Visibility::Hidden };
    log_text.0 = log.0.iter().cloned().collect::<Vec<_>>().join("\n");
}

/// `L2_DEBUG_NPC`: every 5 s, log the nearest monsters to the hero.
pub fn debug_npcs(
    time: Res<Time>,
    mut timer: Local<f32>,
    hero: Single<&Transform, With<Hero>>,
    npcs: Query<(&Npc, &Transform), Without<Hero>>,
) {
    if std::env::var_os("L2_DEBUG_NPC").is_none() {
        return;
    }
    *timer -= time.delta_secs();
    if *timer > 0.0 {
        return;
    }
    *timer = 5.0;
    let mut list: Vec<_> = npcs.iter().map(|(n, t)| (t.translation.distance(hero.translation), n, t.translation)).collect();
    list.sort_by(|a, b| a.0.total_cmp(&b.0));
    info!("hero at {:?}, {} npcs", hero.translation, list.len());
    for (d, n, p) in list.iter().take(6) {
        info!("  {} ({}, {}) at {:.1} m, y {:.1} vs hero {:.1}, model {}", n.template.name, n.template.id, n.template.kind, d, p.y, hero.translation.y, n.actor.is_some());
    }
}
