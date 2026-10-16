//! Monsters and bosses use the offensive skills from their datapack
//! `skillList`, with the client's cast animations and effects.

use bevy::prelude::*;
use rand::Rng;

use crate::datapack::SkillBook;
use crate::effects::{spawn_effect_with, EffectCache};
use crate::gamedata::GameData;
use crate::l2assets::{L2Library, UU_TO_M};
use crate::mmo::{Hero, Phase};
use crate::rpg::{HeroStats, Log, Npc, NpcState, HERO_RADIUS};
use crate::skillfx::{is_projectile, place, place_shot, Pawn};
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
    hero: Single<(Entity, &Hero, &Transform, &mut HeroStats)>,
    mut npcs: Query<(Entity, &mut Npc, &Transform), Without<Hero>>,
    mut anims: Query<&mut Animated>,
) {
    let dt = time.delta_secs();
    let (hero_e, hero, hero_tr, mut stats) = hero.into_inner();
    let hero_half = anims.get(hero.actor).map_or(0.9, |a| a.model.height * 0.5).max(0.3);
    let hero_pawn = Pawn {
        entity: hero_e,
        actor: Some(hero.actor),
        feet: hero_tr.translation,
        rotation: hero_tr.rotation,
        radius: HERO_RADIUS * UU_TO_M,
        half_height: hero_half,
    };
    if stats.hp <= 0.0 {
        return;
    }
    let mut rng = rand::rng();
    let (meshes, materials, images) = &mut assets;
    for (npc_e, mut npc, tr) in &mut npcs {
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
        let caster = Pawn {
            entity: npc_e,
            actor: npc.actor,
            feet: tr.translation,
            rotation: tr.rotation,
            radius: (npc.template.radius * UU_TO_M).max(0.2),
            half_height: (npc.template.height * UU_TO_M).max(0.3),
        };
        let bone_of = |a: Entity, n: &[&str]| anims.get(a).ok().and_then(|x| x.bone(n));
        let flying_time = visual.map_or(0.0, |v| v.flying_time);
        let mut flies = false;
        let mut spawns = Vec::new();
        for spec in visual.into_iter().flat_map(|v| v.casting.iter()) {
            spawns.push((spec.class.clone(), place(spec, &caster, Some(&hero_pawn), &bone_of)));
        }
        for spec in visual.into_iter().flat_map(|v| v.shot.iter()) {
            flies |= is_projectile(spec, &caster, Some(&hero_pawn), flying_time);
            spawns.push((spec.class.clone(), place_shot(spec, &caster, Some(&hero_pawn), flying_time, &bone_of)));
        }
        // The damage is already done; without a projectile the hit shows now.
        if !flies {
            for spec in visual.into_iter().flat_map(|v| v.explosion.iter()) {
                spawns.push((spec.class.clone(), place(spec, &caster, Some(&hero_pawn), &bone_of)));
            }
        }
        for (class, mut s) in spawns {
            if let Some(asset) = cache.load(&mut lib, &class, meshes, materials, images) {
                s.target = s.target.or(Some(hero_e));
                spawn_effect_with(&mut commands, &asset, s);
            }
        }
    }
}
