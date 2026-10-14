//! NPC dialogs: datapack HTML pages in a window, with bypass links routed to
//! page links, NPC chat pages and quest / AI scripts (`quest.rs`).
//!
//! F / gamepad X on a friendly NPC (or a second click) walks up to it and
//! opens its first page. Links: mouse, or Up/Down + Enter / gamepad A;
//! Esc / gamepad B closes.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use bevy::prelude::*;

use crate::datapack::{datapack_root, Datapack};
use crate::html::{self, Block};
use crate::input::PlayerInput;
use crate::items::{Inventory, ItemData};
use crate::l2assets::UU_TO_M;
use crate::mmo::{Hero, Phase};
use crate::quest::{self, Player, QuestLog, Quests, ALREADY_COMPLETED, NO_QUEST};
use crate::rpg::{HeroStats, Log, Npc, NpcState};
use crate::services;
use crate::skinned::Animated;
use crate::streaming::{ue_to_bevy, TileStreamer};

/// Datapack pages: `html/**` by file name and script folders by name.
#[derive(Resource, Default)]
pub struct Pages {
    root: PathBuf,
    html_root: PathBuf,
    html: HashMap<String, PathBuf>,
    scripts: HashMap<String, PathBuf>,
}

impl Pages {
    fn load(root: &Path) -> Self {
        let mut p = Pages { root: root.to_path_buf(), html_root: root.join("html"), ..default() };
        fn walk(dir: &Path, f: &mut dyn FnMut(&Path, bool)) {
            let Ok(rd) = std::fs::read_dir(dir) else { return };
            for e in rd.flatten() {
                let path = e.path();
                let is_dir = path.is_dir();
                f(&path, is_dir);
                if is_dir {
                    walk(&path, f);
                }
            }
        }
        let html_root = p.html_root.clone();
        walk(&html_root, &mut |path, is_dir| {
            let rel = path.strip_prefix(&html_root).unwrap_or(path).to_string_lossy().replace('\\', "/");
            if is_dir || rel.starts_with("admin") || rel.starts_with("CommunityBoard") || rel.starts_with("mods") {
                return;
            }
            let name = path.file_name().unwrap().to_string_lossy().to_ascii_lowercase();
            // Prefer the generic folders when an NPC id appears twice.
            let prefer = rel.starts_with("default/");
            if prefer || !p.html.contains_key(&name) {
                p.html.insert(name, path.to_path_buf());
            }
        });
        walk(&root.join("scripts"), &mut |path, is_dir| {
            if is_dir {
                let name = path.file_name().unwrap().to_string_lossy().to_ascii_lowercase();
                p.scripts.entry(name).or_insert_with(|| path.to_path_buf());
            }
        });
        info!("dialogs: {} html pages, {} script folders", p.html.len(), p.scripts.len());
        p
    }

    fn read(path: &Path) -> Option<String> {
        let bytes = std::fs::read(path).ok()?;
        Some(String::from_utf8_lossy(&bytes).into_owned())
    }

    /// A page under `html/` by relative path (`default/SupportMagic.htm`).
    fn link(&self, rel: &str) -> Option<String> {
        let rel = rel.trim_start_matches("data/html/").trim_start_matches('/');
        Self::read(&self.html_root.join(rel))
            .or_else(|| self.html.get(&rel.rsplit('/').next()?.to_ascii_lowercase()).and_then(|p| Self::read(p)))
    }

    /// The NPC's own page `<id>.htm` or chat page `<id>-<val>.htm`.
    fn npc(&self, id: u32, val: u32) -> Option<String> {
        let name = if val == 0 { format!("{id}.htm") } else { format!("{id}-{val}.htm") };
        self.html.get(&name).or_else(|| self.html.get(&format!("{name}l"))).and_then(|p| Self::read(p))
    }

    /// What a script returned: literal HTML, a marker, or a file in its folder.
    fn script(&self, dir: &str, result: &str) -> Option<String> {
        if result.trim_start().starts_with('<') && result != NO_QUEST && result != ALREADY_COMPLETED {
            return Some(result.to_string());
        }
        match result {
            NO_QUEST => return self.link("noquest.htm"),
            ALREADY_COMPLETED => return self.link("alreadyCompleted.html"),
            _ => {}
        }
        self.scripts
            .get(&dir.to_ascii_lowercase())
            .and_then(|d| Self::read(&d.join(result)))
            .or_else(|| self.html.get(&result.to_ascii_lowercase()).and_then(|p| Self::read(p)))
    }
}

