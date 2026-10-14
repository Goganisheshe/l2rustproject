//! Q00327 Recover the Farmland, ported from L2J Mobius Classic.

use crate::items::ADENA;
use crate::quest;
use crate::quest::{Ctx, Player, NO_QUEST};

fn page(s: &str) -> Option<String> {
    Some(s.to_string())
}

const IRIS: u32 = 30034;
const ASHA: u32 = 30313;
const NESTLE: u32 = 30314;
const LEIKAN: u32 = 30382;
const PIOTUR: u32 = 30597;
const TUREK_ORK_WARLORD: u32 = 20495;
const TUREK_ORK_SHAMAN: u32 = 20501;
const TUREK_DOG_TAG: u32 = 1846;
const TUREK_MEDALLION: u32 = 1847;
const LEIKANS_LETTER: u32 = 5012;
const CLAY_URN_FRAGMENT: u32 = 1848;
const BRASS_TRINKET_PIECE: u32 = 1849;
const BRONZE_MIRROR_PIECE: u32 = 1850;
const JADE_NECKLACE_BEAD: u32 = 1851;
const ANCIENT_CLAY_URN: u32 = 1852;
const ANCIENT_BRASS_TIARA: u32 = 1853;
const ANCIENT_BRONZE_MIRROR: u32 = 1854;
const ANCIENT_JADE_NECKLACE: u32 = 1855;
const QUICK_STEP_POTION: u32 = 734;
const SWIFT_ATTACK_POTION: u32 = 735;
const SCROLL_OF_ESCAPE: u32 = 736;
const SCROLL_OF_RESURRECTION: u32 = 737;
const HEALING_POTION: u32 = 1061;
const SOULSHOT_D: u32 = 1463;
const SPIRITSHOT_D: u32 = 2510;

/// Iris pays experience per fragment.
const FRAGMENT_EXP: [(&str, u32, u64); 4] = [
    ("30034-03.html", CLAY_URN_FRAGMENT, 307),
    ("30034-04.html", BRASS_TRINKET_PIECE, 368),
    ("30034-05.html", BRONZE_MIRROR_PIECE, 368),
    ("30034-06.html", JADE_NECKLACE_BEAD, 430),
];
const FULL_EXP: [(u32, u64); 4] = [(ANCIENT_CLAY_URN, 2766), (ANCIENT_BRASS_TIARA, 3227), (ANCIENT_BRONZE_MIRROR, 3227), (ANCIENT_JADE_NECKLACE, 3919)];
/// Asha restores an antique from five fragments: (event, fragment, antique,
/// roll range, success below, missing page).
const RESTORE: [(&str, u32, u32, u32, u32, &str); 4] = [
    ("30313-03.html", CLAY_URN_FRAGMENT, ANCIENT_CLAY_URN, 6, 5, "30313-02.html"),
    ("30313-05.html", BRASS_TRINKET_PIECE, ANCIENT_BRASS_TIARA, 7, 6, "30313-04.html"),
    ("30313-07.html", BRONZE_MIRROR_PIECE, ANCIENT_BRONZE_MIRROR, 7, 6, "30313-06.html"),
    ("30313-09.html", JADE_NECKLACE_BEAD, ANCIENT_JADE_NECKLACE, 8, 7, "30313-08.html"),
];
const FRAGMENT_DROPS: [(u32, u32); 7] =
    [(20496, 21), (20499, 19), (20500, 18), (TUREK_ORK_SHAMAN, 22), (20497, 21), (20498, 20), (TUREK_ORK_WARLORD, 26)];

fn rand_between(p: &Player, a: u32, b: u32) -> u32 {
    a + p.random(b - a + 1)
}

