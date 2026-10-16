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
use crate::effects::{spawn_effect, EffectCache};
use crate::gamedata::{GameData, SkillDef};
use crate::l2assets::{L2Library, UU_TO_M};
use crate::mmo::{Hero, Phase};
use crate::rpg::{award_kill, Buff, HeroStats, Log, Npc, NpcState, Target};
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
    last_level: u32,
    last_sp: u64,
    last_class: u32,
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
    stage: usize,
    fired: bool,
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
    (pages, mut press): (Res<HotbarPages>, ResMut<HotbarPress>),
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
    // Cast time scales with casting speed (magic) or attack speed.
    let magic = data.visuals.get(&id).is_some_and(|v| v.magic);
    let speed = if magic { stats.matk_speed / 333.0 } else { stats.patk_speed / 300.0 };
    let total = (def.hit_time / speed.max(0.1)).clamp(0.3, 4.0);
    skills.cast = Some(Cast { skill: id, level, target: tgt, time: 0.0, total, stage: usize::MAX, fired: false });
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
    mut assets: (ResMut<Assets<Mesh>>, ResMut<Assets<StandardMaterial>>, ResMut<Assets<Image>>),
    items: Res<crate::items::ItemData>,
    hero: Single<(Entity, &Hero, &mut Transform, &mut HeroStats, &mut HeroSkills, Option<&mut crate::items::Inventory>)>,
    mut npcs: Query<(&mut Npc, &Transform), Without<Hero>>,
    mut anims: Query<&mut Animated>,
) {
    let dt = time.delta_secs();
    let (hero_e, hero, mut tr, mut stats, mut skills, inv) = hero.into_inner();
    for c in skills.cooldowns.values_mut() {
        c.0 -= dt;
    }
    let Some(mut cast) = skills.cast.take() else { return };
    let Some(def) = book.skills.get(&cast.skill).cloned() else { return };
    let visual: Option<Arc<SkillDef>> = data.visuals.get(&cast.skill).cloned();
    let (meshes, materials, images) = &mut assets;
    let mut fx = |name: &str, at: Transform, follow: Option<Entity>| {
        if let Some(asset) = cache.load(&mut lib, name, meshes, materials, images) {
            spawn_effect(&mut commands, &asset, at, follow);
        }
    };
    let target_pos = cast.target.and_then(|e| npcs.get(e).ok()).map(|(_, t)| t.translation);
    if let Some(p) = target_pos {
        let to = (p - tr.translation).with_y(0.0);
        if to.length() > 0.01 {
            tr.rotation = Quat::from_rotation_y((-to.z).atan2(to.x));
        }
    }

    // Animation stages: a looping cast clip, then the release clip(s).
    let anims_list: Vec<String> = visual.as_ref().map(|v| v.anims.clone()).unwrap_or_default();
    let release_at = if anims_list.len() > 1 { cast.total * 0.7 } else { cast.total * 0.45 };
    let stage = if anims_list.len() > 1 && cast.time >= release_at { anims_list.len() - 1 } else { 0 };
    if stage != cast.stage {
        cast.stage = stage;
        if let (Ok(mut a), Some(base)) = (anims.get_mut(hero.actor), anims_list.get(stage)) {
            let names = [format!("{base}_{}", hero.stance), format!("{base}_1HS"), base.clone(), format!("Atk01_{}", hero.stance)];
            let names: Vec<&str> = names.iter().map(String::as_str).collect();
            let looping = stage == 0 && anims_list.len() > 1;
            a.restart(&names, looping);
            a.speed = 1.0;
        }
        if stage == 0 {
            for spec in visual.iter().flat_map(|v| v.casting.iter()) {
                let at = Transform::from_translation(tr.translation + Vec3::Y + tr.rotation * spec.offset).with_rotation(tr.rotation);
                fx(&spec.class, at, spec.attached.then_some(hero_e));
            }
        }
    }

    cast.time += dt;
    if !cast.fired && cast.time >= release_at {
        cast.fired = true;
        for spec in visual.iter().flat_map(|v| v.shot.iter()) {
            let at = match (spec.on_target, target_pos) {
                (true, Some(p)) => Transform::from_translation(p + Vec3::Y),
                _ => Transform::from_translation(tr.translation + Vec3::Y + tr.rotation * spec.offset),
            }
            .with_rotation(tr.rotation);
            fx(&spec.class, at, None);
        }
        let at_target = target_pos.unwrap_or(tr.translation);
        for spec in visual.iter().flat_map(|v| v.explosion.iter()) {
            fx(&spec.class, Transform::from_translation(at_target + Vec3::Y + spec.offset), None);
        }
        // Soulshots power physical skills, spiritshots magic.
        let shot = inv.map_or(1.0, |mut i| crate::items::use_shot(&mut i, &items, visual.as_ref().is_some_and(|v| v.magic)));
        apply_effects(&def, cast.level, cast.target, &mut stats, &mut npcs, &dp, &mut log, shot);
    }
    if cast.time >= cast.total {
        let reuse = def.reuse.max(0.5);
        skills.cooldowns.insert(cast.skill, (reuse, reuse));
        return;
    }
    skills.cast = Some(cast);
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
