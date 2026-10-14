//! Q00344 1000 Years, the End of Lamentation and Q00500 Brothers Bound in
//! Chains, ported from L2J Mobius Classic.

use crate::items::ADENA;
use crate::quest;
use crate::quest::{Ctx, Player, ALREADY_COMPLETED, NO_QUEST};

fn page(s: &str) -> Option<String> {
    Some(s.to_string())
}

// ---------------------------------------------------------------------------
// Q00344 1000 Years, the End of Lamentation
// ---------------------------------------------------------------------------

const KAIEN: u32 = 30623;
const GARVARENTZ: u32 = 30704;
const GILMORE: u32 = 30754;
const RODEMAI: u32 = 30756;
const ORVEN: u32 = 30857;
const ARTICLES: u32 = 4269;
const OLD_KEY: u32 = 4270;
const OLD_HILT: u32 = 4271;
const TOTEM_NECKLACE: u32 = 4272;
const CRUCIFIX: u32 = 4273;
const CAVE_SERVANTS: [(u32, f64); 10] = [
    (20236, 0.58),
    (20238, 0.75),
    (20237, 0.78),
    (20239, 0.79),
    (20240, 0.85),
    (20272, 0.58),
    (20273, 0.78),
    (20274, 0.75),
    (20275, 0.79),
    (20276, 0.85),
];

/// Trades a relic for one of three rewards by roll thresholds (inclusive).
fn relic_reward(p: &mut Player, relic: u32, table: &[(u32, u32, u64)]) {
    p.take(relic, -1);
    let r = p.random(100);
    if let Some(&(_, item, n)) = table.iter().find(|(max, _, _)| r <= *max) {
        p.give(item, n);
    }
}

quest!(Q00344, 344, "Q00344_1000YearsTheEndOfLamentation", "1000 Years, the End of Lamentation",
    start: [GILMORE], talk: [KAIEN, GARVARENTZ, GILMORE, RODEMAI, ORVEN],
    kills: CAVE_SERVANTS.iter().map(|m| m.0).collect(),
    items: [ARTICLES, OLD_KEY, OLD_HILT, TOTEM_NECKLACE, CRUCIFIX], {
    fn can_start(&self, p: &Player) -> Result<(), &'static str> {
        if p.level() > 52 { Err(NO_QUEST) } else { Ok(()) }
    }
    fn on_event(&self, c: &mut Ctx, event: &str) -> Option<String> {
        match event {
            "30754-03.htm" | "30754-16.html" => page(event),
            "30754-04.htm" if c.qs.is_created() => {
                c.qs.start_quest();
                page(event)
            }
            "30754-08.html" if c.qs.is_cond(1) => {
                let count = c.p.count(ARTICLES);
                if count < 1 {
                    return page("30754-07.html");
                }
                c.p.take(ARTICLES, -1);
                if c.p.random(1000) as u64 >= count {
                    c.p.give(ADENA, count * 60);
                    return page(event);
                }
                c.qs.set_cond(2);
                let (memo, relic) = match c.p.random(4) {
                    0 => (1, OLD_HILT),
                    1 => (2, OLD_KEY),
                    2 => (3, TOTEM_NECKLACE),
                    _ => (4, CRUCIFIX),
                };
                c.qs.set_memo(memo);
                c.p.give(relic, 1);
                page("30754-09.html")
            }
            "30754-17.html" if c.qs.is_cond(1) => {
                c.qs.exit_quest(true);
                page(event)
            }
            "relic_info" => match c.qs.memo() {
                1 => page("30754-10.html"),
                2 => page("30754-11.html"),
                3 => page("30754-12.html"),
                4 => page("30754-13.html"),
                _ => None,
            },
            _ => None,
        }
    }
    fn on_talk(&self, c: &mut Ctx) -> String {
        let memo = c.qs.memo();
        match c.npc {
            GILMORE => {
                if c.qs.is_created() {
                    return if c.p.level() >= 48 { "30754-02.htm" } else { "30754-01.htm" }.into();
                }
                if !c.qs.is_started() {
                    return ALREADY_COMPLETED.into();
                }
                if c.qs.is_cond(1) {
                    return if c.p.has(ARTICLES) { "30754-06.html" } else { "30754-05.html" }.into();
                }
                if [OLD_KEY, OLD_HILT, TOTEM_NECKLACE, CRUCIFIX].iter().any(|&r| c.p.has(r)) {
                    return "30754-14.html".into();
                }
                c.qs.set_cond(1);
                "30754-15.html".into()
            }
            KAIEN if memo == 1 => {
                if !c.p.has(OLD_HILT) {
                    return "30623-02.html".into();
                }
                relic_reward(c.p, OLD_HILT, &[(52, 1874, 25), (76, 1887, 10), (98, 951, 1), (100, 133, 1)]);
                c.qs.set_cond(1);
                "30623-01.html".into()
            }
            RODEMAI if memo == 2 => {
                if !c.p.has(OLD_KEY) {
                    return "30756-02.html".into();
                }
                relic_reward(c.p, OLD_KEY, &[(39, 1879, 55), (89, 951, 1), (100, 885, 1)]);
                c.qs.set_cond(1);
                "30756-01.html".into()
            }
            GARVARENTZ if memo == 3 => {
                if !c.p.has(TOTEM_NECKLACE) {
                    return "30704-02.html".into();
                }
                relic_reward(c.p, TOTEM_NECKLACE, &[(47, 1882, 70), (97, 1881, 50), (100, 191, 1)]);
                c.qs.set_cond(1);
                "30704-01.html".into()
            }
            ORVEN if memo == 4 => {
                if !c.p.has(CRUCIFIX) {
                    return "30857-02.html".into();
                }
                relic_reward(c.p, CRUCIFIX, &[(49, 1875, 19), (69, 952, 5), (100, 2437, 1)]);
                c.qs.set_cond(1);
                "30857-01.html".into()
            }
            _ => NO_QUEST.into(),
        }
    }
    fn on_kill(&self, c: &mut Ctx, npc: u32) {
        let chance = CAVE_SERVANTS.iter().find(|m| m.0 == npc).map_or(0.0, |m| m.1);
        if c.qs.is_started() {
            c.p.give_randomly(ARTICLES, 1, 0, chance);
        }
    }
});

// ---------------------------------------------------------------------------
// Q00500 Brothers Bound in Chains
// ---------------------------------------------------------------------------

/// The Dark Judge absolves player killers. Offline there is no PvP, so
/// the hero never has PK kills and the judge always answers with his
/// "no sins" page, as the server does for a lawful player.
const DARK_JUDGE: u32 = 30981;

quest!(Q00500, 500, "Q00500_BrothersBoundInChains", "Brothers Bound in Chains",
    start: [DARK_JUDGE], talk: [DARK_JUDGE], kills: Vec::new(), items: [70806, 70807], {
    fn on_event(&self, _c: &mut Ctx, event: &str) -> Option<String> {
        match event {
            "30981-02.htm" | "30981-03.htm" => page(event),
            _ => page(NO_QUEST),
        }
    }
    fn on_talk(&self, c: &mut Ctx) -> String {
        if c.qs.is_created() { "30981-nopk.htm".into() } else { NO_QUEST.into() }
    }
});
