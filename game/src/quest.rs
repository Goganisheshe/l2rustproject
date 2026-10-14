//! Quest engine modelled on L2J's `Quest` scripts: per-quest state with a
//! condition step and variables, NPC talk / event / kill handlers that return
//! the HTML page to show. Quest scripts live in `quests/`, one per datapack
//! quest, ported from its Java.

use std::collections::HashMap;

use bevy::prelude::*;

use crate::character::CLASSES;
use crate::datapack::Datapack;
use crate::items::{Inventory, ItemData, ADENA};
use crate::rpg::{HeroStats, Log};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum QState {
    #[default]
    Created,
    Started,
    Completed,
}

#[derive(Clone, Debug, Default)]
pub struct QuestState {
    pub state: QState,
    pub cond: u32,
    pub vars: HashMap<String, i64>,
}

impl QuestState {
    pub fn is_created(&self) -> bool {
        self.state == QState::Created
    }
    pub fn is_started(&self) -> bool {
        self.state == QState::Started
    }
    pub fn is_completed(&self) -> bool {
        self.state == QState::Completed
    }
    pub fn is_cond(&self, c: u32) -> bool {
        self.is_started() && self.cond == c
    }
    pub fn get_int(&self, k: &str) -> i64 {
        self.vars.get(k).copied().unwrap_or(0)
    }
    pub fn set(&mut self, k: &str, v: i64) {
        self.vars.insert(k.to_string(), v);
    }
    pub fn unset(&mut self, k: &str) {
        self.vars.remove(k);
    }
    pub fn start_quest(&mut self) {
        self.state = QState::Started;
        self.cond = 1;
    }
    pub fn set_cond(&mut self, c: u32) {
        self.cond = c;
    }
    /// Finishes the quest; a repeatable one can be taken again.
    pub fn exit_quest(&mut self, repeatable: bool) {
        self.state = if repeatable { QState::Created } else { QState::Completed };
        self.cond = 0;
        self.vars.clear();
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Race {
    Human,
    Elf,
    DarkElf,
    Orc,
    Dwarf,
    Kamael,
}

/// Quest progress of the hero.
#[derive(Resource, Default)]
pub struct QuestLog {
    pub states: HashMap<u32, QuestState>,
}

/// The hero as quest scripts see it.
pub struct Player<'a> {
    pub stats: &'a mut HeroStats,
    pub inv: &'a mut Inventory,
    /// Index into `CLASSES`.
    pub class: usize,
    pub items: &'a ItemData,
    pub dp: &'a Datapack,
    pub log: &'a mut Log,
    /// Teleport requested by the script (Unreal units).
    pub teleport: Option<[f32; 3]>,
    /// Large on-screen message.
    pub screen: Option<String>,
    /// The script opened the class change window.
    pub class_change: bool,
}

impl Player<'_> {
    pub fn level(&self) -> u32 {
        self.stats.level
    }
    pub fn class_id(&self) -> u32 {
        self.stats.template.class_id
    }
    pub fn race(&self) -> Race {
        let p = CLASSES[self.class].prefix;
        if p.contains("DarkElf") {
            Race::DarkElf
        } else if p.contains("Elf") {
            Race::Elf
        } else if p.contains("Orc") || p.contains("Shaman") {
            Race::Orc
        } else if p.contains("Dwarf") {
            Race::Dwarf
        } else if p.contains("Kamael") {
            Race::Kamael
        } else {
            Race::Human
        }
    }
    pub fn is_mage(&self) -> bool {
        let p = CLASSES[self.class].prefix;
        p.contains("Magic") || p.contains("Shaman") || matches!(self.class_id(), 25..=30 | 38..=43)
    }
    pub fn count(&self, item: u32) -> u64 {
        self.inv.items.get(&item).copied().unwrap_or(0)
    }
    pub fn has(&self, item: u32) -> bool {
        self.count(item) > 0
    }
    pub fn give(&mut self, item: u32, count: u64) {
        if count == 0 {
            return;
        }
        *self.inv.items.entry(item).or_default() += count;
        let name = self.items.items.get(&item).map_or(format!("item {item}"), |t| t.name.clone());
        if item == ADENA {
            self.log.push(format!("You earned {count} Adena"));
        } else if count > 1 {
            self.log.push(format!("You earned {name} x{count}"));
        } else {
            self.log.push(format!("You earned {name}"));
        }
    }
    /// Takes up to `count` (or all with -1).
    pub fn take(&mut self, item: u32, count: i64) {
        let have = self.count(item);
        let n = if count < 0 { have } else { (count as u64).min(have) };
        if n == 0 {
            return;
        }
        if n == have {
            self.inv.items.remove(&item);
            self.inv.equipped.retain(|_, v| *v != item);
        } else {
            *self.inv.items.get_mut(&item).unwrap() -= n;
        }
    }
    pub fn add_exp_sp(&mut self, exp: u64, sp: u64) {
        self.stats.exp += exp;
        self.stats.sp += sp;
        self.log.push(format!("You earned {exp} XP and {sp} SP"));
        let new_level = self.dp.level_for(self.stats.exp);
        if new_level > self.stats.level {
            self.stats.level = new_level;
            self.stats.recalc();
            self.stats.hp = self.stats.max_hp;
            self.stats.mp = self.stats.max_mp;
            self.log.push(format!("Level up! You are now level {new_level}"));
        }
    }
    pub fn tele(&mut self, x: f32, y: f32, z: f32) {
        self.teleport = Some([x, y, z]);
    }
    pub fn screen_message(&mut self, text: &str) {
        let text = text.replace('#', "\n");
        self.log.push(text.replace('\n', " "));
        self.screen = Some(text);
    }
}