/// Set by targeting: walk to this NPC and talk.
#[derive(Resource, Default)]
pub struct TalkRequest(pub Option<Entity>);

#[derive(Resource, Default)]
struct Dialog {
    npc: Option<Entity>,
    npc_id: u32,
    npc_name: String,
    blocks: Vec<Block>,
    selected: usize,
    dirty: bool,
    pending: Option<String>,
}

#[derive(Component)]
struct DialogRoot;

#[derive(Component)]
struct DialogLink(usize);

/// Big centered message (quest steps).
#[derive(Resource, Default)]
struct ScreenMsg {
    text: String,
    timer: f32,
}

#[derive(Component)]
struct ScreenMsgText;

#[derive(Component)]
struct QuestTracker;

pub struct DialogPlugin;

impl Plugin for DialogPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TalkRequest>()
            .init_resource::<Dialog>()
            .init_resource::<ScreenMsg>()
            .init_resource::<QuestLog>()
            .insert_resource(Quests(crate::quests::all()))
            .add_systems(PreStartup, |mut commands: Commands| commands.insert_resource(Pages::load(&datapack_root())))
            .add_systems(OnEnter(Phase::InWorld), spawn_overlays)
            .add_systems(
                Update,
                (debug_talk, approach_npc, dialog_input, run_bypass, quest_kills, render_dialog, screen_message, quest_tracker)
                    .chain()
                    .run_if(in_state(Phase::InWorld)),
            );
    }
}

fn spawn_overlays(mut commands: Commands) {
    commands.spawn((
        ScreenMsgText,
        Text::new(""),
        TextFont { font_size: FontSize::Px(26.0), ..default() },
        TextColor(Color::srgb(1.0, 0.9, 0.5)),
        TextLayout { justify: Justify::Center, ..default() },
        Node { position_type: PositionType::Absolute, top: Val::Percent(22.0), width: Val::Percent(100.0), justify_content: JustifyContent::Center, ..default() },
    ));
    commands.spawn((
        QuestTracker,
        Text::new(""),
        TextFont { font_size: FontSize::Px(14.0), ..default() },
        TextColor(Color::srgb(0.95, 0.9, 0.7)),
        Node { position_type: PositionType::Absolute, top: Val::Px(140.0), left: Val::Px(10.0), max_width: Val::Px(360.0), ..default() },
    ));
}

/// Tests: `L2_TALK=<npc id>` walks to that NPC and talks; `L2_BYPASS` then
/// runs `|`-separated bypasses one per second (e.g. accepting a quest).
fn debug_talk(
    time: Res<Time>,
    mut req: ResMut<TalkRequest>,
    mut dialog: ResMut<Dialog>,
    hero: Single<&Transform, With<Hero>>,
    npcs: Query<(Entity, &Npc, &Transform), Without<Hero>>,
    mut state: Local<(bool, usize, f32)>,
) {
    let Some(id) = std::env::var("L2_TALK").ok().and_then(|v| v.parse::<u32>().ok()) else { return };
    if !state.0 {
        let best = npcs
            .iter()
            .filter(|(_, n, _)| n.template.id == id)
            .min_by(|a, b| a.2.translation.distance(hero.translation).total_cmp(&b.2.translation.distance(hero.translation)));
        if let Some((e, _, _)) = best {
            req.0 = Some(e);
            state.0 = true;
        }
        return;
    }
    if dialog.npc.is_none() {
        return;
    }
    state.2 += time.delta_secs();
    let cmds: Vec<String> = std::env::var("L2_BYPASS").unwrap_or_default().split('|').filter(|c| !c.is_empty()).map(String::from).collect();
    if state.2 > 1.5 && state.1 < cmds.len() {
        state.2 = 0.0;
        dialog.pending = Some(cmds[state.1].clone());
        state.1 += 1;
    }
}

