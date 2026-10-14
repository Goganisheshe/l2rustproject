//! Q00662 A Game of Cards, ported from L2J Mobius Classic: Klump deals five
//! cards for 50 Red Gems; matching symbols win prizes.

use crate::quest;
use crate::quest::{Ctx, QuestState, ALREADY_COMPLETED, NO_QUEST};

fn page(s: &str) -> Option<String> {
    Some(s.to_string())
}

const KLUMP: u32 = 30845;
const RED_GEM: u32 = 8765;
const ZIGGOS_GEMSTONE: u32 = 8868;
const CHIPS: u64 = 50;
const MONSTERS: [(u32, u32); 24] = [
    (20672, 357),
    (20673, 357),
    (20674, 583),
    (20677, 435),
    (20955, 358),
    (20958, 283),
    (20959, 455),
    (20961, 365),
    (20962, 348),
    (20965, 457),
    (20966, 493),
    (20968, 418),
    (20972, 350),
    (20973, 453),
    (21002, 315),
    (21004, 320),
    (21006, 335),
    (21008, 462),
    (21010, 397),
    (21109, 507),
    (21112, 552),
    (21114, 587),
    (21116, 812),
    (20142, 232),
];

fn script_page(file: &str) -> String {
    let path = crate::datapack::datapack_root().join("scripts/quests/Q00662_AGameOfCards").join(file);
    std::fs::read_to_string(path).unwrap_or_default()
}

fn symbol(v: i64) -> &'static str {
    ["ERROR", "!", "=", "T", "V", "O", "P", "S", "E", "H", "A", "R", "D", "I", "N"].get(v as usize).copied().unwrap_or("ERROR")
}

/// The five cards (1..=14) and the turned-card mask.
fn cards(qs: &QuestState) -> ([i64; 5], i64) {
    let v1 = qs.get_int("v1");
    let ex = qs.get_int("ExMemoState");
    ([v1 % 100, (v1 % 10000) / 100, (v1 % 1_000_000) / 10000, (v1 % 100_000_000) / 1_000_000, ex % 100], ex / 100)
}

/// Shows turned cards face up and the rest as "?".
fn reveal(mut html: String, cards: [i64; 5], mask: i64) -> String {
    for (i, card) in cards.iter().enumerate() {
        let up = (mask >> i) & 1 == 1;
        html = html.replace(&format!("FontColor{}", i + 1), if up { "FF6F6F" } else { "FFFF00" });
        html = html.replace(&format!("Cell{}", i + 1), if up { symbol(*card) } else { "?" });
    }
    html
}

/// Klump's scoring: tens for pairs with the first card, then pairs among
/// the rest (ones once a pair is found), exactly as the Java script counts.
fn score(c: [i64; 5]) -> i64 {
    let [i1, i2, i3, i4, i5] = c;
    let (mut i6, mut i8) = (0i64, 0i64);
    if !c.iter().all(|v| (1..=14).contains(v)) {
        return 0;
    }
    for (other, bit) in [(i2, 8), (i3, 4), (i4, 2), (i5, 1)] {
        if i1 == other {
            i6 += 10;
            i8 += bit;
        }
    }
    // Later pairs add 10 while no pair was found yet, else 1 (once a pair
    // with the first card exists); `i8` marks cards already matched.
    let inc = |i6: i64| if i6 % 100 < 10 { Some(10) } else if i6 % 10 == 0 { Some(1) } else { None };
    if let Some(d) = inc(i6) {
        if i8 % 16 < 8 {
            if i8 % 8 < 4 && i2 == i3 {
                i6 += d;
                i8 += 4;
            }
            if i8 % 4 < 2 && i2 == i4 {
                i6 += d;
                i8 += 2;
            }
            if i8 % 2 < 1 && i2 == i5 {
                i6 += d;
                i8 += 1;
            }
        }
    }
    if let Some(d) = inc(i6) {
        if i8 % 8 < 4 {
            if i8 % 4 < 2 && i3 == i4 {
                i6 += d;
                i8 += 2;
            }
            if i8 % 2 < 1 && i3 == i5 {
                i6 += d;
                i8 += 1;
            }
        }
    }
    if let Some(d) = inc(i6) {
        if i8 % 4 < 2 && i8 % 2 < 1 && i4 == i5 {
            i6 += d;
        }
    }
    i6
}

