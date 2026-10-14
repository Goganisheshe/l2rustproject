//! Q00255 Tutorial, ported from L2J Mobius Classic: new characters start in
//! their race's training ground, get the tutorial pages, recover the Blue
//! Gemstone from the gremlins for the Newbie Helper, then the supervisor
//! sends them to the village. Question marks of the client tutorial open
//! their page directly here.

use crate::quest::{Ctx, Quest, Race};

pub struct Tutorial;

pub const TUTORIAL: u32 = 255;
const NEWBIE_HELPERS: [u32; 7] = [30009, 30019, 30400, 30131, 30575, 30530, 34108];
const SUPERVISORS: [u32; 7] = [30008, 30017, 30370, 30129, 30573, 30528, 34109];
const GREMLINS: [u32; 2] = [18342, 20001];
const BLUE_GEM: u32 = 6353;
const SOULSHOTS: (u32, u64) = (91927, 200);
const SPIRITSHOTS: (u32, u64) = (91928, 100);
const SCROLL_OF_ESCAPE: (u32, u64) = (10650, 5);
const WIND_WALK_POTION: (u32, u64) = (49036, 5);

/// First tutorial page per starting class.
pub fn start_page(class_id: u32) -> Option<&'static str> {
    Some(match class_id {
        0 => "tutorial_human_fighter001.html",
        10 => "tutorial_human_mage001.html",
        18 => "tutorial_elven_fighter001.html",
        25 => "tutorial_elven_mage001.html",
        31 => "tutorial_delf_fighter001.html",
        38 => "tutorial_delf_mage001.html",
        44 => "tutorial_orc_fighter001.html",
        49 => "tutorial_orc_mage001.html",
        53 => "tutorial_dwarven_fighter001.html",
        192 => "tutorial_kamael_male001.html",
        _ => return None,
    })
}

/// Where the supervisor sends the hero when the tutorial is done.
fn complete_location(class_id: u32) -> [f32; 3] {
    match class_id {
        18 | 25 => [45479.0, 48318.0, -3056.0],
        31 | 38 => [12161.0, 16674.0, -4584.0],
        44 | 49 => [-45113.0, -113598.0, -192.0],
        53 => [115575.0, -178014.0, -904.0],
        192 => [-118073.0, 45131.0, 368.0],
        _ => [-84046.0, 243283.0, -3728.0],
    }
}

/// Mystics other than orcs get spiritshots.
fn spirit_user(c: &Ctx) -> bool {
    c.p.is_mage() && c.p.race() != Race::Orc
}

impl Quest for Tutorial {
    fn id(&self) -> u32 {
        TUTORIAL
    }
    fn dir(&self) -> &'static str {
        "Q00255_Tutorial"
    }
    fn title(&self) -> &'static str {
        "Tutorial"
    }
    fn start_npcs(&self) -> Vec<u32> {
        Vec::new()
    }
    fn talk_npcs(&self) -> Vec<u32> {
        NEWBIE_HELPERS.iter().chain(&SUPERVISORS).copied().collect()
    }
    fn first_talk_npcs(&self) -> Vec<u32> {
        self.talk_npcs()
    }
    fn kill_ids(&self) -> Vec<u32> {
        GREMLINS.to_vec()
    }
    fn quest_items(&self) -> Vec<u32> {
        vec![BLUE_GEM]
    }
    fn on_event(&self, c: &mut Ctx, event: &str) -> Option<String> {
        match event {
            "start_newbie_tutorial" if c.qs.memo() < 4 => {
                c.qs.start_quest();
                c.qs.set_memo(1);
                c.p.screen_message("Speak with the Newbie Helper.");
                start_page(c.p.class_id()).map(String::from)
            }
            "tutorial_02.html" | "tutorial_03.html" if c.qs.memo() == 1 => Some(event.into()),
            // Question mark 1: where to find the Newbie Helper.
            "question_mark_1" if c.qs.memo() == 1 => {
                c.p.screen_message("Speak with the Newbie Helper.");
                Some("tutorial_04.html".into())
            }
            "reward_2" if c.qs.memo() == 4 => {
                c.qs.set_memo(5);
                let (item, n) = if spirit_user(c) { SPIRITSHOTS } else { SOULSHOTS };
                c.p.give(item, n);
                let [x, y, z] = complete_location(c.p.class_id());
                c.p.tele(x, y, z);
                None
            }
            "close_tutorial" => {
                c.p.close_window = true;
                None
            }
            _ => None,
        }
    }
    fn on_first_talk(&self, c: &mut Ctx) -> Option<String> {
        let npc = c.npc;
        if NEWBIE_HELPERS.contains(&npc) {
            if c.p.has(BLUE_GEM) {
                c.qs.set_memo(3);
            }
            let fighter = !c.p.is_mage();
            let orc = c.p.race() == Race::Orc;
            return Some(match c.qs.memo() {
                0 | 1 => {
                    c.qs.set_memo(2);
                    if fighter {
                        "tutorial_05_fighter.html"
                    } else if orc {
                        "tutorial_05_mystic_orc.html"
                    } else {
                        "tutorial_05_mystic.html"
                    }
                    .into()
                }
                2 => if fighter {
                    "tutorial_05_fighter_back.html"
                } else if orc {
                    "tutorial_05_mystic_orc_back.html"
                } else {
                    "tutorial_05_mystic_back.html"
                }
                .into(),
                3 => {
                    c.qs.set_memo(4);
                    c.p.take(BLUE_GEM, -1);
                    c.p.give(SCROLL_OF_ESCAPE.0, SCROLL_OF_ESCAPE.1);
                    c.p.give(WIND_WALK_POTION.0, WIND_WALK_POTION.1);
                    if spirit_user(c) {
                        c.p.give(SPIRITSHOTS.0, SPIRITSHOTS.1);
                        format!("{npc}-3.html")
                    } else {
                        c.p.give(SOULSHOTS.0, SOULSHOTS.1);
                        format!("{npc}-2.html")
                    }
                }
                4 => format!("{npc}-4.html"),
                _ => format!("{npc}-5.html"),
            });
        }
        Some(match c.qs.memo() {
            0..=3 => format!("{npc}-1.html"),
            4 => format!("{npc}-2.html"),
            _ => format!("{npc}-4.html"),
        })
    }
    fn on_talk(&self, c: &mut Ctx) -> String {
        self.on_first_talk(c).unwrap_or_default()
    }
    fn on_kill(&self, c: &mut Ctx, _npc: u32) {
        if c.qs.memo() < 3 && !c.p.has(BLUE_GEM) && c.p.random(100) < 50 {
            c.p.give(BLUE_GEM, 1);
            c.qs.set_memo(3);
            // Question mark 2: bring the gem back.
            c.p.tutorial = Some("tutorial_06.html".into());
        }
    }
}
