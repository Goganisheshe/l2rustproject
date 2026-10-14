//! Classic adventurer quests (Q10963..Q10967): talk to NPCs, hunt a number of
//! monsters in a zone, collect the reward. Their Java sources share one
//! shape, so each is a table row here.

use crate::quest::{Ctx, Player, Quest, QuestState, ALREADY_COMPLETED, NO_QUEST};

pub struct Adventure {
    pub id: u32,
    pub dir: &'static str,
    pub title: &'static str,
    pub start: u32,
    pub talk: &'static [u32],
    pub kills: &'static [u32],
    pub min_level: u32,
    pub max_level: u32,
    pub start_event: &'static str,
    /// Events that just show their page.
    pub plain: &'static [&'static str],
    /// Events that advance to a condition.
    pub cond_events: &'static [(&'static str, u32)],
    pub teleports: &'static [(&'static str, [f32; 3])],
    pub reward_event: &'static str,
    pub exp: u64,
    pub sp: u64,
    pub rewards: &'static [(u32, u64)],
    pub reward_message: Option<&'static str>,
    /// Hunting step: condition, kills needed, then the next condition.
    pub hunt_cond: u32,
    pub kill_target: i64,
    pub hunt_message: &'static str,
    pub hunt_items: &'static [(u32, u64)],
    pub created_page: &'static str,
    /// (npc, condition, page) while the quest runs.
    pub pages: &'static [(u32, u32, &'static str)],
}

const KILL_COUNT_VAR: &str = "KillCount";
const RAYMOND: u32 = 30289;
const BATHIS: u32 = 30332;
const MAXIMILLIAN: u32 = 30120;
const BELLA: u32 = 30256;

pub static ADVENTURES: &[Adventure] = &[
    Adventure {
        id: 10963,
        dir: "Q10963_ExploringTheAntNest",
        title: "Exploring the Ant Nest",
        start: RAYMOND,
        talk: &[RAYMOND],
        kills: &[20075, 20079, 20080, 20081, 20082, 20084, 20086, 20087, 20088, 20089, 20090],
        min_level: 34,
        max_level: 37,
        start_event: "30289-03.htm",
        plain: &["30289-01.htm", "30289-02.htm"],
        cond_events: &[],
        teleports: &[],
        reward_event: "30289-05.html",
        exp: 3_000_000,
        sp: 75_000,
        rewards: &[],
        reward_message: None,
        hunt_cond: 1,
        kill_target: 500,
        hunt_message: "Monsters of the Ant Nest are killed.#Use the teleport to get to High Priest Raymond in Gludio.",
        hunt_items: &[],
        created_page: "30289.htm",
        pages: &[(RAYMOND, 2, "30289-04.html")],
    },
    Adventure {
        id: 10964,
        dir: "Q10964_SecretGarden",
        title: "Secret Garden",
        start: BATHIS,
        talk: &[BATHIS, RAYMOND],
        kills: &[20145, 20158, 20176, 20248, 20249],
        min_level: 30,
        max_level: 34,
        start_event: "30332-01.htm",
        plain: &["30289-02.html"],
        cond_events: &[("30289-01.html", 2)],
        teleports: &[],
        reward_event: "30289-03.html",
        exp: 2_500_000,
        sp: 75_000,
        rewards: &[],
        reward_message: Some("The mission Adventurer's Journey II is now available."),
        hunt_cond: 2,
        kill_target: 500,
        hunt_message: "Monsters of the Gorgon Flower Garden are killed.#Use the teleport to get to High Priest Raymond in Gludio.",
        hunt_items: &[],
        created_page: "30332.htm",
        pages: &[(BATHIS, 1, "30332-01.htm"), (RAYMOND, 1, "30289.html"), (RAYMOND, 3, "30289-02.html")],
    },
    Adventure {
        id: 10965,
        dir: "Q10965_DeathMysteries",
        title: "Death Mysteries",
        start: RAYMOND,
        talk: &[RAYMOND, MAXIMILLIAN],
        kills: &[20176, 20550, 20551, 20552, 20553, 20554],
        min_level: 37,
        max_level: 40,
        start_event: "30289-03.htm",
        plain: &["30289-01.htm", "30289-02.htm", "30120-01.html", "30120-02.html"],
        cond_events: &[("30120-03.html", 2)],
        teleports: &[("TELEPORT_TO_MAXIMILLIAN", [86845.0, 148626.0, -3402.0])],
        reward_event: "30120-05.html",
        exp: 3_000_000,
        sp: 75_000,
        rewards: &[(91933, 1), (91935, 1)],
        reward_message: Some("You've got Adventurer's Agathion Bracelet and Adventurer's Agathion Griffin."),
        hunt_cond: 2,
        kill_target: 500,
        hunt_message: "Monsters of the Death Pass are killed.#Use the teleport or the Scroll of Escape to get to High Priest Maximilian in Giran.",
        hunt_items: &[],
        created_page: "30289.htm",
        pages: &[(RAYMOND, 1, "30289-01.htm"), (MAXIMILLIAN, 1, "30120.html"), (MAXIMILLIAN, 3, "30120-04.html")],
    },
    Adventure {
        id: 10966,
        dir: "Q10966_ATripBegins",
        title: "A Trip Begins",
        start: BATHIS,
        talk: &[BATHIS, BELLA],
        kills: &[20926, 20051, 20054, 20060, 20062, 20064],
        min_level: 20,
        max_level: 25,
        start_event: "30332-03.htm",
        plain: &["30332-01.htm", "30332-02.htm"],
        cond_events: &[("30256-01.html", 2)],
        teleports: &[],
        reward_event: "30332-05.html",
        exp: 500_000,
        sp: 12_500,
        rewards: &[(10650, 10), (91745, 1), (91756, 1), (91934, 1)],
        reward_message: None,
        hunt_cond: 2,
        kill_target: 300,
        hunt_message: "You have taken your first step as an adventurer.#Return to Bathis and get your reward.",
        hunt_items: &[(91651, 1)],
        created_page: "30332.htm",
        pages: &[(BATHIS, 1, "30332-03.htm"), (BATHIS, 3, "30332-04.html"), (BELLA, 1, "30256.html")],
    },
    Adventure {
        id: 10967,
        dir: "Q10967_CulturedAdventurer",
        title: "Cultured Adventurer",
        start: BATHIS,
        talk: &[BATHIS],
        kills: &[20063, 20439, 20066, 20438, 20076],
        min_level: 25,
        max_level: 30,
        start_event: "30332-03.htm",
        plain: &["30332-01.htm", "30332-02.htm"],
        cond_events: &[],
        teleports: &[],
        reward_event: "30332-05.html",
        exp: 2_000_000,
        sp: 50_000,
        rewards: &[(91932, 1), (91936, 1)],
        reward_message: Some("You've obtained Adventurer's Brooch and Adventurer's Gem Fragment."),
        hunt_cond: 1,
        kill_target: 300,
        hunt_message: "Monsters of the Abandoned Camp are killed.#Use the teleport to get to Bathis in Gludio.",
        hunt_items: &[],
        created_page: "30332.htm",
        pages: &[(BATHIS, 2, "30332-04.html")],
    },
];