/// Walks the hero to the requested NPC, then opens its first page.
#[allow(clippy::too_many_arguments)]
fn approach_npc(
    time: Res<Time>,
    input: Res<PlayerInput>,
    mut req: ResMut<TalkRequest>,
    mut dialog: ResMut<Dialog>,
    hero: Single<(&Hero, &mut Transform, &HeroStats)>,
    npcs: Query<(&Npc, &Transform), Without<Hero>>,
    mut anims: Query<&mut Animated>,
) {
    let Some(e) = req.0 else { return };
    let (hero, mut tr, stats) = hero.into_inner();
    let Ok((npc, ntr)) = npcs.get(e) else {
        req.0 = None;
        return;
    };
    if input.move_dir.length() > 0.1 || npc.state == NpcState::Dead || stats.hp <= 0.0 {
        req.0 = None;
        return;
    }
    let to = (ntr.translation - tr.translation).with_y(0.0);
    if to.length() > 0.01 {
        tr.rotation = Quat::from_rotation_y((-to.z).atan2(to.x));
    }
    let reach = 1.5 + npc.template.radius * UU_TO_M;
    if to.length() > reach {
        tr.translation += to.normalize() * (stats.run_speed * UU_TO_M * time.delta_secs()).min(to.length());
        if let Ok(mut a) = anims.get_mut(hero.actor) {
            a.play(&hero.clips("run", "run").each_ref().map(String::as_str), true);
        }
        return;
    }
    req.0 = None;
    if let Ok(mut a) = anims.get_mut(hero.actor) {
        a.play(&hero.clips("wait", "Wait").each_ref().map(String::as_str), true);
    }
    dialog.npc = Some(e);
    dialog.npc_id = npc.template.id;
    dialog.npc_name = npc.template.name.clone();
    dialog.pending = Some("<first>".into());
}

#[allow(clippy::too_many_arguments)]
fn dialog_input(
    keys: Res<ButtonInput<KeyCode>>,
    gamepads: Query<&Gamepad>,
    input: Res<PlayerInput>,
    mut dialog: ResMut<Dialog>,
    hero: Single<&Transform, With<Hero>>,
    npcs: Query<&Transform, (With<Npc>, Without<Hero>)>,
) {
    let Some(npc) = dialog.npc else { return };
    let far = npcs.get(npc).map_or(true, |t| t.translation.distance(hero.translation) > 8.0);
    if far || keys.just_pressed(KeyCode::Escape) || gamepads.iter().any(|g| g.just_pressed(GamepadButton::East)) {
        dialog.npc = None;
        dialog.dirty = true;
        return;
    }
    let link_ids: Vec<usize> =
        dialog.blocks.iter().enumerate().filter(|(_, b)| matches!(b, Block::Link { .. })).map(|(i, _)| i).collect();
    if link_ids.is_empty() {
        return;
    }
    if input.nav != 0 {
        dialog.selected = (dialog.selected as i32 + input.nav).rem_euclid(link_ids.len() as i32) as usize;
        dialog.dirty = true;
    }
    let pick = keys.just_pressed(KeyCode::Enter) || gamepads.iter().any(|g| g.just_pressed(GamepadButton::South));
    if pick {
        if let Some(Block::Link { bypass, .. }) = link_ids.get(dialog.selected).and_then(|&i| dialog.blocks.get(i)) {
            dialog.pending = Some(bypass.clone());
        }
    }
}

