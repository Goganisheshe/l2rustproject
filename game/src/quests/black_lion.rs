//! Q00333 Hunt of the Black Lion, ported from L2J Mobius Classic: Captain
//! Sophya's four hunting orders, cargo boxes opened by Reedfoot and Morgon,
//! and the Shilen statue / ancient tablet side trades.

use crate::items::ADENA;
use crate::quest;
use crate::quest::{Ctx, Player, NO_QUEST};

fn page(s: &str) -> Option<String> {
    Some(s.to_string())
}

const UNDRIAS: u32 = 30130;
const RUPIO: u32 = 30471;
const LOCKIRIN: u32 = 30531;
const SOPHYA: u32 = 30735;
const REEDFOOT: u32 = 30736;
const MORGON: u32 = 30737;

const BLACK_LION_MARK: u32 = 1369;
const CARGO: [u32; 4] = [3440, 3441, 3442, 3443];
const STATUE: [u32; 4] = [3457, 3458, 3459, 3460];
const COMPLETE_STATUE_OF_SHILEN: u32 = 3461;
const TABLET: [u32; 4] = [3462, 3463, 3464, 3465];
const COMPLETE_ANCIENT_TABLET: u32 = 3466;
const ORDERS: [u32; 4] = [3671, 3672, 3673, 3674];
const LIONS_CLAW: u32 = 3675;
const LIONS_EYE: u32 = 3676;
const GUILD_COIN: u32 = 3677;
const UNDEAD_ASH: u32 = 3848;
const BLOODY_AXE_INSIGNIA: u32 = 3849;
const DELU_LIZARDMAN_FANG: u32 = 3850;
const STAKATO_TALON: u32 = 3851;
const TROPHIES: [u32; 4] = [UNDEAD_ASH, BLOODY_AXE_INSIGNIA, DELU_LIZARDMAN_FANG, STAKATO_TALON];

const ALACRITY_POTION: u32 = 735;
const SCROLL_OF_ESCAPE: u32 = 736;
const HEALING_POTION: u32 = 1061;
const SOULSHOT_D: u32 = 1463;
const SPIRITSHOT_D: u32 = 2510;
/// Reedfoot's cargo contents: (chance below, [items], [pages]).
const CARGO_GOODS: [(u32, [u32; 3], [&str; 3]); 4] = [
    (40, [3444, 3445, 3446], ["30736-04a.html", "30736-04b.html", "30736-04c.html"]),
    (60, [3447, 3448, 3449], ["30736-04d.html", "30736-04e.html", "30736-04f.html"]),
    (70, [3450, 3451, 3452], ["30736-04g.html", "30736-04h.html", "30736-04i.html"]),
    (75, [3453, 3454, 3455], ["30736-04j.html", "30736-04k.html", "30736-04l.html"]),
];
const IMPERIAL_DIAMOND: u32 = 3456;

const MARSH_STAKATO: u32 = 20157;
const MARSH_STAKATO_WORKER: u32 = 20230;
const MARSH_STAKATO_SOLDIER: u32 = 20232;
const MARSH_STAKATO_DRONE: u32 = 20234;
const DELU_LIZARDMAN: u32 = 20251;
const DELU_LIZARDMAN_SCOUT: u32 = 20252;
const DELU_LIZARDMAN_WARRIOR: u32 = 20253;
const DELU_LIZARDMAN_HEADHUNTER: u32 = 27151;
const MARSH_STAKATO_MARQUESS: u32 = 27152;

/// Ordinary drops: monster, order index, trophy chance (out of 100),
/// cargo chance.
const HUNT: [(u32, usize, u32, u32); 17] = [
    (MARSH_STAKATO, 3, 55, 12),
    (20160, 0, 50, 11), // Neer Crawler
    (20171, 0, 60, 8),  // Specter
    (20197, 0, 60, 9),  // Sorrow Maiden
    (20198, 0, 50, 12), // Neer Crawler Berserker
    (20200, 0, 50, 13), // Strain
    (20201, 0, 50, 15), // Ghoul
    (20207, 1, 50, 9),  // Ol Mahum Guerilla
    (20208, 1, 50, 10), // Ol Mahum Raider
    (20209, 1, 50, 11), // Ol Mahum Marksman
    (20210, 1, 50, 12), // Ol Mahum Sergeant
    (20211, 1, 50, 13), // Ol Mahum Captain
    (MARSH_STAKATO_WORKER, 3, 60, 13),
    (MARSH_STAKATO_SOLDIER, 3, 56, 14),
    (MARSH_STAKATO_DRONE, 3, 60, 15),
    (DELU_LIZARDMAN, 2, 50, 14),
    (DELU_LIZARDMAN_SCOUT, 2, 50, 14),
];

