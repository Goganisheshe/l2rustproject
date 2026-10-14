//! Ported datapack scripts: quests and NPC AI with dialogs.

mod newbie;
mod newbie_guide;

use crate::quest::Quest;

pub fn all() -> Vec<Box<dyn Quest>> {
    let mut v: Vec<Box<dyn Quest>> = vec![Box::new(newbie_guide::NewbieGuide)];
    v.extend(newbie::FIRST.iter().map(|q| Box::new(q.clone()) as Box<dyn Quest>));
    v.extend(newbie::SECOND.iter().map(|q| Box::new(q.clone()) as Box<dyn Quest>));
    v
}
