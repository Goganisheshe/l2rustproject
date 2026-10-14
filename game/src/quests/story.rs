//! Story quests: Q10866 Punitive Operation on the Devil Isle and Q10673
//! Saga of Legend (which opens the third class transfer), ported from L2J
//! Mobius Classic.

use crate::items::ADENA;
use crate::quest;
use crate::quest::{Ctx, Player, Race, ALREADY_COMPLETED, NO_QUEST};

fn page(s: &str) -> Option<String> {
    Some(s.to_string())
}

// ---------------------------------------------------------------------------
// Q10866 Punitive Operation on the Devil Isle
// ---------------------------------------------------------------------------

const RODEMAI: u32 = 30756;
const EIN: u32 = 34017;
const FETHIN: u32 = 34019;
const NIKIA: u32 = 34020;

quest!(Q10866, 10866, "Q10866_PunitiveOperationOnTheDevilIsle", "Punitive Operation on the Devil Isle",
    start: [RODEMAI], talk: [RODEMAI, EIN, FETHIN, NIKIA], kills: Vec::new(), items: [], {
    fn on_event(&self, c: &mut Ctx, event: &str) -> Option<String> {
        match event {
            "30756-02.htm" => {
                c.qs.start_quest();
                page(event)
            }
            "34017-02.html" => {
                c.qs.set_cond(2);
                page(event)
            }
            "34019-02.html" => {
                c.qs.set_cond(3);
                page(event)
            }
            "34020-02.html" if c.qs.is_started() => {
                c.p.add_exp_sp(150_000, 4_500);
                c.p.give(ADENA, 13_136);
                c.qs.exit_quest(false);
                page(event)
            }
            _ => None,
        }
    }
    fn on_talk(&self, c: &mut Ctx) -> String {
        if c.qs.is_created() {
            return if c.p.level() >= 70 { "30756-01.htm" } else { "no_lvl.html" }.into();
        }
        if c.qs.is_completed() {
            return if c.npc == RODEMAI { ALREADY_COMPLETED } else { NO_QUEST }.into();
        }
        match (c.npc, c.qs.cond) {
            (RODEMAI, 1) => "30756-02.html",
            (EIN, 1) => "34017-01.html",
            (EIN, 2) => "34017-02.html",
            (FETHIN, 2) => "34019-01.html",
            (FETHIN, 3) => "34019-02.html",
            (NIKIA, 3) => "34020-01.html",
            _ => NO_QUEST,
        }
        .into()
    }
});

// ---------------------------------------------------------------------------
// Q10673 Saga of Legend
// ---------------------------------------------------------------------------

pub const SAGA_OF_LEGEND: u32 = 10673;
const ORVEN: u32 = 30857;
const MAGICAL_TABLET: u32 = 90045;
const SAGA_MOBS: [u32; 40] = [
    20965, 20970, 20966, 20971, 20967, 20973, 20968, 20969, 20972, 24025, 24046, 24032, 24041, 24026, 24042, 24047, 24033, 24048, 24043,
    24050, 24049, 24034, 24027, 24052, 24051, 24035, 24028, 24053, 24054, 24036, 24037, 24055, 24030, 24029, 24044, 24045, 24031, 24040,
    24039, 24038,
];

/// Mount spellbook granted on the third transfer after the saga.
pub fn saga_mount(race: Race) -> u32 {
    match race {
        Race::Human => 90038,
        Race::Elf => 90039,
        Race::DarkElf => 90040,
        Race::Dwarf => 90041,
        Race::Orc => 90042,
        Race::Kamael => 91946,
    }
}

