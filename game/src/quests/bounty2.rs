//! More 300-series collection quests (Q00354..Q00370), ported from L2J Mobius.

use crate::items::ADENA;
use crate::quest;
use crate::quest::{Ctx, Player, NO_QUEST};

fn page(s: &str) -> Option<String> {
    Some(s.to_string())
}

fn max_level(p: &Player, max: u32) -> Result<(), &'static str> {
    if p.level() > max { Err(NO_QUEST) } else { Ok(()) }
}

// ---------------------------------------------------------------------------
// Q00354 Conquest of Alligator Island
// ---------------------------------------------------------------------------

const KLUCK: u32 = 30895;
const ALLIGATOR_TOOTH: u32 = 5863;
const ALLIGATORS: [(u32, f64); 4] = [(20804, 0.84), (20805, 0.91), (20806, 0.88), (20807, 0.92)];
const NOS_LAD: u32 = 20808;

quest!(Q00354, 354, "Q00354_ConquestOfAlligatorIsland", "Conquest of Alligator Island",
    start: [KLUCK], talk: [KLUCK], kills: ALLIGATORS.iter().map(|m| m.0).chain([NOS_LAD]).collect(), items: [ALLIGATOR_TOOTH], {
    fn can_start(&self, p: &Player) -> Result<(), &'static str> {
        max_level(p, 49)
    }
    fn on_event(&self, c: &mut Ctx, event: &str) -> Option<String> {
        match event {
            "30895-04.html" | "30895-05.html" | "30895-09.html" => page(event),
            "30895-02.html" => {
                c.qs.start_quest();
                page(event)
            }
            "ADENA" => {
                if c.p.count(ALLIGATOR_TOOTH) >= 400 {
                    c.p.give(ADENA, 2000);
                    c.p.take(ALLIGATOR_TOOTH, -1);
                    page("30895-06.html")
                } else {
                    page("30895-08.html")
                }
            }
            "30895-10.html" => {
                c.qs.exit_quest(true);
                page(event)
            }
            _ => None,
        }
    }
    fn on_kill(&self, c: &mut Ctx, npc: u32) {
        if !c.qs.is_started() {
            return;
        }
        if let Some(&(_, chance)) = ALLIGATORS.iter().find(|m| m.0 == npc) {
            c.p.give_randomly(ALLIGATOR_TOOTH, 1, 0, chance);
        } else {
            let n = if c.p.random(100) < 14 { 2 } else { 1 };
            c.p.give_randomly(ALLIGATOR_TOOTH, n, 0, 1.0);
        }
    }
    fn on_talk(&self, c: &mut Ctx) -> String {
        if c.qs.is_created() {
            if c.p.level() >= 38 { "30895-01.htm" } else { "30895-03.html" }.into()
        } else if c.qs.is_started() {
            "30895-04.html".into()
        } else {
            NO_QUEST.into()
        }
    }
});

// ---------------------------------------------------------------------------
// Q00355 Family Honor
// ---------------------------------------------------------------------------

const GALIBREDO: u32 = 30181;
const PATRIN: u32 = 30929;
const GALFREDO_ROMERS_BUST: u32 = 4252;
const SCULPTOR_BERONA: u32 = 4350;
const ANCIENT_STATUE_PROTOTYPE: u32 = 4351;
const ANCIENT_STATUE_ORIGINAL: u32 = 4352;
const ANCIENT_STATUE_REPLICA: u32 = 4353;
const ANCIENT_STATUE_FORGERY: u32 = 4354;
const TIMAK_ORCS: [(u32, u32, u32); 4] = [(20767, 560, 684), (20768, 530, 650), (20769, 420, 516), (20770, 440, 560)];

