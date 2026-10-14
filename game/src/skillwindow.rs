//! The skill window (K / gamepad right stick press), as in Classic: learned
//! skills, and the class tree's next skill levels to learn for SP (and the
//! spellbooks or Adena some entries ask for). Clicking a learned active skill
//! puts it on the hotbar or takes it off.

use bevy::prelude::*;

use crate::datapack::{Datapack, SkillBook, TreeEntry};
use crate::input::PlayerInput;
use crate::items::{Inventory, ItemData};
use crate::mmo::{Hero, Phase};
use crate::rpg::{HeroStats, Log};
use crate::skillplay::{HeroSkills, HOTBAR};

#[derive(Resource, Default)]
struct SkillUi {
    open: bool,
    selected: usize,
    /// Row clicked with the mouse.
    clicked: Option<usize>,
    shown: Option<u64>,
}

#[derive(Component)]
struct SkillRoot;

/// One line of the window.
enum Row {
    Learn(TreeEntry),
    Later(TreeEntry),
    Known(u32, u32),
}

pub struct SkillWindowPlugin;

impl Plugin for SkillWindowPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SkillUi>().add_systems(Update, skill_window.run_if(in_state(Phase::InWorld)));
    }
}

/// Next learnable level of each skill in the class tree (the lowest entry
/// above what the hero knows), split into "now" and "later".
fn rows(tree: &[TreeEntry], skills: &HeroSkills, level: u32) -> Vec<Row> {
    let mut next: Vec<TreeEntry> = Vec::new();
    for e in tree {
        let have = skills.learned.get(&e.skill).copied().unwrap_or(0);
        if e.level != have + 1 || e.auto_get {
            continue;
        }
        if !next.iter().any(|n| n.skill == e.skill) {
            next.push(e.clone());
        }
    }
    next.sort_by_key(|e| (e.get_level, e.skill));
    let mut out: Vec<Row> = Vec::new();
    out.extend(next.iter().filter(|e| e.get_level <= level).cloned().map(Row::Learn));
    out.extend(next.iter().filter(|e| e.get_level > level).take(8).cloned().map(Row::Later));
    out.extend(skills.learned.iter().map(|(id, lv)| Row::Known(*id, *lv)));
    out
}

/// Pays for and learns one tree entry. Returns an error line on failure.
pub fn learn(entry: &TreeEntry, stats: &mut HeroStats, inv: &mut Inventory, skills: &mut HeroSkills, book: &SkillBook, items: &ItemData) -> Result<String, String> {
    let name = book.skills.get(&entry.skill).map_or(format!("skill {}", entry.skill), |s| s.name.clone());
    if entry.get_level > stats.level {
        return Err(format!("{name} requires level {}.", entry.get_level));
    }
    if stats.sp < entry.sp {
        return Err(format!("Not enough SP for {name} ({} needed).", entry.sp));
    }
    for (id, n) in &entry.items {
        if inv.items.get(id).copied().unwrap_or(0) < *n {
            let item = items.items.get(id).map_or(format!("item {id}"), |t| t.name.clone());
            return Err(format!("{name} requires {item} x{n}."));
        }
    }
    stats.sp -= entry.sp;
    for (id, n) in &entry.items {
        let left = inv.items.get_mut(id).unwrap();
        *left -= n;
        if *left == 0 {
            inv.items.remove(id);
        }
    }
    let first = !skills.learned.contains_key(&entry.skill);
    skills.learned.insert(entry.skill, entry.level);
    if first && book.skills.get(&entry.skill).is_some_and(|s| s.is_active()) && skills.hotbar.len() < HOTBAR {
        skills.hotbar.push(entry.skill);
    }
    Ok(format!("You have learned {name} Lv {}.", entry.level))
}

