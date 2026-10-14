//! Repeatable "bring me monster parts" quests (300 series), ported from
//! L2J Mobius Classic. Each pays Adena for the collected quest items.

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
// Q00300 Hunting Leto Lizardman
// ---------------------------------------------------------------------------

const RATH: u32 = 30126;
const BRACELET_OF_LIZARDMAN: u32 = 7139;
const LETO_DROPS: [(u32, u32); 5] = [(20577, 360), (20578, 390), (20579, 410), (20580, 790), (20582, 890)];

quest!(Q00300, 300, "Q00300_HuntingLetoLizardman", "Hunting Leto Lizardman",
    start: [RATH], talk: [RATH], kills: LETO_DROPS.iter().map(|m| m.0).collect(), items: [BRACELET_OF_LIZARDMAN], {
    fn can_start(&self, p: &Player) -> Result<(), &'static str> {
        max_level(p, 39)
    }
    fn on_event(&self, c: &mut Ctx, event: &str) -> Option<String> {
        match event {
            "30126-03.htm" if c.qs.is_created() => {
                c.qs.start_quest();
                page(event)
            }
            "30126-06.html" => {
                if c.p.count(BRACELET_OF_LIZARDMAN) >= 60 {
                    c.p.take(BRACELET_OF_LIZARDMAN, -1);
                    let r = c.p.random(1000);
                    if r < 500 {
                        c.p.give(ADENA, 5000);
                    } else if r < 750 {
                        c.p.give(1867, 50); // Animal Skin
                    } else {
                        c.p.give(1872, 50); // Animal Bone
                    }
                    c.qs.exit_quest(true);
                    page(event)
                } else {
                    page("30126-07.html")
                }
            }
            _ => None,
        }
    }
    fn on_kill(&self, c: &mut Ctx, npc: u32) {
        let chance = LETO_DROPS.iter().find(|m| m.0 == npc).map_or(0, |m| m.1);
        if c.qs.is_cond(1) && c.p.random(1000) < chance {
            c.p.give(BRACELET_OF_LIZARDMAN, 1);
            if c.p.count(BRACELET_OF_LIZARDMAN) == 60 {
                c.qs.set_cond(2);
            }
        }
    }
    fn on_talk(&self, c: &mut Ctx) -> String {
        if c.qs.is_created() {
            return if c.p.level() >= 34 { "30126-01.htm" } else { "30126-02.htm" }.into();
        }
        match c.qs.cond {
            1 => "30126-04.html".into(),
            2 if c.p.count(BRACELET_OF_LIZARDMAN) >= 60 => "30126-05.html".into(),
            _ => NO_QUEST.into(),
        }
    }
});

// ---------------------------------------------------------------------------
// Q00326 Vanquish Remnants
// ---------------------------------------------------------------------------

const LEOPOLD: u32 = 30435;
const RED_CROSS_BADGE: u32 = 1359;
const BLUE_CROSS_BADGE: u32 = 1360;
const BLACK_CROSS_BADGE: u32 = 1361;
const BLACK_LION_MARK: u32 = 1369;
const REMNANTS: [(u32, u32, u32); 9] = [
    (20053, 61, RED_CROSS_BADGE),
    (20058, 61, RED_CROSS_BADGE),
    (20061, 57, BLUE_CROSS_BADGE),
    (20063, 63, BLUE_CROSS_BADGE),
    (20066, 59, BLACK_CROSS_BADGE),
    (20436, 55, BLUE_CROSS_BADGE),
    (20437, 59, RED_CROSS_BADGE),
    (20438, 60, BLACK_CROSS_BADGE),
    (20439, 62, BLUE_CROSS_BADGE),
];

