//! Ported datapack scripts: quests and NPC AI with dialogs.

mod adventure;
mod baium;
mod bounty;
mod bounty2;
mod newbie;
pub mod story;
mod newbie_guide;

use crate::quest::Quest;

pub fn all() -> Vec<Box<dyn Quest>> {
    let mut v: Vec<Box<dyn Quest>> = vec![Box::new(newbie_guide::NewbieGuide), Box::new(baium::Baium)];
    v.push(Box::new(bounty::Q00300));
    v.push(Box::new(bounty::Q00326));
    v.push(Box::new(bounty::Q00328));
    v.push(Box::new(bounty::Q00329));
    v.push(Box::new(bounty::Q00331));
    v.push(Box::new(bounty2::Q00354));
    v.push(Box::new(bounty2::Q00355));
    v.push(Box::new(bounty2::Q00356));
    v.push(Box::new(bounty2::Q00358));
    v.push(Box::new(bounty2::Q00360));
    v.push(Box::new(bounty2::Q00369));
    v.push(Box::new(bounty2::Q00370));
    v.push(Box::new(story::Q10866));
    v.push(Box::new(story::Q10673));
    v.extend(adventure::ADVENTURES.iter().map(|q| Box::new(q) as Box<dyn Quest>));
    v.extend(newbie::FIRST.iter().map(|q| Box::new(q.clone()) as Box<dyn Quest>));
    v.extend(newbie::SECOND.iter().map(|q| Box::new(q.clone()) as Box<dyn Quest>));
    v
}