quest!(Q00327, 327, "Q00327_RecoverTheFarmland", "Recover the Farmland",
    start: [LEIKAN, PIOTUR], talk: [LEIKAN, PIOTUR, IRIS, ASHA, NESTLE],
    kills: FRAGMENT_DROPS.iter().map(|m| m.0).collect(), items: [TUREK_DOG_TAG, TUREK_MEDALLION, LEIKANS_LETTER], {
    fn can_start(&self, p: &Player) -> Result<(), &'static str> {
        if p.level() > 34 { Err(NO_QUEST) } else { Ok(()) }
    }
    fn on_event(&self, c: &mut Ctx, event: &str) -> Option<String> {
        if let Some(&(_, item, exp)) = FRAGMENT_EXP.iter().find(|f| f.0 == event) {
            if !c.p.has(item) {
                return page("30034-02.html");
            }
            let n = c.p.count(item);
            c.p.add_exp_sp(n * exp, 0);
            c.p.take(item, -1);
            return page(event);
        }
        if let Some(&(_, fragment, antique, range, below, missing)) = RESTORE.iter().find(|r| r.0 == event) {
            if c.p.count(fragment) < 5 {
                return page(missing);
            }
            c.p.take(fragment, 5);
            if c.p.random(range) < below {
                c.p.give(antique, 1);
                return page(event);
            }
            return page("30313-10.html");
        }
        match event {
            "30034-01.html" | "30313-01.html" | "30314-02.html" | "30314-08.html" | "30314-09.html" | "30382-05a.html"
            | "30382-05b.html" | "30597-03.html" | "30597-07.html" => page(event),
            "30382-03.htm" => {
                c.qs.start_quest();
                c.p.give(LEIKANS_LETTER, 1);
                c.qs.set_cond(2);
                page(event)
            }
            "30597-03.htm" => {
                c.qs.start_quest();
                page(event)
            }
            "30597-06.html" => {
                c.qs.exit_quest(true);
                page(event)
            }
            "30034-07.html" => {
                let mut rewarded = false;
                for (item, exp) in FULL_EXP {
                    if c.p.has(item) {
                        let n = c.p.count(item);
                        c.p.add_exp_sp(n * exp, 0);
                        c.p.take(item, -1);
                        rewarded = true;
                    }
                }
                page(if rewarded { event } else { "30034-02.html" })
            }
            "30314-03.html" => {
                if !c.p.has(ANCIENT_CLAY_URN) {
                    return page("30314-07.html");
                }
                let n = rand_between(c.p, 70, 110);
                c.p.give(SOULSHOT_D, n as u64);
                c.p.take(ANCIENT_CLAY_URN, 1);
                page(event)
            }
            "30314-04.html" => {
                if !c.p.has(ANCIENT_BRASS_TIARA) {
                    return page("30314-07.html");
                }
                let r = c.p.random(100);
                let item = if r < 40 {
                    HEALING_POTION
                } else if r < 84 {
                    QUICK_STEP_POTION
                } else {
                    SWIFT_ATTACK_POTION
                };
                c.p.give(item, 1);
                c.p.take(ANCIENT_BRASS_TIARA, 1);
                page(event)
            }
            "30314-05.html" => {
                if !c.p.has(ANCIENT_BRONZE_MIRROR) {
                    return page("30314-07.html");
                }
                let item = if c.p.random(100) < 59 { SCROLL_OF_ESCAPE } else { SCROLL_OF_RESURRECTION };
                c.p.give(item, 1);
                c.p.take(ANCIENT_BRONZE_MIRROR, 1);
                page(event)
            }
            "30314-06.html" => {
                if !c.p.has(ANCIENT_JADE_NECKLACE) {
                    return page("30314-07.html");
                }
                let n = rand_between(c.p, 50, 90);
                c.p.give(SPIRITSHOT_D, n as u64);
                c.p.take(ANCIENT_JADE_NECKLACE, 1);
                page(event)
            }
            _ => None,
        }
    }
    fn on_kill(&self, c: &mut Ctx, npc: u32) {
        if !c.qs.is_started() {
            return;
        }
        if npc == TUREK_ORK_SHAMAN || npc == TUREK_ORK_WARLORD {
            c.p.give(TUREK_MEDALLION, 1);
        } else {
            c.p.give(TUREK_DOG_TAG, 1);
        }
        let chance = FRAGMENT_DROPS.iter().find(|m| m.0 == npc).map_or(0, |m| m.1);
        if c.p.random(100) < chance {
            let item = rand_between(c.p, CLAY_URN_FRAGMENT, JADE_NECKLACE_BEAD);
            c.p.give(item, 1);
        }
    }
    fn on_talk(&self, c: &mut Ctx) -> String {
        let level_ok = c.p.level() >= 25;
        match c.npc {
            LEIKAN if c.qs.is_created() => if level_ok { "30382-02.htm" } else { "30382-01.htm" }.into(),
            LEIKAN if c.qs.is_started() => {
                if c.p.has(LEIKANS_LETTER) {
                    "30382-04.html".into()
                } else {
                    c.qs.set_cond(5);
                    "30382-05.html".into()
                }
            }
            PIOTUR if c.qs.is_created() => if level_ok { "30597-02.htm" } else { "30597-01.htm" }.into(),
            PIOTUR if c.qs.is_started() => {
                if c.p.has(LEIKANS_LETTER) {
                    c.p.take(LEIKANS_LETTER, -1);
                    c.qs.set_cond(3);
                    return "30597-03a.htm".into();
                }
                let tags = c.p.count(TUREK_DOG_TAG);
                let medals = c.p.count(TUREK_MEDALLION);
                if tags == 0 && medals == 0 {
                    return "30597-04.html".into();
                }
                c.p.give(ADENA, tags * 8 + medals * 8 + if tags + medals >= 10 { 1000 } else { 0 });
                c.p.take_all(&[TUREK_DOG_TAG, TUREK_MEDALLION]);
                c.qs.set_cond(4);
                "30597-05.html".into()
            }
            IRIS if c.qs.is_started() => "30034-01.html".into(),
            ASHA if c.qs.is_started() => "30313-01.html".into(),
            NESTLE if c.qs.is_started() => "30314-01.html".into(),
            _ => NO_QUEST.into(),
        }
    }
});