#[allow(clippy::too_many_arguments)]
fn skill_window(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    gamepads: Query<&Gamepad>,
    input: Res<PlayerInput>,
    book: Res<SkillBook>,
    dp: Res<Datapack>,
    items: Res<ItemData>,
    mut ui: ResMut<SkillUi>,
    mut log: ResMut<Log>,
    hero: Single<(&mut HeroStats, Option<&mut Inventory>, Option<&mut HeroSkills>), With<Hero>>,
    roots: Query<Entity, With<SkillRoot>>,
) {
    let toggle = keys.just_pressed(KeyCode::KeyK) || gamepads.iter().any(|g| g.just_pressed(GamepadButton::RightThumb));
    if toggle {
        ui.open = !ui.open;
        ui.shown = None;
    }
    if ui.open && keys.just_pressed(KeyCode::Escape) {
        ui.open = false;
    }
    if std::env::var_os("L2_SKILL_WINDOW").is_some() && ui.shown.is_none() && !toggle {
        ui.open = true;
    }
    let (mut stats, inv, skills) = hero.into_inner();
    let (Some(mut inv), Some(mut skills)) = (inv, skills) else { return };
    if !ui.open {
        for e in &roots {
            commands.entity(e).despawn();
        }
        ui.shown = None;
        return;
    }
    let tree = book.class_tree(stats.template.class_id, &dp);
    let list = rows(&tree, &skills, stats.level);
    if list.is_empty() {
        return;
    }
    if input.nav != 0 {
        ui.selected = (ui.selected as i32 + input.nav).rem_euclid(list.len() as i32) as usize;
        ui.shown = None;
    }
    ui.selected = ui.selected.min(list.len() - 1);
    let pick = ui.clicked.take().or((input.confirm && !toggle).then_some(ui.selected));
    if let Some(i) = pick {
        ui.selected = i;
        match list.get(i) {
            Some(Row::Learn(e)) | Some(Row::Later(e)) => match learn(e, &mut stats, &mut inv, &mut skills, &book, &items) {
                Ok(m) | Err(m) => log.push(m),
            },
            Some(Row::Known(id, _)) => {
                if let Some(slot) = skills.hotbar.iter().position(|s| s == id) {
                    skills.hotbar.remove(slot);
                } else if book.skills.get(id).is_some_and(|s| s.is_active()) {
                    if skills.hotbar.len() < HOTBAR {
                        skills.hotbar.push(*id);
                    } else {
                        log.push("The hotbar is full.");
                    }
                } else {
                    log.push("Passive skills work by themselves.");
                }
            }
            None => {}
        }
        ui.shown = None;
    }
    // Redraw when anything shown changes.
    let state = (ui.selected as u64) ^ (stats.sp << 8) ^ ((skills.learned.len() as u64) << 40) ^ ((skills.hotbar.len() as u64) << 50) ^ ((stats.level as u64) << 56);
    if ui.shown == Some(state) {
        return;
    }
    ui.shown = Some(state);
    for e in &roots {
        commands.entity(e).despawn();
    }
    let list = rows(&tree, &skills, stats.level);
    let selected = ui.selected;
    let name = |id: u32| book.skills.get(&id).map_or(format!("skill {id}"), |s| s.name.clone());
    commands
        .spawn((
            SkillRoot,
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(16.0),
                top: Val::Px(150.0),
                width: Val::Px(440.0),
                max_height: Val::Percent(75.0),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(Val::Px(8.0)),
                row_gap: Val::Px(2.0),
                overflow: Overflow::clip(),
                ..default()
            },
            BackgroundColor(Color::srgba(0.04, 0.04, 0.07, 0.9)),
        ))
        .with_children(|p| {
            p.spawn((Text::new(format!("Skills   SP: {}", stats.sp)), TextFont { font_size: FontSize::Px(18.0), ..default() }));
            // Keep the selected line in view: show a window of rows around it.
            let start = selected.saturating_sub(10);
            for (i, row) in list.iter().enumerate().skip(start).take(24) {
                let (text, color) = match row {
                    Row::Learn(e) => {
                        let extra: String = e
                            .items
                            .iter()
                            .map(|(id, n)| format!(" + {} x{n}", items.items.get(id).map_or(id.to_string(), |t| t.name.clone())))
                            .collect();
                        (format!("Learn {} Lv {} - {} SP{extra}", name(e.skill), e.level, e.sp), Color::srgb(0.6, 1.0, 0.6))
                    }
                    Row::Later(e) => (format!("{} Lv {} (level {})", name(e.skill), e.level, e.get_level), Color::srgb(0.55, 0.55, 0.55)),
                    Row::Known(id, lv) => {
                        let bar = if skills.hotbar.contains(id) { "  [hotbar]" } else { "" };
                        (format!("{} Lv {lv}{bar}", name(*id)), Color::WHITE)
                    }
                };
                let color = if i == selected { Color::srgb(1.0, 0.85, 0.3) } else { color };
                p.spawn((Text::new(text), TextFont { font_size: FontSize::Px(14.0), ..default() }, TextColor(color)))
                    .observe(move |_: On<bevy::picking::events::PointerClick>, mut ui: ResMut<SkillUi>| ui.clicked = Some(i));
            }
        });
}
