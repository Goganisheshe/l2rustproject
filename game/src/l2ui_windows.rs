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
            .init_resource::<OptionsOpen>()
            .init_resource::<HelpOpen>()
            .init_resource::<GameOptions>()
            .add_systems(
                Update,
                (inventory_wnd, fill_inventory, status_wnd, fill_status, system_menu, options_window, help_window, apply_options)
                    .chain()
                    .run_if(in_state(Phase::InWorld)),
            );
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
    (mut options, mut help): (ResMut<OptionsOpen>, ResMut<HelpOpen>),
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
    // A row is clicked by its icon (btnX) or its label (txtX).
    let row = |c: &str| -> &'static str {
        match c {
            "btnQuit" | "txtQuit" => "quit",
            "btnRestart" | "txtRestart" => "restart",
            "btnOption" | "txtOption" => "options",
            "btnHelpHtml" | "txtHelpHtml" => "help",
            "btnArchive" | "txtArchive" => "Архив",
            "btnMovieCapture" | "txtMovieRec" => "Запись видео",
            "btnPersonalConnections" | "txtPersonalConnections" => "Контакты",
            "btnPost" | "txtPost" => "Почтовый ящик",
            "btnBBS" | "txtBBS" => "Форум",
            "btnMacro" | "txtMacro" => "Макросы",
            "btnPetition" | "txtPetition" => "Петиция",
            "btnHomepage" | "txtHomepage" => "Сайт игры",
            "btn24hz" | "txt24hz" => "24Hz",
            "close" => "close",
            _ => "",
        }
    };
    let picked: Vec<&str> = clicks.0.iter().filter(|(w, _)| w == "SystemMenuWnd").map(|(_, c)| row(c)).filter(|r| !r.is_empty()).collect();
    clicks.0.retain(|(w, _)| w != "SystemMenuWnd");
    let open_menu = find_root(&roots, "SystemMenuWnd");
    for action in picked {
        if let Some(e) = open_menu {
            commands.entity(e).despawn();
        }
        match action {
            "quit" => {
                exit.write(AppExit::Success);
            }
            // Back to the character select: the game saves on exit and
            // starts again at the character list.
            "restart" => {
                if let Ok(exe) = std::env::current_exe() {
                    let mut cmd = std::process::Command::new(exe);
                    cmd.env("L2_LOBBY", "1");
                    for k in ["L2_CLASS", "L2_CONTINUE", "L2_SCREENSHOT", "L2_INVENTORY", "L2_STATUS", "L2_SYSMENU", "L2_MAP", "L2_QUESTS", "L2_TALK", "L2_DIE"] {
                        cmd.env_remove(k);
                    }
                    if let Ok(dir) = std::env::current_dir() {
                        cmd.current_dir(dir);
                    }
                    if cmd.spawn().is_ok() {
                        exit.write(AppExit::Success);
                    } else {
                        log.push("Не удалось перезапустить игру.");
                    }
                }
            }
            "options" => options.0 = true,
            "help" => help.0 = true,
            "close" => {}
            other => log.push(format!("{other}: недоступно в офлайн-режиме.")),
        }
    }
    for (_, c) in clicks.0.iter().filter(|(w, c)| w == "Menu" && matches!(c.as_str(), "Action" | "Clan")) {
        let name = if c == "Action" { "Действия" } else { "Клан" };
        log.push(format!("{name}: недоступно в офлайн-режиме."));
    }
    clicks.0.retain(|(w, c)| !(w == "Menu" && matches!(c.as_str(), "Action" | "Clan")));
}

#[derive(Resource, Default)]
pub struct OptionsOpen(pub bool);

#[derive(Resource, Default)]
pub struct HelpOpen(pub bool);

/// Settings changed in the options window.
#[derive(Resource)]
pub struct GameOptions {
    pub fullscreen: bool,
    pub ui_scale: f32,
}

impl Default for GameOptions {
    fn default() -> Self {
        Self { fullscreen: false, ui_scale: 1.0 }
    }
}

#[derive(Component)]
struct OptionsRoot;

#[derive(Component)]
struct HelpRoot;