quest!(Q00355, 355, "Q00355_FamilyHonor", "Family Honor",
    start: [GALIBREDO], talk: [GALIBREDO, PATRIN], kills: TIMAK_ORCS.iter().map(|m| m.0).collect(), items: [GALFREDO_ROMERS_BUST], {
    fn can_start(&self, p: &Player) -> Result<(), &'static str> {
        max_level(p, 49)
    }
    fn on_event(&self, c: &mut Ctx, event: &str) -> Option<String> {
        match event {
            "30181-02.htm" | "30181-09.html" | "30929-01.html" | "30929-02.html" => page(event),
            "30181-03.htm" => {
                c.qs.start_quest();
                page(event)
            }
            "30181-06.html" => {
                let n = c.p.count(GALFREDO_ROMERS_BUST);
                if n < 1 {
                    page(event)
                } else {
                    c.p.give(ADENA, n * 20);
                    c.p.take(GALFREDO_ROMERS_BUST, -1);
                    page(if n >= 100 { "30181-07.html" } else { "30181-08.html" })
                }
            }
            "30181-10.html" => {
                let n = c.p.count(GALFREDO_ROMERS_BUST);
                if n > 0 {
                    c.p.give(ADENA, n * 120);
                }
                c.p.take(GALFREDO_ROMERS_BUST, -1);
                c.qs.exit_quest(true);
                page(event)
            }
            "30929-03.html" => {
                if !c.p.has(SCULPTOR_BERONA) {
                    return page("30929-08.html");
                }
                let r = c.p.random(100);
                let out = if r < 2 {
                    c.p.give(ANCIENT_STATUE_PROTOTYPE, 1);
                    event
                } else if r < 32 {
                    c.p.give(ANCIENT_STATUE_ORIGINAL, 1);
                    "30929-04.html"
                } else if r < 62 {
                    c.p.give(ANCIENT_STATUE_REPLICA, 1);
                    "30929-05.html"
                } else if r < 77 {
                    c.p.give(ANCIENT_STATUE_FORGERY, 1);
                    "30929-06.html"
                } else {
                    "30929-07.html"
                };
                c.p.take(SCULPTOR_BERONA, 1);
                page(out)
            }
            _ => None,
        }
    }
    fn on_kill(&self, c: &mut Ctx, npc: u32) {
        let Some(&(_, first, second)) = TIMAK_ORCS.iter().find(|m| m.0 == npc) else { return };
        let r = c.p.random(1000);
        if r < first {
            c.p.give_randomly(GALFREDO_ROMERS_BUST, 1, 0, 1.0);
        } else if r < second {
            c.p.give_randomly(SCULPTOR_BERONA, 1, 0, 1.0);
        }
    }
    fn on_talk(&self, c: &mut Ctx) -> String {
        if c.qs.is_created() {
            if c.p.level() >= 36 { "30181-01.htm" } else { "30181-04.html" }.into()
        } else if c.qs.is_started() {
            if c.npc == GALIBREDO {
                if c.p.has(SCULPTOR_BERONA) { "30181-11.html" } else { "30181-05.html" }.into()
            } else {
                "30929-01.html".into()
            }
        } else {
            NO_QUEST.into()
        }
    }
});

// ---------------------------------------------------------------------------
// Q00356 Dig Up the Sea of Spores
// ---------------------------------------------------------------------------

const GAUEN: u32 = 30717;
const CARNIVORE_SPORE: u32 = 5865;
const HERBIVOROUS_SPORE: u32 = 5866;
const ROTTING_TREE: u32 = 20558;
const SPORE_ZOMBIE: u32 = 20562;

