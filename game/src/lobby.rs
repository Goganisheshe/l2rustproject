//! The screens before the world, as in the client: login, character select
//! and character creation. The 3D backdrop is the client's own lobby map
//! (`maps/Lobby01_Classic.unr`) seen from its camera points; the logo and
//! copyright come from `l2font-ru.utx`, frames and buttons from L2UI. The
//! client draws these screens with Flash movies (`ui4/6/7.ugx`), so their
//! layout is rebuilt here. Offline there is no account: "Войти" goes straight
//! to the characters, and there is no server list.

use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::mesh::skinning::SkinnedMeshInverseBindposes;
use bevy::prelude::*;

use crate::character::CLASSES;
use crate::datapack::Datapack;
use crate::l2ui::{spawn_button, spawn_frame_at, UiAssets, UiClicks};
use crate::mmo::{load_class_model, Creation, Phase};
use crate::save::{self, PendingLoad, SaveGame};
use crate::skinned::{spawn_model, ModelCache};
use crate::streaming::{ue_rotator, ue_to_bevy, TileEntity, TileStreamer};

/// The lobby map package (`Lobby01_Classic` is preferred by the loader).
const LOBBY_MAP: &str = "Lobby01";
/// Login camera: in front of the painted backdrop of the lobby map (near its
/// InterpolationPoint3, moved up so the painting fills the screen).
const LOGIN_CAM: ([f32; 3], [i32; 3]) = ([147738.5, -246214.7, 4120.5], [376, 16384, 0]);
/// Where characters stand on the select / creation screens (LookTarget).
const STAGE: [f32; 3] = [150887.8, -238978.7, -12590.5];
/// The select camera's point (InterpolationPoint25), looking at the stage.
const SELECT_CAM: [f32; 3] = [150886.4, -239539.6, -12639.7];

pub struct LobbyPlugin;

impl Plugin for LobbyPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LobbyState>()
            .add_systems(Startup, request_lobby)
            .add_systems(OnEnter(Phase::Login), enter_login)
            .add_systems(Update, login.run_if(in_state(Phase::Login)))
            .add_systems(OnEnter(Phase::Lobby), enter_select)
            .add_systems(Update, (select, show_stage).chain().run_if(in_state(Phase::Lobby)))
            .add_systems(OnEnter(Phase::Create), enter_create)
            .add_systems(Update, (create, name_input, show_stage).chain().run_if(in_state(Phase::Create)))
            .add_systems(OnEnter(Phase::Loading), leave_lobby)
            .add_systems(Update, login_camera.run_if(in_state(Phase::Login)));
    }
}

#[derive(Resource, Default)]
struct LobbyState {
    slots: Vec<Option<SaveGame>>,
    selected: usize,
    /// Asking to confirm the deletion of the selected character.
    confirm_delete: bool,
    /// Class index shown on the stage, and whether the screen needs a rebuild.
    staged: Option<usize>,
    dirty: bool,
    /// Creation: race row, mystic, female.
    race: usize,
    mystic: bool,
    female: bool,
    name: String,
    error: String,
}

/// Races on the creation screen: label, fighter class name, mystic class name.
const RACES: [(&str, &str, Option<&str>); 6] = [
    ("Человек", "Human Fighter", Some("Human Mystic")),
    ("Эльф", "Elf", Some("Elf Mystic")),
    ("Тёмный эльф", "Dark Elf", Some("Dark Elf Mystic")),
    ("Орк", "Orc Fighter", Some("Orc Mystic")),
    ("Гном", "Dwarf", None),
    ("Камаэль", "Kamael", None),
];

fn creation_class(race: usize, mystic: bool, female: bool) -> usize {
    let (_, fighter, magic) = RACES[race];
    let base = if mystic { magic.unwrap_or(fighter) } else { fighter };
    let name = format!("{base} ({})", if female { "F" } else { "M" });
    CLASSES.iter().position(|c| c.name == name).unwrap_or(0)
}

