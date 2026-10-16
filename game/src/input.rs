//! Unified input: keyboard and any connected gamepad feed one `PlayerInput`.
//! Gamepads come from Bevy's gilrs backend, so hot-plugging just works.

use bevy::prelude::*;

const STICK_DEADZONE: f32 = 0.2;
/// Stick deflection that counts as a menu "press" when navigating.
const STICK_NAV_THRESHOLD: f32 = 0.6;

#[derive(Resource, Default, Debug)]
pub struct PlayerInput {
    /// Movement on the ground plane, length <= 1.
    pub move_dir: Vec2,
    /// Confirm / accept (Enter, Space, gamepad South).
    pub confirm: bool,
    /// Pause toggle (Esc, gamepad Start).
    pub pause: bool,
    /// Menu navigation step: -1 = up/left, +1 = down/right.
    pub nav: i32,
    stick_latched: bool,
}

pub struct InputPlugin;

impl Plugin for InputPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PlayerInput>().add_systems(
            PreUpdate,
            (activate_pads, gather_input).chain().after(bevy::input::InputSystems),
        );
    }
}

/// Marks a gamepad the game listens to. A pad counts only once one of its
/// buttons was pressed: virtual pads, wheels and some mice or headsets show
/// up as gamepads with an axis stuck off-centre, which would otherwise spin
/// the camera and walk the hero on their own.
#[derive(Component)]
pub struct ActivePad;

fn pad_name(name: Option<&Name>) -> &str {
    name.map(|n| n.as_str()).unwrap_or("unnamed")
}

fn activate_pads(
    mut commands: Commands,
    connected: Query<Option<&Name>, Added<Gamepad>>,
    pads: Query<(Entity, &Gamepad, Option<&Name>), Without<ActivePad>>,
) {
    for name in &connected {
        info!("gamepad connected: {} (ignored until one of its buttons is pressed)", pad_name(name));
    }
    for (entity, pad, name) in &pads {
        // Triggers are analog and can sit half-pressed on odd devices.
        let pressed = pad
            .get_just_pressed()
            .any(|b| !matches!(b, GamepadButton::LeftTrigger2 | GamepadButton::RightTrigger2));
        if pressed {
            info!("gamepad active: {}", pad_name(name));
            commands.entity(entity).insert(ActivePad);
        }
    }
}

fn gather_input(
    mut input: ResMut<PlayerInput>,
    keys: Res<ButtonInput<KeyCode>>,
    gamepads: Query<&Gamepad, With<crate::input::ActivePad>>,
    time: Res<Time>,
) {
    let mut dir = Vec2::ZERO;
    if keys.any_pressed([KeyCode::KeyW, KeyCode::ArrowUp]) {
        dir.y += 1.0;
    }
    if keys.any_pressed([KeyCode::KeyS, KeyCode::ArrowDown]) {
        dir.y -= 1.0;
    }
    if keys.any_pressed([KeyCode::KeyA, KeyCode::ArrowLeft]) {
        dir.x -= 1.0;
    }
    if keys.any_pressed([KeyCode::KeyD, KeyCode::ArrowRight]) {
        dir.x += 1.0;
    }
    let mut confirm = keys.any_just_pressed([KeyCode::Enter, KeyCode::Space]);
    let mut pause = keys.just_pressed(KeyCode::Escape);
    let mut nav = 0;
    if keys.any_just_pressed([KeyCode::KeyW, KeyCode::ArrowUp, KeyCode::KeyA, KeyCode::ArrowLeft]) {
        nav -= 1;
    }
    if keys.any_just_pressed([KeyCode::KeyS, KeyCode::ArrowDown, KeyCode::KeyD, KeyCode::ArrowRight]) {
        nav += 1;
    }

    let mut stick_any = false;
    for pad in &gamepads {
        let stick = pad.left_stick();
        if stick.length() > STICK_DEADZONE {
            dir += stick;
        }
        let mut dpad = Vec2::ZERO;
        if pad.pressed(GamepadButton::DPadUp) {
            dpad.y += 1.0;
        }
        if pad.pressed(GamepadButton::DPadDown) {
            dpad.y -= 1.0;
        }
        if pad.pressed(GamepadButton::DPadLeft) {
            dpad.x -= 1.0;
        }
        if pad.pressed(GamepadButton::DPadRight) {
            dpad.x += 1.0;
        }
        dir += dpad;

        confirm |= pad.just_pressed(GamepadButton::South);
        pause |= pad.just_pressed(GamepadButton::Start);
        if pad.just_pressed(GamepadButton::DPadUp) || pad.just_pressed(GamepadButton::DPadLeft) {
            nav -= 1;
        }
        if pad.just_pressed(GamepadButton::DPadDown) || pad.just_pressed(GamepadButton::DPadRight) {
            nav += 1;
        }
        // Treat a strong stick flick as one discrete menu step.
        let major = if stick.x.abs() > stick.y.abs() { stick.x } else { -stick.y };
        if major.abs() > STICK_NAV_THRESHOLD {
            stick_any = true;
            if !input.stick_latched {
                nav += major.signum() as i32;
            }
        }
    }
    input.stick_latched = stick_any;

    // L2_AUTOPILOT walks in a slow circle (for unattended test runs).
    if std::env::var_os("L2_AUTOPILOT").is_some() {
        let t = time.elapsed_secs() * 0.5;
        dir = Vec2::new(t.cos(), t.sin());
    }
    input.move_dir = dir.clamp_length_max(1.0);
    input.confirm = confirm;
    input.pause = pause;
    input.nav = nav.signum();
}