quest!(Q00356, 356, "Q00356_DigUpTheSeaOfSpores", "Dig Up the Sea of Spores",
    start: [GAUEN], talk: [GAUEN], kills: vec![ROTTING_TREE, SPORE_ZOMBIE], items: [HERBIVOROUS_SPORE, CARNIVORE_SPORE], {
    fn can_start(&self, p: &Player) -> Result<(), &'static str> {
        max_level(p, 51)
    }
    fn on_event(&self, c: &mut Ctx, event: &str) -> Option<String> {
        match event {
            "30717-02.htm" | "30717-03.htm" | "30717-04.htm" | "30717-10.html" | "30717-18.html" => page(event),
            "30717-05.htm" => {
                c.qs.start_quest();
                page(event)
            }
            "30717-09.html" => {
                c.p.take_all(&[CARNIVORE_SPORE, HERBIVOROUS_SPORE]);
                page(event)
            }
            "30717-11.html" | "30717-14.html" => {
                c.qs.exit_quest(true);
                page(event)
            }
            "FINISH" => {
                let v = c.p.random(100);
                let (adena, out) = if v < 20 {
                    (3000, "30717-15.html")
                } else if v < 70 {
                    (1300, "30717-16.html")
                } else {
                    (1300, "30717-17.html")
                };
                c.p.give(ADENA, adena);
                c.qs.exit_quest(true);
                page(out)
            }
            _ => None,
        }
    }
    fn on_kill(&self, c: &mut Ctx, npc: u32) {
        if !c.qs.is_started() {
            return;
        }
        let (drop, other, chance) =
            if npc == ROTTING_TREE { (HERBIVOROUS_SPORE, CARNIVORE_SPORE, 0.73) } else { (CARNIVORE_SPORE, HERBIVOROUS_SPORE, 0.94) };
        if c.p.give_randomly(drop, 1, 100, chance) {
            c.qs.set_cond(if c.p.count(other) >= 100 { 3 } else { 2 });
        }
    }
    fn on_talk(&self, c: &mut Ctx) -> String {
        if c.qs.is_created() {
            return if c.p.level() >= 43 { "30717-01.htm" } else { "30717-06.htm" }.into();
        }
        if !c.qs.is_started() {
            return NO_QUEST.into();
        }
        let herb = c.p.count(HERBIVOROUS_SPORE) >= 100;
        let carn = c.p.count(CARNIVORE_SPORE) >= 100;
        match (herb, carn) {
            (true, true) => "30717-13.html",
            (false, true) => "30717-12.html",
            (true, false) => "30717-08.html",
            _ => "30717-07.html",
        }
        .into()
    }
});

// ---------------------------------------------------------------------------
// Q00358 Illegitimate Child of the Goddess
// ---------------------------------------------------------------------------

const OLTRAN: u32 = 30862;
const SNAKE_SCALE: u32 = 5868;
const GODDESS_REWARDS: [u32; 8] = [4975, 4973, 4974, 4939, 4937, 4938, 4936, 4980];
const SNAKES: [(u32, f64); 2] = [(20672, 0.71), (20673, 0.74)];

quest!(Q00358, 358, "Q00358_IllegitimateChildOfTheGoddess", "Illegitimate Child of the Goddess",
    start: [OLTRAN], talk: [OLTRAN], kills: SNAKES.iter().map(|m| m.0).collect(), items: [SNAKE_SCALE], {
    fn can_start(&self, p: &Player) -> Result<(), &'static str> {
        max_level(p, 67)
    }
    fn on_event(&self, c: &mut Ctx, event: &str) -> Option<String> {
        match event {
            "30862-02.htm" | "30862-03.htm" => page(event),
            "30862-04.htm" => {
                c.qs.start_quest();
                page(event)
            }
            _ => None,
        }
    }
    fn on_kill(&self, c: &mut Ctx, npc: u32) {
        let chance = SNAKES.iter().find(|m| m.0 == npc).map_or(0.0, |m| m.1);
        if c.qs.is_cond(1) && c.p.give_randomly(SNAKE_SCALE, 1, 108, chance) {
            c.qs.set_cond(2);
        }
    }
    fn on_talk(&self, c: &mut Ctx) -> String {
        if c.qs.is_created() {
            return if c.p.level() >= 63 { "30862-01.htm" } else { "30862-05.html" }.into();
        }
        if !c.qs.is_started() {
            return NO_QUEST.into();
        }
        if c.p.count(SNAKE_SCALE) < 108 {
            return "30862-06.html".into();
        }
        let reward = GODDESS_REWARDS[c.p.random(GODDESS_REWARDS.len() as u32) as usize];
        c.p.give(reward, 1);
        c.qs.exit_quest(true);
        "30862-07.html".into()
    }
});

// ---------------------------------------------------------------------------
// Q00360 Plunder Their Supplies
// ---------------------------------------------------------------------------

