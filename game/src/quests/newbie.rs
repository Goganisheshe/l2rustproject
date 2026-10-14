//! Classic newbie quest chain, ported from L2J Mobius.
//!
//! Every race has two hunting quests: the Newbie Guide sends the hero to
//! hunt near the village and report to a trader (`FirstHunt`: Q10961, Q10981,
//! Q10983, Q10985, Q10987, Q10989), then the trader sends them to hunt and
//! report to Captain Bathis in Gludio, who hands out an armor set
//! (`SecondHunt`: Q10962, Q10982, Q10984, Q10986, Q10988, Q10990). The Java
//! sources of each family are identical but for constants, which
//! `tools/newbie_extract.py` pulls into `newbie_data.rs`.

use crate::quest::{Ctx, Player, Quest, QuestState, ALREADY_COMPLETED, NO_QUEST};

#[derive(Clone)]
pub struct FirstHunt {
    pub id: u32,
    pub dir: &'static str,
    pub title: &'static str,
    pub guide: u32,
    pub trader: u32,
    pub kills: &'static [u32],
    pub kill_target: i64,
    /// Scroll of Escape back to the trader, given when the hunt is done.
    pub soe: (u32, u64),
    pub max_level: u32,
    pub teleport: [f32; 3],
    pub message: &'static str,
    pub start_event: &'static str,
    pub reward_event: &'static str,
    pub exp: u64,
    pub sp: u64,
    pub rewards: &'static [(u32, u64)],
    /// Talk pages: not started, guide while hunting, trader when done.
    pub pages: [&'static str; 3],
}

#[derive(Clone)]
pub struct SecondHunt {
    pub id: u32,
    pub dir: &'static str,
    pub title: &'static str,
    pub giver: u32,
    pub captain: u32,
    pub kills: &'static [u32],
    pub kill_target: i64,
    /// Quest item collected per kill instead of a kill count: (item, needed).
    pub collect: Option<(u32, u64)>,
    pub soe: (u32, u64),
    pub max_level: u32,
    pub teleport: [f32; 3],
    pub message: &'static str,
    pub start_event: &'static str,
    /// Pages shown as-is.
    pub plain_events: &'static [&'static str],
    pub next_quest: &'static str,
    pub exp: u64,
    pub sp: u64,
    /// Armor choice pages and what each gives.
    pub choices: &'static [(&'static str, &'static [(u32, u64)])],
    /// Talk pages: not started, giver while hunting, captain when done.
    pub pages: [&'static str; 3],
}

const KILL_COUNT_VAR: &str = "KillCount";

include!("newbie_data.rs");

impl Quest for FirstHunt {
    fn id(&self) -> u32 {
        self.id
    }
    fn dir(&self) -> &'static str {
        self.dir
    }
    fn title(&self) -> &'static str {
        self.title
    }
    fn start_npcs(&self) -> Vec<u32> {
        vec![self.guide]
    }
    fn talk_npcs(&self) -> Vec<u32> {
        vec![self.guide, self.trader]
    }
    fn kill_ids(&self) -> Vec<u32> {
        self.kills.to_vec()
    }
    fn quest_items(&self) -> Vec<u32> {
        vec![self.soe.0]
    }
    fn can_start(&self, p: &Player) -> Result<(), &'static str> {
        if p.level() > self.max_level { Err("no_lvl.html") } else { Ok(()) }
    }

    fn on_event(&self, c: &mut Ctx, event: &str) -> Option<String> {
        if event == "TELEPORT_TO_HUNTING_GROUND" {
            let [x, y, z] = self.teleport;
            c.p.tele(x, y, z);
            return None;
        }
        if event == self.start_event {
            c.qs.start_quest();
            return Some(event.to_string());
        }
        if event == self.reward_event && c.qs.is_started() {
            c.p.add_exp_sp(self.exp, self.sp);
            for &(item, n) in self.rewards {
                c.p.give(item, n);
            }
            c.qs.exit_quest(false);
            return Some(event.to_string());
        }
        None
    }

    fn on_kill(&self, c: &mut Ctx, _npc: u32) {
        if !c.qs.is_cond(1) {
            return;
        }
        let count = c.qs.get_int(KILL_COUNT_VAR) + 1;
        if count < self.kill_target {
            c.qs.set(KILL_COUNT_VAR, count);
        } else {
            c.qs.set_cond(2);
            c.qs.unset(KILL_COUNT_VAR);
            c.p.screen_message(self.message);
            c.p.give(self.soe.0, self.soe.1);
        }
    }

    fn on_talk(&self, c: &mut Ctx) -> String {
        let qs = &c.qs;
        let page = if qs.is_created() {
            Some(self.pages[0])
        } else if qs.is_started() {
            if c.npc == self.guide && qs.is_cond(1) {
                Some(self.pages[1])
            } else if c.npc == self.trader && qs.is_cond(2) {
                Some(self.pages[2])
            } else {
                None
            }
        } else if c.npc == self.guide {
            Some(ALREADY_COMPLETED)
        } else {
            None
        };
        page.unwrap_or(NO_QUEST).to_string()
    }

    fn progress(&self, qs: &QuestState) -> Option<String> {
        match qs.cond {
            1 => Some(format!("Hunt monsters: {}/{}", qs.get_int(KILL_COUNT_VAR), self.kill_target)),
            2 => Some("Use the Scroll of Escape and report back".into()),
            _ => None,
        }
    }
}

