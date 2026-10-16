//! The client's shortcut bars over `skillplay`'s hotbar: up to five rows of
//! 12 slots (the expand button on the main bar adds rows, the reduce button
//! takes them back to one), each row showing one of ten pages (its arrow
//! buttons; Page Up / Page Down for the main row). Skills drag from the skill
//! window onto a slot, from slot to slot (swapping), and off the bar to take
//! them off; a right click empties a slot, a left click casts it. Slots shade
//! while their skill cools down, and a cast bar shows the skill being cast.

use std::collections::HashMap;

use bevy::picking::events::{PointerClick, PointerDrag, PointerDragDrop, PointerDragEnd, PointerDragStart};
use bevy::picking::Pickable;
use bevy::prelude::*;

use crate::datapack::SkillBook;
use crate::l2assets::L2Library;
use crate::l2ui::{CtrlText, UiClicks, UiCtrl, UiWindow};
use crate::mmo::{Hero, Phase};
use crate::skillplay::{HeroSkills, HotbarPages, HotbarPress, BAR_SLOTS, MAX_BARS, PAGES};

pub struct HotbarPlugin;

impl Plugin for HotbarPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SkillDrag>().add_systems(
            Update,
            (bind_slots, hotbar_rows, slot_icons, cooldown_shades, cast_bar).chain().run_if(in_state(Phase::InWorld)),
        );
    }
}

/// A skill icon that can be dragged onto a hotbar slot; `from` is the hotbar
/// slot it sits in (none for the skill window).
#[derive(Component, Clone, Copy)]
pub struct DragSkill {
    pub skill: u32,
    pub from: Option<usize>,
}

/// The skill drag in progress.
#[derive(Resource, Default)]
pub struct SkillDrag {
    /// A skill icon is being dragged (its click is not a click).
    pub active: bool,
    /// It was dropped on a slot.
    dropped: bool,
    /// The icon following the cursor.
    ghost: Option<Entity>,
}

/// A slot of a shortcut bar: row (0 = main bar) and slot in the row.
#[derive(Component, Clone, Copy)]
pub struct HotbarSlot {
    pub bar: usize,
    pub slot: usize,
}

/// Things drawn into the slots, rebuilt when the hotbar changes.
#[derive(Component)]
struct SlotContent;

/// Dark cover over an icon whose height follows the cooldown left.
#[derive(Component)]
struct CooldownShade(u32);

/// Row of a shortcut bar window: `ShortcutWndHorizontal` is the main one,
/// `ShortcutWndHorizontal_1`.. `_4` the added rows.
fn bar_of(window: &str) -> Option<usize> {
    if window == "ShortcutWndHorizontal" {
        return Some(0);
    }
    window.strip_prefix("ShortcutWndHorizontal_")?.parse().ok().filter(|k| (1..MAX_BARS).contains(k))
}

fn bar_window(bar: usize) -> String {
    if bar == 0 {
        "ShortcutWndHorizontal".into()
    } else {
        format!("ShortcutWndHorizontal_{bar}")
    }
}

/// Key shown in the corner of a slot.
fn key_hint(bar: usize, slot: usize) -> String {
    const KEYS: [&str; BAR_SLOTS] = ["1", "2", "3", "4", "5", "6", "7", "8", "9", "0", "-", "="];
    match bar {
        0 => KEYS[slot].into(),
        1 => format!("C{}", KEYS[slot]),
        2 => format!("A{}", KEYS[slot]),
        _ => String::new(),
    }
}