fn sum(p: &Player, items: &[u32]) -> u64 {
    items.iter().map(|&i| p.count(i)).sum()
}

fn has_all(p: &Player, items: &[u32]) -> bool {
    items.iter().all(|&i| p.has(i))
}

/// Removes the first cargo box the hero carries.
fn take_one_cargo(p: &mut Player) {
    if let Some(&b) = CARGO.iter().find(|&&b| p.has(b)) {
        p.take(b, 1);
    }
}

/// Sophya's prize for ten Lion's Claws.
fn claw_prize(p: &mut Player, chance: u32, tier: usize) {
    let heal = [20, 25, 50][tier];
    let (ss, sps) = [(100, 50), (200, 100), (400, 200)][tier];
    let soe = [20, 20, 30][tier];
    let alacrity = [3, 3, 4][tier];
    if chance < 25 {
        p.give(HEALING_POTION, heal);
    } else if chance < 50 {
        if p.is_mage() {
            p.give(SPIRITSHOT_D, sps);
        } else {
            p.give(SOULSHOT_D, ss);
        }
    } else if chance < 75 {
        p.give(SCROLL_OF_ESCAPE, soe);
    } else {
        p.give(ALACRITY_POTION, alacrity);
    }
    p.take(LIONS_CLAW, 10);
}