#[derive(Component)]
struct LobbyUi;

#[derive(Component)]
struct StageActor;

#[derive(Component)]
struct NameText;

fn request_lobby(mut streamer: ResMut<TileStreamer>) {
    streamer.request_named(LOBBY_MAP);
}

fn set_camera(cam: &mut Transform, at: Vec3, look: Vec3) {
    *cam = Transform::from_translation(at).looking_at(look, Vec3::Y);
}

/// Logo and copyright line of the client.
fn spawn_branding(commands: &mut Commands, ua: &mut UiAssets, phase: Phase, big: bool) {
    let screen = ua.screen();
    if let Some(t) = ua.tex.get("l2font-ru.Skins.L2SubLogo_Classic-ru", &mut ua.lib, &mut ua.images) {
        commands.spawn((
            LobbyUi,
            DespawnOnExit(phase),
            ImageNode::new(t.image),
            if big {
                Node { position_type: PositionType::Absolute, left: Val::Px(((screen.x - 512.0) / 2.0).round()), top: Val::Px(40.0), width: Val::Px(512.0), height: Val::Px(256.0), ..default() }
            } else {
                Node { position_type: PositionType::Absolute, left: Val::Px(0.0), top: Val::Px(-10.0), width: Val::Px(256.0), height: Val::Px(128.0), ..default() }
            },
        ));
    }
    if let Some(t) = ua.tex.get("l2font-ru.Skins.LobbyCopyright-ru", &mut ua.lib, &mut ua.images) {
        commands.spawn((
            LobbyUi,
            DespawnOnExit(phase),
            ImageNode::new(t.image),
            Node { position_type: PositionType::Absolute, left: Val::Px(((screen.x - 1024.0) / 2.0).round()), bottom: Val::Px(0.0), width: Val::Px(1024.0), height: Val::Px(64.0), ..default() },
        ));
    }
}

fn label(commands: &mut Commands, parent: Entity, text: impl Into<String>, pos: Vec2, size: f32, color: Color) -> Entity {
    commands
        .spawn((
            Text::new(text),
            TextFont { font_size: FontSize::Px(size), ..default() },
            TextColor(color),
            Node { position_type: PositionType::Absolute, left: Val::Px(pos.x), top: Val::Px(pos.y), ..default() },
            ChildOf(parent),
        ))
        .id()
}

const GOLD: Color = Color::srgb(0.93, 0.8, 0.45);
const PALE: Color = Color::srgb(0.85, 0.82, 0.75);

// ---------------------------------------------------------------- login

fn enter_login(
    mut commands: Commands,
    mut ua: UiAssets,
    mut next: ResMut<NextState<Phase>>,
    mut c: ResMut<Creation>,
    mut pending: ResMut<PendingLoad>,
) {
    // Tests skip the screens: L2_CLASS starts a new character, L2_CONTINUE
    // plays the first saved one.
    if let Some(i) = std::env::var("L2_CLASS").ok().and_then(|k| crate::character::find_class(&k)) {
        c.index = i;
        c.name = "Tester".into();
        let free = save::list().iter().position(|s| s.is_none()).unwrap_or(save::SLOTS - 1);
        *save::SLOT.lock().unwrap() = free;
        next.set(Phase::Loading);
        return;
    }
    if std::env::var_os("L2_CONTINUE").is_some() {
        if let Some((slot, s)) = save::list().into_iter().enumerate().find_map(|(i, s)| s.map(|s| (i, s))) {
            save::load_slot(slot, s, &mut c, &mut pending);
            next.set(Phase::Loading);
            return;
        }
    }
    spawn_branding(&mut commands, &mut ua, Phase::Login, true);
    let screen = ua.screen();
    let root = commands
        .spawn((
            LobbyUi,
            DespawnOnExit(Phase::Login),
            Node { position_type: PositionType::Absolute, left: Val::Px(((screen.x - 160.0) / 2.0).round()), top: Val::Px((screen.y * 0.68).round()), width: Val::Px(160.0), height: Val::Px(80.0), ..default() },
        ))
        .id();
    spawn_button(&mut commands, &mut ua, root, Vec2::ZERO, Vec2::new(160.0, 32.0), "Войти", Color::WHITE, "Login", "enter");
    spawn_button(&mut commands, &mut ua, root, Vec2::new(0.0, 42.0), Vec2::new(160.0, 32.0), "Выход", Color::WHITE, "Login", "quit");
}

