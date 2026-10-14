//! Client windows opened on demand: the inventory (I) and the character
//! status (Alt+T), built from interface.xdat and filled from the game.

use std::collections::HashMap;

use bevy::prelude::*;

use crate::datapack::Datapack;
use crate::items::{Inventory, InventoryUi, ItemClick, ItemData, ADENA};
use crate::l2assets::L2Library;
use crate::l2ui::{spawn_window, BarFill, CtrlText, Interface, ItemGrid, UiCtrl, UiTextures, UiWindow};
use crate::mmo::{Hero, Phase};
use crate::rpg::HeroStats;

pub struct L2UiWindowsPlugin;

impl Plugin for L2UiWindowsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<StatusOpen>()
            .add_systems(Update, (inventory_wnd, fill_inventory, status_wnd, fill_status, system_menu).chain().run_if(in_state(Phase::InWorld)));
    }
}

/// Alt+X: the system menu. Offline, only "Quit" does something (saving on
/// the way out); the rest are online services.
fn system_menu(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    mut ua: crate::l2ui::UiAssets,
    roots: Query<(Entity, &UiWindow)>,
    mut clicks: ResMut<crate::l2ui::UiClicks>,
    mut log: ResMut<crate::rpg::Log>,
    mut exit: MessageWriter<AppExit>,
    mut env_done: Local<bool>,
) {
    let alt = keys.pressed(KeyCode::AltLeft) || keys.pressed(KeyCode::AltRight);
    let open = find_root(&roots, "SystemMenuWnd");
    let by_env = !*env_done && std::env::var_os("L2_SYSMENU").is_some();
    *env_done = true;
    if by_env || clicks.take("Menu", "SystemMenu") || alt && keys.just_pressed(KeyCode::KeyX) || (open.is_some() && keys.just_pressed(KeyCode::Escape)) {
        match open {
            Some(e) => commands.entity(e).despawn(),
            None => {
                let screen = ua.screen();
                if let Some(i) = ua.iface.as_deref() {
                    // It hangs off the main menu button (bottom right), which
                    // lives outside interface.xdat.
                    if let Some(root) = spawn_window(&mut commands, i, &i.0, "SystemMenuWnd", screen, &|_| false, &mut ua.tex, &mut ua.lib, &mut ua.images) {
                        commands.entity(root).entry::<Node>().and_modify(|mut n| {
                            n.left = Val::Auto;
                            n.top = Val::Auto;
                            n.right = Val::Px(3.0);
                            n.bottom = Val::Px(60.0);
                        });
                    }
                }
            }
        }
    }
    if clicks.take("SystemMenuWnd", "btnQuit") {
        exit.write(AppExit::Success);
    }
    for (_, c) in clicks.0.iter().filter(|(w, c)| w == "SystemMenuWnd" || (w == "Menu" && matches!(c.as_str(), "Action" | "Clan"))) {
        log.push(format!("{c}: not available offline."));
    }
    clicks.0.retain(|(w, c)| w != "SystemMenuWnd" && !(w == "Menu" && matches!(c.as_str(), "Action" | "Clan")));
}

fn screen(windows: &Query<&Window>) -> Vec2 {
    windows.iter().next().map_or(Vec2::new(1920.0, 1080.0), |w| Vec2::new(w.width(), w.height()))
}

fn find_root(roots: &Query<(Entity, &UiWindow)>, name: &str) -> Option<Entity> {
    roots.iter().find(|(_, w)| w.0 == name).map(|(e, _)| e)
}

/// Classic inventory: the classic equipment panel and the first item tab.
fn inventory_hidden(name: &str) -> bool {
    if let Some(n) = name.strip_prefix("show:") {
        return n == "Equip_classic";
    }
    const PARTS: [&str; 14] = [
        "Equip_Live", "Talisman", "Jewel", "Henna", "Alchemy", "Brooch", "Charge_", "ObjectViewport", "BtnRotate", "_expand", "Expand",
        "Crystallize", "Adenacalculate", "EnchantJewel",
    ];
    PARTS.iter().any(|p| name.contains(p)) || name.starts_with("InventoryItem_") || name == "QuestItem"
}

