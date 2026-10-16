//! Class skills in the offline game: learning from the datapack skill tree as
//! the hero levels, the hotbar (pages of 12 slots, as the client's shortcut
//! bar) and casting with the client's animations and effects. Damage, heals
//! and buffs follow the datapack skill effects.
//!
//! Keys 1–0, -, = cast the first bar's slots, Ctrl+ those the second bar's,
//! Alt+ those the third's. Gamepad: hold LB + A/B/X/Y for slots 1–4, hold
//! LT + A/B/X/Y for slots 5–8.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use bevy::prelude::*;


use crate::datapack::{Datapack, ServerSkill, SkillBook};
use crate::effects::{spawn_effect_with, EffectCache, EffectSpawn};
use crate::gamedata::{GameData, SkillDef};
use crate::l2assets::{L2Library, UU_TO_M};
use crate::mmo::{Hero, Phase};
use crate::rpg::{award_kill, Buff, HeroStats, Log, Npc, NpcState, Target, HERO_RADIUS};
use crate::skillfx::{is_projectile, place, place_shot, Pawn};
use crate::skinned::Animated;

/// Slots of one hotbar row (the client's shortcut bar).
pub const BAR_SLOTS: usize = 12;
/// Pages of hotbar slots; every visible row shows one page.
pub const PAGES: usize = 10;
/// Hotbar slots in all pages.
pub const HOTBAR: usize = BAR_SLOTS * PAGES;
/// Hotbar rows on screen at most (the main bar and four more).
pub const MAX_BARS: usize = 5;

/// Which hotbar rows are shown and the page each one shows.
#[derive(Resource)]
pub struct HotbarPages {
    pub bars: usize,
    pub pages: [usize; MAX_BARS],
}

impl Default for HotbarPages {
    fn default() -> Self {
        HotbarPages { bars: 1, pages: [0, 1, 2, 3, 4] }
    }
}

impl HotbarPages {
    /// Hotbar index of slot `slot` (0-based) of row `bar`.
    pub fn index(&self, bar: usize, slot: usize) -> usize {
        self.pages[bar.min(MAX_BARS - 1)] * BAR_SLOTS + slot
    }
}

/// A hotbar slot clicked with the mouse this frame (hotbar index).
#[derive(Resource, Default)]
pub struct HotbarPress(pub Option<usize>);

pub struct SkillPlayPlugin;

impl Plugin for SkillPlayPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (learn_skills, cast_input, run_cast, tick_buffs, update_hotbar).chain().run_if(in_state(Phase::InWorld)),
        )
        .init_resource::<HotbarPages>()
        .init_resource::<HotbarPress>();
    }
}

#[derive(Component, Default)]
pub struct HeroSkills {
    /// Learned skill id -> level.
    pub learned: BTreeMap<u32, u32>,
    /// Skill per hotbar slot over all pages; 0 is an empty slot. Trailing
    /// empty slots are left out.
    pub hotbar: Vec<u32>,
    /// Skill id -> (seconds left, full reuse time).
    cooldowns: HashMap<u32, (f32, f32)>,
    cast: Option<Cast>,
    /// Skills gone off whose projectile is still flying.
    pending: Vec<PendingHit>,
    last_level: u32,
    last_sp: u64,
    last_class: u32,
}

/// A skill that lands when its projectile arrives.
#[derive(Clone, Copy)]
struct PendingHit {
    skill: u32,
    level: u32,
    target: Option<Entity>,
    /// Soulshot / spiritshot multiplier paid at release.
    shot: f32,
    left: f32,
}

impl HeroSkills {
    /// Skills from a save game; `learn_skills` resumes from this level / SP.
    pub fn restored(learned: BTreeMap<u32, u32>, hotbar: Vec<u32>, level: u32, sp: u64, class: u32) -> Self {
        let mut s = HeroSkills { learned, hotbar, last_level: level, last_sp: sp, last_class: class, ..default() };
        s.hotbar.truncate(HOTBAR);
        s.trim();
        s
    }

    /// Skill in hotbar slot `i`, 0 when empty.
    pub fn slot(&self, i: usize) -> u32 {
        self.hotbar.get(i).copied().unwrap_or(0)
    }

