//! Drives the original client windows built by `l2ui.rs`: the status window
//! (HP / MP / CP, level, name), the target window, the horizontal shortcut
//! bar with skill icons and the chat window with the system log.

use std::collections::HashMap;

use bevy::prelude::*;

use crate::datapack::{Datapack, SkillBook};
use crate::l2assets::L2Library;
use crate::l2ui::{spawn_window, BarFill, CtrlText, Interface, UiCtrl, UiTextures, UiWindow};
use crate::mmo::{Hero, Phase};
use crate::rpg::{HeroStats, Log, Npc, Target};
use crate::skillplay::HeroSkills;

pub struct L2UiBindPlugin;

impl Plugin for L2UiBindPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(Phase::InWorld), build_hud)
            .add_systems(Update, (status_window, target_window, shortcut_icons, chat_window).run_if(in_state(Phase::InWorld)));
    }
}

/// Controls only the client's scripts would show (premium, Finality, vitality
/// states, alternative chat tabs...).
fn hidden(name: &str) -> bool {
    const PARTS: [&str; 12] = [
        "_lv100", "Premium", "Finality", "LifeForce", "VpDetail", "SkillProgress", "Joypad", "Tooltip", "Lock", "Rotate", "_Add", "Language",
    ];
    PARTS.iter().any(|p| name.contains(p))
        || matches!(name, "TradeChat" | "PartyChat" | "ClanChat" | "AllyChat" | "ReduceButton" | "texAltitude" | "texDist" | "NpcInfo" | "barMP")
        || (name.starts_with("txt") && (name.contains("Pledge") || name.contains("Alliance")))
        || name.starts_with("ShortcutWndVertical")
        // Target buffs, clan crests and rank are not shown offline.
        || name.starts_with("btnBuffMoreView")
        || matches!(name, "BuffWnd" | "StatusIcons" | "texPledgeCrest" | "texPledgeAllianceCrest" | "RankName")
        || (name.starts_with("ShortcutWndHorizontal_"))
}

fn build_hud(
    mut commands: Commands,
    iface: Option<Res<Interface>>,
    windows: Query<&Window>,
    mut tex: ResMut<UiTextures>,
    mut lib: ResMut<L2Library>,
    mut images: ResMut<Assets<Image>>,
) {
    let Some(iface) = iface else { return };
    let screen = windows.iter().next().map_or(Vec2::new(1920.0, 1080.0), |w| Vec2::new(w.width(), w.height()));
    let x = &iface.0;
    for name in ["StatusWnd", "TargetStatusWnd", "ChatWnd"] {
        let Some(_) = spawn_window(&mut commands, &iface, x, name, screen, &hidden, &mut tex, &mut lib, &mut images) else {
            warn!("window {name} not in interface.xdat");
            continue;
        };
    }
    spawn_menu_bar(&mut commands, &mut tex, &mut lib, &mut images);
    // The horizontal shortcut bar is a sub-window of ShortcutWnd with its own
    // default position.
    if let Some(sw) = x.window("ShortcutWnd") {
        if let Some(h) = sw.children().iter().find(|c| c.name() == "ShortcutWndHorizontal") {
            let mut sub = (**x).clone();
            sub.windows = vec![h.clone()];
            spawn_window(&mut commands, &iface, &sub, "ShortcutWndHorizontal", screen, &hidden, &mut tex, &mut lib, &mut images);
        }
    }
}

/// Buttons of the main menu bar, in the client's order.
pub const MENU_BUTTONS: [&str; 8] = ["Character", "Inventory", "Action", "Skill", "Quest", "Clan", "Map", "SystemMenu"];

/// A main menu button and its three states.
#[derive(Component)]
struct MenuButton {
    normal: Handle<Image>,
    over: Handle<Image>,
    down: Handle<Image>,
}

