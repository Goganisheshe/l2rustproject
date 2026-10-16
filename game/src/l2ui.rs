//! The original client interface: windows from `system/interface.xdat`
//! (positions, sizes, anchors, textures from the `L2UI_*` packages) built as
//! Bevy UI nodes, with the controls the game drives (bars, texts, item
//! slots, chat) bound by window / control name in `l2ui_bind.rs`.

use std::collections::HashMap;
use std::sync::Arc;

use bevy::prelude::*;
use bevy::ui::widget::NodeImageMode;
use ue2::xdat::{Element, Xdat};

use crate::l2assets::L2Library;

#[derive(Resource, Clone)]
pub struct Interface(pub Arc<Xdat>, pub Arc<HashMap<u32, String>>);

impl Interface {
    /// A `sysstring` entry (UI labels, window titles).
    pub fn string(&self, id: i32) -> Option<&str> {
        (id > 0).then(|| self.1.get(&(id as u32)).map(String::as_str)).flatten()
    }
}

/// Loaded UI textures by xdat name (`L2UI_CT1.Windows.Windows_DF_Small_Bg`).
#[derive(Resource, Default)]
pub struct UiTextures(HashMap<String, Option<UiTex>>, HashMap<String, Vec2>);

impl UiTextures {
    /// Where the player dragged a window to, if anywhere.
    pub fn moved(&self, window: &str) -> Option<Vec2> {
        self.1.get(window).copied()
    }
}

/// Windows move by dragging them, as in the client, and keep the spot when
/// they are rebuilt.
pub fn make_draggable(commands: &mut Commands, root: Entity, name: &str) {
    // The newest window, and the one last clicked, is drawn on top.
    static TOP: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(10);
    let raise = || GlobalZIndex(TOP.fetch_add(1, std::sync::atomic::Ordering::Relaxed));
    commands.entity(root).insert(raise());
    commands.entity(root).observe(move |_: On<bevy::picking::events::PointerPress>, mut commands: Commands| {
        commands.entity(root).insert(raise());
    });
    let name = name.to_string();
    commands.entity(root).observe(move |d: On<bevy::picking::events::PointerDrag>, mut nodes: Query<&mut Node>, mut tex: ResMut<UiTextures>| {
        if d.button != PointerButton::Primary {
            return;
        }
        let Ok(mut n) = nodes.get_mut(root) else { return };
        let (Val::Px(x), Val::Px(y)) = (n.left, n.top) else { return };
        let p = Vec2::new(x, y) + d.delta;
        n.left = Val::Px(p.x);
        n.top = Val::Px(p.y);
        tex.1.insert(name.clone(), p);
    });
}

#[derive(Clone)]
pub struct UiTex {
    pub image: Handle<Image>,
    pub size: Vec2,
    pub split9: Option<[i32; 6]>,
}

impl UiTextures {
    pub fn get(&mut self, name: &str, lib: &mut L2Library, images: &mut Assets<Image>) -> Option<UiTex> {
        if name.is_empty() || name.eq_ignore_ascii_case("undefined") || name.eq_ignore_ascii_case("none") {
            return None;
        }
        self.0
            .entry(name.to_ascii_lowercase())
            .or_insert_with(|| {
                let (pkg, rest) = name.split_once('.')?;
                let object = rest.rsplit('.').next()?;
                let l = lib.lib.as_mut()?;
                let h = l.find(pkg, object)?;
                let t = ue2::texture::read_texture(&h.package, h.export).ok()?;
                let size = Vec2::new(t.width as f32, t.height as f32);
                let split9 = t.split9;
                let img = Image::new(
                    bevy::render::render_resource::Extent3d { width: t.width, height: t.height, depth_or_array_layers: 1 },
                    bevy::render::render_resource::TextureDimension::D2,
                    t.rgba,
                    bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb,
                    bevy::asset::RenderAssetUsages::RENDER_WORLD,
                );
                Some(UiTex { image: images.add(img), size, split9 })
            })
            .clone()
    }
}

/// A built control: its window and name in the xdat.
#[derive(Component, Clone)]
pub struct UiCtrl {
    pub window: String,
    pub name: String,
    pub class: String,
    pub size: Vec2,
}