/// The login view; `L2_LOBBY_CAM=x,y,z,pitch,yaw` (Unreal units / rotator
/// units) tries other spots.
fn login_camera(mut cam: Single<&mut Transform, With<Camera3d>>) {
    let (mut at, mut rot) = LOGIN_CAM;
    if let Some(v) = std::env::var("L2_LOBBY_CAM").ok().map(|s| s.split(',').filter_map(|x| x.trim().parse::<f32>().ok()).collect::<Vec<_>>()) {
        if v.len() == 5 {
            at = [v[0], v[1], v[2]];
            rot = [v[3] as i32, v[4] as i32, 0];
        }
    }
    let from = ue_to_bevy(at);
    let fwd = ue_rotator(rot) * Vec3::X;
    set_camera(&mut cam, from, from + fwd);
}

fn login(
    keys: Res<ButtonInput<KeyCode>>,
    gamepads: Query<&Gamepad>,
    mut clicks: ResMut<UiClicks>,
    mut next: ResMut<NextState<Phase>>,
    mut exit: MessageWriter<AppExit>,
) {
    let pad = |b: GamepadButton| gamepads.iter().any(|g| g.just_pressed(b));
    // Tests: L2_LOBBY=1 opens the character list, L2_LOBBY=create the creation screen.
    let env = std::env::var("L2_LOBBY").ok();
    if env.as_deref() == Some("create") {
        next.set(Phase::Create);
    } else if clicks.take("Login", "enter") || keys.just_pressed(KeyCode::Enter) || pad(GamepadButton::South) || env.is_some() {
        next.set(Phase::Lobby);
    }
    if clicks.take("Login", "quit") || keys.just_pressed(KeyCode::Escape) {
        exit.write(AppExit::Success);
    }
}

// ---------------------------------------------------------------- select

fn enter_select(mut st: ResMut<LobbyState>, mut cam: Single<&mut Transform, With<Camera3d>>) {
    st.slots = save::list();
    st.selected = st.slots.iter().position(|s| s.is_some()).unwrap_or(0);
    st.confirm_delete = false;
    st.staged = None;
    st.dirty = true;
    let stage = ue_to_bevy(STAGE);
    let from = stage + (ue_to_bevy(SELECT_CAM) - stage).with_y(0.0).normalize() * 4.2 + Vec3::Y * 1.3;
    set_camera(&mut cam, from, stage + Vec3::Y * 0.95);
}