/// Executes the pending bypass (or first talk) and shows the resulting page.
#[allow(clippy::too_many_arguments)]
fn run_bypass(
    mut dialog: ResMut<Dialog>,
    pages: Res<Pages>,
    quests: Res<Quests>,
    mut qlog: ResMut<QuestLog>,
    items: Res<ItemData>,
    dp: Res<Datapack>,
    mut log: ResMut<Log>,
    mut screen: ResMut<ScreenMsg>,
    mut streamer: ResMut<TileStreamer>,
    mut class_ui: ResMut<crate::classchange::ClassChangeUi>,
    mut spawn_req: ResMut<crate::rpg::SpawnRequests>,
    mut commands: Commands,
    hero: Single<(&Hero, &mut Transform, &mut HeroStats, &mut Inventory)>,
) {
    let Some(cmd) = dialog.pending.take() else { return };
    let Some(_) = dialog.npc else { return };
    let (hero, mut tr, mut stats, mut inv) = hero.into_inner();
    let npc = dialog.npc_id;
    let mut p = Player {
        stats: &mut stats,
        inv: &mut inv,
        class: hero.class,
        items: &items,
        dp: &dp,
        log: &mut log,
        teleport: None,
        screen: None,
        class_change: false,
        spawns: Vec::new(),
        despawn_npc: false,
        kill_pos: [0.0; 3],
    };
    let page = execute(&cmd, npc, &pages, &quests, &mut qlog, &mut p);
    let (teleport, msg, class_change) = (p.teleport, p.screen.take(), p.class_change);
    spawn_req.0.append(&mut p.spawns);
    if p.despawn_npc {
        if let Some(e) = dialog.npc.take() {
            commands.entity(e).despawn();
        }
        dialog.dirty = true;
    }
    if class_change {
        class_ui.open = true;
    }
    if let Some(m) = msg {
        screen.text = m;
        screen.timer = 6.0;
    }
    if let Some(at) = teleport {
        let pos = ue_to_bevy(at);
        tr.translation = pos;
        streamer.request_around(pos);
        dialog.npc = None;
        dialog.dirty = true;
        return;
    }
    match page {
        Some(src) => {
            let src = src
                .replace("%objectId%", "0")
                .replace("%npcname%", &dialog.npc_name)
                .replace("%playername%", "Adventurer")
                .replace("%npcId%", &npc.to_string());
            dialog.blocks = html::parse(&src);
            dialog.selected = 0;
        }
        None if cmd == "<first>" => {
            dialog.blocks = html::parse(&format!("<html><body>{}:<br>...</body></html>", dialog.npc_name));
        }
        None => {} // the script did its work without a page
    }
    dialog.dirty = true;
}

/// Routes one bypass command; returns the page to show.
fn execute(cmd: &str, npc: u32, pages: &Pages, quests: &Quests, qlog: &mut QuestLog, p: &mut Player) -> Option<String> {
    if cmd == "<first>" {
        // A script with a first-talk handler overrides the NPC page.
        for q in &quests.0 {
            if q.first_talk_npcs().contains(&npc) {
                let qs = qlog.states.entry(q.id()).or_default();
                if let Some(r) = q.on_first_talk(&mut quest::Ctx { p, qs, npc }) {
                    return pages.script(q.dir(), &r);
                }
            }
        }
        return pages.npc(npc, 0).or_else(|| pages.link("npcdefault.htm"));
    }
    let (head, args) = cmd.split_once(' ').map_or((cmd, ""), |(h, a)| (h, a.trim()));
    // npc_<objectId>_<Command> <args>
    if let Some(rest) = head.strip_prefix("npc_") {
        let sub = rest.split_once('_').map_or("", |(_, c)| c);
        let num = |i: usize| args.split_whitespace().nth(i).and_then(|v| v.parse::<u32>().ok()).unwrap_or(0);
        let root = &pages.root;
        return match sub {
            "Chat" => pages.npc(npc, args.parse().unwrap_or(0)),
            "Quest" => quest_command(args, npc, pages, quests, qlog, p),
            "Link" => pages.link(args),
            "Buy" => Some(services::buy_page(root, p, num(0), 0, "")),
            "Sell" => Some(services::sell_page(p, 0, 0, "")),
            "multisell" | "Multisell" | "exc_multisell" => Some(services::multisell_page(root, p, num(0), 0, "")),
            "showTeleportList" | "showTeleports" => Some(services::teleport_page(root, p, 0, "")),
            "teleport" => {
                let mut a = args.split_whitespace();
                let kind = a.next().unwrap_or("OTHER");
                let index = a.next().and_then(|v| v.parse().ok()).unwrap_or(0);
                if !services::npc_teleport(root, p, npc, kind, index) {
                    p.log.push("This teleport is not available yet.");
                }
                None
            }
            other => {
                p.log.push(format!("Not available yet: {other}"));
                None
            }
        };
    }
    let num = |i: usize| args.split_whitespace().nth(i).and_then(|v| v.parse::<u32>().ok()).unwrap_or(0);
    let root = &pages.root;
    match head {
        "shop_list" => return Some(services::buy_page(root, p, num(0), num(1) as usize, "")),
        "shop_buy" => return Some(services::buy(root, p, num(0), num(1), num(2).max(1) as u64, num(3) as usize)),
        "shop_sell" => return Some(services::sell_page(p, num(0), num(1) as usize, "")),
        "shop_sellitem" => return Some(services::sell(p, num(0), num(1), num(2) as usize)),
        "ms_list" => return Some(services::multisell_page(root, p, num(0), num(1) as usize, "")),
        "ms_exchange" => return Some(services::exchange(root, p, num(0), num(1) as usize, num(2) as usize)),
        "tele_list" => return Some(services::teleport_page(root, p, num(0) as usize, "")),
        "tele_go" => return services::teleport(root, p, num(0), num(1) as usize),
        _ => {}
    }
    match head {
        "Link" => pages.link(args),
        "Quest" | "Script" => {
            let (name, ev) = args.split_once(' ').map_or((args, ""), |(n, e)| (n, e.trim()));
            if name.is_empty() {
                return quest_command("", npc, pages, quests, qlog, p);
            }
            match quests.by_dir(name) {
                Some(q) if ev.is_empty() => {
                    let r = quest::talk(q, qlog, p, npc);
                    pages.script(q.dir(), &r)
                }
                Some(q) => quest::event(q, qlog, p, npc, ev).and_then(|r| pages.script(q.dir(), &r)),
                // Unported script: its static pages still work.
                None if ev.ends_with(".htm") || ev.ends_with(".html") => pages.script(name, ev),
                None => {
                    p.log.push(format!("Script {name} is not available yet"));
                    None
                }
            }
        }
        _ => {
            p.log.push(format!("Not available yet: {head}"));
            None
        }
    }
}

