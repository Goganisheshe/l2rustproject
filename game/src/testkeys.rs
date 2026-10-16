//! Test keys for trying high-level characters and skills:
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
use crate::items::{Inventory, ADENA};
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
    mut test: ResMut<TestMode>,
    mut log: ResMut<Log>,
    hero: Single<(&mut HeroStats, Option<&mut Inventory>, Option<&mut HeroSkills>), With<Hero>>,
) {
    let (mut stats, inv, skills) = hero.into_inner();
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
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
        if let Some(mut inv) = inv {
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