#[allow(clippy::too_many_arguments)]
fn select(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    gamepads: Query<&Gamepad>,
    input: Res<crate::input::PlayerInput>,
    dp: Res<Datapack>,
    mut st: ResMut<LobbyState>,
    mut ua: UiAssets,
    mut clicks: ResMut<UiClicks>,
    roots: Query<Entity, With<LobbyUi>>,
    mut next: ResMut<NextState<Phase>>,
    mut c: ResMut<Creation>,
    mut pending: ResMut<PendingLoad>,
) {
    let pad = |b: GamepadButton| gamepads.iter().any(|g| g.just_pressed(b));
    let chars: Vec<usize> = (0..st.slots.len()).filter(|i| st.slots[*i].is_some()).collect();
    for i in 0..save::SLOTS {
        if clicks.take("Lobby", &format!("slot{i}")) && st.slots.get(i).is_some_and(|s| s.is_some()) {
            st.selected = i;
            st.dirty = true;
        }
    }
    if input.nav != 0 && !chars.is_empty() && !st.confirm_delete {
        let at = chars.iter().position(|&i| i == st.selected).unwrap_or(0) as i32;
        st.selected = chars[(at + input.nav).rem_euclid(chars.len() as i32) as usize];
        st.dirty = true;
    }
    let has = st.slots.get(st.selected).is_some_and(|s| s.is_some());
    if st.confirm_delete {
        if clicks.take("Lobby", "yes") {
            save::delete(st.selected);
            st.slots = save::list();
            st.selected = st.slots.iter().position(|s| s.is_some()).unwrap_or(0);
            st.confirm_delete = false;
            st.dirty = true;
        } else if clicks.take("Lobby", "no") || keys.just_pressed(KeyCode::Escape) {
            st.confirm_delete = false;
            st.dirty = true;
        }
    } else if (clicks.take("Lobby", "start") || keys.just_pressed(KeyCode::Enter) || pad(GamepadButton::South)) && has {
        let s = st.slots[st.selected].clone().unwrap();
        save::load_slot(st.selected, s, &mut c, &mut pending);
        next.set(Phase::Loading);
        return;
    } else if clicks.take("Lobby", "create") {
        if st.slots.iter().all(|s| s.is_some()) {
            st.error = "Все слоты заняты.".into();
            st.dirty = true;
        } else {
            next.set(Phase::Create);
            return;
        }
    } else if clicks.take("Lobby", "delete") && has {
        st.confirm_delete = true;
        st.dirty = true;
    } else if clicks.take("Lobby", "back") || keys.just_pressed(KeyCode::Escape) {
        next.set(Phase::Login);
        return;
    }
    // Stage: the selected character.
    st.staged = st.slots.get(st.selected).cloned().flatten().map(|s| s.class_index.min(CLASSES.len() - 1));
    if !st.dirty {
        return;
    }
    st.dirty = false;
    for e in &roots {
        commands.entity(e).despawn();
    }
    spawn_branding(&mut commands, &mut ua, Phase::Lobby, false);
    let screen = ua.screen();
    // Character list on the right.
    let size = Vec2::new(250.0, 56.0 + save::SLOTS as f32 * 46.0);
    let list = spawn_frame_at(&mut commands, &mut ua, "LobbyCharacters", "Выбор персонажа", Vec2::new(screen.x - size.x - 24.0, 110.0), size);
    commands.entity(list).insert((LobbyUi, DespawnOnExit(Phase::Lobby)));
    for i in 0..save::SLOTS {
        let y = 40.0 + i as f32 * 46.0;
        match &st.slots[i] {
            Some(s) => {
                let color = if i == st.selected { GOLD } else { Color::WHITE };
                spawn_button(&mut commands, &mut ua, list, Vec2::new(14.0, y), Vec2::new(222.0, 40.0), &s.name, color, "Lobby", &format!("slot{i}"));
                label(&mut commands, list, format!("{}  ур. {}", dp.class_name(s.class_id), s.level), Vec2::new(22.0, y + 27.0), 10.0, PALE);
            }
            None => {
                label(&mut commands, list, "Пусто", Vec2::new(100.0, y + 12.0), 12.0, Color::srgb(0.5, 0.5, 0.5));
            }
        }
    }
    // Buttons along the bottom.
    let bar = commands
        .spawn((
            LobbyUi,
            DespawnOnExit(Phase::Lobby),
            Node { position_type: PositionType::Absolute, left: Val::Px(((screen.x - 560.0) / 2.0).round()), bottom: Val::Px(80.0), width: Val::Px(560.0), height: Val::Px(36.0), ..default() },
        ))
        .id();
    for (i, (text, id)) in [("Создать", "create"), ("Удалить", "delete"), ("Начать игру", "start"), ("Назад", "back")].iter().enumerate() {
        spawn_button(&mut commands, &mut ua, bar, Vec2::new(i as f32 * 140.0, 0.0), Vec2::new(130.0, 32.0), text, Color::WHITE, "Lobby", id);
    }
    if !st.error.is_empty() {
        let msg = std::mem::take(&mut st.error);
        label(&mut commands, bar, msg, Vec2::new(0.0, -26.0), 13.0, Color::srgb(1.0, 0.5, 0.4));
    }
    if st.confirm_delete {
        let name = st.slots[st.selected].as_ref().map(|s| s.name.clone()).unwrap_or_default();
        let size = Vec2::new(300.0, 120.0);
        let dlg = spawn_frame_at(&mut commands, &mut ua, "LobbyDelete", "Удаление персонажа", ((screen - size) / 2.0).round(), size);
        commands.entity(dlg).insert((LobbyUi, DespawnOnExit(Phase::Lobby)));
        label(&mut commands, dlg, format!("Удалить персонажа {name}?"), Vec2::new(18.0, 42.0), 13.0, PALE);
        spawn_button(&mut commands, &mut ua, dlg, Vec2::new(40.0, 78.0), Vec2::new(100.0, 27.0), "Да", Color::WHITE, "Lobby", "yes");
        spawn_button(&mut commands, &mut ua, dlg, Vec2::new(160.0, 78.0), Vec2::new(100.0, 27.0), "Нет", Color::WHITE, "Lobby", "no");
    }
}

