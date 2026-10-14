//! Monsters and bosses use the offensive skills from their datapack
//! `skillList`, with the client's cast animations and effects.

use bevy::prelude::*;
use rand::Rng;

use crate::datapack::SkillBook;
use crate::effects::{spawn_effect, EffectCache};
use crate::gamedata::GameData;
use crate::l2assets::{L2Library, UU_TO_M};
use crate::mmo::{Hero, Phase};
use crate::rpg::{HeroStats, Log, Npc, NpcState};
use crate::skinned::Animated;

pub struct NpcSkillsPlugin;

impl Plugin for NpcSkillsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, npc_skills.run_if(in_state(Phase::InWorld)));
    }
}

#[allow(clippy::too_many_arguments)]
fn npc_skills(
    mut commands: Commands,
    time: Res<Time>,
    book: Res<SkillBook>,
    data: Res<GameData>,
    mut log: ResMut<Log>,
    mut lib: ResMut<L2Library>,
    mut cache: ResMut<EffectCache>,
    mut assets: (ResMut<Assets<Mesh>>, ResMut<Assets<StandardMaterial>>, ResMut<Assets<Image>>),
    hero: Single<(&Transform, &mut HeroStats), With<Hero>>,
    mut npcs: Query<(&mut Npc, &Transform), Without<Hero>>,
    mut anims: Query<&mut Animated>,
) {
    let dt = time.delta_secs();
    let (hero_tr, mut stats) = hero.into_inner();
    if stats.hp <= 0.0 {
        return;
    }
    let mut rng = rand::rng();
    let (meshes, materials, images) = &mut assets;
    for (mut npc, tr) in &mut npcs {
        if npc.state != NpcState::Chase || npc.template.skills.is_empty() {
            continue;
        }
        npc.skill_timer -= dt;
        if npc.skill_timer > 0.0 {
            continue;
        }
        let usable: Vec<(u32, u32)> = npc
            .template
            .skills
            .iter()
            .copied()
            .filter(|(id, _)| book.skills.get(id).is_some_and(|s| s.is_active() && s.is_offensive()))
            .collect();
        if usable.is_empty() {
            npc.skill_timer = f32::MAX;
            continue;
        }
        let (id, level) = usable[rng.random_range(0..usable.len())];
        let def = &book.skills[&id];
        let dist = tr.translation.distance(hero_tr.translation);
        let range = def.cast_range.max(npc.template.atk_range) * UU_TO_M + npc.template.radius * UU_TO_M + 1.0;
        if dist > range {
            continue; // try again once in range
        }
        npc.skill_timer = if npc.template.is_boss() { rng.random_range(4.0..8.0) } else { rng.random_range(6.0..12.0) };
        let mut total = 0.0;
        for e in &def.effects {
            let power = e.power.at(level);
            let dmg = match e.name.as_str() {
                "PhysicalDamage" => crate::formulas::physical(&npc.template.combat(), &stats.combat(), power, 1.0, true, &mut rng).map_or(0.0, |d| d.0),
                "MagicalDamage" | "MagicalAttackMp" | "HpDrain" => {
                    crate::formulas::magical(&npc.template.combat(), &stats.combat(), power, 1.0, &mut rng)
                }
                _ => 0.0,
            };
            total += dmg;
        }
        if total <= 0.0 {
            continue;
        }
        stats.hp -= total;
        log.push(format!("{} uses {}: {:.0} damage", npc.template.name, def.name, total));
        let visual = data.visuals.get(&id);
        if let (Some(actor), Some(v)) = (npc.actor, visual) {
            if let Ok(mut a) = anims.get_mut(actor) {
                let mut names: Vec<&str> = v.anims.iter().map(String::as_str).collect();
                names.extend(["SpAtk01", "atk02", "atk01"]);
                a.restart(&names, false);
            }
        }
        for spec in visual.into_iter().flat_map(|v| v.casting.iter().chain(v.shot.iter())) {
            if let Some(asset) = cache.load(&mut lib, &spec.class, meshes, materials, images) {
                let at = Transform::from_translation(tr.translation + Vec3::Y + tr.rotation * spec.offset).with_rotation(tr.rotation);
                spawn_effect(&mut commands, &asset, at, None);
            }
        }
        for spec in visual.into_iter().flat_map(|v| v.explosion.iter()) {
            if let Some(asset) = cache.load(&mut lib, &spec.class, meshes, materials, images) {
                spawn_effect(&mut commands, &asset, Transform::from_translation(hero_tr.translation + Vec3::Y + spec.offset), None);
            }
        }
    }
}
