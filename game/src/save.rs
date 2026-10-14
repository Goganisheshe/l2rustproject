//! Save game: the hero's class, progress, position, inventory, skills and
//! quests in `saves/hero.json` (or `L2_SAVE`). Saved every minute, on F5 and
//! on exit; C / gamepad Y on the character screen continues.

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

fn path() -> PathBuf {
    std::env::var_os("L2_SAVE").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("saves/hero.json"))
}

pub fn read() -> Option<SaveGame> {
    let src = std::fs::read_to_string(path()).ok()?;
    serde_json::from_str(&src).map_err(|e| warn!("save file: {e}")).ok()
}

/// A save chosen on the character screen, applied when the hero spawns.
#[derive(Resource, Default)]
struct PendingLoad(Option<SaveGame>);

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
            .add_systems(Update, continue_game.run_if(in_state(Phase::Create)))
            .add_systems(
                Update,
                (restore.before(crate::items::give_starting_items), save_game).chain().run_if(in_state(Phase::InWorld)),
            )
            .add_systems(Last, save_on_exit);
    }
}

fn continue_game(
    keys: Res<ButtonInput<KeyCode>>,
    gamepads: Query<&Gamepad>,
    mut pending: ResMut<PendingLoad>,
    mut c: ResMut<Creation>,
    mut next: ResMut<NextState<Phase>>,
    mut hint: Local<Option<Entity>>,
    mut commands: Commands,
    dp: Res<Datapack>,
) {
    let wanted = keys.just_pressed(KeyCode::KeyC)
        || gamepads.iter().any(|g| g.just_pressed(GamepadButton::North))
        || std::env::var_os("L2_CONTINUE").is_some();
    if hint.is_none() {
        let text = match read() {
            Some(s) => format!("C / Y: continue {} Lv {}", dp.class_name(s.class_id), s.level),
            None => String::new(),
        };
        *hint = Some(
            commands
                .spawn((
                    Text::new(text),
                    TextFont { font_size: FontSize::Px(18.0), ..default() },
                    Node { position_type: PositionType::Absolute, bottom: Val::Px(30.0), left: Val::Px(48.0), ..default() },
                    DespawnOnExit(Phase::Create),
                ))
                .id(),
        );
    }
    if !wanted || pending.0.is_some() {
        return;
    }
    let Some(save) = read() else { return };
    c.index = save.class_index;
    *START_OVERRIDE.lock().unwrap() = Some(save.pos);
    pending.0 = Some(save);
    next.set(Phase::Loading);
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
