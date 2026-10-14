//! `ai/others/NewbieGuide`: the race guides' pages and advice.

use crate::quest::{Ctx, Quest, Race};

pub struct NewbieGuide;

const GUIDES: [(u32, Race); 6] = [
    (30598, Race::Human),
    (30599, Race::Elf),
    (30600, Race::DarkElf),
    (30601, Race::Dwarf),
    (30602, Race::Orc),
    (34110, Race::Kamael),
];

impl Quest for NewbieGuide {
    fn id(&self) -> u32 {
        1_000_001
    }
    fn dir(&self) -> &'static str {
        "NewbieGuide"
    }
    fn title(&self) -> &'static str {
        "Newbie Guide"
    }
    fn start_npcs(&self) -> Vec<u32> {
        Vec::new()
    }
    fn talk_npcs(&self) -> Vec<u32> {
        Vec::new()
    }
    fn first_talk_npcs(&self) -> Vec<u32> {
        GUIDES.iter().map(|g| g.0).collect()
    }
    fn on_first_talk(&self, c: &mut Ctx) -> Option<String> {
        let race = GUIDES.iter().find(|g| g.0 == c.npc).map(|g| g.1)?;
        if race != c.p.race() {
            return Some(format!("{}-no.htm", c.npc));
        }
        Some(format!("{}.htm", c.npc))
    }
    fn on_event(&self, c: &mut Ctx, event: &str) -> Option<String> {
        if event == "0" {
            return Some(format!("{}.htm", c.npc));
        }
        Some(format!("{}-{event}{}.htm", c.npc, if c.p.is_mage() { "m" } else { "f" }))
    }
    fn on_talk(&self, c: &mut Ctx) -> String {
        format!("{}.htm", c.npc)
    }
}
