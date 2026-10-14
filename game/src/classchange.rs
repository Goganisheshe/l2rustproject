//! Class transfer as in Classic: at level 20, 40 and 76 the hero picks the
//! next profession from the datapack's class tree (classList.xml). The new
//! class brings its own base template and skill tree.

use bevy::prelude::*;

use crate::datapack::Datapack;
use crate::input::PlayerInput;
use crate::mmo::{Hero, Phase};
use crate::rpg::{HeroStats, Log};

#[derive(Resource, Default)]
pub struct ClassChangeUi {
    pub open: bool,
    selected: usize,
    shown: Option<(usize, u32)>,
    /// Class for which the window already opened by itself.
    prompted: Option<u32>,
}

#[derive(Component)]
struct ClassChangeRoot;

pub struct ClassChangePlugin;

impl Plugin for ClassChangePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ClassChangeUi>().add_systems(Update, class_change.run_if(in_state(Phase::InWorld)));
    }
}

/// Level needed for the next transfer after `depth` transfers.
pub fn transfer_level(depth: u32) -> Option<u32> {
    match depth {
        0 => Some(20),
        1 => Some(40),
        2 => Some(76),
        _ => None,
    }
}

#[allow(clippy::too_many_arguments)]
fn class_change(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    gamepads: Query<&Gamepad>,
    input: Res<PlayerInput>,
    dp: Res<Datapack>,
    mut ui: ResMut<ClassChangeUi>,
    mut log: ResMut<Log>,
    mut stats: Single<&mut HeroStats, With<Hero>>,
    roots: Query<Entity, With<ClassChangeRoot>>,
) {
    let class = stats.template.class_id;
    let options = dp.next_classes(class);
    let eligible = !options.is_empty() && transfer_level(dp.class_depth(class)).is_some_and(|l| stats.level >= l);
    // Opens by itself once per class when the hero qualifies; K / D-pad right reopens.
    if eligible && ui.prompted != Some(class) {
        ui.prompted = Some(class);
        ui.open = true;
        log.push("You can change your class now (K).");
    }
    if keys.just_pressed(KeyCode::KeyK) || gamepads.iter().any(|g| g.just_pressed(GamepadButton::DPadRight)) {
        ui.open = !ui.open;
        if ui.open && !eligible {
            ui.open = false;
            match transfer_level(dp.class_depth(class)) {
                Some(l) if !options.is_empty() => log.push(format!("Class transfer is available from level {l}.")),
                _ => log.push("There is no further class transfer."),
            }
        }
    }
    if ui.open && (keys.just_pressed(KeyCode::Escape) || gamepads.iter().any(|g| g.just_pressed(GamepadButton::East))) {
        ui.open = false;
    }
    if !ui.open || !eligible {
        ui.open = false;
        ui.shown = None;
        for e in &roots {
            commands.entity(e).despawn();
        }
        return;
    }
    if input.nav != 0 {
        ui.selected = (ui.selected as i32 + input.nav).rem_euclid(options.len() as i32) as usize;
    }
    ui.selected = ui.selected.min(options.len() - 1);
    if input.confirm || std::env::var("L2_CLASS_PICK").is_ok() {
        let pick = std::env::var("L2_CLASS_PICK").ok().and_then(|v| v.parse::<usize>().ok()).unwrap_or(ui.selected);
        let new = options[pick.min(options.len() - 1)];
        if let Some(t) = dp.templates_by_id.get(&new) {
            let mut t = t.clone();
            t.creation_points = stats.template.creation_points.clone();
            stats.template = t;
        } else {
            stats.template.class_id = new;
        }
        stats.class_key = dp.class_name(new).to_string();
        stats.recalc();
        log.push(format!("Congratulations! You are now a {}.", dp.class_name(new)));
        ui.open = false;
        return;
    }
    if ui.shown == Some((ui.selected, class)) {
        return;
    }
    ui.shown = Some((ui.selected, class));
    for e in &roots {
        commands.entity(e).despawn();
    }
    commands
        .spawn((
            ClassChangeRoot,
            Node {
                position_type: PositionType::Absolute,
                left: Val::Percent(35.0),
                top: Val::Percent(25.0),
                width: Val::Px(360.0),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(Val::Px(12.0)),
                row_gap: Val::Px(6.0),
                ..default()
            },
            BackgroundColor(Color::srgba(0.05, 0.04, 0.08, 0.92)),
        ))
        .with_children(|p| {
            p.spawn((
                Text::new(format!("Class transfer: {}", dp.class_name(class))),
                TextFont { font_size: FontSize::Px(20.0), ..default() },
                TextColor(Color::srgb(1.0, 0.85, 0.3)),
            ));
            for (i, id) in options.iter().enumerate() {
                let on = i == ui.selected;
                p.spawn((
                    Text::new(format!("{} {}", if on { ">" } else { " " }, dp.class_name(*id))),
                    TextFont { font_size: FontSize::Px(18.0), ..default() },
                    TextColor(if on { Color::srgb(1.0, 0.85, 0.3) } else { Color::WHITE }),
                ));
            }
            p.spawn((
                Text::new("Up/Down choose, Enter / A confirm, Esc / B later"),
                TextFont { font_size: FontSize::Px(13.0), ..default() },
                TextColor(Color::srgb(0.7, 0.7, 0.7)),
            ));
        });
}