/// Root of a built top-level window.
#[derive(Component)]
pub struct UiWindow(pub String);

/// The filled part of a status bar (its width follows the value).
#[derive(Component)]
pub struct BarFill {
    pub full: f32,
}

/// Text node of a control.
#[derive(Component)]
pub struct CtrlText;

/// What building a client window needs, as one system parameter.
#[derive(bevy::ecs::system::SystemParam)]
pub struct UiAssets<'w, 's> {
    pub iface: Option<Res<'w, Interface>>,
    pub windows: Query<'w, 's, &'static Window>,
    pub tex: ResMut<'w, UiTextures>,
    pub lib: ResMut<'w, L2Library>,
    pub images: ResMut<'w, Assets<Image>>,
}

impl UiAssets<'_, '_> {
    pub fn screen(&self) -> Vec2 {
        self.windows.iter().next().map_or(Vec2::new(1920.0, 1080.0), |w| Vec2::new(w.width(), w.height()))
    }
}

/// Buttons of client windows clicked this frame, as (window, control).
#[derive(Resource, Default)]
pub struct UiClicks(pub Vec<(String, String)>);

impl UiClicks {
    /// Whether `ctrl` of `window` was clicked (consumes the click).
    pub fn take(&mut self, window: &str, ctrl: &str) -> bool {
        let i = self.0.iter().position(|(w, c)| w == window && c == ctrl);
        i.map(|i| self.0.remove(i)).is_some()
    }
}

/// Header text of a framed window.
#[derive(Component)]
pub struct WindowTitle;

/// Icon grid of an ItemWindow: where slot `i` goes.
#[derive(Component, Clone, Copy)]
pub struct ItemGrid {
    pub cols: u32,
    pub icon: Vec2,
    pub gap: Vec2,
    pub offset: Vec2,
}

impl ItemGrid {
    pub fn slot(&self, i: u32) -> Vec2 {
        let (c, r) = (i % self.cols.max(1), i / self.cols.max(1));
        self.offset + Vec2::new(c as f32 * (self.icon.x + self.gap.x), r as f32 * (self.icon.y + self.gap.y))
    }
}

pub struct L2UiPlugin;

impl Plugin for L2UiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<UiTextures>()
            .init_resource::<UiClicks>()
            .add_systems(PreStartup, load_interface)
            .add_systems(Last, |mut c: ResMut<UiClicks>| c.0.clear());
    }
}