/// The system menu's "Настройки": screen mode and interface scale.
#[allow(clippy::too_many_arguments)]
fn options_window(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    mut open: ResMut<OptionsOpen>,
    mut opts: ResMut<GameOptions>,
    mut ua: crate::l2ui::UiAssets,
    mut clicks: ResMut<crate::l2ui::UiClicks>,
    roots: Query<Entity, With<OptionsRoot>>,
    mut shown: Local<Option<(bool, i32)>>,
) {
    if clicks.take("OptionWnd", "close") || clicks.take("OptionWnd", "ok") || (open.0 && keys.just_pressed(KeyCode::Escape)) {
        open.0 = false;
    }
    if clicks.take("OptionWnd", "fullscreen") {
        opts.fullscreen = !opts.fullscreen;
    }
    if clicks.take("OptionWnd", "smaller") {
        opts.ui_scale = (opts.ui_scale - 0.1).max(0.7);
    }
    if clicks.take("OptionWnd", "bigger") {
        opts.ui_scale = (opts.ui_scale + 0.1).min(1.6);
    }
    let state = (opts.fullscreen, (opts.ui_scale * 100.0).round() as i32);
    if !open.0 {
        for e in &roots {
            commands.entity(e).despawn();
        }
        *shown = None;
        return;
    }
    if *shown == Some(state) && !roots.is_empty() {
        return;
    }
    *shown = Some(state);
    for e in &roots {
        commands.entity(e).despawn();
    }
    let root = crate::l2ui::spawn_frame(&mut commands, &mut ua, "OptionWnd", "Настройки", Vec2::new(280.0, 170.0));
    commands.entity(root).insert(OptionsRoot);
    crate::l2ui::add_close_button(&mut commands, &mut ua.tex, &mut ua.lib, &mut ua.images, root, 280.0, "OptionWnd");
    let text = |commands: &mut Commands, s: String, y: f32| {
        commands.spawn((
            Text::new(s),
            TextFont { font_size: FontSize::Px(12.0), ..default() },
            TextColor(Color::srgb(0.85, 0.82, 0.75)),
            Node { position_type: PositionType::Absolute, left: Val::Px(16.0), top: Val::Px(y), ..default() },
            ChildOf(root),
        ));
    };
    let white = Color::WHITE;
    text(&mut commands, "Полноэкранный режим".into(), 48.0);
    let label = if opts.fullscreen { "Вкл" } else { "Выкл" };
    crate::l2ui::spawn_button(&mut commands, &mut ua, root, Vec2::new(184.0, 42.0), Vec2::new(80.0, 25.0), label, white, "OptionWnd", "fullscreen");
    text(&mut commands, format!("Масштаб интерфейса: {}%", state.1), 86.0);
    crate::l2ui::spawn_button(&mut commands, &mut ua, root, Vec2::new(184.0, 80.0), Vec2::new(38.0, 25.0), "-", white, "OptionWnd", "smaller");
    crate::l2ui::spawn_button(&mut commands, &mut ua, root, Vec2::new(226.0, 80.0), Vec2::new(38.0, 25.0), "+", white, "OptionWnd", "bigger");
    crate::l2ui::spawn_button(&mut commands, &mut ua, root, Vec2::new(184.0, 128.0), Vec2::new(80.0, 27.0), "OK", white, "OptionWnd", "ok");
}

fn apply_options(opts: Res<GameOptions>, mut windows: Query<&mut Window>, mut scale: ResMut<UiScale>) {
    if !opts.is_changed() {
        return;
    }
    if let Ok(mut w) = windows.single_mut() {
        let mode = if opts.fullscreen {
            bevy::window::WindowMode::BorderlessFullscreen(bevy::window::MonitorSelection::Current)
        } else {
            bevy::window::WindowMode::Windowed
        };
        if w.mode != mode {
            w.mode = mode;
        }
    }
    if (scale.0 - opts.ui_scale).abs() > 0.001 {
        scale.0 = opts.ui_scale;
    }
}

/// The system menu's "Помощь": the controls.
fn help_window(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    mut open: ResMut<HelpOpen>,
    mut ua: crate::l2ui::UiAssets,
    mut clicks: ResMut<crate::l2ui::UiClicks>,
    roots: Query<Entity, With<HelpRoot>>,
) {
    if clicks.take("HelpWnd", "close") || (open.0 && keys.just_pressed(KeyCode::Escape)) {
        open.0 = false;
    }
    if !open.0 {
        for e in &roots {
            commands.entity(e).despawn();
        }
        return;
    }
    if !roots.is_empty() {
        return;
    }
    const HELP: &str = "Левый клик по земле: идти\n\
Клик по монстру: цель, второй клик: атака\n\
Клик по NPC: подойти и поговорить\n\
F / X: атаковать цель, Tab: ближайшая цель\n\
1-9: умения на панели, V: сесть / встать\n\
Z: подобрать предметы\n\
I: инвентарь, K: умения, Alt+T: персонаж\n\
Alt+U: квесты, M: карта, Alt+X: меню\n\
C: смена профессии, F5: сохранить\n\
Правая кнопка мыши: поворот камеры, колесо: зум\n\
Esc: закрыть окно";
    let size = Vec2::new(340.0, 260.0);
    let root = crate::l2ui::spawn_frame(&mut commands, &mut ua, "HelpWnd", "Помощь", size);
    commands.entity(root).insert(HelpRoot);
    crate::l2ui::add_close_button(&mut commands, &mut ua.tex, &mut ua.lib, &mut ua.images, root, size.x, "HelpWnd");
    commands.spawn((
        Text::new(HELP),
        TextFont { font_size: FontSize::Px(12.0), ..default() },
        TextColor(Color::srgb(0.85, 0.82, 0.75)),
        Node { position_type: PositionType::Absolute, left: Val::Px(16.0), top: Val::Px(40.0), width: Val::Px(size.x - 32.0), ..default() },
        ChildOf(root),
    ));
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
    if clicks.take("DetailStatusWndClassic", "close") || (open.0 && keys.just_pressed(KeyCode::Escape)) {
        open.0 = false;
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
    creation: Res<crate::mmo::Creation>,
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
            "txtName1" if !creation.name.is_empty() => creation.name.clone(),
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
