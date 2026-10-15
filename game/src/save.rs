//! Save games: one file per character, `saves/char<N>.json` (N = the slot on
//! the character select screen; `L2_SAVE` forces one file). Each holds the
//! hero's name, class, progress, position, inventory, skills and quests.
//! Saved every minute, on F5 and on exit.

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::datapack::Datapack;
use crate::items::Inventory;
use crate::mmo::{Creation, Hero, Phase, START_OVERRIDE};
use crate::quest::{QState, QuestLog, QuestState};
use crate::rpg::{HeroStats, Log};
use crate::skillplay::HeroSkills;
use crate::streaming::{bevy_to_ue, ue_to_bevy};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SaveGame {
    pub version: u32,
    #[serde(default)]
    pub name: String,
    /// Index into `CLASSES` (race, sex and look).
    pub class_index: usize,
    pub class_id: u32,
    pub level: u32,
    pub exp: u64,
    pub sp: u64,
    pub hp: f32,
    pub mp: f32,
    /// Unreal units.
    pub pos: [f32; 3],
    pub respawn: [f32; 3],
    pub items: BTreeMap<u32, u64>,
    pub equipped: BTreeMap<String, u32>,
    #[serde(default)]
    pub auto_shots: Vec<u32>,
    pub skills: BTreeMap<u32, u32>,
    pub hotbar: Vec<u32>,
    /// quest id -> (state 0 created / 1 started / 2 completed, cond, vars).
    pub quests: BTreeMap<u32, (u8, u32, HashMap<String, i64>)>,
}

/// Character slots on the select screen, as in the client.
pub const SLOTS: usize = 7;

/// Slot of the character being played.
pub static SLOT: std::sync::Mutex<usize> = std::sync::Mutex::new(0);

fn slot_path(slot: usize) -> PathBuf {
    PathBuf::from(format!("saves/char{slot}.json"))
}

fn path() -> PathBuf {
    std::env::var_os("L2_SAVE").map(PathBuf::from).unwrap_or_else(|| slot_path(*SLOT.lock().unwrap()))
}

fn read_file(p: &std::path::Path) -> Option<SaveGame> {
    let src = std::fs::read_to_string(p).ok()?;
    serde_json::from_str(&src).map_err(|e| warn!("save file {}: {e}", p.display())).ok()
}

/// The characters in their slots. A save from before slots
/// (`saves/hero.json`) becomes the first free slot.
pub fn list() -> Vec<Option<SaveGame>> {
    let legacy = PathBuf::from("saves/hero.json");
    let mut out: Vec<Option<SaveGame>> = (0..SLOTS).map(|i| read_file(&slot_path(i))).collect();
    if legacy.exists() {
        if let Some(free) = out.iter().position(|s| s.is_none()) {
            if std::fs::rename(&legacy, slot_path(free)).is_ok() {
                out[free] = read_file(&slot_path(free));
            }
        }
    }
    for s in out.iter_mut().flatten() {
        if s.name.is_empty() {
            s.name = "Hero".into();
        }
    }
    out
}

/// Deletes a character for good.
pub fn delete(slot: usize) {
    let _ = std::fs::remove_file(slot_path(slot));
}

/// A save chosen on the character screen, applied when the hero spawns.
#[derive(Resource, Default)]
pub struct PendingLoad(pub Option<SaveGame>);

#[derive(Resource)]
struct AutoSave(Timer);

pub struct SavePlugin;

impl Plugin for SavePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PendingLoad>()
            .insert_resource(AutoSave(Timer::from_seconds(
                std::env::var("L2_AUTOSAVE_SECS").ok().and_then(|v| v.parse().ok()).unwrap_or(60.0),
                TimerMode::Repeating,
            )))
            .add_systems(
                Update,
                (restore.before(crate::items::give_starting_items), save_game).chain().run_if(in_state(Phase::InWorld)),
            )
            .add_systems(Last, save_on_exit);
    }
}

/// Starts playing the character in `slot`.
pub fn load_slot(slot: usize, save: SaveGame, c: &mut Creation, pending: &mut PendingLoad) {
    *SLOT.lock().unwrap() = slot;
    c.index = save.class_index.min(crate::character::CLASSES.len() - 1);
    c.name = save.name.clone();
    *START_OVERRIDE.lock().unwrap() = Some(save.pos);
    pending.0 = Some(save);
}