#[allow(clippy::too_many_arguments)]
fn inventory_wnd(
    mut commands: Commands,
    ui: Res<InventoryUi>,
    iface: Option<Res<Interface>>,
    windows: Query<&Window>,
    roots: Query<(Entity, &UiWindow)>,
    mut tex: ResMut<UiTextures>,
    mut lib: ResMut<L2Library>,
    mut images: ResMut<Assets<Image>>,
) {
    let Some(iface) = iface else { return };
    let open = find_root(&roots, "InventoryWnd");
    match (ui.open, open) {
        (true, None) => {
            spawn_window(&mut commands, &iface, &iface.0, "InventoryWnd", screen(&windows), &inventory_hidden, &mut tex, &mut lib, &mut images);
        }
        (false, Some(e)) => commands.entity(e).despawn(),
        _ => {}
    }
}

/// Equipment slot controls of the classic panel and the inventory slot they show.
const EQUIP_SLOTS: [(&str, &str); 16] = [
    ("EquipItem_Head", "head"),
    ("EquipItem_Chest", "chest"),
    ("EquipItem_Legs", "legs"),
    ("EquipItem_Feet", "feet"),
    ("EquipItem_Gloves", "gloves"),
    ("EquipItem_Cloak", "back"),
    ("EquipItem_Underwear", "underwear"),
    ("EquipItem_RHand", "rhand"),
    ("EquipItem_LHand", "lhand"),
    ("EquipItem_Neck", "neck"),
    ("EquipItem_LEar", "ear"),
    ("EquipItem_LFinger", "finger"),
    ("EquipItem_Hair", "hair"),
    ("EquipItem_Hair2", "hair2"),
    ("EquipItem_Waist", "waist"),
    ("EquipItem_Brooch", "brooch"),
];

#[derive(Component)]
struct InvContent;