const COLEMAN: u32 = 30873;
const SUPPLY_ITEMS: u32 = 5872;
const TAIK_ORCS: [(u32, u32); 2] = [(20666, 50), (20669, 75)];

quest!(Q00360, 360, "Q00360_PlunderTheirSupplies", "Plunder Their Supplies",
    start: [COLEMAN], talk: [COLEMAN], kills: TAIK_ORCS.iter().map(|m| m.0).collect(), items: [SUPPLY_ITEMS], {
    fn can_start(&self, p: &Player) -> Result<(), &'static str> {
        max_level(p, 59)
    }
    fn on_event(&self, c: &mut Ctx, event: &str) -> Option<String> {
        match event {
            "30873-03.htm" | "30873-09.html" => page(event),
            "30873-04.htm" => {
                c.qs.start_quest();
                page(event)
            }
            "30873-10.html" => {
                c.qs.exit_quest(false);
                page(event)
            }
            _ => None,
        }
    }
    fn on_kill(&self, c: &mut Ctx, npc: u32) {
        let chance = TAIK_ORCS.iter().find(|m| m.0 == npc).map_or(0, |m| m.1);
        if c.qs.is_started() && c.p.random(100) < chance {
            c.p.give(SUPPLY_ITEMS, 1);
        }
    }
    fn on_talk(&self, c: &mut Ctx) -> String {
        if c.qs.is_created() {
            return if c.p.level() >= 52 { "30873-02.htm" } else { "30873-01.html" }.into();
        }
        if c.qs.is_started() && c.p.count(SUPPLY_ITEMS) >= 500 {
            c.p.give(ADENA, 14000);
            c.p.take(SUPPLY_ITEMS, -1);
            return "30873-06.html".into();
        }
        // The Java returns the "no quest" page while fewer than 500 supplies
        // are collected (its `< 0` check never matches); keep the progress
        // page instead so the quest stays readable.
        if c.qs.is_started() { "30873-05.html".into() } else { NO_QUEST.into() }
    }
});

// ---------------------------------------------------------------------------
// Q00369 Collector of Jewels
// ---------------------------------------------------------------------------

const NELL: u32 = 30376;
const FLARE_SHARD: u32 = 5882;
const FREEZING_SHARD: u32 = 5883;
const JEWEL_MOBS: [(u32, u32, u32, u64); 6] = [
    (20609, FLARE_SHARD, 75, 1),
    (20612, FLARE_SHARD, 91, 1),
    (20749, FLARE_SHARD, 100, 2),
    (20616, FREEZING_SHARD, 81, 1),
    (20619, FREEZING_SHARD, 87, 1),
    (20747, FREEZING_SHARD, 100, 2),
];

quest!(Q00369, 369, "Q00369_CollectorOfJewels", "Collector of Jewels",
    start: [NELL], talk: [NELL], kills: JEWEL_MOBS.iter().map(|m| m.0).collect(), items: [FLARE_SHARD, FREEZING_SHARD], {
    fn can_start(&self, p: &Player) -> Result<(), &'static str> {
        max_level(p, 37)
    }
    fn on_event(&self, c: &mut Ctx, event: &str) -> Option<String> {
        match event {
            "30376-02.htm" => {
                c.qs.start_quest();
                c.qs.set_memo(1);
                page(event)
            }
            "30376-05.html" => page(event),
            "30376-06.html" if c.qs.memo() == 2 => {
                c.qs.set_memo(3);
                c.qs.set_cond(3);
                page(event)
            }
            "30376-07.html" => {
                c.qs.exit_quest(true);
                page(event)
            }
            _ => None,
        }
    }
    fn on_kill(&self, c: &mut Ctx, npc: u32) {
        let Some(&(_, item, chance, count)) = JEWEL_MOBS.iter().find(|m| m.0 == npc) else { return };
        let memo = c.qs.memo();
        if !(memo == 1 || memo == 3) || c.p.random(100) >= chance {
            return;
        }
        let (limit, cond) = if memo == 1 { (50, 2) } else { (200, 4) };
        if c.p.give_randomly(item, count, limit, 1.0) && c.p.count(FLARE_SHARD) + c.p.count(FREEZING_SHARD) >= limit * 2 {
            c.qs.set_cond(cond);
        }
    }
    fn on_talk(&self, c: &mut Ctx) -> String {
        if c.qs.is_created() {
            return if c.p.level() >= 25 { "30376-01.htm" } else { "30376-03.html" }.into();
        }
        if !c.qs.is_started() {
            return NO_QUEST.into();
        }
        let shards = c.p.count(FLARE_SHARD) + c.p.count(FREEZING_SHARD);
        match c.qs.memo() {
            1 if shards >= 100 => {
                c.p.give(ADENA, 3000);
                c.p.take_all(&[FLARE_SHARD, FREEZING_SHARD]);
                c.qs.set_memo(2);
                "30376-04.html".into()
            }
            1 => "30376-08.html".into(),
            2 => "30376-09.html".into(),
            3 if shards >= 400 => {
                c.p.give(ADENA, 12000);
                c.p.take_all(&[FLARE_SHARD, FREEZING_SHARD]);
                c.qs.exit_quest(true);
                "30376-10.html".into()
            }
            3 => "30376-11.html".into(),
            _ => NO_QUEST.into(),
        }
    }
});

