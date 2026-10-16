//! Test keys for trying high-level characters, skills and items:
//! - F8: one of every item of the datapack (100 of stackable ones) into the
//!   inventory; Ctrl+F8 empties the bag (what is worn stays);
//! - F9: one level up (Shift+F9: ten);
//! - F10: 1 000 000 SP and 10 000 000 Adena;
//! - F11: every skill of the class tree up to the hero's level, for free
//!   (again after each level and class change), like `L2_ALL_SKILLS`;
//! - F12: full HP / MP and every skill ready.
//!
//! Class transfers still go through the class window (C), which opens by
//! itself on reaching level 20, 40 and 76.

use bevy::prelude::*;

use crate::datapack::Datapack;
use crate::items::{Inventory, ItemData, ADENA};
use crate::mmo::{Hero, Phase};
use crate::rpg::{HeroStats, Log};
use crate::skillplay::{HeroSkills, TestMode};

pub struct TestKeysPlugin;

impl Plugin for TestKeysPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, test_keys.run_if(in_state(Phase::InWorld)));
    }
}

fn test_keys(
    keys: Res<ButtonInput<KeyCode>>,
    dp: Res<Datapack>,
    items: Res<ItemData>,
    mut test: ResMut<TestMode>,
    mut log: ResMut<Log>,
    hero: Single<(&mut HeroStats, Option<&mut Inventory>, Option<&mut HeroSkills>), With<Hero>>,
) {
    let (mut stats, mut inv, skills) = hero.into_inner();
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    let ctrl = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
    if keys.just_pressed(KeyCode::F8) {
        if let Some(inv) = inv.as_mut() {
            if ctrl {
                let worn: Vec<u32> = inv.equipped.values().copied().collect();
                inv.items.retain(|id, _| worn.contains(id));
                inv.auto_shots.clear();
                log.push("[test] Bag emptied");
            } else {
                let mut added = 0;
                for t in items.items.values() {
                    let n = if t.equippable() { 1 } else { 100 };
                    let have = inv.items.entry(t.id).or_insert(0);
                    if *have < n {
                        *have = n;
                        added += 1;
                    }
                }
                log.push(format!("[test] {added} items added ({} kinds in the bag; mouse wheel scrolls the inventory)", inv.items.len()));
            }
        }
    }
    if keys.just_pressed(KeyCode::F9) {
        let max = dp.exp.len().saturating_sub(1).max(1) as u32;
        let level = (stats.level + if shift { 10 } else { 1 }).min(max);
        if level == stats.level {
            log.push(format!("[test] Level {level} is the highest."));
        } else {
            stats.exp = stats.exp.max(dp.exp.get(level as usize).copied().unwrap_or(0));
            stats.level = dp.level_for(stats.exp).max(level);
            stats.recalc();
            stats.hp = stats.max_hp;
            stats.mp = stats.max_mp;
            log.push(format!("[test] Level {}", stats.level));
        }
    }
    if keys.just_pressed(KeyCode::F10) {
        stats.sp += 1_000_000;
        if let Some(inv) = inv.as_mut() {
            *inv.items.entry(ADENA).or_insert(0) += 10_000_000;
        }
        log.push("[test] +1 000 000 SP, +10 000 000 Adena");
    }
    if keys.just_pressed(KeyCode::F11) {
        test.all_skills = !test.all_skills;
        log.push(if test.all_skills { "[test] All class skills: on" } else { "[test] All class skills: off" });
    }
    if keys.just_pressed(KeyCode::F12) {
        stats.hp = stats.max_hp;
        stats.mp = stats.max_mp;
        if let Some(mut s) = skills {
            s.reset_cooldowns();
        }
        log.push("[test] HP / MP full, skills ready");
    }
}
