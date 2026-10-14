//! L2 Survivor: Lineage II assets driven by a Vampire Survivors style loop.

mod input;
mod character;
mod effects;
mod gamedata;
mod l2assets;
mod skinned;
mod survivor;
mod viewer;

use bevy::prelude::*;

fn main() {
    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window { title: "L2 Survivor".into(), ..default() }),
        ..default()
    }))
    .add_plugins((input::InputPlugin, l2assets::L2AssetsPlugin, skinned::SkinnedPlugin, effects::EffectsPlugin))
    .add_plugins(gamedata::GameDataPlugin)
    .add_systems(Update, debug_screenshot)
    .add_systems(Startup, cyrillic_font);
    match std::env::var("L2_MODEL") {
        Ok(spec) => app.add_plugins(viewer::ViewerPlugin { spec }),
        Err(_) => app.add_plugins(survivor::SurvivorPlugin),
    };
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