/// Puts the model of `LobbyState::staged` on the stage.
#[allow(clippy::too_many_arguments)]
fn show_stage(
    mut commands: Commands,
    mut st: ResMut<LobbyState>,
    mut c: ResMut<Creation>,
    mut ua: UiAssets,
    mut cache: ResMut<ModelCache>,
    mut assets: (ResMut<Assets<Mesh>>, ResMut<Assets<StandardMaterial>>, ResMut<Assets<SkinnedMeshInverseBindposes>>),
    actors: Query<Entity, With<StageActor>>,
    mut shown: Local<Option<usize>>,
    phase: Res<State<Phase>>,
) {
    if *shown == st.staged && !actors.is_empty() == st.staged.is_some() {
        return;
    }
    *shown = st.staged;
    for e in &actors {
        commands.entity(e).despawn();
    }
    let Some(index) = st.staged else { return };
    c.index = index;
    let (meshes, materials, bindposes) = &mut assets;
    load_class_model(&mut c, &mut ua.lib, &mut cache, meshes, materials, &mut ua.images, bindposes);
    let stage = ue_to_bevy(STAGE);
    let cam = stage + (ue_to_bevy(SELECT_CAM) - stage).with_y(0.0).normalize();
    let root = commands
        .spawn((StageActor, DespawnOnExit(*phase.get()), Transform::from_translation(stage).looking_at(cam.with_y(stage.y), Vec3::Y) * Transform::from_rotation(Quat::from_rotation_y(std::f32::consts::FRAC_PI_2)), Visibility::default()))
        .id();
    if let Some(model) = c.model.clone() {
        spawn_model(&mut commands, &model, root, "wait_1HS", &c.attachments());
    }
}

// ---------------------------------------------------------------- create

fn enter_create(mut st: ResMut<LobbyState>, mut cam: Single<&mut Transform, With<Camera3d>>) {
    st.name.clear();
    st.error.clear();
    st.dirty = true;
    st.staged = Some(creation_class(st.race, st.mystic, st.female));
    let stage = ue_to_bevy(STAGE);
    let from = stage + (ue_to_bevy(SELECT_CAM) - stage).with_y(0.0).normalize() * 4.2 + Vec3::Y * 1.3;
    // A little to the side: the panel sits on the left.
    let side = (from - stage).cross(Vec3::Y).normalize() * 0.9;
    set_camera(&mut cam, from + side, stage + side + Vec3::Y * 0.95);
}