quest!(Q10673, SAGA_OF_LEGEND, "Q10673_SagaOfLegend", "Saga of Legend",
    start: [ORVEN], talk: [ORVEN], kills: SAGA_MOBS.to_vec(), items: [], {
    fn can_start(&self, p: &Player) -> Result<(), &'static str> {
        // Level 76 and in a second profession (THIRD_CLASS_GROUP).
        if p.level() < 76 || p.dp.class_depth(p.class_id()) != 2 { Err("30857-00.htm") } else { Ok(()) }
    }
    fn on_event(&self, c: &mut Ctx, event: &str) -> Option<String> {
        match event {
            "30857-02.htm" | "30857-03.htm" | "30857-04.htm" | "30857-06.html" => page(event),
            "30857-05.htm" => {
                c.qs.start_quest();
                page(event)
            }
            "30857-07.html" => {
                c.qs.set_cond(2);
                page(event)
            }
            "30857-07a.html" => {
                c.qs.set_cond(3);
                page(event)
            }
            "30857-10.html" if c.qs.is_cond(4) => {
                c.p.give(MAGICAL_TABLET, 10);
                c.qs.exit_quest(false);
                if c.p.dp.class_depth(c.p.class_id()) == 2 {
                    c.p.class_change = true;
                }
                page(event)
            }
            _ => None,
        }
    }
    fn on_kill(&self, c: &mut Ctx, _npc: u32) {
        if !c.qs.is_started() || c.qs.cond <= 1 {
            return;
        }
        let n = c.qs.get_int("KillCount") + 1;
        if n < 700 {
            c.qs.set("KillCount", n);
        } else {
            c.qs.set_cond(4);
            c.qs.unset("KillCount");
        }
    }
    fn on_talk(&self, c: &mut Ctx) -> String {
        if c.qs.is_created() {
            return "30857-01.htm".into();
        }
        if c.qs.is_completed() {
            return ALREADY_COMPLETED.into();
        }
        match c.qs.cond {
            1 => "30857-05.htm",
            2 => "30857-08.html",
            3 => "30857-08a.html",
            4 => "30857-09.html",
            _ => NO_QUEST,
        }
        .into()
    }
    fn progress(&self, qs: &crate::quest::QuestState) -> Option<String> {
        (qs.cond == 2 || qs.cond == 3).then(|| format!("Hunt monsters: {}/700", qs.get_int("KillCount")))
    }
});

// ---------------------------------------------------------------------------
// Q00127 Fishing Specialist's Request
// ---------------------------------------------------------------------------

const PIERRE: u32 = 30013;
const FERMA: u32 = 30015;
const BAIKAL: u32 = 30016;
const PIERRE_LETTER: u32 = 49510;
const FISH_REPORT: u32 = 49504;
const SEALED_BOTTLE: u32 = 49505;
const FISHING_ROD_CHEST: u32 = 49507;

quest!(Q00127, 127, "Q00127_FishingSpecialistsRequest", "Fishing Specialist's Request",
    start: [PIERRE], talk: [PIERRE, FERMA, BAIKAL], kills: Vec::new(), items: [PIERRE_LETTER, FISH_REPORT, SEALED_BOTTLE], {
    fn can_start(&self, p: &Player) -> Result<(), &'static str> {
        if p.level() < 20 { Err("30013-00.htm") } else { Ok(()) }
    }
    fn on_event(&self, c: &mut Ctx, event: &str) -> Option<String> {
        match event {
            "30013-02.html" => {
                c.qs.start_quest();
                c.p.give(PIERRE_LETTER, 1);
                page(event)
            }
            "teleport_to_ferma" => {
                c.p.tele(105276.0, 162500.0, -3600.0);
                None
            }
            _ => None,
        }
    }
    fn on_talk(&self, c: &mut Ctx) -> String {
        if c.qs.is_created() {
            return if c.npc != PIERRE {
                NO_QUEST
            } else if c.p.level() < 20 {
                "30013-00.htm"
            } else {
                "30013-01.htm"
            }
            .into();
        }
        if c.qs.is_completed() {
            return ALREADY_COMPLETED.into();
        }
        match (c.npc, c.qs.cond) {
            (PIERRE, 1 | 2) => "30013-03.html".into(),
            (PIERRE, 3) => {
                c.p.take(SEALED_BOTTLE, -1);
                c.p.give(FISHING_ROD_CHEST, 1);
                c.qs.exit_quest(false);
                "30013-04.html".into()
            }
            (FERMA, 1) => {
                c.p.take(PIERRE_LETTER, -1);
                c.p.give(FISH_REPORT, 1);
                c.qs.set_cond(2);
                "30015-01.html".into()
            }
            (FERMA, 2) => "30015-02.html".into(),
            (FERMA, 3) => "30015-03.html".into(),
            (BAIKAL, 2) => {
                c.p.take(FISH_REPORT, -1);
                c.p.give(SEALED_BOTTLE, 1);
                c.qs.set_cond(3);
                "30016-01.html".into()
            }
            (BAIKAL, 3) => "30016-02.html".into(),
            _ => NO_QUEST.into(),
        }
    }
});