fn load_interface(mut commands: Commands) {
    let root = crate::paths::client_dir();
    match std::fs::read(root.join("system/interface.xdat")).map_err(|e| e.to_string()).and_then(|d| ue2::xdat::parse(&d).map_err(|e| e.to_string())) {
        Ok(x) => {
            let sys = root.join("system");
            let load = |f: &str| ue2::dat::read_dat(sys.join(f)).ok().and_then(|d| ue2::dat::parse_sysstring(&d).ok()).unwrap_or_default();
            // Russian labels; entries the Russian table leaves untranslated
            // ("???", empty) come from the English one.
            let mut strings = load("sysstring-ru.dat");
            for (id, en) in load("sysstring-e.dat") {
                let e = strings.entry(id).or_default();
                if e.trim().is_empty() || e.contains("??") {
                    *e = en;
                }
            }
            info!("interface.xdat: {} windows, {} sysstrings", x.windows.len(), strings.len());
            commands.insert_resource(Interface(Arc::new(x), Arc::new(strings)));
        }
        Err(e) => warn!("interface.xdat: {e}"),
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct Rect {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

/// Point of a rect for an xdat alignment (NONE / TOP_LEFT ... BOTTOM_RIGHT).
fn point(r: Rect, align: i32) -> Vec2 {
    let (fx, fy) = match align {
        2 => (0.5, 0.0),
        3 => (1.0, 0.0),
        4 => (0.0, 0.5),
        5 => (0.5, 0.5),
        6 => (1.0, 0.5),
        7 => (0.0, 1.0),
        8 => (0.5, 1.0),
        9 => (1.0, 1.0),
        _ => (0.0, 0.0),
    };
    Vec2::new(r.x + r.w * fx, r.y + r.h * fy)
}

fn size_of(e: &Element, parent: Rect) -> Vec2 {
    if e.flag("equalToParentSize") {
        return Vec2::new(parent.w, parent.h);
    }
    if !e.flag("size") {
        // Item slots default to one icon; other controls fill the parent.
        return match e.class.as_str() {
            "ShortcutItemWindow" => Vec2::splat(36.0),
            "ItemWindow" => Vec2::splat(34.0),
            _ => Vec2::new(parent.w, parent.h),
        };
    }
    let rel = |v: i32, p: f32| if v <= 0 { (p + v as f32).max(0.0) } else { v as f32 };
    Vec2::new(rel(e.int("width"), parent.w), rel(e.int("height"), parent.h))
}

/// Lays out a window's controls (flattened) relative to its own top left.
/// Width of an autosized text control's label (it grows with its text, and
/// controls attached to its right follow).
type Measure<'a> = &'a dyn Fn(&Element) -> Option<f32>;

fn layout(win: &Element, root: Rect, hide: &dyn Fn(&str) -> bool, measure: Measure, out: &mut Vec<(Element, Rect, usize)>) {
    // (element, parent rect, depth)
    let mut pending: Vec<(&Element, Rect, usize)> = win.children().iter().map(|c| (c, root, 1)).collect();
    let mut rects: HashMap<String, Rect> = HashMap::new();
    rects.insert(win.name().to_ascii_lowercase(), root);
    // Anchors may point at controls defined later: resolve in passes.
    for _ in 0..6 {
        let mut next = Vec::new();
        for (e, parent, depth) in pending {
            let target = e.str("relativeTo").rsplit('.').next().unwrap_or("").to_ascii_lowercase();
            let anchor = if !e.flag("usePosition") || target.is_empty() {
                Some(parent)
            } else {
                rects.get(&target).copied()
            };
            let Some(anchor) = anchor else {
                next.push((e, parent, depth));
                continue;
            };
            let mut size = size_of(e, parent);
            if e.class == "TextBox" && e.flag("autosize") {
                if let Some(w) = measure(e) {
                    size.x = w;
                }
            }
            let pos = if e.flag("usePosition") {
                // `anchorPoint` is the point on the control it is attached
                // to, `relativePoint` the control's own point (a label's
                // value: own top left at the label's top right).
                let rel = e.int("anchorPoint").max(1);
                let own = e.int("relativePoint").max(1);
                let target_pt = point(anchor, rel);
                let own_off = point(Rect { x: 0.0, y: 0.0, w: size.x, h: size.y }, own);
                target_pt - own_off + Vec2::new(e.float("x"), e.float("y"))
            } else {
                Vec2::new(parent.x, parent.y)
            };
            let r = Rect { x: pos.x, y: pos.y, w: size.x, h: size.y };
            rects.insert(e.name().to_ascii_lowercase(), r);
            // Hidden controls take their whole subtree with them. Sub-windows
            // the client's scripts show are asked for as `show:<name>`.
            let script_shown = || hide(&format!("show:{}", e.name()));
            if hide(e.name()) || (e.class == "Window" && e.flag("hidden") && !script_shown()) {
                continue;
            }
            out.push((e.clone(), r, depth));
            for c in e.children() {
                next.push((c, r, depth + 1));
            }
        }
        if next.is_empty() {
            break;
        }
        pending = next;
    }
}

/// Screen rect of a top-level window from its default position.
fn window_rect(x: &Xdat, win: &Element, screen: Vec2) -> Rect {
    let def = x.wnd_def_pos.iter().find(|p| p.str("wnd").eq_ignore_ascii_case(win.name()));
    let mut w = win.int("width") as f32;
    let mut h = win.int("height") as f32;
    if let Some(d) = def {
        if d.int("width") > 0 {
            w = d.int("width") as f32;
        }
        if d.int("height") > 0 {
            h = d.int("height") as f32;
        }
    }
    let (ax, ay, align) = def.map_or((win.float("x"), win.float("y"), win.int("relativePoint")), |d| (d.int("x") as f32, d.int("y") as f32, d.int("alignment")));
    let screen_rect = Rect { x: 0.0, y: 0.0, w: screen.x, h: screen.y };
    let p = point(screen_rect, align.max(1)) - point(Rect { x: 0.0, y: 0.0, w, h }, align.max(1)) + Vec2::new(ax, ay);
    Rect { x: p.x, y: p.y, w, h }
}

/// A window the client builds from script, drawn with the client's frame
/// art: the large framed background and a title. Returns the root.
pub fn spawn_frame(commands: &mut Commands, ua: &mut UiAssets, name: &str, title: &str, size: Vec2) -> Entity {
    let screen = ua.screen();
    let pos = Vec2::new(((screen.x - size.x) / 2.0).round(), ((screen.y - size.y) / 3.0).round());
    spawn_frame_at(commands, ua, name, title, pos, size)
}

/// `spawn_frame` at a given screen position.
pub fn spawn_frame_at(commands: &mut Commands, ua: &mut UiAssets, name: &str, title: &str, pos: Vec2, size: Vec2) -> Entity {
    let root = commands
        .spawn((
            UiWindow(name.into()),
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(pos.x),
                top: Val::Px(pos.y),
                width: Val::Px(size.x),
                height: Val::Px(size.y),
                ..default()
            },
        ))
        .id();
    if let Some(p) = ua.tex.moved(name) {
        commands.entity(root).entry::<Node>().and_modify(move |mut n| {
            n.left = Val::Px(p.x);
            n.top = Val::Px(p.y);
        });
    }
    make_draggable(commands, root, name);
    if let Some(t) = ua.tex.get("L2UI_CT1.Windows.Windows_DF_Large_Bg", &mut ua.lib, &mut ua.images) {
        commands.entity(root).insert(image_node(&t, None));
    }
    commands.spawn((
        WindowTitle,
        Text::new(title),
        TextFont { font_size: FontSize::Px(12.0), ..default() },
        TextColor(Color::srgb(0.85, 0.8, 0.65)),
        Node { position_type: PositionType::Absolute, left: Val::Px(12.0), top: Val::Px(8.0), ..default() },
        ChildOf(root),
    ));
    root
}

/// A client-style button (Button_DF) on `parent`; clicks arrive in
/// `UiClicks` as (`window`, `ctrl`).
#[allow(clippy::too_many_arguments)]
pub fn spawn_button(commands: &mut Commands, ua: &mut UiAssets, parent: Entity, pos: Vec2, size: Vec2, label: &str, color: Color, window: &str, ctrl: &str) -> Entity {
    let id = commands
        .spawn((
            Node { position_type: PositionType::Absolute, left: Val::Px(pos.x), top: Val::Px(pos.y), width: Val::Px(size.x), height: Val::Px(size.y), padding: UiRect::top(Val::Px(((size.y - 15.0) / 2.0).max(0.0))), ..default() },
            Text::new(label),
            TextFont { font_size: FontSize::Px(12.0), ..default() },
            TextColor(color),
            TextLayout { justify: Justify::Center, ..default() },
            ChildOf(parent),
        ))
        .id();
    if let Some(t) = ua.tex.get("L2UI_CT1.Button.Button_DF", &mut ua.lib, &mut ua.images) {
        commands.entity(id).insert(image_node(&t, None));
    }
    let (w, c) = (window.to_string(), ctrl.to_string());
    commands.entity(id).observe(move |_: On<bevy::picking::events::PointerClick>, mut clicks: ResMut<UiClicks>| clicks.0.push((w.clone(), c.clone())));
    id
}

/// The X button closing a window; its click arrives as (`window`, "close").
pub fn add_close_button(commands: &mut Commands, tex: &mut UiTextures, lib: &mut L2Library, images: &mut Assets<Image>, root: Entity, width: f32, window: &str) {
    let normal = tex.get("L2UI_CT1.Frames.frames_df_btn_close", lib, images);
    let over = tex.get("L2UI_CT1.Frames.frames_df_btn_close_over", lib, images).or(normal.clone());
    let mut b = commands.spawn((
        Node { position_type: PositionType::Absolute, left: Val::Px(width - 24.0), top: Val::Px(8.0), width: Val::Px(15.0), height: Val::Px(15.0), ..default() },
        // Above the window's own controls, which are spawned after it.
        ZIndex(100),
        ChildOf(root),
    ));
    match &normal {
        Some(t) => {
            b.insert(image_node(t, None));
        }
        None => {
            b.insert((Text::new("x"), TextFont { font_size: FontSize::Px(12.0), ..default() }));
        }
    }
    let w = window.to_string();
    let (n_img, o_img) = (normal.map(|t| t.image), over.map(|t| t.image));
    let (n2, o2) = (n_img.clone(), o_img.clone());
    b.observe(move |e: On<bevy::picking::events::PointerOver>, mut q: Query<&mut ImageNode>| {
        if let (Ok(mut img), Some(o)) = (q.get_mut(e.entity), o2.clone()) {
            img.image = o;
        }
    })
    .observe(move |e: On<bevy::picking::events::PointerOut>, mut q: Query<&mut ImageNode>| {
        if let (Ok(mut img), Some(n)) = (q.get_mut(e.entity), n2.clone()) {
            img.image = n;
        }
    })
    .observe(move |_: On<bevy::picking::events::PointerClick>, mut clicks: ResMut<UiClicks>| clicks.0.push((w.clone(), "close".into())));
    let _ = (n_img, o_img);
}

/// Labels the Russian client leaves as Korean placeholders or empty (its
/// scripts fill them from strings this offline build does not have).
fn label_override(window: &str, ctrl: &str) -> Option<&'static str> {
    match (window, ctrl) {
        ("SystemMenuWnd", "txtArchive") => Some("Архив"),
        ("SystemMenuWnd", "txtMovieRec") => Some("Запись видео"),
        ("SystemMenuWnd", "txtBBS") => Some("Форум"),
        ("SystemMenuWnd", "txtMacro") => Some("Макросы"),
        _ => None,
    }
}

/// Position (inside its window) and size of a control, as laid out.
pub fn ctrl_rect(x: &Xdat, window: &str, ctrl: &str, screen: Vec2) -> Option<(Vec2, Vec2)> {
    let win = x.window(window)?;
    let r = window_rect(x, win, screen);
    let mut items = Vec::new();
    layout(win, Rect { x: 0.0, y: 0.0, w: r.w, h: r.h }, &|_| false, &|_| None, &mut items);
    items.into_iter().find(|(e, _, _)| e.name() == ctrl).map(|(_, r, _)| (Vec2::new(r.x, r.y), Vec2::new(r.w, r.h)))
}

fn image_node(t: &UiTex, e: Option<&Element>) -> ImageNode {
    let mut node = ImageNode::new(t.image.clone());
    node.image_mode = NodeImageMode::Stretch;
    if let Some(e) = e {
        let (u, v, us, vs) = (e.float("u"), e.float("v"), e.float("uSize"), e.float("vSize"));
        if us > 0.0 && vs > 0.0 {
            node.rect = Some(bevy::math::Rect::new(u, v, u + us, v + vs));
        }
        if e.int("type") == 2 {
            node.image_mode = NodeImageMode::Tiled { tile_x: true, tile_y: true, stretch_value: 1.0 };
        }
    }
    // Split9 stores cumulative positions: left border up to X1, right border
    // from X2 to the edge (same for Y).
    if let Some([x1, x2, _, y1, y2, _]) = t.split9 {
        if node.rect.is_none() {
            node.image_mode = NodeImageMode::Sliced(TextureSlicer {
                border: BorderRect {
                    min_inset: Vec2::new(x1 as f32, y1 as f32),
                    max_inset: Vec2::new((t.size.x - x2 as f32).max(0.0), (t.size.y - y2 as f32).max(0.0)),
                },
                ..default()
            });
        }
    }
    node
}

fn abs_node(r: Rect, origin: Vec2) -> Node {
    Node {
        position_type: PositionType::Absolute,
        left: Val::Px(r.x - origin.x),
        top: Val::Px(r.y - origin.y),
        width: Val::Px(r.w),
        height: Val::Px(r.h),
        ..default()
    }
}

/// Builds a top-level window. `hide` names controls left out (states the
/// client's scripts would switch off).
#[allow(clippy::too_many_arguments)]
pub fn spawn_window(
    commands: &mut Commands,
    iface: &Interface,
    x: &Xdat,
    name: &str,
    screen: Vec2,
    hide: &dyn Fn(&str) -> bool,
    tex: &mut UiTextures,
    lib: &mut L2Library,
    images: &mut Assets<Image>,
) -> Option<Entity> {
    let win = x.window(name)?;
    let mut rect = window_rect(x, win, screen);
    if let Some(p) = tex.moved(win.name()) {
        rect.x = p.x;
        rect.y = p.y;
    }
    let root = commands
        .spawn((
            UiWindow(win.name().to_string()),
            UiCtrl { window: win.name().into(), name: win.name().into(), class: "Window".into(), size: Vec2::new(rect.w, rect.h) },
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(rect.x),
                top: Val::Px(rect.y),
                width: Val::Px(rect.w),
                height: Val::Px(rect.h),
                ..default()
            },
        ))
        .id();
    make_draggable(commands, root, win.name());
    // Framed windows with `exitbutton` get the X in the top right corner
    // (the NPC dialog has one in the client too).
    if win.flag("exitbutton") || win.name() == "NPCDialogWnd" {
        add_close_button(commands, tex, lib, images, root, rect.w, win.name());
    }
    if let Some(t) = tex.get(win.str("backTex"), lib, images) {
        commands.entity(root).insert(image_node(&t, None));
    }
    // Framed windows show their title in the header.
    if let Some(title) = iface.string(win.int("title")) {
        commands.spawn((
            WindowTitle,
            Text::new(title),
            TextFont { font_size: FontSize::Px(12.0), ..default() },
            TextColor(Color::srgb(0.85, 0.8, 0.65)),
            Node { position_type: PositionType::Absolute, left: Val::Px(12.0), top: Val::Px(8.0), ..default() },
            ChildOf(root),
        ));
    }
    let mut items = Vec::new();
    // Labels: about 6.5 px per character at the UI font size.
    let measure = |e: &Element| {
        let text = e.str("text");
        let text = if text == "undefined" || text.is_empty() { iface.string(e.int("sysstring")).unwrap_or("") } else { text };
        (!text.is_empty()).then(|| text.chars().count() as f32 * 6.5)
    };
    layout(win, Rect { x: 0.0, y: 0.0, w: rect.w, h: rect.h }, hide, &measure, &mut items);
    let dump = std::env::var_os("L2_UI_DUMP").is_some();
    for (e, r, _) in items {
        let n = e.name();
        if dump {
            info!("{} {} [{}] at {},{} size {}x{} (window at {},{})", win.name(), n, e.class, r.x, r.y, r.w, r.h, rect.x, rect.y);
        }
        let ctrl = UiCtrl { window: win.name().into(), name: n.into(), class: e.class.clone(), size: Vec2::new(r.w, r.h) };
        let node = abs_node(r, Vec2::ZERO);
        let id = commands.spawn((ctrl, node, ChildOf(root))).id();
        match e.class.as_str() {
            "Window" => {
                commands.entity(id).insert(bevy::picking::Pickable::IGNORE);
                if let Some(t) = tex.get(e.str("backTex"), lib, images) {
                    commands.entity(id).insert(image_node(&t, None));
                }
            }
            "Texture" => {
                // Decoration: clicks go through to the buttons under it, as
                // in the client (the system menu's row bars lie over them).
                commands.entity(id).insert(bevy::picking::Pickable::IGNORE);
                if let Some(t) = tex.get(e.str("file"), lib, images) {
                    let mut img = image_node(&t, Some(&e));
                    // Type 1 ("Normal") draws the texture at its own size,
                    // cut to the control.
                    if e.int("type") == 1 && img.rect.is_none() && t.split9.is_none() {
                        let (w, h) = (r.w.min(t.size.x), r.h.min(t.size.y));
                        img.rect = Some(bevy::math::Rect::new(0.0, 0.0, w, h));
                        commands.entity(id).insert(abs_node(Rect { w, h, ..r }, Vec2::ZERO));
                    }
                    commands.entity(id).insert(img);
                }
            }
            "Button" => {
                if let Some(t) = tex.get(e.str("normalTex"), lib, images) {
                    commands.entity(id).insert(image_node(&t, None));
                }
                let (w, c) = (win.name().to_string(), n.to_string());
                commands.entity(id).observe(move |_: On<bevy::picking::events::PointerClick>, mut clicks: ResMut<UiClicks>| clicks.0.push((w.clone(), c.clone())));
                let label = e.str("buttonNameText");
                let label = if label.is_empty() || label == "undefined" { iface.string(e.int("buttonName")).unwrap_or("") } else { label };
                if !label.is_empty() {
                    commands.entity(id).insert((
                        Text::new(label),
                        TextFont { font_size: FontSize::Px(11.0), ..default() },
                        TextLayout { justify: Justify::Center, ..default() },
                    ));
                    // Label centred vertically, as the client draws it.
                    let pad = ((r.h - 14.0) / 2.0).max(0.0);
                    commands.entity(id).entry::<Node>().and_modify(move |mut n| n.padding = UiRect::top(Val::Px(pad)));
                }
            }
            "Tab" => {
                // A row of tab buttons; the first is the open one. Clicks
                // arrive as (window, "<tab control>#<index>").
                let mut x = 0.0;
                for (i, t) in e.list("tabs").iter().enumerate() {
                    let w = if t.int("width") > 0 { t.int("width") as f32 } else { 70.0 };
                    let h = if t.int("height") > 0 { t.int("height") as f32 } else { r.h };
                    let texture = if i == 0 { t.str("pushedTex") } else { t.str("normalTex") };
                    let label = t.str("buttonNameText");
                    let label = if label.is_empty() || label == "undefined" { iface.string(t.int("buttonName")).unwrap_or("") } else { label };
                    let mut b = commands.spawn((
                        Node {
                            position_type: PositionType::Absolute,
                            left: Val::Px(x),
                            top: Val::Px(0.0),
                            width: Val::Px(w),
                            height: Val::Px(h),
                            padding: UiRect::top(Val::Px(((h - 14.0) / 2.0).max(0.0))),
                            ..default()
                        },
                        Text::new(label),
                        TextFont { font_size: FontSize::Px(11.0), ..default() },
                        TextColor(if i == 0 { Color::srgb(0.95, 0.85, 0.55) } else { Color::srgb(0.7, 0.68, 0.62) }),
                        TextLayout { justify: Justify::Center, linebreak: LineBreak::NoWrap },
                        ChildOf(id),
                    ));
                    if let Some(tx) = tex.get(texture, lib, images) {
                        b.insert(image_node(&tx, None));
                    }
                    let (wn, c) = (win.name().to_string(), format!("{n}#{i}"));
                    b.observe(move |_: On<bevy::picking::events::PointerClick>, mut clicks: ResMut<UiClicks>| clicks.0.push((wn.clone(), c.clone())));
                    x += w + t.int("gap").max(0) as f32;
                }
            }
            "StatusBar" => {
                // Background, then the fill whose width follows the value.
                let parts = |l: &str, c: &str, r: &str, tex: &mut UiTextures, lib: &mut L2Library, images: &mut Assets<Image>| {
                    (tex.get(e.str(l), lib, images), tex.get(e.str(c), lib, images), tex.get(e.str(r), lib, images))
                };
                let (bl, bc, br) = parts("backLeftTex", "backTex", "backRightTex", tex, lib, images);
                let (fl, fc, fr) = parts("foreLeftTex", "foreTex", "foreRightTex", tex, lib, images);
                spawn_bar(commands, id, r, bl, bc, br, None);
                let fill = commands
                    .spawn((BarFill { full: r.w }, Node { position_type: PositionType::Absolute, width: Val::Px(r.w), height: Val::Px(r.h), overflow: Overflow::clip(), ..default() }, ChildOf(id)))
                    .id();
                spawn_bar(commands, fill, r, fl, fc, fr, Some(r.w));
                commands.spawn((
                    CtrlText,
                    Text::new(""),
                    TextFont { font_size: FontSize::Px(10.0), ..default() },
                    TextLayout { justify: Justify::Center, ..default() },
                    Node { position_type: PositionType::Absolute, width: Val::Px(r.w), top: Val::Px(-1.0), ..default() },
                    ChildOf(id),
                ));
            }
            "BarCtrl" => {
                commands.entity(id).insert(BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.6)));
                commands.spawn((
                    BarFill { full: r.w },
                    Node { position_type: PositionType::Absolute, width: Val::Px(r.w), height: Val::Px(r.h), ..default() },
                    BackgroundColor(Color::srgb(0.75, 0.15, 0.1)),
                    ChildOf(id),
                ));
            }
            "TextBox" | "NameCtrl" => {
                let text = e.str("text");
                let text = if text == "undefined" || text.is_empty() { iface.string(e.int("sysstring")).unwrap_or("") } else { text };
                let text = label_override(win.name(), n).unwrap_or(text);
                let (w, c) = (win.name().to_string(), n.to_string());
                commands.entity(id).observe(move |_: On<bevy::picking::events::PointerClick>, mut clicks: ResMut<UiClicks>| clicks.0.push((w.clone(), c.clone())));
                commands.entity(id).insert((
                    CtrlText,
                    Text::new(text),
                    TextFont { font_size: FontSize::Px(11.0), ..default() },
                    TextColor(Color::srgb(0.85, 0.82, 0.75)),
                    TextLayout { linebreak: LineBreak::NoWrap, ..default() },
                ));
                // Script-sized boxes (negative sizes larger than the window)
                // come out empty: let the text size them.
                if r.h < 12.0 || r.w < 8.0 {
                    commands.entity(id).entry::<Node>().and_modify(|mut n| {
                        n.height = Val::Auto;
                        n.width = Val::Auto;
                    });
                }
            }
            "ItemWindow" => {
                let iw = Vec2::new(e.int("iconWidth").max(32) as f32, e.int("iconHeight").max(32) as f32);
                commands.entity(id).insert(ItemGrid {
                    cols: e.int("col").max(1) as u32,
                    icon: iw,
                    gap: Vec2::new(e.int("gapX") as f32, e.int("gapY") as f32),
                    // -9999 means unset.
                    offset: Vec2::new(e.int("offsetX").max(0) as f32, e.int("offsetY").max(0) as f32),
                });
            }
            "ChatWindow" => {
                commands.entity(id).insert((
                    CtrlText,
                    Text::new(""),
                    TextFont { font_size: FontSize::Px(12.0), ..default() },
                    Node { position_type: PositionType::Absolute, left: Val::Px(r.x + 6.0), top: Val::Px(r.y + 4.0), width: Val::Px(r.w - 10.0), height: Val::Px(r.h - 6.0), flex_direction: FlexDirection::ColumnReverse, overflow: Overflow::clip(), ..default() },
                ));
            }
            _ => {}
        }
    }
    Some(root)
}

#[allow(clippy::too_many_arguments)]
fn spawn_bar(commands: &mut Commands, parent: Entity, r: Rect, l: Option<UiTex>, c: Option<UiTex>, rt: Option<UiTex>, fixed: Option<f32>) {
    let w = fixed.unwrap_or(r.w);
    let lw = l.as_ref().map_or(0.0, |t| t.size.x.min(w / 2.0));
    let rw = rt.as_ref().map_or(0.0, |t| t.size.x.min(w / 2.0));
    let mut put = |t: Option<UiTex>, x: f32, width: f32| {
        if let Some(t) = t {
            commands.spawn((
                ImageNode { image_mode: NodeImageMode::Stretch, ..ImageNode::new(t.image) },
                Node { position_type: PositionType::Absolute, left: Val::Px(x), width: Val::Px(width.max(0.0)), height: Val::Px(r.h), ..default() },
                ChildOf(parent),
            ));
        }
    };
    put(l, 0.0, lw);
    put(c, lw, w - lw - rw);
    put(rt, w - rw, rw);
}