#[allow(clippy::too_many_arguments)]
fn create(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    dp: Res<Datapack>,
    mut st: ResMut<LobbyState>,
    mut ua: UiAssets,
    mut clicks: ResMut<UiClicks>,
    roots: Query<Entity, With<LobbyUi>>,
    mut next: ResMut<NextState<Phase>>,
    mut c: ResMut<Creation>,
) {
    for r in 0..RACES.len() {
        if clicks.take("Create", &format!("race{r}")) {
            st.race = r;
            st.dirty = true;
        }
    }
    if RACES[st.race].2.is_none() {
        st.mystic = false;
    }
    for (id, mystic) in [("fighter", false), ("mystic", true)] {
        if clicks.take("Create", id) && (!mystic || RACES[st.race].2.is_some()) {
            st.mystic = mystic;
            st.dirty = true;
        }
    }
    for (id, female) in [("male", false), ("female", true)] {
        if clicks.take("Create", id) {
            st.female = female;
            st.dirty = true;
        }
    }
    st.staged = Some(creation_class(st.race, st.mystic, st.female));
    if clicks.take("Create", "cancel") || keys.just_pressed(KeyCode::Escape) {
        next.set(Phase::Lobby);
        return;
    }
    if clicks.take("Create", "ok") || keys.just_pressed(KeyCode::Enter) {
        let name = st.name.trim().to_string();
        let taken = save::list().iter().flatten().any(|s| s.name.eq_ignore_ascii_case(&name));
        let free = save::list().iter().position(|s| s.is_none());
        if name.is_empty() {
            st.error = "Введите имя персонажа.".into();
        } else if taken {
            st.error = "Это имя уже занято.".into();
        } else if let Some(slot) = free {
            *save::SLOT.lock().unwrap() = slot;
            c.index = creation_class(st.race, st.mystic, st.female);
            c.name = name;
            next.set(Phase::Loading);
            return;
        } else {
            st.error = "Все слоты заняты.".into();
        }
        st.dirty = true;
    }
    if !st.dirty {
        return;
    }
    st.dirty = false;
    for e in &roots {
        commands.entity(e).despawn();
    }
    spawn_branding(&mut commands, &mut ua, Phase::Create, false);
    let size = Vec2::new(300.0, 470.0);
    let panel = spawn_frame_at(&mut commands, &mut ua, "CharacterCreate", "Создание персонажа", Vec2::new(24.0, 110.0), size);
    commands.entity(panel).insert((LobbyUi, DespawnOnExit(Phase::Create)));
    let button = |commands: &mut Commands, ua: &mut UiAssets, pos: Vec2, w: f32, text: &str, on: bool, id: &str| {
        spawn_button(commands, ua, panel, pos, Vec2::new(w, 27.0), text, if on { GOLD } else { Color::WHITE }, "Create", id);
    };
    label(&mut commands, panel, "Раса", Vec2::new(16.0, 38.0), 12.0, GOLD);
    for (r, (text, _, _)) in RACES.iter().enumerate() {
        let pos = Vec2::new(16.0 + (r % 3) as f32 * 90.0, 56.0 + (r / 3) as f32 * 30.0);
        button(&mut commands, &mut ua, pos, 86.0, text, r == st.race, &format!("race{r}"));
    }
    label(&mut commands, panel, "Тип", Vec2::new(16.0, 122.0), 12.0, GOLD);
    button(&mut commands, &mut ua, Vec2::new(16.0, 140.0), 130.0, "Воин", !st.mystic, "fighter");
    if RACES[st.race].2.is_some() {
        button(&mut commands, &mut ua, Vec2::new(152.0, 140.0), 130.0, "Маг", st.mystic, "mystic");
    }
    label(&mut commands, panel, "Пол", Vec2::new(16.0, 176.0), 12.0, GOLD);
    button(&mut commands, &mut ua, Vec2::new(16.0, 194.0), 130.0, "Мужской", !st.female, "male");
    button(&mut commands, &mut ua, Vec2::new(152.0, 194.0), 130.0, "Женский", st.female, "female");
    // Class and base stats from the datapack template.
    let class = &CLASSES[creation_class(st.race, st.mystic, st.female)];
    let key = crate::rpg::template_for(class);
    if let Some(t) = dp.classes.get(key) {
        label(&mut commands, panel, dp.class_name(t.class_id), Vec2::new(16.0, 236.0), 14.0, GOLD);
        let stats = [("STR", t.str_), ("DEX", t.dex), ("CON", t.con), ("INT", t.int), ("WIT", t.wit), ("MEN", t.men)];
        for (i, (n, v)) in stats.iter().enumerate() {
            let pos = Vec2::new(16.0 + (i % 3) as f32 * 92.0, 262.0 + (i / 3) as f32 * 20.0);
            label(&mut commands, panel, format!("{n}  {v:.0}"), pos, 12.0, PALE);
        }
    }
    label(&mut commands, panel, "Имя", Vec2::new(16.0, 316.0), 12.0, GOLD);
    let field = commands
        .spawn((
            Node { position_type: PositionType::Absolute, left: Val::Px(16.0), top: Val::Px(334.0), width: Val::Px(266.0), height: Val::Px(24.0), padding: UiRect::axes(Val::Px(6.0), Val::Px(4.0)), border: UiRect::all(Val::Px(1.0)), ..default() },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.6)),
            BorderColor::all(Color::srgb(0.45, 0.4, 0.3)),
            ChildOf(panel),
        ))
        .id();
    commands.spawn((NameText, Text::new(format!("{}_", st.name)), TextFont { font_size: FontSize::Px(13.0), ..default() }, TextColor(Color::WHITE), ChildOf(field)));
    if !st.error.is_empty() {
        label(&mut commands, panel, st.error.clone(), Vec2::new(16.0, 366.0), 12.0, Color::srgb(1.0, 0.5, 0.4));
    }
    button(&mut commands, &mut ua, Vec2::new(16.0, size.y - 46.0), 130.0, "Создать", false, "ok");
    button(&mut commands, &mut ua, Vec2::new(152.0, size.y - 46.0), 130.0, "Отмена", false, "cancel");
}