/// "Quest" link: the quest by name, the only quest of this NPC, or a list.
fn quest_command(name: &str, npc: u32, pages: &Pages, quests: &Quests, qlog: &mut QuestLog, p: &mut Player) -> Option<String> {
    if !name.is_empty() {
        let q = quests.by_dir(name)?;
        let r = quest::talk(q, qlog, p, npc);
        return pages.script(q.dir(), &r);
    }
    let list = quest::npc_quests(quests, qlog, npc);
    match list.len() {
        0 => pages.link("noquest.htm"),
        1 => {
            let r = quest::talk(list[0], qlog, p, npc);
            pages.script(list[0].dir(), &r)
        }
        _ => {
            let mut s = String::from("<html><body>Quests:<br>");
            for q in list {
                let st = qlog.states.get(&q.id()).map(|s| s.state).unwrap_or_default();
                let mark = if st == quest::QState::Started { " (In progress)" } else { "" };
                s.push_str(&format!("<Button action=\"bypass -h Quest {}\">{}{mark}</Button>", q.dir(), q.title()));
            }
            s.push_str("</body></html>");
            Some(s)
        }
    }
}

fn quest_kills(
    quests: Res<Quests>,
    mut qlog: ResMut<QuestLog>,
    items: Res<ItemData>,
    dp: Res<Datapack>,
    mut log: ResMut<Log>,
    mut screen: ResMut<ScreenMsg>,
    mut spawn_req: ResMut<crate::rpg::SpawnRequests>,
    hero: Single<(&Hero, &mut HeroStats, &mut Inventory)>,
    mut npcs: Query<(&mut Npc, &Transform)>,
) {
    let (hero, mut stats, mut inv) = hero.into_inner();
    let mut p = Player {
        stats: &mut stats,
        inv: &mut inv,
        class: hero.class,
        items: &items,
        dp: &dp,
        log: &mut log,
        teleport: None,
        screen: None,
        class_change: false,
        spawns: Vec::new(),
        despawn_npc: false,
        kill_pos: [0.0; 3],
    };
    for (mut npc, tr) in &mut npcs {
        if npc.state != NpcState::Dead || npc.quest_seen {
            continue;
        }
        npc.quest_seen = true;
        p.kill_pos = crate::streaming::bevy_to_ue(tr.translation);
        quest::kill(&quests, &mut qlog, &mut p, npc.template.id);
    }
    spawn_req.0.append(&mut p.spawns);
    if let Some(m) = p.screen.take() {
        screen.text = m;
        screen.timer = 6.0;
    }
}

