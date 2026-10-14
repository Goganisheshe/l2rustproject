//! World viewer: `L2_WORLD=20_21[,20_22,...]` streams map tiles and lets you
//! fly around with keyboard or gamepad.
//!
//! Controls: WASD / left stick move, Q/E or triggers down/up, arrow keys /
//! right stick look, Shift / South button for speed.

use bevy::prelude::*;

use crate::streaming::{TileStreamer, TILE_UU};

pub struct WorldPlugin {
    pub tiles: Vec<String>,
}

#[derive(Resource)]
struct Tiles(Vec<(i32, i32)>);

#[derive(Component)]
struct FlyCam {
    yaw: f32,
    pitch: f32,
    placed: bool,
}

impl Plugin for WorldPlugin {
    fn build(&self, app: &mut App) {
        let tiles = self
            .tiles
            .iter()
            .filter_map(|t| {
                let (x, y) = t.split_once('_')?;
                Some((x.parse().ok()?, y.parse().ok()?))
            })
            .collect();
        app.insert_resource(Tiles(tiles))
            .insert_resource(ClearColor(Color::srgb(0.55, 0.68, 0.85)))
            .add_systems(Startup, setup)
            .add_systems(Update, (place_camera, fly));
    }
}

fn setup(mut commands: Commands, tiles: Res<Tiles>, mut streamer: ResMut<TileStreamer>) {
    for &t in &tiles.0 {
        streamer.request(t);
    }
    commands.spawn((
        DirectionalLight { illuminance: 9000.0, shadow_maps_enabled: true, ..default() },
        Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -0.9, 0.5, 0.0)),
    ));
    commands.insert_resource(GlobalAmbientLight { brightness: 600.0, ..default() });
    commands.spawn((
        Camera3d::default(),
        Projection::Perspective(PerspectiveProjection { far: 3000.0, ..default() }),
        Transform::default(),
        FlyCam { yaw: 0.0, pitch: -0.5, placed: false },
    ));
}

/// Puts the camera above the first tile's center once its terrain is in.
fn place_camera(tiles: Res<Tiles>, streamer: Res<TileStreamer>, mut cam: Single<(&mut Transform, &mut FlyCam)>) {
    let (ref mut tr, ref mut fc) = *cam;
    if fc.placed {
        return;
    }
    let Some(&(tx, ty)) = tiles.0.first() else { return };
    let ue_center = [((tx - 20) as f32 + 0.5) * TILE_UU, ((ty - 18) as f32 + 0.5) * TILE_UU, 0.0];
    let center = crate::streaming::ue_to_bevy(ue_center);
    if let Some(y) = streamer.ground_y(center) {
        tr.translation = center.with_y(y + 60.0);
        fc.placed = true;
    }
}

fn fly(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    gamepads: Query<&Gamepad>,
    mut cam: Single<(&mut Transform, &mut FlyCam)>,
) {
    let dt = time.delta_secs();
    let (ref mut tr, ref mut fc) = *cam;
    let mut mv = Vec3::ZERO;
    let mut look = Vec2::ZERO;
    let mut fast = keys.pressed(KeyCode::ShiftLeft);
    let k = |c: KeyCode| if keys.pressed(c) { 1.0 } else { 0.0 };
    mv.z += k(KeyCode::KeyS) - k(KeyCode::KeyW);
    mv.x += k(KeyCode::KeyD) - k(KeyCode::KeyA);
    mv.y += k(KeyCode::KeyE) - k(KeyCode::KeyQ);
    look.x += k(KeyCode::ArrowRight) - k(KeyCode::ArrowLeft);
    look.y += k(KeyCode::ArrowUp) - k(KeyCode::ArrowDown);
    for pad in &gamepads {
        let l = pad.left_stick();
        if l.length() > 0.15 {
            mv.x += l.x;
            mv.z -= l.y;
        }
        let r = pad.right_stick();
        if r.length() > 0.15 {
            look += r;
        }
        mv.y += pad.get(GamepadButton::RightTrigger2).unwrap_or(0.0) - pad.get(GamepadButton::LeftTrigger2).unwrap_or(0.0);
        fast |= pad.pressed(GamepadButton::South);
    }
    fc.yaw -= look.x * 1.6 * dt;
    fc.pitch = (fc.pitch + look.y * 1.2 * dt).clamp(-1.5, 1.5);
    tr.rotation = Quat::from_euler(EulerRot::YXZ, fc.yaw, fc.pitch, 0.0);
    let speed = if fast { 120.0 } else { 30.0 };
    let dir = tr.rotation * Vec3::new(mv.x, 0.0, mv.z) + Vec3::Y * mv.y;
    tr.translation += dir * speed * dt;
}