/// Typing the character's name (letters and digits, up to 16).
fn name_input(mut keys: MessageReader<KeyboardInput>, mut st: ResMut<LobbyState>, mut text: Query<&mut Text, With<NameText>>) {
    let mut changed = false;
    for k in keys.read() {
        if !k.state.is_pressed() {
            continue;
        }
        match &k.logical_key {
            Key::Backspace => {
                changed |= st.name.pop().is_some();
            }
            Key::Character(s) => {
                for ch in s.chars().filter(|c| c.is_alphanumeric()) {
                    if st.name.chars().count() < 16 {
                        st.name.push(ch);
                        changed = true;
                    }
                }
            }
            _ => {}
        }
    }
    if changed {
        for mut t in &mut text {
            t.0 = format!("{}_", st.name);
        }
    }
}

/// Leaving for the world: drop the lobby scene and load the hero's model.
#[allow(clippy::too_many_arguments)]
fn leave_lobby(
    mut commands: Commands,
    mut streamer: ResMut<TileStreamer>,
    tiles: Query<(Entity, &TileEntity)>,
    mut c: ResMut<Creation>,
    mut ua: UiAssets,
    mut cache: ResMut<ModelCache>,
    mut assets: (ResMut<Assets<Mesh>>, ResMut<Assets<StandardMaterial>>, ResMut<Assets<SkinnedMeshInverseBindposes>>),
) {
    let (meshes, materials, bindposes) = &mut assets;
    load_class_model(&mut c, &mut ua.lib, &mut cache, meshes, materials, &mut ua.images, bindposes);
    for (e, t) in &tiles {
        if t.0 == LOBBY_MAP {
            commands.entity(e).despawn();
        }
    }
    streamer.forget(LOBBY_MAP);
}