    /// Puts `id` (0 empties) in hotbar slot `i`.
    pub fn set_slot(&mut self, i: usize, id: u32) {
        if i >= HOTBAR {
            return;
        }
        if self.hotbar.len() <= i {
            self.hotbar.resize(i + 1, 0);
        }
        self.hotbar[i] = id;
        self.trim();
    }

    /// Puts `id` in the first empty slot unless it is on the hotbar already.
    /// False when every slot is taken.
    pub fn add_to_hotbar(&mut self, id: u32) -> bool {
        if self.on_hotbar(id) {
            return true;
        }
        match (0..HOTBAR).find(|i| self.slot(*i) == 0) {
            Some(i) => {
                self.set_slot(i, id);
                true
            }
            None => false,
        }
    }

    /// Takes `id` off every hotbar slot.
    pub fn remove_from_hotbar(&mut self, id: u32) {
        for s in self.hotbar.iter_mut().filter(|s| **s == id) {
            *s = 0;
        }
        self.trim();
    }

    pub fn on_hotbar(&self, id: u32) -> bool {
        id != 0 && self.hotbar.contains(&id)
    }

    /// (seconds left, full reuse time) while `id` is cooling down.
    pub fn cooldown(&self, id: u32) -> Option<(f32, f32)> {
        self.cooldowns.get(&id).copied().filter(|c| c.0 > 0.0)
    }

    /// The skill being cast: (skill id, seconds cast, cast time).
    pub fn casting(&self) -> Option<(u32, f32, f32)> {
        self.cast.as_ref().map(|c| (c.skill, c.time, c.total))
    }

    fn trim(&mut self) {
        while self.hotbar.last() == Some(&0) {
            self.hotbar.pop();
        }
    }
}

struct Cast {
    skill: u32,
    level: u32,
    target: Option<Entity>,
    time: f32,
    total: f32,
    /// Step of `plan` playing.
    stage: usize,
    fired: bool,
    /// MP paid, given back if the cast is interrupted.
    mp: f32,
    plan: Vec<Step>,
    /// When the skill goes off.
    release_at: f32,
}

/// One clip of a cast animation: candidate clip names, start time, speed.
struct Step {
    names: Vec<String>,
    start: f32,
    speed: f32,
    looping: bool,
}

/// Lays the skill's animation combo over the cast time. Looping combos
/// (`CastMid`, `CastEnd`, `MagicShot`) loop their first clip and play the
/// others at the end; the rest play their clips one after another, sped up or
/// slowed down to fill the cast. Returns the steps and the moment the skill
/// goes off: the start of the last clip (the shot), or for a single clip a
/// little before the middle of the cast.
fn plan_cast(visual: Option<&SkillDef>, hero: &Hero, model: Option<&crate::skinned::Model>, total: f32) -> (Vec<Step>, f32) {
    let mut bases: Vec<String> = visual.map(|v| v.anims.clone()).unwrap_or_default();
    if bases.is_empty() {
        bases.push("Atk01".into());
    }
    let names: Vec<Vec<String>> = bases
        .iter()
        .map(|b| vec![format!("{b}_{}", hero.stance), format!("{b}_1HS"), b.clone(), format!("Atk01_{}", hero.stance)])
        .collect();
    let lens: Vec<f32> = names
        .iter()
        .map(|n| model.and_then(|m| m.clip_seconds(&n.iter().map(String::as_str).collect::<Vec<_>>())).unwrap_or(0.6).max(0.05))
        .collect();
    let mut steps = Vec::new();
    if visual.is_some_and(|v| v.looped) && names.len() > 1 {
        let tail: f32 = lens[1..].iter().sum();
        let tail_time = tail.min(total * 0.5).max(0.01);
        let speed = tail / tail_time;
        steps.push(Step { names: names[0].clone(), start: 0.0, speed: 1.0, looping: true });
        let mut t = total - tail_time;
        for (n, l) in names[1..].iter().zip(&lens[1..]) {
            steps.push(Step { names: n.clone(), start: t, speed, looping: false });
            t += l / speed;
        }
    } else {
        let speed = (lens.iter().sum::<f32>() / total.max(0.01)).clamp(0.5, 3.0);
        let mut t = 0.0;
        for (n, l) in names.iter().zip(&lens) {
            steps.push(Step { names: n.clone(), start: t, speed, looping: false });
            t += l / speed;
        }
    }
    let release = match steps.len() {
        1 => total * 0.45,
        _ => steps.last().map_or(0.0, |s| s.start),
    };
    (steps, release.min(total))
}