pub struct Ctx<'a, 'p> {
    pub p: &'a mut Player<'p>,
    pub qs: &'a mut QuestState,
    /// NPC being talked to (0 for kills).
    pub npc: u32,
}

/// A ported datapack quest.
pub trait Quest: Send + Sync {
    fn id(&self) -> u32;
    /// Script directory, e.g. `Q10961_EffectiveTraining` (holds its pages).
    fn dir(&self) -> &'static str;
    fn title(&self) -> &'static str;
    fn start_npcs(&self) -> Vec<u32>;
    fn talk_npcs(&self) -> Vec<u32>;
    fn kill_ids(&self) -> Vec<u32> {
        Vec::new()
    }
    /// Items removed when the quest is aborted or finished.
    fn quest_items(&self) -> Vec<u32> {
        Vec::new()
    }
    /// NPCs whose first page this script provides.
    fn first_talk_npcs(&self) -> Vec<u32> {
        Vec::new()
    }
    fn on_first_talk(&self, _c: &mut Ctx) -> Option<String> {
        None
    }
    /// Start condition: `Err(page)` when the hero may not take the quest.
    fn can_start(&self, _p: &Player) -> Result<(), &'static str> {
        Ok(())
    }
    fn on_event(&self, c: &mut Ctx, event: &str) -> Option<String>;
    fn on_talk(&self, c: &mut Ctx) -> String;
    fn on_kill(&self, _c: &mut Ctx, _npc: u32) {}
    /// Progress line for the quest window.
    fn progress(&self, _qs: &QuestState) -> Option<String> {
        None
    }
}

/// Helper for `can_start`: level range check returning the given page.
pub fn level_range(p: &Player, min: u32, max: u32, page: &'static str) -> Result<(), &'static str> {
    if p.level() < min || p.level() > max { Err(page) } else { Ok(()) }
}

pub const NO_QUEST: &str = "<<noquest>>";
pub const ALREADY_COMPLETED: &str = "<<completed>>";

#[derive(Resource)]
pub struct Quests(pub Vec<Box<dyn Quest>>);

impl Quests {
    pub fn by_dir(&self, dir: &str) -> Option<&dyn Quest> {
        self.0.iter().find(|q| q.dir().eq_ignore_ascii_case(dir)).map(|q| q.as_ref())
    }
    pub fn by_id(&self, id: u32) -> Option<&dyn Quest> {
        self.0.iter().find(|q| q.id() == id).map(|q| q.as_ref())
    }
}

/// Talks to `npc` about quest `q`, applying its start condition.
pub fn talk(q: &dyn Quest, log: &mut QuestLog, p: &mut Player, npc: u32) -> String {
    let qs = log.states.entry(q.id()).or_default();
    if qs.is_created() && q.start_npcs().contains(&npc) {
        if let Err(page) = q.can_start(p) {
            return page.to_string();
        }
    }
    q.on_talk(&mut Ctx { p, qs, npc })
}

pub fn event(q: &dyn Quest, log: &mut QuestLog, p: &mut Player, npc: u32, ev: &str) -> Option<String> {
    let qs = log.states.entry(q.id()).or_default();
    let was_started = qs.is_started();
    let out = q.on_event(&mut Ctx { p, qs, npc }, ev);
    if was_started && !qs.is_started() {
        for item in q.quest_items() {
            p.take(item, -1);
        }
    }
    out
}

/// Quests this NPC takes part in, for the "Quest" link.
pub fn npc_quests<'a>(quests: &'a Quests, log: &QuestLog, npc: u32) -> Vec<&'a dyn Quest> {
    quests
        .0
        .iter()
        .filter(|q| {
            let st = log.states.get(&q.id()).map(|s| s.state).unwrap_or_default();
            (q.start_npcs().contains(&npc) && st != QState::Started) || (q.talk_npcs().contains(&npc) && st == QState::Started)
        })
        .map(|q| q.as_ref())
        .collect()
}

/// Feeds a monster kill to every started quest listening for it.
pub fn kill(quests: &Quests, log: &mut QuestLog, p: &mut Player, npc: u32) {
    for q in &quests.0 {
        if !q.kill_ids().contains(&npc) {
            continue;
        }
        let Some(qs) = log.states.get_mut(&q.id()) else { continue };
        if !qs.is_started() {
            continue;
        }
        let was_started = qs.is_started();
        q.on_kill(&mut Ctx { p, qs, npc: 0 }, npc);
        if was_started && !qs.is_started() {
            for item in q.quest_items() {
                p.take(item, -1);
            }
        }
    }
}