/// The main menu (bottom right). The client draws it with a Flash movie,
/// so it is rebuilt here from its `Icon_DF_MenuWnd_*` button art; clicks
/// arrive as `UiClicks` from window "Menu".
fn spawn_menu_bar(commands: &mut Commands, tex: &mut UiTextures, lib: &mut L2Library, images: &mut Assets<Image>) {
    let bar = commands
        .spawn((
            UiWindow("Menu".into()),
            Node { position_type: PositionType::Absolute, right: Val::Px(4.0), bottom: Val::Px(12.0), column_gap: Val::Px(-12.0), ..default() },
        ))
        .id();
    for name in MENU_BUTTONS {
        let get = |suffix: &str, tex: &mut UiTextures, lib: &mut L2Library, images: &mut Assets<Image>| tex.get(&format!("L2UI_CT1.Icon.Icon_DF_MenuWnd_{name}{suffix}"), lib, images).map(|t| t.image);
        let Some(normal) = get("", tex, lib, images) else { continue };
        let over = get("_Over", tex, lib, images).unwrap_or(normal.clone());
        let down = get("_Down", tex, lib, images).unwrap_or(normal.clone());
        commands
            .spawn((ImageNode::new(normal.clone()), Node { width: Val::Px(52.0), height: Val::Px(52.0), ..default() }, MenuButton { normal, over, down }, ChildOf(bar)))
            .observe(|e: On<bevy::picking::events::PointerOver>, mut q: Query<(&MenuButton, &mut ImageNode)>| {
                if let Ok((b, mut img)) = q.get_mut(e.entity) {
                    img.image = b.over.clone();
                }
            })
            .observe(|e: On<bevy::picking::events::PointerOut>, mut q: Query<(&MenuButton, &mut ImageNode)>| {
                if let Ok((b, mut img)) = q.get_mut(e.entity) {
                    img.image = b.normal.clone();
                }
            })
            .observe(|e: On<bevy::picking::events::PointerPress>, mut q: Query<(&MenuButton, &mut ImageNode)>| {
                if let Ok((b, mut img)) = q.get_mut(e.entity) {
                    img.image = b.down.clone();
                }
            })
            .observe(move |e: On<bevy::picking::events::PointerClick>, mut q: Query<(&MenuButton, &mut ImageNode)>, mut clicks: ResMut<crate::l2ui::UiClicks>| {
                if let Ok((b, mut img)) = q.get_mut(e.entity) {
                    img.image = b.over.clone();
                }
                clicks.0.push(("Menu".into(), name.into()));
            });
    }
}

fn status_window(
    dp: Res<Datapack>,
    creation: Res<crate::mmo::Creation>,
    stats: Single<&HeroStats, With<Hero>>,
    ctrls: Query<(&UiCtrl, &Children)>,
    mut fills: Query<(&BarFill, &mut Node)>,
    mut texts: Query<&mut Text, (With<CtrlText>, Without<UiCtrl>)>,
    named_texts: Query<(&UiCtrl, &mut Text), With<CtrlText>>,
) {
    let s = *stats;
    let bars = [("HPBar", s.hp, s.max_hp), ("MPBar", s.mp, s.max_mp), ("CPBar", s.max_hp, s.max_hp)];
    for (ctrl, children) in &ctrls {
        if ctrl.window != "StatusWnd" {
            continue;
        }
        let Some(&(_, v, max)) = bars.iter().find(|b| b.0 == ctrl.name) else { continue };
        let frac = (v / max.max(1.0)).clamp(0.0, 1.0);
        for c in children.iter() {
            if let Ok((fill, mut node)) = fills.get_mut(c) {
                node.width = Val::Px(fill.full * frac);
            }
            if let Ok(mut t) = texts.get_mut(c) {
                let s = format!("{:.0}/{:.0}", v.max(0.0), max);
                if t.0 != s {
                    t.0 = s;
                }
            }
        }
    }
    for (ctrl, mut t) in named_texts {
        if ctrl.window == "StatusWnd" {
            if let Some(v) = status_texts(&dp, s, &ctrl.name, &creation.name) {
                if t.0 != v {
                    t.0 = v;
                }
            }
        }
    }
}

/// Level and name texts of the status window.
fn status_texts(dp: &Datapack, s: &HeroStats, ctrl: &str, name: &str) -> Option<String> {
    match ctrl {
        "StatusWnd_LevelTextBox" => Some(s.level.to_string()),
        "UserName" => Some(if name.is_empty() { dp.class_name(s.template.class_id).to_string() } else { name.to_string() }),
        _ => None,
    }
}