/// Gives the class tree's `autoGet` skills on reaching their level; the rest
/// are learned for SP in the skill window (`skillwindow.rs`).
/// `L2_ALL_SKILLS=1` (tests) gives every skill of the tree up to the hero's
/// level for free, again after each class change.
fn learn_skills(
    mut commands: Commands,
    book: Res<SkillBook>,
    dp: Res<crate::datapack::Datapack>,
    mut log: ResMut<Log>,
    hero: Single<(Entity, &mut HeroStats, Option<&mut HeroSkills>), With<Hero>>,
) {
    let (e, mut stats, skills) = hero.into_inner();
    let Some(mut skills) = skills else {
        commands.entity(e).insert(HeroSkills::default());
        return;
    };
    let class_id = stats.template.class_id;
    if skills.last_level == stats.level && skills.last_sp == stats.sp && skills.last_class == class_id {
        return;
    }
    let tree = book.class_tree(class_id, &dp);
    let all = std::env::var_os("L2_ALL_SKILLS").is_some();
    let mut changed = true;
    while changed {
        changed = false;
        for entry in &tree {
            let have = skills.learned.get(&entry.skill).copied().unwrap_or(0);
            let free = all;
            if !(entry.auto_get || free) || entry.get_level > stats.level || entry.level != have + 1 || (!free && stats.sp < entry.sp) {
                continue;
            }
            let Some(def) = book.skills.get(&entry.skill) else { continue };
            if !free {
                stats.sp -= entry.sp;
            }
            skills.learned.insert(entry.skill, entry.level);
            if have == 0 && def.is_active() {
                skills.add_to_hotbar(entry.skill);
            }
            log.push(format!("Learned {} Lv {}", def.name, entry.level));
            changed = true;
        }
    }
    skills.last_level = stats.level;
    skills.last_sp = stats.sp;
    skills.last_class = class_id;
}

/// Hotbar keys of one row, slot by slot.
pub const SLOT_KEYS: [KeyCode; BAR_SLOTS] = [
    KeyCode::Digit1,
    KeyCode::Digit2,
    KeyCode::Digit3,
    KeyCode::Digit4,
    KeyCode::Digit5,
    KeyCode::Digit6,
    KeyCode::Digit7,
    KeyCode::Digit8,
    KeyCode::Digit9,
    KeyCode::Digit0,
    KeyCode::Minus,
    KeyCode::Equal,
];