#[allow(clippy::too_many_arguments)]
fn fill_inventory(
    mut commands: Commands,
    data: Res<ItemData>,
    mut lib: ResMut<L2Library>,
    mut images: ResMut<Assets<Image>>,
    inv: Option<Single<&Inventory, With<Hero>>>,
    ctrls: Query<(Entity, &UiCtrl, Option<&ItemGrid>)>,
    placed: Query<(&Node, &ChildOf), Without<InvContent>>,
    mut texts: Query<(&UiCtrl, &mut Text), With<CtrlText>>,
    content: Query<Entity, With<InvContent>>,
    mut shown: Local<Option<(Vec<(u32, u64)>, Vec<u32>, usize)>>,
    mut icons: Local<HashMap<String, Option<Handle<Image>>>>,
) {
    let Some(inv) = inv else { return };
    let grid = ctrls.iter().find(|(_, c, _)| c.window == "InventoryWnd" && c.name == "InventoryItem");
    let Some((grid_e, _, Some(grid))) = grid else {
        *shown = None;
        return;
    };
    let items: Vec<(u32, u64)> = inv.items.iter().filter(|(id, _)| !inv.equipped.values().any(|e| e == *id)).map(|(k, v)| (*k, *v)).collect();
    let equipped: Vec<u32> = inv.equipped.values().copied().collect();
    let state = (items.clone(), equipped.clone(), inv.auto_shots.len());
    if shown.as_ref() == Some(&state) {
        return;
    }
    *shown = Some(state);
    debug!("inventory window: {} in bag, equipped {:?}", items.len(), inv.equipped);
    for e in &content {
        commands.entity(e).despawn();
    }
    let mut icon = |id: u32| {
        let name = data.items.get(&id).map(|t| t.icon.clone()).unwrap_or_default();
        icons
            .entry(name.clone())
            .or_insert_with(|| {
                let (pkg, obj) = name.split_once('.')?;
                lib.texture(pkg, obj, false).map(|img| images.add(img))
            })
            .clone()
    };
    let mut put = |commands: &mut Commands, parent: Entity, pos: Vec2, size: Vec2, id: u32, count: u64, auto: bool| {
        let mut ec = commands.spawn((
            InvContent,
            Node { position_type: PositionType::Absolute, left: Val::Px(pos.x), top: Val::Px(pos.y), width: Val::Px(size.x), height: Val::Px(size.y), ..default() },
            ChildOf(parent),
        ));
        if let Some(img) = icon(id) {
            ec.insert(ImageNode::new(img));
        } else {
            ec.insert(BackgroundColor(Color::srgb(0.3, 0.3, 0.3)));
        }
        ec.observe(move |_: On<bevy::picking::events::PointerClick>, mut click: ResMut<ItemClick>| click.0 = Some(id));
        let e = ec.id();
        if count > 1 || auto {
            let label = if auto { format!("{count} A") } else { count.to_string() };
            commands.spawn((
                Text::new(label),
                TextFont { font_size: FontSize::Px(10.0), ..default() },
                TextColor(if auto { Color::srgb(1.0, 0.85, 0.3) } else { Color::WHITE }),
                Node { position_type: PositionType::Absolute, right: Val::Px(1.0), bottom: Val::Px(0.0), ..default() },
                ChildOf(e),
            ));
        }
    };
    let px = |v: Val| if let Val::Px(p) = v { p } else { 0.0 };
    if let Ok((node, parent)) = placed.get(grid_e) {
        let origin = Vec2::new(px(node.left), px(node.top));
        debug!("inventory grid at {origin} {:?}", (grid.cols, grid.icon, grid.gap, grid.offset));
        for (i, (id, count)) in items.iter().enumerate() {
            put(&mut commands, parent.parent(), origin + grid.slot(i as u32), grid.icon, *id, *count, inv.auto_shots.contains(id));
        }
    }
    for (ctrl_name, slot) in EQUIP_SLOTS {
        let Some(&id) = inv.equipped.get(slot) else { continue };
        // On the window itself, above the slot background drawn after the slots.
        if let Some((e, c, _)) = ctrls.iter().find(|(_, c, _)| c.window == "InventoryWnd" && c.name == ctrl_name) {
            let Ok((node, parent)) = placed.get(e) else { continue };
            put(&mut commands, parent.parent(), Vec2::new(px(node.left), px(node.top)), c.size, id, 1, false);
        }
    }
    let adena = inv.items.get(&ADENA).copied().unwrap_or(0);
    for (c, mut t) in &mut texts {
        if c.window != "InventoryWnd" {
            continue;
        }
        let v = match c.name.as_str() {
            "AdenaText" => adena.to_string(),
            "ItemCount" => format!("({}/80)", inv.items.len()),
            _ => continue,
        };
        t.0 = v;
    }
}

#[derive(Resource, Default)]
struct StatusOpen(bool);

#[allow(clippy::too_many_arguments)]
fn status_wnd(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    mut open: ResMut<StatusOpen>,
    iface: Option<Res<Interface>>,
    windows: Query<&Window>,
    roots: Query<(Entity, &UiWindow)>,
    mut tex: ResMut<UiTextures>,
    mut lib: ResMut<L2Library>,
    mut images: ResMut<Assets<Image>>,
    mut env_done: Local<bool>,
    mut clicks: ResMut<crate::l2ui::UiClicks>,
) {
    let Some(iface) = iface else { return };
    let alt = keys.pressed(KeyCode::AltLeft) || keys.pressed(KeyCode::AltRight);
    if clicks.take("Menu", "Character") || alt && keys.just_pressed(KeyCode::KeyT) {
        open.0 = !open.0;
    }
    if !*env_done && std::env::var_os("L2_STATUS").is_some() {
        *env_done = true;
        open.0 = true;
    }
    let root = find_root(&roots, "DetailStatusWndClassic");
    match (open.0, root) {
        (true, None) => {
            let hide = |n: &str| n.contains("Social") || n.contains("Criminal") || n.contains("Sulffrage") || n.contains("RaidPoint") || n.contains("Pledge") || n.contains("Rank");
            spawn_window(&mut commands, &iface, &iface.0, "DetailStatusWndClassic", screen(&windows), &hide, &mut tex, &mut lib, &mut images);
        }
        (false, Some(e)) => commands.entity(e).despawn(),
        _ => {}
    }
}

