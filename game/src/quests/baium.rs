//! `ai/bosses/Baium`: the stone statue on top of the Tower of Insolence wakes
//! Baium and his archangels.

use crate::quest::{Ctx, Quest};

pub struct Baium;

const STONE: u32 = 29025;
const BAIUM: u32 = 29020;
const ARCHANGEL: u32 = 29021;
const SPAWN: [f32; 3] = [116033.0, 17447.0, 10107.0];
const ARCHANGELS: [[f32; 3]; 5] = [
    [115792.0, 16608.0, 10136.0],
    [115168.0, 17200.0, 10136.0],
    [115780.0, 15564.0, 10136.0],
    [114880.0, 16236.0, 10136.0],
    [114239.0, 17168.0, 10136.0],
];

impl Quest for Baium {
    fn id(&self) -> u32 {
        1_000_002
    }
    fn dir(&self) -> &'static str {
        "Baium"
    }
    fn title(&self) -> &'static str {
        "Baium"
    }
    fn start_npcs(&self) -> Vec<u32> {
        Vec::new()
    }
    fn talk_npcs(&self) -> Vec<u32> {
        vec![STONE]
    }
    fn on_event(&self, c: &mut Ctx, event: &str) -> Option<String> {
        if event == "WAKE_UP" && c.npc == STONE {
            c.p.spawns.push((BAIUM, SPAWN));
            c.p.spawns.extend(ARCHANGELS.iter().map(|p| (ARCHANGEL, *p)));
            c.p.despawn_npc = true;
            c.p.screen_message("Who dares to wake me?");
        }
        None
    }
    fn on_talk(&self, _c: &mut Ctx) -> String {
        "29025.htm".to_string()
    }
}