#[allow(clippy::too_many_arguments)]
fn cast_input(
    keys: Res<ButtonInput<KeyCode>>,
    gamepads: Query<&Gamepad, With<crate::input::ActivePad>>,
    book: Res<SkillBook>,
    data: Res<GameData>,
    target: Res<Target>,
    (pages, mut press, mut move_to): (Res<HotbarPages>, ResMut<HotbarPress>, ResMut<crate::mmo::MoveTarget>),
    mut log: ResMut<Log>,
    hero: Single<(Entity, &mut HeroStats, &mut HeroSkills, &Transform), With<Hero>>,
    npcs: Query<(&Npc, &Transform), Without<Hero>>,
) {
    let (hero_e, mut stats, mut skills, tr) = hero.into_inner();
    let clicked = press.0.take();
    if skills.cast.is_some() || stats.hp <= 0.0 {
        return;
    }
    let ctrl = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
    let alt = keys.any_pressed([KeyCode::AltLeft, KeyCode::AltRight]);
    let bar = if ctrl {
        1
    } else if alt {
        2
    } else {
        0
    };
    let mut slot = SLOT_KEYS.iter().position(|k| keys.just_pressed(*k)).map(|i| pages.index(bar, i)).or(clicked);
    for pad in &gamepads {
        let face = [GamepadButton::South, GamepadButton::East, GamepadButton::West, GamepadButton::North];
        if let Some(i) = face.iter().position(|b| pad.just_pressed(*b)) {
            if pad.pressed(GamepadButton::LeftTrigger) {
                slot = Some(pages.index(0, i));
            } else if pad.pressed(GamepadButton::LeftTrigger2) {
                slot = Some(pages.index(0, 4 + i));
            }
        }
    }
    // L2_AUTOCAST: cast the first ready offensive hotbar skill on the target (tests).
    let target_near = target.0.and_then(|e| npcs.get(e).ok()).is_some_and(|(n, t)| {
        n.state != NpcState::Dead && t.translation.distance(tr.translation) < 18.0
    });
    if slot.is_none() && std::env::var_os("L2_AUTOCAST").is_some() && target_near {
        slot = skills.hotbar.iter().position(|id| {
            skills.cooldown(*id).is_none() && book.skills.get(id).is_some_and(|s| s.is_offensive())
        });
    }
    let Some(id) = slot.map(|i| skills.slot(i)).filter(|id| *id != 0) else { return };
    let Some(def) = book.skills.get(&id) else { return };
    let level = skills.learned.get(&id).copied().unwrap_or(1);
    if let Some((left, _)) = skills.cooldown(id) {
        log.push(format!("{} is not ready yet ({left:.0} s).", def.name));
        return;
    }
    let mp = def.mp.at(level);
    if stats.mp < mp {
        log.push(format!("Not enough MP for {}", def.name));
        return;
    }
    let offensive = def.is_offensive();
    let tgt = if offensive {
        match target.0.and_then(|e| npcs.get(e).ok().map(|n| (e, n))) {
            Some((e, (n, t))) if n.state != NpcState::Dead => {
                let reach = (def.cast_range.max(40.0) + n.template.radius) * UU_TO_M + 0.5;
                if t.translation.distance(tr.translation) > reach {
                    log.push(format!("{}: target is too far", def.name));
                    return;
                }
                Some(e)
            }
            _ if target.0 == Some(hero_e) => {
                log.push(format!("{}: invalid target", def.name));
                return;
            }
            _ => {
                log.push(format!("{}: no target", def.name));
                return;
            }
        }
    } else {
        // Buffs and heals land on the hero: there is no party offline.
        None
    };
    stats.mp -= mp;
    stats.attacking = false;
    // Casting stops the hero where he stands.
    move_to.0 = None;
    // Cast time scales with casting speed (magic) or attack speed.
    let magic = data.visuals.get(&id).is_some_and(|v| v.magic);
    let speed = if magic { stats.matk_speed / 333.0 } else { stats.patk_speed / 300.0 };
    let total = (def.hit_time / speed.max(0.1)).clamp(0.3, 4.0);
    skills.cast = Some(Cast { skill: id, level, target: tgt, time: 0.0, total, stage: usize::MAX, fired: false, mp, plan: Vec::new(), release_at: 0.0 });
}