quest!(Q00326, 326, "Q00326_VanquishRemnants", "Vanquish Remnants",
    start: [LEOPOLD], talk: [LEOPOLD], kills: REMNANTS.iter().map(|m| m.0).collect(),
    items: [RED_CROSS_BADGE, BLUE_CROSS_BADGE, BLACK_CROSS_BADGE], {
    fn can_start(&self, p: &Player) -> Result<(), &'static str> {
        max_level(p, 30)
    }
    fn on_event(&self, c: &mut Ctx, event: &str) -> Option<String> {
        match event {
            "30435-03.htm" => {
                c.qs.start_quest();
                page(event)
            }
            "30435-07.html" => {
                c.qs.exit_quest(true);
                page(event)
            }
            "30435-08.html" => page(event),
            _ => None,
        }
    }
    fn on_kill(&self, c: &mut Ctx, npc: u32) {
        let Some(&(_, chance, item)) = REMNANTS.iter().find(|m| m.0 == npc) else { return };
        if c.qs.is_started() && c.p.random(100) < chance {
            c.p.give(item, 1);
        }
    }
    fn on_talk(&self, c: &mut Ctx) -> String {
        if c.qs.is_created() {
            return if c.p.level() >= 21 { "30435-02.htm" } else { "30435-01.htm" }.into();
        }
        if !c.qs.is_started() {
            return NO_QUEST.into();
        }
        let red = c.p.count(RED_CROSS_BADGE);
        let blue = c.p.count(BLUE_CROSS_BADGE);
        let black = c.p.count(BLACK_CROSS_BADGE);
        let sum = red + blue + black;
        if sum == 0 {
            return "30435-04.html".into();
        }
        if sum >= 100 && !c.p.has(BLACK_LION_MARK) {
            c.p.give(BLACK_LION_MARK, 1);
        }
        c.p.give(ADENA, red * 10 + blue * 10 + black * 12 + if sum >= 10 { 1000 } else { 0 });
        c.p.take_all(&[RED_CROSS_BADGE, BLUE_CROSS_BADGE, BLACK_CROSS_BADGE]);
        if sum >= 100 {
            if c.p.has(BLACK_LION_MARK) { "30435-09.html" } else { "30435-06.html" }
        } else {
            "30435-05.html"
        }
        .into()
    }
});

// ---------------------------------------------------------------------------
// Q00328 Sense for Business
// ---------------------------------------------------------------------------

const SARIEN: u32 = 30436;
const MONSTER_EYE_CARCASS: u32 = 1347;
const MONSTER_EYE_LENS: u32 = 1366;
const BASILISK_GIZZARD: u32 = 1348;
const MONSTER_EYES: [(u32, u32, u32); 4] = [(20055, 61, 62), (20059, 61, 62), (20067, 72, 74), (20068, 78, 79)];
const BASILISKS: [(u32, u32); 2] = [(20070, 60), (20072, 63)];

quest!(Q00328, 328, "Q00328_SenseForBusiness", "Sense for Business",
    start: [SARIEN], talk: [SARIEN],
    kills: MONSTER_EYES.iter().map(|m| m.0).chain(BASILISKS.iter().map(|m| m.0)).collect(),
    items: [MONSTER_EYE_CARCASS, MONSTER_EYE_LENS, BASILISK_GIZZARD], {
    fn on_event(&self, c: &mut Ctx, event: &str) -> Option<String> {
        match event {
            "30436-03.htm" => {
                c.qs.start_quest();
                page(event)
            }
            "30436-06.html" => {
                c.qs.exit_quest(true);
                page(event)
            }
            _ => None,
        }
    }
    fn on_talk(&self, c: &mut Ctx) -> String {
        if c.qs.is_created() {
            return if c.p.level() < 21 { "30436-01.htm" } else { "30436-02.htm" }.into();
        }
        if !c.qs.is_started() {
            return NO_QUEST.into();
        }
        let carcass = c.p.count(MONSTER_EYE_CARCASS);
        let lens = c.p.count(MONSTER_EYE_LENS);
        let gizzards = c.p.count(BASILISK_GIZZARD);
        let sum = carcass + lens + gizzards;
        if sum == 0 {
            return "30436-04.html".into();
        }
        c.p.give(ADENA, carcass * 2 + lens * 10 + gizzards * 2 + if sum >= 10 { 100 } else { 0 });
        c.p.take_all(&[MONSTER_EYE_CARCASS, MONSTER_EYE_LENS, BASILISK_GIZZARD]);
        "30436-05.html".into()
    }
    fn on_kill(&self, c: &mut Ctx, npc: u32) {
        if !c.qs.is_started() {
            return;
        }
        let chance = c.p.random(100);
        if let Some(&(_, carcass, lens)) = MONSTER_EYES.iter().find(|m| m.0 == npc) {
            if chance < carcass {
                c.p.give(MONSTER_EYE_CARCASS, 1);
            } else if chance < lens {
                c.p.give(MONSTER_EYE_LENS, 1);
            }
        } else if let Some(&(_, gizzard)) = BASILISKS.iter().find(|m| m.0 == npc) {
            if chance < gizzard {
                c.p.give(BASILISK_GIZZARD, 1);
            }
        }
    }
});

// ---------------------------------------------------------------------------
// Q00329 Curiosity of a Dwarf
// ---------------------------------------------------------------------------

const TRADER_ROLENTO: u32 = 30437;
const GOLEM_HEARTSTONE: u32 = 1346;
const BROKEN_HEARTSTONE: u32 = 1365;
const GOLEMS: [(u32, u32, u32); 2] = [(20083, 3, 54), (20085, 3, 58)];