#[allow(clippy::type_complexity)]
fn fill_status(
    dp: Res<Datapack>,
    stats: Single<&HeroStats, With<Hero>>,
    ctrls: Query<(&UiCtrl, &Children)>,
    mut fills: Query<(&BarFill, &mut Node)>,
    mut bar_texts: Query<&mut Text, (With<CtrlText>, Without<UiCtrl>)>,
    mut texts: Query<(&UiCtrl, &mut Text), With<CtrlText>>,
) {
    let s = *stats;
    let cur = dp.exp.get(s.level as usize).copied().unwrap_or(0);
    let next = dp.exp.get(s.level as usize + 1).copied().unwrap_or(cur + 1);
    let exp = s.exp.saturating_sub(cur) as f32 / next.saturating_sub(cur).max(1) as f32;
    let bars = [
        ("texhp", s.hp / s.max_hp.max(1.0), format!("{:.0}/{:.0}", s.hp.max(0.0), s.max_hp)),
        ("texMP", s.mp / s.max_mp.max(1.0), format!("{:.0}/{:.0}", s.mp, s.max_mp)),
        ("texcp", 1.0, format!("{:.0}/{:.0}", s.max_hp, s.max_hp)),
        ("texexp", exp, format!("{:.2}%", exp * 100.0)),
    ];
    for (c, children) in &ctrls {
        if c.window != "DetailStatusWndClassic" {
            continue;
        }
        let Some((_, frac, label)) = bars.iter().find(|b| b.0 == c.name) else { continue };
        for ch in children.iter() {
            if let Ok((fill, mut node)) = fills.get_mut(ch) {
                node.width = Val::Px(fill.full * frac.clamp(0.0, 1.0));
            }
            if let Ok(mut t) = bar_texts.get_mut(ch) {
                if &t.0 != label {
                    t.0 = label.clone();
                }
            }
        }
    }
    let t = &s.template;
    for (c, mut text) in &mut texts {
        if c.window != "DetailStatusWndClassic" {
            continue;
        }
        let v = match c.name.as_str() {
            "txtName1" => dp.class_name(t.class_id).to_string(),
            "txtLvName" => s.level.to_string(),
            "txtClassName" => dp.class_name(t.class_id).to_string(),
            "txtSP" => s.sp.to_string(),
            "txtPhysicalAttack" => format!("{:.0}", s.patk),
            "txtPhysicalDefense" => format!("{:.0}", s.pdef),
            "txtHitRate" => format!("{:.0}", s.accuracy),
            "txtPhysicalAvoid" => format!("{:.0}", s.evasion),
            "txtCriticalRate" => format!("{:.0}", s.crit),
            "txtPhysicalAttackSpeed" => format!("{:.0}", s.patk_speed),
            "txtMovingSpeed" => format!("{:.0}", s.run_speed),
            "txtMagicalAttack" => format!("{:.0}", s.matk),
            "txtMagicDefense" => format!("{:.0}", s.mdef),
            "txtMagicHit" => format!("{:.0}", s.accuracy),
            "txtMagicAvoid" => format!("{:.0}", s.evasion),
            "txtMagicCritical" => "50".to_string(),
            "txtMagicCastingSpeed" => format!("{:.0}", s.matk_speed),
            "txtSTR" => format!("{:.0}", t.str_),
            "txtDEX" => format!("{:.0}", t.dex),
            "txtCON" => format!("{:.0}", t.con),
            "txtINT" => format!("{:.0}", t.int),
            "txtWIT" => format!("{:.0}", t.wit),
            "txtMEN" => format!("{:.0}", t.men),
            _ => continue,
        };
        if text.0 != v {
            text.0 = v;
        }
    }
}
