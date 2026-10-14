//! Lineage II rebuilt in Rust on Bevy from the original client's assets.

mod input;
mod items;
mod character;
mod classchange;
mod collision;
mod datapack;
mod dialog;
mod html;
mod quest;
mod quests;
mod effects;
mod formulas;
mod gamedata;
mod l2assets;
mod loot;
mod mmo;
mod npcskills;
mod rpg;
mod save;
mod services;
mod skillplay;
mod skillwindow;
mod statswindow;
mod skinned;
mod streaming;
mod viewer;
mod world;

use bevy::prelude::*;

fn main() {
    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window { title: "Lineage II (Rust)".into(), ..default() }),
        ..default()
    }))
    .add_plugins((input::InputPlugin, l2assets::L2AssetsPlugin, skinned::SkinnedPlugin, effects::EffectsPlugin))
    .add_plugins(gamedata::GameDataPlugin)
    .add_systems(Update, debug_screenshot)
    .add_systems(Startup, cyrillic_font);
    // Modes: L2_WORLD = world viewer, L2_MODEL = model viewer; default = the game.
    if let Ok(tiles) = std::env::var("L2_WORLD") {
        let tiles = tiles.split(',').map(|t| t.trim().to_string()).collect();
        app.add_plugins((streaming::TileStreamPlugin, world::WorldPlugin { tiles }));
    } else if let Ok(spec) = std::env::var("L2_MODEL") {
        app.add_plugins(viewer::ViewerPlugin { spec });
    } else {
        app.add_plugins((streaming::TileStreamPlugin, datapack::DatapackPlugin, mmo::MmoPlugin, rpg::RpgPlugin, skillplay::SkillPlayPlugin, items::ItemsPlugin, dialog::DialogPlugin, classchange::ClassChangePlugin, save::SavePlugin, npcskills::NpcSkillsPlugin, skillwindow::SkillWindowPlugin, loot::LootPlugin, statswindow::StatusWindowPlugin));
    }
    app.run();
}

/// `L2_SCREENSHOT=<path>` saves one frame after a few seconds, then exits.
/// Used for automated visual checks.
fn debug_screenshot(
    mut commands: Commands,
    time: Res<Time>,
    mut taken: Local<u32>,
    mut exit: MessageWriter<AppExit>,
) {
    let Ok(path) = std::env::var("L2_SCREENSHOT") else { return };
    let t = time.elapsed_secs();
    let env = |k: &str, d: f32| std::env::var(k).ok().and_then(|v| v.parse().ok()).unwrap_or(d);
    let at = env("L2_SCREENSHOT_AT", 8.0);
    // L2_SCREENSHOT_COUNT frames, L2_SCREENSHOT_STEP seconds apart: `shot.png`
    // becomes `shot_0.png`, `shot_1.png`...
    let count = env("L2_SCREENSHOT_COUNT", 1.0).max(1.0) as u32;
    let step = env("L2_SCREENSHOT_STEP", 0.25);
    if *taken < count && t > at + step * *taken as f32 {
        let file = if count == 1 { path.clone() } else { path.replace(".png", &format!("_{}.png", *taken)) };
        *taken += 1;
        commands
            .spawn(bevy::render::view::screenshot::Screenshot::primary_window())
            .observe(bevy::render::view::screenshot::save_to_disk(file));
    }
    if t > at + step * count as f32 + 2.0 {
        exit.write(AppExit::Success);
    }
}

/// Skill and monster names from the client are partly Russian; the built-in
/// UI font has no Cyrillic, so swap in a system font when one is available
/// (`L2_FONT` overrides the path).
fn cyrillic_font(mut fonts: ResMut<Assets<Font>>) {
    let candidates = [
        std::env::var("L2_FONT").unwrap_or_default(),
        "C:/Windows/Fonts/segoeui.ttf".into(),
        "C:/Windows/Fonts/arial.ttf".into(),
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf".into(),
    ];
    for path in candidates.iter().filter(|p| !p.is_empty()) {
        if let Ok(bytes) = std::fs::read(path) {
            let _ = fonts.insert(Handle::<Font>::default().id(), Font::from_bytes(bytes));
            info!("UI font: {path}");
            return;
        }
    }
}