#[allow(clippy::type_complexity)]
fn target_window(
    target: Res<Target>,
    npcs: Query<&Npc>,
    mut roots: Query<(&UiWindow, &mut Visibility)>,
    ctrls: Query<(&UiCtrl, Option<&Children>)>,
    mut fills: Query<(&BarFill, &mut Node)>,
    mut texts: Query<(&UiCtrl, &mut Text), With<CtrlText>>,
) {
    let tgt = target.0.and_then(|e| npcs.get(e).ok());
    for (w, mut vis) in &mut roots {
        if w.0 == "TargetStatusWnd" {
            *vis = if tgt.is_some() { Visibility::Inherited } else { Visibility::Hidden };
        }
    }
    let Some(n) = tgt else { return };
    for (ctrl, children) in &ctrls {
        if ctrl.window != "TargetStatusWnd" {
            continue;
        }
        let frac = match ctrl.name.as_str() {
            "barHP" => n.hp / n.template.hp.max(1.0),
            "barMP" => 1.0,
            _ => continue,
        };
        for c in children.into_iter().flatten() {
            if let Ok((fill, mut node)) = fills.get_mut(*c) {
                node.width = Val::Px(fill.full * frac.clamp(0.0, 1.0));
            }
        }
    }
    let name = format!("{}  Lv {}", n.template.name, n.template.level);
    for (ctrl, mut t) in &mut texts {
        if ctrl.window == "TargetStatusWnd" && ctrl.name == "UserName" && t.0 != name {
            t.0 = name.clone();
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn shortcut_icons(
    mut commands: Commands,
    book: Res<SkillBook>,
    mut lib: ResMut<L2Library>,
    mut images: ResMut<Assets<Image>>,
    skills: Option<Single<&HeroSkills, With<Hero>>>,
    slots: Query<(Entity, &UiCtrl)>,
    mut shown: Local<Vec<u32>>,
    mut icons: Local<HashMap<String, Option<Handle<Image>>>>,
    mut spawned: Local<Vec<Entity>>,
) {
    let Some(skills) = skills else { return };
    if *shown == skills.hotbar && !spawned.is_empty() {
        return;
    }
    *shown = skills.hotbar.clone();
    for e in spawned.drain(..) {
        commands.entity(e).despawn();
    }
    for (e, ctrl) in &slots {
        if ctrl.window != "ShortcutWndHorizontal" {
            continue;
        }
        let Some(i) = ctrl.name.strip_prefix("Shortcut").and_then(|n| n.parse::<usize>().ok()) else { continue };
        let Some(skill) = skills.hotbar.get(i - 1) else { continue };
        let icon_name = book.skills.get(skill).map(|s| s.icon.clone()).unwrap_or_default();
        let icon = icons
            .entry(icon_name.clone())
            .or_insert_with(|| {
                let (pkg, obj) = icon_name.split_once('.')?;
                lib.texture(pkg, obj, false).map(|img| images.add(img))
            })
            .clone();
        if let Some(img) = icon {
            let id = commands
                .spawn((ImageNode::new(img), Node { position_type: PositionType::Absolute, width: Val::Px(32.0), height: Val::Px(32.0), left: Val::Px(2.0), top: Val::Px(2.0), ..default() }, ChildOf(e)))
                .id();
            spawned.push(id);
        }
        let label = commands
            .spawn((
                Text::new(i.to_string()),
                TextFont { font_size: FontSize::Px(10.0), ..default() },
                Node { position_type: PositionType::Absolute, left: Val::Px(3.0), top: Val::Px(1.0), ..default() },
                ChildOf(e),
            ))
            .id();
        spawned.push(label);
    }
}

fn chat_window(log: Res<Log>, mut texts: Query<(&UiCtrl, &mut Text), With<CtrlText>>) {
    if !log.is_changed() {
        return;
    }
    let body = log.0.iter().cloned().collect::<Vec<_>>().join("\n");
    for (ctrl, mut t) in &mut texts {
        if ctrl.window == "ChatWnd" && ctrl.name == "NormalChat" {
            t.0 = body.clone();
        }
    }
}