/// Applies a pending save once the hero and its components exist.
#[allow(clippy::type_complexity)]
fn restore(
    mut commands: Commands,
    mut pending: ResMut<PendingLoad>,
    dp: Res<Datapack>,
    mut qlog: ResMut<QuestLog>,
    mut log: ResMut<Log>,
    hero: Single<(Entity, &mut HeroStats, Option<&mut Inventory>), With<Hero>>,
) {
    let Some(save) = pending.0.take() else { return };
    let (e, mut stats, inv) = hero.into_inner();
    if let Some(t) = dp.templates_by_id.get(&save.class_id) {
        let mut t = t.clone();
        t.creation_points = stats.template.creation_points.clone();
        stats.template = t;
    }
    stats.class_key = dp.class_name(save.class_id).to_string();
    stats.level = save.level;
    stats.exp = save.exp;
    stats.sp = save.sp;
    stats.respawn = ue_to_bevy(save.respawn);
    stats.recalc();
    stats.hp = save.hp.min(stats.max_hp).max(1.0);
    stats.mp = save.mp.min(stats.max_mp);
    let restored = Inventory {
        items: save.items.clone(),
        equipped: save.equipped.clone(),
        auto_shots: save.auto_shots.iter().copied().collect(),
    };
    match inv {
        Some(mut inv) => *inv = restored,
        None => {
            commands.entity(e).insert(restored);
        }
    }
    commands.entity(e).insert(HeroSkills::restored(save.skills.clone(), save.hotbar.clone(), save.level, save.sp, save.class_id));
    qlog.states = save
        .quests
        .iter()
        .map(|(id, (st, cond, vars))| {
            let state = match st {
                1 => QState::Started,
                2 => QState::Completed,
                _ => QState::Created,
            };
            (*id, QuestState { state, cond: *cond, vars: vars.clone() })
        })
        .collect();
    log.push("Game loaded.");
}

pub fn snapshot(
    c: &Creation,
    stats: &HeroStats,
    tr: &Transform,
    inv: Option<&Inventory>,
    skills: Option<&HeroSkills>,
    qlog: &QuestLog,
) -> SaveGame {
    SaveGame {
        version: 1,
        name: c.name.clone(),
        class_index: c.index,
        class_id: stats.template.class_id,
        level: stats.level,
        exp: stats.exp,
        sp: stats.sp,
        hp: stats.hp,
        mp: stats.mp,
        pos: bevy_to_ue(tr.translation),
        respawn: bevy_to_ue(stats.respawn),
        items: inv.map(|i| i.items.clone()).unwrap_or_default(),
        equipped: inv.map(|i| i.equipped.clone()).unwrap_or_default(),
        auto_shots: inv.map(|i| i.auto_shots.iter().copied().collect()).unwrap_or_default(),
        skills: skills.map(|s| s.learned.clone()).unwrap_or_default(),
        hotbar: skills.map(|s| s.hotbar.clone()).unwrap_or_default(),
        quests: qlog
            .states
            .iter()
            .map(|(id, q)| {
                let st = match q.state {
                    QState::Created => 0,
                    QState::Started => 1,
                    QState::Completed => 2,
                };
                (*id, (st, q.cond, q.vars.clone()))
            })
            .collect(),
    }
}

pub fn write(save: &SaveGame) {
    let p = path();
    if let Some(dir) = p.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    match serde_json::to_string_pretty(save) {
        Ok(s) => {
            if let Err(e) = std::fs::write(&p, s) {
                warn!("could not save to {}: {e}", p.display());
            }
        }
        Err(e) => warn!("could not serialize save: {e}"),
    }
}

#[allow(clippy::too_many_arguments)]
fn save_game(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    mut timer: ResMut<AutoSave>,
    pending: Res<PendingLoad>,
    c: Res<Creation>,
    qlog: Res<QuestLog>,
    mut log: ResMut<Log>,
    hero: Single<(&HeroStats, &Transform, Option<&Inventory>, Option<&HeroSkills>), With<Hero>>,
) {
    let manual = keys.just_pressed(KeyCode::F5);
    if !timer.0.tick(time.delta()).just_finished() && !manual {
        return;
    }
    let (stats, tr, inv, skills) = hero.into_inner();
    // Not before the starting kit / a pending load is in place.
    if pending.0.is_some() || inv.is_none() || stats.hp <= 0.0 {
        return;
    }
    write(&snapshot(&c, stats, tr, inv, skills, &qlog));
    if manual {
        log.push("Game saved.");
    }
}

fn save_on_exit(
    mut exits: MessageReader<AppExit>,
    state: Option<Res<State<Phase>>>,
    c: Option<Res<Creation>>,
    qlog: Option<Res<QuestLog>>,
    hero: Query<(&HeroStats, &Transform, Option<&Inventory>, Option<&HeroSkills>), With<Hero>>,
) {
    if exits.read().next().is_none() || std::env::var_os("L2_SCREENSHOT").is_some() {
        return;
    }
    if state.is_none_or(|s| *s.get() != Phase::InWorld) {
        return;
    }
    let (Some(c), Some(qlog)) = (c, qlog) else { return };
    if let Ok((stats, tr, inv, skills)) = hero.single() {
        if inv.is_some() {
            write(&snapshot(&c, stats, tr, inv, skills, &qlog));
        }
    }
}