fn render_dialog(
    mut commands: Commands,
    mut dialog: ResMut<Dialog>,
    roots: Query<Entity, With<DialogRoot>>,
) {
    if !dialog.dirty {
        return;
    }
    dialog.dirty = false;
    for e in &roots {
        commands.entity(e).despawn();
    }
    if dialog.npc.is_none() {
        return;
    }
    let selected = dialog.selected;
    commands
        .spawn((
            DialogRoot,
            Node {
                position_type: PositionType::Absolute,
                right: Val::Px(16.0),
                top: Val::Px(80.0),
                width: Val::Px(420.0),
                max_height: Val::Percent(80.0),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(Val::Px(10.0)),
                row_gap: Val::Px(6.0),
                overflow: Overflow::clip(),
                ..default()
            },
            BackgroundColor(Color::srgba(0.04, 0.04, 0.07, 0.9)),
        ))
        .with_children(|p| {
            let mut link_no = 0;
            for (i, b) in dialog.blocks.iter().enumerate() {
                match b {
                    Block::Text(spans) => {
                        p.spawn((Text::default(), TextFont { font_size: FontSize::Px(15.0), ..default() })).with_children(|t| {
                            for s in spans {
                                t.spawn((
                                    TextSpan::new(s.text.clone()),
                                    TextFont { font_size: FontSize::Px(15.0), ..default() },
                                    TextColor(span_color(s.color.as_deref())),
                                ));
                            }
                        });
                    }
                    Block::Link { label, .. } => {
                        let on = link_no == selected;
                        link_no += 1;
                        p.spawn((
                            DialogLink(i),
                            Node { padding: UiRect::axes(Val::Px(8.0), Val::Px(4.0)), ..default() },
                            BackgroundColor(if on { Color::srgba(0.35, 0.28, 0.1, 0.9) } else { Color::srgba(0.15, 0.15, 0.2, 0.9) }),
                        ))
                        .observe(move |_: On<PointerClick>, mut d: ResMut<Dialog>| {
                            if let Some(Block::Link { bypass, .. }) = d.blocks.get(i) {
                                d.pending = Some(bypass.clone());
                            }
                        })
                        .with_children(|b| {
                            b.spawn((
                                Text::new(label.clone()),
                                TextFont { font_size: FontSize::Px(15.0), ..default() },
                                TextColor(if on { Color::srgb(1.0, 0.85, 0.3) } else { Color::srgb(0.75, 0.85, 1.0) }),
                            ));
                        });
                    }
                }
            }
        });
}

fn span_color(c: Option<&str>) -> Color {
    match c {
        None => Color::WHITE,
        Some(c) if c.eq_ignore_ascii_case("LEVEL") => Color::srgb(1.0, 0.85, 0.3),
        Some(c) => {
            let h = c.trim_start_matches('#');
            match u32::from_str_radix(h, 16) {
                Ok(v) if h.len() == 6 => {
                    Color::srgb_u8((v >> 16) as u8, (v >> 8) as u8, v as u8)
                }
                _ => Color::WHITE,
            }
        }
    }
}

fn screen_message(time: Res<Time>, mut msg: ResMut<ScreenMsg>, mut text: Single<&mut Text, With<ScreenMsgText>>) {
    if msg.timer > 0.0 {
        msg.timer -= time.delta_secs();
        if text.0 != msg.text {
            text.0 = msg.text.clone();
        }
    } else if !text.0.is_empty() {
        text.0.clear();
    }
}

fn quest_tracker(quests: Res<Quests>, qlog: Res<QuestLog>, mut text: Single<&mut Text, With<QuestTracker>>) {
    if !qlog.is_changed() {
        return;
    }
    let mut s = String::new();
    let mut started: Vec<_> = qlog.states.iter().filter(|(_, st)| st.is_started()).collect();
    started.sort_by_key(|(id, _)| **id);
    for (id, st) in started {
        let Some(q) = quests.by_id(*id) else { continue };
        s.push_str(q.title());
        if let Some(line) = q.progress(st) {
            s.push_str(&format!("\n  {line}"));
        }
        s.push('\n');
    }
    text.0 = s;
}