quest!(Q00662, 662, "Q00662_AGameOfCards", "A Game of Cards",
    start: [KLUMP], talk: [KLUMP], kills: MONSTERS.iter().map(|m| m.0).collect(), items: [], {
    fn on_event(&self, c: &mut Ctx, event: &str) -> Option<String> {
        match event {
            "30845-03.htm" if c.p.level() >= 61 => {
                if c.qs.is_created() {
                    c.qs.start_quest();
                }
                page(event)
            }
            "30845-06.html" | "30845-08.html" | "30845-09.html" | "30845-09a.html" | "30845-09b.html" | "30845-10.html" => page(event),
            "30845-07.html" => {
                c.qs.exit_quest(true);
                page(event)
            }
            "return" => page(if c.p.count(RED_GEM) < CHIPS { "30845-04.html" } else { "30845-05.html" }),
            "30845-11.html" if c.p.count(RED_GEM) >= CHIPS => {
                // Five distinct draws from 1..=70, folded onto 14 symbols.
                let mut v = [0i64; 5];
                loop {
                    for x in v.iter_mut() {
                        *x = c.p.random(70) as i64 + 1;
                    }
                    let distinct = (0..5).all(|a| (a + 1..5).all(|b| v[a] != v[b]));
                    if distinct {
                        break;
                    }
                }
                for x in v.iter_mut() {
                    *x = (*x - 1) % 14 + 1;
                }
                c.qs.set("v1", v[3] * 1_000_000 + v[2] * 10000 + v[1] * 100 + v[0]);
                c.qs.set("ExMemoState", v[4]);
                c.p.take(RED_GEM, CHIPS as i64);
                page(event)
            }
            "turncard1" | "turncard2" | "turncard3" | "turncard4" | "turncard5" => {
                let (cards, mut mask) = cards(c.qs);
                let bit = 1 << (event.as_bytes()[8] - b'1');
                mask |= bit;
                if mask % 32 < 31 {
                    c.qs.set("ExMemoState", mask * 100 + cards[4]);
                    return Some(reveal(script_page("30845-12.html"), cards, mask));
                }
                let s = score(cards);
                let (rewards, file): (&[(u32, u64)], &str) = match s {
                    40 => (&[(ZIGGOS_GEMSTONE, 43), (959, 3), (729, 1)], "30845-13.html"),
                    30 => (&[(959, 2), (951, 2)], "30845-14.html"),
                    21 | 12 => (&[(729, 1), (947, 2), (955, 1)], "30845-15.html"),
                    20 => (&[(951, 2)], "30845-16.html"),
                    11 => (&[(951, 1)], "30845-17.html"),
                    10 => (&[(956, 2)], "30845-18.html"),
                    _ => (&[], "30845-19.html"),
                };
                for &(item, n) in rewards {
                    c.p.give(item, n);
                }
                c.qs.set("ExMemoState", 0);
                c.qs.set("v1", 0);
                Some(reveal(script_page(file), cards, mask))
            }
            "playagain" => page(if c.p.count(RED_GEM) < CHIPS { "30845-21.html" } else { "30845-20.html" }),
            _ => None,
        }
    }
    fn on_talk(&self, c: &mut Ctx) -> String {
        if c.qs.is_created() {
            return if c.p.level() < 61 { "30845-02.html" } else { "30845-01.htm" }.into();
        }
        if c.qs.is_completed() {
            return ALREADY_COMPLETED.into();
        }
        if c.qs.is_cond(1) {
            return if c.p.count(RED_GEM) < CHIPS { "30845-04.html" } else { "30845-05.html" }.into();
        }
        if c.qs.get_int("ExMemoState") != 0 {
            let (cards, mask) = cards(c.qs);
            return reveal(script_page("30845-11a.html"), cards, mask);
        }
        NO_QUEST.into()
    }
    fn on_kill(&self, c: &mut Ctx, npc: u32) {
        // The Java passes the per-mille chance as a multiplier and would hand
        // out hundreds of gems per kill; here it is a chance out of 1000.
        let chance = MONSTERS.iter().find(|m| m.0 == npc).map_or(0, |m| m.1);
        if c.qs.is_started() && c.p.random(1000) < chance {
            c.p.give(RED_GEM, 1);
        }
    }
});