#[allow(clippy::too_many_arguments)]
fn run_cast(
    mut commands: Commands,
    time: Res<Time>,
    book: Res<SkillBook>,
    data: Res<GameData>,
    dp: Res<Datapack>,
    mut log: ResMut<Log>,
    mut lib: ResMut<L2Library>,
    mut cache: ResMut<EffectCache>,
    mut assets: FxAssets,
    items: Res<crate::items::ItemData>,
    hero: Single<(Entity, &Hero, &mut Transform, &mut HeroStats, &mut HeroSkills, Option<&mut crate::items::Inventory>)>,
    mut npcs: Query<(&mut Npc, &Transform), Without<Hero>>,
    mut anims: Query<&mut Animated>,
    (input, move_to): (Res<crate::input::PlayerInput>, Res<crate::mmo::MoveTarget>),
) {
    let dt = time.delta_secs();
    let (hero_e, hero, mut tr, mut stats, mut skills, inv) = hero.into_inner();
    for c in skills.cooldowns.values_mut() {
        c.0 -= dt;
    }
    // Projectiles arriving.
    let mut landed = Vec::new();
    skills.pending.retain_mut(|h| {
        h.left -= dt;
        let due = h.left <= 0.0;
        if due {
            landed.push(*h);
        }
        !due
    });
    for hit in landed {
        let Some(def) = book.skills.get(&hit.skill).cloned() else { continue };
        let visual = data.visuals.get(&hit.skill).cloned();
        let caster = hero_pawn(hero_e, hero, &tr, &anims);
        let target = hit.target.and_then(|e| npc_pawn(e, &npcs)).unwrap_or(caster);
        land(hit, &def, visual.as_deref(), &caster, &target, &mut commands, &mut cache, &mut lib, &mut assets, &anims);
        apply_effects(&def, hit.level, hit.target, &mut stats, &mut npcs, &dp, &mut log, hit.shot);
    }
    let Some(mut cast) = skills.cast.take() else { return };
    // Moving (or dying) before the skill goes off interrupts the cast.
    if !cast.fired && (input.move_dir.length() > 0.1 || move_to.0.is_some() || stats.hp <= 0.0) {
        stats.mp = (stats.mp + cast.mp).min(stats.max_mp);
        if let Ok(mut a) = anims.get_mut(hero.actor) {
            a.speed = 1.0;
        }
        log.push("Casting has been interrupted.");
        return;
    }
    let Some(def) = book.skills.get(&cast.skill).cloned() else { return };
    let visual: Option<Arc<SkillDef>> = data.visuals.get(&cast.skill).cloned();
    let target_pos = cast.target.and_then(|e| npcs.get(e).ok()).map(|(_, t)| t.translation);
    if let Some(p) = target_pos {
        let to = (p - tr.translation).with_y(0.0);
        if to.length() > 0.01 {
            tr.rotation = Quat::from_rotation_y((-to.z).atan2(to.x));
        }
    }
    let caster = hero_pawn(hero_e, hero, &tr, &anims);
    // Buffs and heals land on the hero himself.
    let target = cast.target.and_then(|e| npc_pawn(e, &npcs)).unwrap_or(caster);

    // Animation: the combo's clips laid over the cast time.
    if cast.plan.is_empty() {
        let model = anims.get(hero.actor).ok().map(|a| a.model.clone());
        let (plan, release_at) = plan_cast(visual.as_deref(), hero, model.as_deref(), cast.total);
        cast.plan = plan;
        cast.release_at = release_at;
        let bone_of = |a: Entity, n: &[&str]| anims.get(a).ok().and_then(|x| x.bone(n));
        for spec in visual.iter().flat_map(|v| v.casting.iter()) {
            let s = place(spec, &caster, Some(&target), &bone_of);
            spawn_fx(&mut commands, &mut cache, &mut lib, &mut assets, &spec.class, s, Some(target.entity));
        }
    }
    let stage = cast.plan.iter().rposition(|s| cast.time >= s.start).unwrap_or(0);
    if stage != cast.stage {
        cast.stage = stage;
        if let (Ok(mut a), Some(step)) = (anims.get_mut(hero.actor), cast.plan.get(stage)) {
            let names: Vec<&str> = step.names.iter().map(String::as_str).collect();
            a.restart(&names, step.looping);
            a.speed = step.speed;
        }
    }

    cast.time += dt;
    if !cast.fired && cast.time >= cast.release_at {
        cast.fired = true;
        let flying_time = visual.as_ref().map_or(0.0, |v| v.flying_time);
        let mut flies = false;
        {
            let bone_of = |a: Entity, n: &[&str]| anims.get(a).ok().and_then(|x| x.bone(n));
            for spec in visual.iter().flat_map(|v| v.shot.iter()) {
                flies |= is_projectile(spec, &caster, Some(&target), flying_time);
                let s = place_shot(spec, &caster, Some(&target), flying_time, &bone_of);
                spawn_fx(&mut commands, &mut cache, &mut lib, &mut assets, &spec.class, s, Some(target.entity));
            }
        }
        // Soulshots power physical skills, spiritshots magic.
        let shot = inv.map_or(1.0, |mut i| crate::items::use_shot(&mut i, &items, visual.as_ref().is_some_and(|v| v.magic)));
        let hit = PendingHit { skill: cast.skill, level: cast.level, target: cast.target, shot, left: flying_time };
        if flies {
            skills.pending.push(hit);
        } else {
            land(hit, &def, visual.as_deref(), &caster, &target, &mut commands, &mut cache, &mut lib, &mut assets, &anims);
            apply_effects(&def, hit.level, hit.target, &mut stats, &mut npcs, &dp, &mut log, hit.shot);
        }
    }
    if cast.time >= cast.total {
        if let Ok(mut a) = anims.get_mut(hero.actor) {
            a.speed = 1.0;
        }
        let reuse = def.reuse.max(0.5);
        skills.cooldowns.insert(cast.skill, (reuse, reuse));
        return;
    }
    skills.cast = Some(cast);
}

type FxAssets<'a> = (ResMut<'a, Assets<Mesh>>, ResMut<'a, Assets<StandardMaterial>>, ResMut<'a, Assets<Image>>);