impl Quest for SecondHunt {
    fn id(&self) -> u32 {
        self.id
    }
    fn dir(&self) -> &'static str {
        self.dir
    }
    fn title(&self) -> &'static str {
        self.title
    }
    fn start_npcs(&self) -> Vec<u32> {
        vec![self.giver]
    }
    fn talk_npcs(&self) -> Vec<u32> {
        vec![self.giver, self.captain]
    }
    fn kill_ids(&self) -> Vec<u32> {
        self.kills.to_vec()
    }
    fn quest_items(&self) -> Vec<u32> {
        let mut v = vec![self.soe.0];
        v.extend(self.collect.map(|c| c.0));
        v
    }
    fn can_start(&self, p: &Player) -> Result<(), &'static str> {
        if p.level() > self.max_level { Err("no_lvl.html") } else { Ok(()) }
    }

    fn on_event(&self, c: &mut Ctx, event: &str) -> Option<String> {
        if event == "TELEPORT_TO_HUNTING_GROUND" {
            let [x, y, z] = self.teleport;
            c.p.tele(x, y, z);
            return None;
        }
        if event == "NEXT_QUEST" {
            return Some(self.next_quest.to_string());
        }
        if event == self.start_event {
            c.qs.start_quest();
            return Some(event.to_string());
        }
        if self.plain_events.contains(&event) {
            return Some(event.to_string());
        }
        let (_, items) = self.choices.iter().find(|(e, _)| *e == event)?;
        if !c.qs.is_started() {
            return None;
        }
        c.p.add_exp_sp(self.exp, self.sp);
        for &(item, n) in *items {
            c.p.give(item, n);
        }
        if is_first_class(c.p.class_id()) {
            c.p.screen_message("You've finished the tutorial.#Take your 1st class transfer and complete your training with Bathis to become stronger.");
            c.p.class_change = true;
        }
        c.qs.exit_quest(false);
        Some(event.to_string())
    }

    fn on_kill(&self, c: &mut Ctx, _npc: u32) {
        if !c.qs.is_cond(1) {
            return;
        }
        if let Some((item, need)) = self.collect {
            if c.p.count(item) < need {
                c.p.give(item, 1);
            }
            if c.p.count(item) >= need {
                c.qs.set_cond(2);
                c.p.screen_message(self.message);
                c.p.give(self.soe.0, self.soe.1);
            }
            return;
        }
        let count = c.qs.get_int(KILL_COUNT_VAR) + 1;
        if count < self.kill_target {
            c.qs.set(KILL_COUNT_VAR, count);
        } else {
            c.qs.set_cond(2);
            c.qs.unset(KILL_COUNT_VAR);
            c.p.screen_message(self.message);
            c.p.give(self.soe.0, self.soe.1);
        }
    }

    fn on_talk(&self, c: &mut Ctx) -> String {
        let qs = &c.qs;
        let page = if qs.is_created() {
            Some(self.pages[0])
        } else if qs.is_started() {
            if c.npc == self.giver && qs.is_cond(1) {
                Some(self.pages[1])
            } else if c.npc == self.captain && qs.is_cond(2) {
                Some(self.pages[2])
            } else {
                None
            }
        } else if c.npc == self.giver {
            Some(ALREADY_COMPLETED)
        } else {
            None
        };
        page.unwrap_or(NO_QUEST).to_string()
    }

    fn progress(&self, qs: &QuestState) -> Option<String> {
        match (qs.cond, self.collect) {
            (1, Some((_, need))) => Some(format!("Collect quest items: {need} needed")),
            (1, None) => Some(format!("Hunt monsters: {}/{}", qs.get_int(KILL_COUNT_VAR), self.kill_target)),
            (2, _) => Some("Use the Scroll of Escape and see Captain Bathis in Gludio".into()),
            _ => None,
        }
    }
}

/// L2J `CategoryType.FIRST_CLASS_GROUP`: the starting classes.
pub fn is_first_class(class_id: u32) -> bool {
    matches!(class_id, 0 | 10 | 18 | 25 | 31 | 38 | 44 | 49 | 53 | 123 | 124)
}