impl Quest for &'static Adventure {
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
        vec![self.start]
    }
    fn talk_npcs(&self) -> Vec<u32> {
        self.talk.to_vec()
    }
    fn kill_ids(&self) -> Vec<u32> {
        self.kills.to_vec()
    }
    fn quest_items(&self) -> Vec<u32> {
        self.hunt_items.iter().map(|i| i.0).collect()
    }
    fn can_start(&self, p: &Player) -> Result<(), &'static str> {
        if p.level() < self.min_level || p.level() > self.max_level { Err("no_lvl.html") } else { Ok(()) }
    }
    fn on_event(&self, c: &mut Ctx, event: &str) -> Option<String> {
        if let Some((_, [x, y, z])) = self.teleports.iter().find(|t| t.0 == event) {
            c.p.tele(*x, *y, *z);
            return None;
        }
        if event == self.start_event {
            c.qs.start_quest();
            return Some(event.into());
        }
        if self.plain.contains(&event) {
            return Some(event.into());
        }
        if let Some((_, cond)) = self.cond_events.iter().find(|e| e.0 == event) {
            c.qs.set_cond(*cond);
            return Some(event.into());
        }
        if event == self.reward_event && c.qs.is_started() {
            if let Some(m) = self.reward_message {
                c.p.screen_message(m);
            }
            c.p.add_exp_sp(self.exp, self.sp);
            for &(item, n) in self.rewards {
                c.p.give(item, n);
            }
            c.qs.exit_quest(false);
            return Some(event.into());
        }
        None
    }
    fn on_kill(&self, c: &mut Ctx, _npc: u32) {
        if !c.qs.is_cond(self.hunt_cond) {
            return;
        }
        let count = c.qs.get_int(KILL_COUNT_VAR) + 1;
        if count < self.kill_target {
            c.qs.set(KILL_COUNT_VAR, count);
        } else {
            c.qs.set_cond(self.hunt_cond + 1);
            c.qs.unset(KILL_COUNT_VAR);
            c.p.screen_message(self.hunt_message);
            for &(item, n) in self.hunt_items {
                c.p.give(item, n);
            }
        }
    }
    fn on_talk(&self, c: &mut Ctx) -> String {
        if c.qs.is_created() {
            return self.created_page.into();
        }
        if c.qs.is_started() {
            let cond = c.qs.cond;
            return self.pages.iter().find(|(n, k, _)| *n == c.npc && *k == cond).map_or(NO_QUEST, |p| p.2).into();
        }
        if c.npc == self.start { ALREADY_COMPLETED.into() } else { NO_QUEST.into() }
    }
    fn progress(&self, qs: &QuestState) -> Option<String> {
        (qs.cond == self.hunt_cond).then(|| format!("Hunt monsters: {}/{}", qs.get_int(KILL_COUNT_VAR), self.kill_target))
    }
}