/// Loads effect `class` and spawns it as `s` says; actor beams end on `beam_target`.
fn spawn_fx(commands: &mut Commands, cache: &mut EffectCache, lib: &mut L2Library, assets: &mut FxAssets, class: &str, mut s: EffectSpawn, beam_target: Option<Entity>) {
    let (meshes, materials, images) = assets;
    if let Some(asset) = cache.load(lib, class, meshes, materials, images) {
        s.target = s.target.or(beam_target);
        spawn_effect_with(commands, &asset, s);
    }
}

/// The hero as an effect host: his collision cylinder is half his model tall.
fn hero_pawn(e: Entity, hero: &Hero, tr: &Transform, anims: &Query<&mut Animated>) -> Pawn {
    let half_height = anims.get(hero.actor).map_or(0.9, |a| a.model.height * 0.5).max(0.3);
    Pawn { entity: e, actor: Some(hero.actor), feet: tr.translation, rotation: tr.rotation, radius: HERO_RADIUS * UU_TO_M, half_height }
}

fn npc_pawn(e: Entity, npcs: &Query<(&mut Npc, &Transform), Without<Hero>>) -> Option<Pawn> {
    let (n, t) = npcs.get(e).ok()?;
    Some(Pawn {
        entity: e,
        actor: n.actor,
        feet: t.translation,
        rotation: t.rotation,
        radius: (n.template.radius * UU_TO_M).max(0.2),
        half_height: (n.template.height * UU_TO_M).max(0.3),
    })
}

/// A skill reaching its target: the explosion effects.
#[allow(clippy::too_many_arguments)]
fn land(
    hit: PendingHit,
    _def: &ServerSkill,
    visual: Option<&SkillDef>,
    caster: &Pawn,
    target: &Pawn,
    commands: &mut Commands,
    cache: &mut EffectCache,
    lib: &mut L2Library,
    assets: &mut FxAssets,
    anims: &Query<&mut Animated>,
) {
    let bone_of = |a: Entity, n: &[&str]| anims.get(a).ok().and_then(|x| x.bone(n));
    for spec in visual.iter().flat_map(|v| v.explosion.iter()) {
        let s = place(spec, caster, Some(target), &bone_of);
        spawn_fx(commands, cache, lib, assets, &spec.class, s, hit.target);
    }
}

fn apply_effects(
    def: &ServerSkill,
    level: u32,
    target: Option<Entity>,
    stats: &mut HeroStats,
    npcs: &mut Query<(&mut Npc, &Transform), Without<Hero>>,
    dp: &Datapack,
    log: &mut Log,
    shot: f32,
) {
    let mut rng = rand::rng();
    for e in &def.effects {
        let power = e.power.at(level);
        match e.name.as_str() {
            "PhysicalDamage" | "MagicalDamage" => {
                let Some((mut npc, _)) = target.and_then(|t| npcs.get_mut(t).ok()) else { continue };
                let dmg = if e.name == "PhysicalDamage" {
                    crate::formulas::physical(&stats.combat(), &npc.template.combat(), power, shot, true, &mut rng).map_or(0.0, |d| d.0)
                } else {
                    crate::formulas::magical(&stats.combat(), &npc.template.combat(), power, shot, &mut rng)
                };
                npc.hp -= dmg;
                npc.provoked = true;
                log.push(format!("{} hits {} for {:.0}", def.name, npc.template.name, dmg));
                if npc.hp <= 0.0 {
                    award_kill(&mut npc, stats, dp, log);
                }
            }
            "Heal" => {
                stats.hp = (stats.hp + power).min(stats.max_hp);
                log.push(format!("{} restores {:.0} HP", def.name, power));
            }
            "HealPercent" => {
                let hp = stats.max_hp * power / 100.0;
                stats.hp = (stats.hp + hp).min(stats.max_hp);
                log.push(format!("{} restores {:.0} HP", def.name, hp));
            }
            "ManaHeal" | "ManaHealByLevel" => {
                stats.mp = (stats.mp + power).min(stats.max_mp);
                log.push(format!("{} restores {:.0} MP", def.name, power));
            }
            "PhysicalAttack" | "PhysicalDefence" | "MagicalAttack" | "MagicalDefence" | "Speed" | "PhysicalAttackSpeed"
            | "MagicalAttackSpeed" | "Accuracy" | "PhysicalEvasion" | "CriticalRate" | "HpRegen" | "MpRegen" | "MaxHp"
            | "MaxMp" => {
                let amount = e.amount.at(level);
                stats.buffs.retain(|b| !(b.skill == def.id && b.stat == e.name));
                stats.buffs.push(Buff {
                    skill: def.id,
                    stat: e.name.clone(),
                    amount,
                    percent: e.mode.eq_ignore_ascii_case("PER"),
                    remaining: def.abnormal_time.max(10.0),
                });
                stats.recalc();
                log.push(format!("{}: {} {:+}{}", def.name, e.name, amount, if e.mode == "PER" { "%" } else { "" }));
            }
            _ => {}
        }
    }
}