quest!(Q00333, 333, "Q00333_HuntOfTheBlackLion", "Hunt of the Black Lion",
    start: [SOPHYA], talk: [SOPHYA, UNDRIAS, RUPIO, LOCKIRIN, REEDFOOT, MORGON],
    kills: HUNT.iter().map(|h| h.0).chain([DELU_LIZARDMAN_WARRIOR, DELU_LIZARDMAN_HEADHUNTER, MARSH_STAKATO_MARQUESS]).collect(),
    items: [BLACK_LION_MARK, 3440, 3441, 3442, 3443, 3457, 3458, 3459, 3460, COMPLETE_STATUE_OF_SHILEN, 3462, 3463, 3464, 3465,
            COMPLETE_ANCIENT_TABLET, 3671, 3672, 3673, 3674, LIONS_CLAW, LIONS_EYE, GUILD_COIN, UNDEAD_ASH, BLOODY_AXE_INSIGNIA,
            DELU_LIZARDMAN_FANG, STAKATO_TALON], {
    fn can_start(&self, p: &Player) -> Result<(), &'static str> {
        if p.level() > 39 { Err(NO_QUEST) } else { Ok(()) }
    }
    fn on_event(&self, c: &mut Ctx, event: &str) -> Option<String> {
        let chance = c.p.random(100);
        let chance1 = c.p.random(100);
        let order_page = ["30735-10.html", "30735-11.html", "30735-12.html", "30735-13.html"];
        if let Some(i) = order_page.iter().position(|p| *p == event) {
            if c.p.has(ORDERS[i]) {
                return None;
            }
            c.p.give(ORDERS[i], 1);
            return page(event);
        }
        match event {
            "30735-04.htm" if c.qs.is_created() => {
                c.qs.start_quest();
                page(event)
            }
            "30735-05.html" | "30735-06.html" | "30735-07.html" | "30735-08.html" | "30735-09.html" | "30130-05.html"
            | "30531-05.html" | "30735-21.html" | "30735-24a.html" | "30735-25b.html" | "30736-06.html" | "30736-09.html"
            | "30737-07.html" => page(event),
            "30735-16.html" => {
                let claws = c.p.count(LIONS_CLAW);
                let eyes = c.p.count(LIONS_EYE);
                if claws < 10 {
                    page(event)
                } else if eyes < 4 {
                    c.p.give(LIONS_EYE, 1);
                    claw_prize(c.p, chance, 0);
                    page("30735-17a.html")
                } else if eyes <= 7 {
                    c.p.give(LIONS_EYE, 1);
                    claw_prize(c.p, chance, 1);
                    page("30735-18b.html")
                } else {
                    c.p.take(LIONS_EYE, 8);
                    claw_prize(c.p, chance, 2);
                    page("30735-19b.html")
                }
            }
            "30735-20.html" => {
                c.p.take_all(&ORDERS);
                page(event)
            }
            "30735-26.html" if c.p.has(BLACK_LION_MARK) => {
                c.p.give(ADENA, 12400);
                c.qs.exit_quest(true);
                page(event)
            }
            "30130-04.html" if c.p.has(COMPLETE_STATUE_OF_SHILEN) => {
                c.p.give(ADENA, 30000);
                c.p.take(COMPLETE_STATUE_OF_SHILEN, 1);
                page(event)
            }
            "30471-03.html" => {
                if !has_all(c.p, &STATUE) {
                    return page(event);
                }
                let ok = c.p.random(100) < 50;
                if ok {
                    c.p.give(COMPLETE_STATUE_OF_SHILEN, 1);
                }
                for s in STATUE {
                    c.p.take(s, 1);
                }
                page(if ok { "30471-04.html" } else { "30471-05.html" })
            }
            "30471-06.html" => {
                if !has_all(c.p, &TABLET) {
                    return page(event);
                }
                let ok = c.p.random(100) < 50;
                if ok {
                    c.p.give(COMPLETE_ANCIENT_TABLET, 1);
                }
                for t in TABLET {
                    c.p.take(t, 1);
                }
                page(if ok { "30471-07.html" } else { "30471-08.html" })
            }
            "30531-04.html" if c.p.has(COMPLETE_ANCIENT_TABLET) => {
                c.p.give(ADENA, 30000);
                c.p.take(COMPLETE_ANCIENT_TABLET, 1);
                page(event)
            }
            "30736-03.html" => {
                let boxes = sum(c.p, &CARGO);
                if boxes < 1 {
                    return page("30736-05.html");
                }
                if c.p.count(ADENA) < 650 {
                    return page(event);
                }
                c.p.take(ADENA, 650);
                take_one_cargo(c.p);
                let pick = if chance1 < 33 { 0 } else if chance1 < 66 { 1 } else { 2 };
                if let Some((_, items, pages)) = CARGO_GOODS.iter().find(|g| chance < g.0) {
                    c.p.give(items[pick], 1);
                    return page(pages[pick]);
                }
                if chance < 76 {
                    c.p.give(IMPERIAL_DIAMOND, 1);
                    return page("30736-04m.html");
                }
                let quarter = (chance1 / 25).min(3) as usize;
                if c.p.random(100) < 50 {
                    c.p.give(STATUE[quarter], 1);
                    page("30736-04n.html")
                } else {
                    c.p.give(TABLET[quarter], 1);
                    page("30736-04o.html")
                }
            }
            "30736-07.html" => {
                let memo = c.qs.memo();
                let cost = (200 + memo * 200) as u64;
                if c.p.count(ADENA) < cost {
                    page(event)
                } else if memo * 100 > 200 {
                    page("30736-08.html")
                } else {
                    // One of twenty rumours, 5% each.
                    let letter = (b'a' + (chance / 5).min(19) as u8) as char;
                    c.p.take(ADENA, cost as i64);
                    c.qs.set_memo(memo + 1);
                    Some(format!("30736-08{letter}.html"))
                }
            }
            "30737-06.html" => {
                if sum(c.p, &CARGO) < 1 {
                    return page(event);
                }
                take_one_cargo(c.p);
                if c.p.count(GUILD_COIN) < 80 {
                    c.p.give(GUILD_COIN, 1);
                } else {
                    c.p.take(GUILD_COIN, 80);
                }
                let coins = c.p.count(GUILD_COIN);
                if coins < 40 {
                    c.p.give(ADENA, 100);
                    page("30737-03.html")
                } else if coins < 80 {
                    c.p.give(ADENA, 200);
                    page("30737-04.html")
                } else {
                    c.p.give(ADENA, 300);
                    page("30737-05.html")
                }
            }
            _ => None,
        }
    }
    fn on_kill(&self, c: &mut Ctx, npc: u32) {
        if !c.qs.is_started() {
            return;
        }
        let here = c.p.kill_pos;
        match npc {
            DELU_LIZARDMAN_HEADHUNTER => {
                if c.p.has(ORDERS[2]) {
                    c.p.give(DELU_LIZARDMAN_FANG, 4);
                }
            }
            MARSH_STAKATO_MARQUESS => {
                if c.p.has(ORDERS[3]) {
                    c.p.give(STAKATO_TALON, 8);
                }
            }
            DELU_LIZARDMAN_WARRIOR => {
                if c.p.has(ORDERS[2]) {
                    if c.p.random(2) == 0 {
                        c.p.give(DELU_LIZARDMAN_FANG, 1);
                    }
                    if c.p.random(100) < 15 {
                        c.p.give(CARGO[2], 1);
                    }
                }
                if c.p.random(100) < 3 && c.p.has(ORDERS[2]) {
                    c.p.spawns.extend([(DELU_LIZARDMAN_HEADHUNTER, here), (DELU_LIZARDMAN_HEADHUNTER, here)]);
                }
            }
            _ => {
                let Some(&(_, order, trophy, cargo)) = HUNT.iter().find(|h| h.0 == npc) else { return };
                if c.p.has(ORDERS[order]) {
                    if c.p.random(100) < trophy {
                        c.p.give(TROPHIES[order], 1);
                    }
                    if c.p.random(100) < cargo {
                        c.p.give(CARGO[order], 1);
                    }
                    if order == 3 && c.p.random(100) < 2 {
                        c.p.spawns.push((MARSH_STAKATO_MARQUESS, here));
                    }
                }
                if (npc == DELU_LIZARDMAN || npc == DELU_LIZARDMAN_SCOUT) && c.p.random(100) < 3 && c.p.has(ORDERS[2]) {
                    c.p.spawns.extend([(DELU_LIZARDMAN_HEADHUNTER, here), (DELU_LIZARDMAN_HEADHUNTER, here)]);
                }
            }
        }
    }
    fn on_talk(&self, c: &mut Ctx) -> String {
        if c.qs.is_created() {
            if c.npc != SOPHYA {
                return NO_QUEST.into();
            }
            return if c.p.level() < 25 {
                "30735-01.htm"
            } else if !c.p.has(BLACK_LION_MARK) {
                "30735-02.htm"
            } else {
                "30735-03.htm"
            }
            .into();
        }
        if !c.qs.is_started() {
            return NO_QUEST.into();
        }
        let orders = sum(c.p, &ORDERS);
        let trophies = sum(c.p, &TROPHIES);
        let boxes = sum(c.p, &CARGO);
        match c.npc {
            SOPHYA => {
                if orders == 0 {
                    return "30735-14.html".into();
                }
                if orders != 1 {
                    return NO_QUEST.into();
                }
                if trophies < 1 {
                    return if boxes < 1 { "30735-15.html" } else { "30735-15a.html" }.into();
                }
                let claws = if trophies < 20 {
                    0
                } else if trophies < 50 {
                    1
                } else if trophies < 100 {
                    2
                } else {
                    3
                };
                if claws > 0 {
                    c.p.give(LIONS_CLAW, claws);
                }
                let (ash, insignia, fang, talon) =
                    (c.p.count(UNDEAD_ASH), c.p.count(BLOODY_AXE_INSIGNIA), c.p.count(DELU_LIZARDMAN_FANG), c.p.count(STAKATO_TALON));
                let out = if boxes == 0 {
                    // The Java formula, `fang + 7` included.
                    c.p.give(ADENA, ash * 10 + insignia * 10 + (fang + 7) + talon * 8);
                    "30735-22.html"
                } else {
                    c.p.give(ADENA, ash * 10 + insignia * 10 + fang * 7 + talon * 8);
                    "30735-23.html"
                };
                c.p.take_all(&TROPHIES);
                c.qs.set_memo(0);
                out.into()
            }
            UNDRIAS => {
                if c.p.has(COMPLETE_STATUE_OF_SHILEN) {
                    "30130-03.html"
                } else if sum(c.p, &STATUE) >= 1 {
                    "30130-02.html"
                } else {
                    "30130-01.html"
                }
                .into()
            }
            RUPIO => if sum(c.p, &STATUE) >= 1 || sum(c.p, &TABLET) >= 1 { "30471-02.html" } else { "30471-01.html" }.into(),
            LOCKIRIN => {
                if c.p.has(COMPLETE_ANCIENT_TABLET) {
                    "30531-03.html"
                } else if sum(c.p, &TABLET) >= 1 {
                    "30531-02.html"
                } else {
                    "30531-01.html"
                }
                .into()
            }
            REEDFOOT => if boxes >= 1 { "30736-02.html" } else { "30736-01.html" }.into(),
            MORGON => if boxes >= 1 { "30737-02.html" } else { "30737-01.html" }.into(),
            _ => NO_QUEST.into(),
        }
    }
});