/// Lets a skill icon be dragged onto the hotbar.
pub fn make_draggable(e: &mut EntityCommands, skill: u32, from: Option<usize>) {
    let id = e.id();
    e.insert(DragSkill { skill, from });
    e.observe(move |mut ev: On<PointerDragStart>, mut commands: Commands, mut drag: ResMut<SkillDrag>, images: Query<&ImageNode>| {
        // The drag is the icon's, not the window's.
        ev.propagate(false);
        if ev.button != PointerButton::Primary {
            return;
        }
        drag.active = true;
        drag.dropped = false;
        if let Some(g) = drag.ghost.take() {
            commands.entity(g).try_despawn();
        }
        let Ok(img) = images.get(id) else { return };
        let p = ev.pointer.position;
        let ghost = commands
            .spawn((
                ImageNode::new(img.image.clone()).with_color(Color::srgba(1.0, 1.0, 1.0, 0.8)),
                Node { position_type: PositionType::Absolute, left: Val::Px(p.x - 16.0), top: Val::Px(p.y - 16.0), width: Val::Px(32.0), height: Val::Px(32.0), ..default() },
                Pickable::IGNORE,
                GlobalZIndex(i32::MAX - 1),
            ))
            .id();
        drag.ghost = Some(ghost);
    });
    e.observe(|mut ev: On<PointerDrag>, drag: Res<SkillDrag>, mut nodes: Query<&mut Node>| {
        ev.propagate(false);
        let Some(mut n) = drag.ghost.and_then(|g| nodes.get_mut(g).ok()) else { return };
        let p = ev.pointer.position;
        n.left = Val::Px(p.x - 16.0);
        n.top = Val::Px(p.y - 16.0);
    });
    e.observe(move |mut ev: On<PointerDragEnd>, mut commands: Commands, mut drag: ResMut<SkillDrag>, src: Query<&DragSkill>, mut skills: Query<&mut HeroSkills, With<Hero>>| {
        ev.propagate(false);
        if let Some(g) = drag.ghost.take() {
            commands.entity(g).try_despawn();
        }
        if !drag.active {
            return;
        }
        // Dragged off the bar: the slot empties.
        if !drag.dropped {
            if let (Ok(DragSkill { skill, from: Some(i) }), Ok(mut s)) = (src.get(id), skills.single_mut()) {
                if s.slot(*i) == *skill {
                    s.set_slot(*i, 0);
                }
            }
        }
        drag.active = false;
        drag.dropped = false;
    });
}

/// Gives the shortcut bar slots their clicks and drops once built.
fn bind_slots(mut commands: Commands, ctrls: Query<(Entity, &UiCtrl), Added<UiCtrl>>) {
    for (e, c) in &ctrls {
        if c.class != "ShortcutItemWindow" {
            continue;
        }
        let Some(bar) = bar_of(&c.window) else { continue };
        let Some(n) = c.name.strip_prefix("Shortcut").and_then(|n| n.parse::<usize>().ok()).filter(|n| (1..=BAR_SLOTS).contains(n)) else { continue };
        let slot = n - 1;
        commands
            .entity(e)
            .insert(HotbarSlot { bar, slot })
            .observe(move |mut ev: On<PointerClick>, pages: Res<HotbarPages>, drag: Res<SkillDrag>, mut press: ResMut<HotbarPress>, mut skills: Query<&mut HeroSkills, With<Hero>>| {
                ev.propagate(false);
                if drag.active {
                    return;
                }
                let i = pages.index(bar, slot);
                match ev.button {
                    PointerButton::Primary => press.0 = Some(i),
                    PointerButton::Secondary => {
                        if let Ok(mut s) = skills.single_mut() {
                            s.set_slot(i, 0);
                        }
                    }
                    _ => {}
                }
            })
            .observe(move |mut ev: On<PointerDragDrop>, pages: Res<HotbarPages>, mut drag: ResMut<SkillDrag>, src: Query<&DragSkill>, mut skills: Query<&mut HeroSkills, With<Hero>>| {
                ev.propagate(false);
                // The drop reaches the slot from the icon and the slot alike.
                if drag.dropped || !drag.active {
                    return;
                }
                let Ok(d) = src.get(ev.dropped) else { return };
                let Ok(mut s) = skills.single_mut() else { return };
                let to = pages.index(bar, slot);
                let old = s.slot(to);
                s.set_slot(to, d.skill);
                // From another slot: the two swap.
                if let Some(from) = d.from.filter(|f| *f != to) {
                    s.set_slot(from, old);
                }
                drag.dropped = true;
            });
    }
}