fn tick_buffs(time: Res<Time>, mut log: ResMut<Log>, mut stats: Single<&mut HeroStats, With<Hero>>) {
    let dt = time.delta_secs();
    let before = stats.buffs.len();
    let mut expired = Vec::new();
    stats.buffs.retain_mut(|b| {
        b.remaining -= dt;
        if b.remaining <= 0.0 {
            expired.push(b.skill);
            false
        } else {
            true
        }
    });
    if stats.buffs.len() != before {
        stats.recalc();
        for id in expired {
            log.push(format!("Effect of skill {id} has worn off"));
        }
    }
}

// ---------------------------------------------------------------------------
// Hotbar UI
// ---------------------------------------------------------------------------

#[derive(Component)]
struct HotbarRoot;

#[derive(Component)]
struct HotbarSlot;

#[allow(clippy::too_many_arguments)]
fn update_hotbar(
    mut commands: Commands,
    book: Res<SkillBook>,
    mut lib: ResMut<L2Library>,
    mut images: ResMut<Assets<Image>>,
    hero: Single<&HeroSkills, With<Hero>>,
    pages: Res<HotbarPages>,
    root: Option<Single<Entity, With<HotbarRoot>>>,
    iface: Option<Res<crate::l2ui::Interface>>,
    slots: Query<Entity, With<HotbarSlot>>,
    mut icon_cache: Local<HashMap<String, Option<Handle<Image>>>>,
    mut shown: Local<Vec<(u32, u32, bool)>>,
) {
    // The client's shortcut window shows the hotbar (l2ui_bind).
    if iface.is_some() {
        return;
    }
    let Some(root) = root else {
        commands.spawn((
            HotbarRoot,
            Node {
                position_type: PositionType::Absolute,
                bottom: Val::Px(12.0),
                width: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                column_gap: Val::Px(4.0),
                ..default()
            },
        ));
        return;
    };
    let now: Vec<(u32, u32, bool)> = (0..BAR_SLOTS)
        .map(|i| hero.slot(pages.index(0, i)))
        .map(|id| (id, hero.learned.get(&id).copied().unwrap_or(0), hero.cooldown(id).is_some()))
        .collect();
    if *shown == now {
        return;
    }
    *shown = now.clone();
    for e in &slots {
        commands.entity(e).despawn();
    }
    for (i, (id, level, cooling)) in now.into_iter().enumerate() {
        if id == 0 {
            continue;
        }
        let icon_name = book.skills.get(&id).map(|s| s.icon.clone()).unwrap_or_default();
        let icon = icon_cache
            .entry(icon_name.clone())
            .or_insert_with(|| {
                let (p, o) = icon_name.split_once('.')?;
                lib.texture(p, o, false).map(|img| images.add(img))
            })
            .clone();
        let tint = if cooling { Color::srgb(0.4, 0.4, 0.4) } else { Color::WHITE };
        commands.entity(*root).with_children(|b| {
            b.spawn((HotbarSlot, Node { flex_direction: FlexDirection::Column, align_items: AlignItems::Center, ..default() }))
                .with_children(|s| {
                    match icon {
                        Some(img) => {
                            s.spawn((ImageNode::new(img).with_color(tint), Node { width: Val::Px(40.0), height: Val::Px(40.0), ..default() }));
                        }
                        None => {
                            s.spawn((Node { width: Val::Px(40.0), height: Val::Px(40.0), ..default() }, BackgroundColor(Color::srgb(0.2, 0.2, 0.2))));
                        }
                    }
                    s.spawn((Text::new(format!("{} L{}", i + 1, level)), TextFont { font_size: FontSize::Px(12.0), ..default() }));
                });
        });
    }
}