quest!(Q00329, 329, "Q00329_CuriosityOfADwarf", "Curiosity of a Dwarf",
    start: [TRADER_ROLENTO], talk: [TRADER_ROLENTO], kills: GOLEMS.iter().map(|m| m.0).collect(),
    items: [GOLEM_HEARTSTONE, BROKEN_HEARTSTONE], {
    fn can_start(&self, p: &Player) -> Result<(), &'static str> {
        max_level(p, 38)
    }
    fn on_event(&self, c: &mut Ctx, event: &str) -> Option<String> {
        match event {
            "30437-03.htm" if c.qs.is_created() => {
                c.qs.start_quest();
                page(event)
            }
            "30437-06.html" => {
                c.qs.exit_quest(true);
                page(event)
            }
            "30437-07.html" => page(event),
            _ => None,
        }
    }
    fn on_kill(&self, c: &mut Ctx, npc: u32) {
        let Some(&(_, golem, broken)) = GOLEMS.iter().find(|m| m.0 == npc) else { return };
        if !c.qs.is_started() {
            return;
        }
        let rnd = c.p.random(100);
        if rnd < golem {
            c.p.give(GOLEM_HEARTSTONE, 1);
        } else if rnd < broken {
            c.p.give(BROKEN_HEARTSTONE, 1);
        }
    }
    fn on_talk(&self, c: &mut Ctx) -> String {
        if c.qs.is_created() {
            return if c.p.level() >= 33 { "30437-02.htm" } else { "30437-01.htm" }.into();
        }
        if !c.qs.is_started() {
            return NO_QUEST.into();
        }
        let broken = c.p.count(BROKEN_HEARTSTONE);
        let golem = c.p.count(GOLEM_HEARTSTONE);
        if broken + golem == 0 {
            return "30437-04.html".into();
        }
        c.p.give(ADENA, broken * 5 + golem * 40 + if broken + golem >= 700 { 700 } else { 1000 });
        c.p.take_all(&[GOLEM_HEARTSTONE, BROKEN_HEARTSTONE]);
        "30437-05.html".into()
    }
});

// ---------------------------------------------------------------------------
// Q00331 Arrow of Vengeance
// ---------------------------------------------------------------------------

const BELTON: u32 = 30125;
const HARPY_FEATHER: u32 = 1452;
const MEDUSA_VENOM: u32 = 1453;
const WYRMS_TOOTH: u32 = 1454;
const VENGEANCE: [(u32, u32, u32); 3] = [(20145, 59, HARPY_FEATHER), (20158, 61, MEDUSA_VENOM), (20176, 60, WYRMS_TOOTH)];

quest!(Q00331, 331, "Q00331_ArrowOfVengeance", "Arrow of Vengeance",
    start: [BELTON], talk: [BELTON], kills: VENGEANCE.iter().map(|m| m.0).collect(),
    items: [HARPY_FEATHER, MEDUSA_VENOM, WYRMS_TOOTH], {
    fn can_start(&self, p: &Player) -> Result<(), &'static str> {
        max_level(p, 39)
    }
    fn on_event(&self, c: &mut Ctx, event: &str) -> Option<String> {
        match event {
            "30125-03.htm" => {
                c.qs.start_quest();
                page(event)
            }
            "30125-06.html" => {
                c.qs.exit_quest(true);
                page(event)
            }
            "30125-07.html" => page(event),
            _ => None,
        }
    }
    fn on_talk(&self, c: &mut Ctx) -> String {
        if c.qs.is_created() {
            return if c.p.level() < 32 { "30125-01.htm" } else { "30125-02.htm" }.into();
        }
        if !c.qs.is_started() {
            return NO_QUEST.into();
        }
        let f = c.p.count(HARPY_FEATHER);
        let v = c.p.count(MEDUSA_VENOM);
        let t = c.p.count(WYRMS_TOOTH);
        if f + v + t == 0 {
            return "30125-04.html".into();
        }
        c.p.give(ADENA, f * 6 + v * 7 + t * 9 + if f + v + t >= 10 { 1000 } else { 0 });
        c.p.take_all(&[HARPY_FEATHER, MEDUSA_VENOM, WYRMS_TOOTH]);
        "30125-05.html".into()
    }
    fn on_kill(&self, c: &mut Ctx, npc: u32) {
        let Some(&(_, chance, item)) = VENGEANCE.iter().find(|m| m.0 == npc) else { return };
        if c.qs.is_started() && c.p.random(100) < chance {
            c.p.give(item, 1);
        }
    }
});