// ---------------------------------------------------------------------------
// Q00370 An Elder Sows Seeds
// ---------------------------------------------------------------------------

const CASIAN: u32 = 30612;
const SPELLBOOK_PAGE: u32 = 5916;
const CHAPTERS: [u32; 4] = [5918, 5920, 5919, 5917]; // water, earth, wind, fire
const ANTS1: [(u32, u32); 3] = [(20082, 9), (20086, 9), (20090, 22)];
const ANTS2: [(u32, f64); 2] = [(20084, 0.101), (20089, 0.100)];

fn exchange_chapters(p: &mut Player, take_all: bool) -> bool {
    let min = CHAPTERS.iter().map(|&c| p.count(c)).min().unwrap_or(0);
    if min > 0 {
        p.give(ADENA, min * 3600);
    }
    for c in CHAPTERS {
        p.take(c, if take_all { -1 } else { min as i64 });
    }
    min > 0
}

quest!(Q00370, 370, "Q00370_AnElderSowsSeeds", "An Elder Sows Seeds",
    start: [CASIAN], talk: [CASIAN], kills: ANTS1.iter().map(|m| m.0).chain(ANTS2.iter().map(|m| m.0)).collect(), items: [], {
    fn can_start(&self, p: &Player) -> Result<(), &'static str> {
        max_level(p, 42)
    }
    fn on_event(&self, c: &mut Ctx, event: &str) -> Option<String> {
        match event {
            "30612-02.htm" | "30612-03.htm" | "30612-06.html" | "30612-07.html" | "30612-09.html" => page(event),
            "30612-04.htm" => {
                c.qs.start_quest();
                page(event)
            }
            "REWARD" if c.qs.is_started() => page(if exchange_chapters(c.p, false) { "30612-08.html" } else { "30612-11.html" }),
            "30612-10.html" if c.qs.is_started() => {
                exchange_chapters(c.p, true);
                c.qs.exit_quest(true);
                page(event)
            }
            _ => None,
        }
    }
    fn on_kill(&self, c: &mut Ctx, npc: u32) {
        if !c.qs.is_started() {
            return;
        }
        if let Some(&(_, chance)) = ANTS1.iter().find(|m| m.0 == npc) {
            if c.p.random(100) < chance {
                c.p.give_randomly(SPELLBOOK_PAGE, 1, 0, 1.0);
            }
        } else if let Some(&(_, chance)) = ANTS2.iter().find(|m| m.0 == npc) {
            c.p.give_randomly(SPELLBOOK_PAGE, 1, 0, chance);
        }
    }
    fn on_talk(&self, c: &mut Ctx) -> String {
        if c.qs.is_created() {
            if c.p.level() >= 28 { "30612-01.htm" } else { "30612-05.html" }.into()
        } else if c.qs.is_started() {
            "30612-06.html".into()
        } else {
            NO_QUEST.into()
        }
    }
});