/// Rows shown, their pages and places: they stack above the main bar.
#[allow(clippy::type_complexity)]
fn hotbar_rows(
    keys: Res<ButtonInput<KeyCode>>,
    mut pages: ResMut<HotbarPages>,
    mut clicks: ResMut<UiClicks>,
    mut roots: Query<(&UiWindow, &mut Node, &mut Visibility)>,
    mut ctrls: Query<(&UiCtrl, &mut Visibility), Without<UiWindow>>,
    mut texts: Query<(&UiCtrl, &mut Text), With<CtrlText>>,
) {
    let main = "ShortcutWndHorizontal";
    if clicks.take(main, "ExpandButton") {
        pages.bars = (pages.bars + 1).min(MAX_BARS);
    }
    if clicks.take(main, "ReduceButton") {
        pages.bars = 1;
    }
    let turn = |p: &mut usize, d: i32| *p = (*p as i32 + d).rem_euclid(PAGES as i32) as usize;
    for bar in 0..MAX_BARS {
        let (w, suffix) = (bar_window(bar), if bar == 0 { String::new() } else { (bar + 1).to_string() });
        if clicks.take(&w, &format!("NextBtn{suffix}")) {
            turn(&mut pages.pages[bar], 1);
        }
        if clicks.take(&w, &format!("PrevBtn{suffix}")) {
            turn(&mut pages.pages[bar], -1);
        }
    }
    if keys.just_pressed(KeyCode::PageUp) {
        turn(&mut pages.pages[0], 1);
    }
    if keys.just_pressed(KeyCode::PageDown) {
        turn(&mut pages.pages[0], -1);
    }
    // The added rows follow the main bar.
    let Some((left, top, height)) = roots.iter().find(|r| r.0 .0 == main).and_then(|(_, n, _)| match (n.left, n.top, n.height) {
        (Val::Px(l), Val::Px(t), Val::Px(h)) => Some((l, t, h)),
        _ => None,
    }) else {
        return;
    };
    for (w, mut node, mut vis) in &mut roots {
        let Some(bar) = bar_of(&w.0).filter(|b| *b > 0) else { continue };
        let want = if bar < pages.bars { Visibility::Inherited } else { Visibility::Hidden };
        if *vis != want {
            *vis = want;
        }
        let y = Val::Px(top - height * bar as f32);
        if node.left != Val::Px(left) || node.top != y {
            node.left = Val::Px(left);
            node.top = y;
        }
    }
    for (c, mut vis) in &mut ctrls {
        if c.window != main {
            continue;
        }
        let want = match c.name.as_str() {
            "ExpandButton" => pages.bars < MAX_BARS,
            "ReduceButton" => pages.bars >= MAX_BARS,
            _ => continue,
        };
        let want = if want { Visibility::Inherited } else { Visibility::Hidden };
        if *vis != want {
            *vis = want;
        }
    }
    for (c, mut t) in &mut texts {
        let Some(bar) = bar_of(&c.window) else { continue };
        if c.name == "PageNumTextBox" {
            let s = (pages.pages[bar] + 1).to_string();
            if t.0 != s {
                t.0 = s;
            }
        }
    }
}

/// Skill icons and key hints in the slots.
#[allow(clippy::too_many_arguments)]
fn slot_icons(
    mut commands: Commands,
    book: Res<SkillBook>,
    pages: Res<HotbarPages>,
    mut lib: ResMut<L2Library>,
    mut images: ResMut<Assets<Image>>,
    skills: Option<Single<&HeroSkills, With<Hero>>>,
    slots: Query<(Entity, &HotbarSlot)>,
    old: Query<Entity, With<SlotContent>>,
    mut shown: Local<Option<(Vec<u32>, [usize; MAX_BARS], usize)>>,
    mut icons: Local<HashMap<String, Option<Handle<Image>>>>,
) {
    let Some(skills) = skills else { return };
    let now = (skills.hotbar.clone(), pages.pages, slots.iter().count());
    if shown.as_ref() == Some(&now) {
        return;
    }
    *shown = Some(now);
    for e in &old {
        commands.entity(e).try_despawn();
    }
    for (e, s) in &slots {
        let i = pages.index(s.bar, s.slot);
        let id = skills.slot(i);
        if id != 0 {
            let file = book.skills.get(&id).map(|s| s.icon.clone()).unwrap_or_default();
            let icon = icons
                .entry(file.clone())
                .or_insert_with(|| {
                    let (pkg, obj) = file.split_once('.')?;
                    lib.texture(pkg, obj, false).map(|img| images.add(img))
                })
                .clone();
            let mut node = Node { position_type: PositionType::Absolute, width: Val::Px(32.0), height: Val::Px(32.0), left: Val::Px(2.0), top: Val::Px(2.0), ..default() };
            node.overflow = Overflow::clip();
            let mut ec = commands.spawn((SlotContent, node, ChildOf(e)));
            match icon {
                Some(img) => ec.insert(ImageNode::new(img)),
                None => ec.insert(BackgroundColor(Color::srgb(0.25, 0.22, 0.18))),
            };
            make_draggable(&mut ec, id, Some(i));
            let icon_e = ec.id();
            commands.spawn((
                CooldownShade(id),
                Node { position_type: PositionType::Absolute, left: Val::Px(0.0), bottom: Val::Px(0.0), width: Val::Percent(100.0), height: Val::Percent(0.0), ..default() },
                BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.65)),
                Pickable::IGNORE,
                ChildOf(icon_e),
            ));
        }
        let hint = key_hint(s.bar, s.slot);
        if !hint.is_empty() {
            commands.spawn((
                SlotContent,
                Text::new(hint),
                TextFont { font_size: FontSize::Px(10.0), ..default() },
                TextShadow::default(),
                Node { position_type: PositionType::Absolute, left: Val::Px(3.0), top: Val::Px(1.0), ..default() },
                Pickable::IGNORE,
                GlobalZIndex(1),
                ChildOf(e),
            ));
        }
    }
}

