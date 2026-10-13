//! L2 Survivor: Lineage II assets driven by a Vampire Survivors style loop.

mod input;
mod l2assets;
mod survivor;

use bevy::prelude::*;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "L2 Survivor".into(),
                ..default()
            }),
            ..default()
        }))
        .add_plugins((input::InputPlugin, l2assets::L2AssetsPlugin, survivor::SurvivorPlugin))
        .add_systems(Update, debug_screenshot)
        .run();
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
    if !*done && t > 8.0 {
        *done = true;
        commands
            .spawn(bevy::render::view::screenshot::Screenshot::primary_window())
            .observe(bevy::render::view::screenshot::save_to_disk(path));
    }
    if t > 10.0 {
        exit.write(AppExit::Success);
    }
}
