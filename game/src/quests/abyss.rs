//! Daily quests of the Dungeon of Abyss (Q00933 west wing, Q00935 east
//! wing), ported from L2J Mobius Classic. Both scripts are the same apart
//! from their NPCs, monsters and quest item.

use crate::quest::{Ctx, Quest, QuestState, ALREADY_COMPLETED, NO_QUEST};

pub struct AbyssWing {
    pub id: u32,
    pub dir: &'static str,
    pub title: &'static str,
    /// The two Soul Trackers; their ids prefix the pages.
    pub first: u32,
    pub second: u32,
    pub kills: [u32; 4],
    pub item: u32,
}

pub static WINGS: [AbyssWing; 2] = [
    AbyssWing {
        id: 933,
        dir: "Q00933_ExploringTheWestWingOfTheDungeonOfAbyss",
        title: "Exploring the West Wing of the Dungeon of Abyss",
        first: 31774,
        second: 31775,
        kills: [21638, 21639, 21640, 21641],
        item: 90008,
    },
    AbyssWing {
        id: 935,
        dir: "Q00935_ExploringTheEastWingOfTheDungeonOfAbyss",
        title: "Exploring the East Wing of the Dungeon of Abyss",
        first: 31776,
        second: 31777,
        kills: [21644, 21645, 21646, 21647],
        item: 90009,
    },
];

const POD: u32 = 90136;

/// Days since the epoch: daily quests reset when it changes.
fn today() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64 / 86400)
}

impl Quest for &'static AbyssWing {
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
        vec![self.first, self.second]
    }
    fn talk_npcs(&self) -> Vec<u32> {
        vec![self.first, self.second]
    }
    fn kill_ids(&self) -> Vec<u32> {
        self.kills.to_vec()
    }
    fn quest_items(&self) -> Vec<u32> {
        vec![self.item]
    }
    fn on_event(&self, c: &mut Ctx, event: &str) -> Option<String> {
        let (a, b) = (self.first, self.second);
        let plain: Vec<String> = [a, b].iter().flat_map(|n| (1..=3).map(move |i| format!("{n}-0{i}.htm"))).collect();
        if plain.iter().any(|p| p == event) {
            return Some(event.into());
        }
        if (event == format!("{a}-04.htm") || event == format!("{b}-04.htm")) && c.p.level() >= 40 {
            c.qs.start_quest();
            return Some(event.into());
        }
        if event == "end.htm" {
            c.p.add_exp_sp(250_000, 7_700);
            c.p.give(POD, 1);
            c.qs.exit_quest(false);
            c.qs.set("doneDay", today());
        }
        None
    }
    fn on_talk(&self, c: &mut Ctx) -> String {
        let (a, b) = (self.first, self.second);
        let level = c.p.level();
        if c.qs.is_completed() {
            if c.qs.get_int("doneDay") == today() {
                return ALREADY_COMPLETED.into();
            }
            // A new day: the quest can be taken again.
            *c.qs = QuestState::default();
            return if level < 40 { "nolvl.htm".into() } else { format!("{}-01.htm", c.npc) };
        }
        if c.qs.is_created() {
            return if level < 40 { "nolvl.htm".into() } else { format!("{a}-01.htm") };
        }
        let npc = c.npc;
        if npc != a && npc != b {
            return NO_QUEST.into();
        }
        match c.qs.cond {
            0 if (40..=46).contains(&level) => {
                if npc == b {
                    c.qs.start_quest();
                }
                format!("{npc}-01.htm")
            }
            0 => format!("{npc}-01a.htm"),
            1 => format!("{npc}-04.htm"),
            2 => format!("{npc}-05.htm"),
            _ => NO_QUEST.into(),
        }
    }
    fn on_kill(&self, c: &mut Ctx, _npc: u32) {
        if c.qs.cond != 1 || !c.qs.is_started() {
            return;
        }
        if c.p.count(self.item) < 50 {
            c.p.give(self.item, 1);
        }
        if c.p.count(self.item) >= 50 {
            c.qs.set_cond(2);
        }
    }
    fn progress(&self, qs: &QuestState) -> Option<String> {
        (qs.cond == 1).then(|| "Collect 50 items in the Dungeon of Abyss".to_string())
    }
}