fn cooldown_shades(skills: Option<Single<&HeroSkills, With<Hero>>>, mut shades: Query<(&CooldownShade, &mut Node)>) {
    let Some(skills) = skills else { return };
    for (s, mut n) in &mut shades {
        let frac = skills.cooldown(s.0).map_or(0.0, |(left, total)| (left / total.max(0.01)).clamp(0.0, 1.0));
        let h = Val::Percent(frac * 100.0);
        if n.height != h {
            n.height = h;
        }
    }
}

#[derive(Component)]
struct CastBar;

#[derive(Component)]
struct CastFill;

#[derive(Component)]
struct CastName;

#[derive(Component)]
struct CastTime;

/// The bar under the screen centre filling while a skill is cast.
#[allow(clippy::type_complexity)]
fn cast_bar(
    mut commands: Commands,
    book: Res<SkillBook>,
    skills: Option<Single<&HeroSkills, With<Hero>>>,
    mut bar: Query<&mut Visibility, With<CastBar>>,
    mut fill: Query<&mut Node, With<CastFill>>,
    mut name: Query<&mut Text, (With<CastName>, Without<CastTime>)>,
    mut time: Query<&mut Text, (With<CastTime>, Without<CastName>)>,
) {
    let Ok(mut vis) = bar.single_mut() else {
        let gold = Color::srgb(0.93, 0.85, 0.62);
        commands
            .spawn((
                CastBar,
                Visibility::Hidden,
                Pickable::IGNORE,
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Percent(50.0),
                    top: Val::Percent(68.0),
                    width: Val::Px(260.0),
                    margin: UiRect::left(Val::Px(-130.0)),
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(2.0),
                    ..default()
                },
            ))
            .with_children(|p| {
                p.spawn((Node { justify_content: JustifyContent::SpaceBetween, ..default() }, Pickable::IGNORE)).with_children(|r| {
                    r.spawn((CastName, Text::new(""), TextFont { font_size: FontSize::Px(13.0), ..default() }, TextColor(gold), TextShadow::default(), Pickable::IGNORE));
                    r.spawn((CastTime, Text::new(""), TextFont { font_size: FontSize::Px(12.0), ..default() }, TextColor(gold), TextShadow::default(), Pickable::IGNORE));
                });
                p.spawn((
                    Node { width: Val::Percent(100.0), height: Val::Px(10.0), border: UiRect::all(Val::Px(1.0)), padding: UiRect::all(Val::Px(1.0)), ..default() },
                    BorderColor::all(Color::srgb(0.55, 0.46, 0.3)),
                    BackgroundColor(Color::srgba(0.02, 0.02, 0.03, 0.75)),
                    Pickable::IGNORE,
                ))
                .with_children(|b| {
                    b.spawn((CastFill, Node { width: Val::Percent(0.0), height: Val::Percent(100.0), ..default() }, BackgroundColor(Color::srgb(0.95, 0.72, 0.25)), Pickable::IGNORE));
                });
            });
        return;
    };
    let cast = skills.as_ref().and_then(|s| s.casting());
    let want = if cast.is_some() { Visibility::Inherited } else { Visibility::Hidden };
    if *vis != want {
        *vis = want;
    }
    let Some((id, t, total)) = cast else { return };
    let frac = (t / total.max(0.01)).clamp(0.0, 1.0);
    for mut n in &mut fill {
        n.width = Val::Percent(frac * 100.0);
    }
    let skill = book.skills.get(&id).map_or_else(|| format!("skill {id}"), |s| s.name.clone());
    for mut s in &mut name {
        if s.0 != skill {
            s.0 = skill.clone();
        }
    }
    let left = format!("{:.1}", (total - t).max(0.0));
    for mut s in &mut time {
        if s.0 != left {
            s.0 = left.clone();
        }
    }
}
