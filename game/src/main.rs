//! L2 Survivor: Lineage II assets driven by a Vampire Survivors style loop.

mod input;
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
    .add_plugins((input::InputPlugin, l2assets::L2AssetsPlugin, skinned::SkinnedPlugin))
    .add_systems(Update, debug_screenshot);
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
    mut done: Local<bool>,
    mut exit: MessageWriter<AppExit>,
) {
    let Some(path) = std::env::var_os("L2_SCREENSHOT") else { return };
    let t = time.elapsed_secs();
    let at: f32 = std::env::var("L2_SCREENSHOT_AT").ok().and_then(|v| v.parse().ok()).unwrap_or(8.0);
    if !*done && t > at {
        *done = true;
        commands
            .spawn(bevy::render::view::screenshot::Screenshot::primary_window())
            .observe(bevy::render::view::screenshot::save_to_disk(path));
    }
    if t > at + 2.0 {
        exit.write(AppExit::Success);
    }
}
