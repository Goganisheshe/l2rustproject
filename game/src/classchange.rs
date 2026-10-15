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

/// Race of a class, from the starting class at the root of its tree.
fn race_of_class(dp: &Datapack, class: u32) -> crate::quest::Race {
    use crate::quest::Race;
    let mut c = class;
    while let Some((_, Some(p))) = dp.class_list.get(&c) {
        c = *p;
    }
    match c {
        18 | 25 => Race::Elf,
        31 | 38 => Race::DarkElf,
        44 | 49 => Race::Orc,
        53 => Race::Dwarf,
        0 | 10 => Race::Human,
        _ => Race::Kamael,
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
    mut qlog: ResMut<crate::quest::QuestLog>,
    hero: Single<(&mut HeroStats, Option<&mut crate::items::Inventory>), With<Hero>>,
    roots: Query<Entity, With<ClassChangeRoot>>,
    (mut ua, mut clicks): (crate::l2ui::UiAssets, ResMut<crate::l2ui::UiClicks>),
) {
    let (mut stats, inv) = hero.into_inner();
    let class = stats.template.class_id;
    let options = dp.next_classes(class);
    // The third transfer needs the Saga of Legend (Orven in Aden).
    let saga = crate::quests::story::SAGA_OF_LEGEND;
    let saga_done = qlog.states.get(&saga).is_some_and(|q| q.is_completed());
    let third = dp.class_depth(class) == 2;
    let eligible =
        !options.is_empty() && transfer_level(dp.class_depth(class)).is_some_and(|l| stats.level >= l) && (!third || saga_done);
    // Opens by itself once per class when the hero qualifies; C / D-pad right reopens.
    if eligible && ui.prompted != Some(class) {
        ui.prompted = Some(class);
        ui.open = true;
        log.push("You can change your class now (C).");
    }
    if keys.just_pressed(KeyCode::KeyC) || gamepads.iter().any(|g| g.just_pressed(GamepadButton::DPadRight)) {
        ui.open = !ui.open;
        if ui.open && !eligible {
            ui.open = false;
            match transfer_level(dp.class_depth(class)) {
                Some(l) if !options.is_empty() && stats.level >= l && third => {
                    log.push("Complete the Saga of Legend (Orven in Aden) for the third class transfer.")
                }
                Some(l) if !options.is_empty() => log.push(format!("Class transfer is available from level {l}.")),
                _ => log.push("There is no further class transfer."),
            }
        }
    }
    if clicks.take("ClassChangeWnd", "later") || clicks.take("ClassChangeWnd", "close") {
        ui.open = false;
    }
    let clicked = (0..options.len()).find(|i| clicks.take("ClassChangeWnd", &format!("class{i}")));
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
    if input.confirm || clicked.is_some() || std::env::var("L2_CLASS_PICK").is_ok() {
        let pick = std::env::var("L2_CLASS_PICK").ok().and_then(|v| v.parse::<usize>().ok()).or(clicked).unwrap_or(ui.selected);
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
        // Saga of Legend: a mount spellbook for the race on the third transfer.
        if third {
            if let (Some(mut inv), Some(q)) = (inv, qlog.states.get_mut(&saga)) {
                if q.get_int("mountRewarded") == 0 {
                    q.set("mountRewarded", 1);
                    let book = crate::quests::story::saga_mount(race_of_class(&dp, new));
                    *inv.items.entry(book).or_default() += 1;
                    log.push("You received a mount spellbook.");
                }
            }
        }
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
    // A window in the client's frame art, one button per profession.
    if ua.iface.is_some() {
        let n = options.len() as f32;
        let size = Vec2::new(260.0, 96.0 + n * 30.0);
        let root = crate::l2ui::spawn_frame(&mut commands, &mut ua, "ClassChangeWnd", "Смена профессии", size);
        commands.entity(root).insert(ClassChangeRoot);
        crate::l2ui::add_close_button(&mut commands, &mut ua.tex, &mut ua.lib, &mut ua.images, root, size.x, "ClassChangeWnd");
        commands.spawn((
            Text::new(format!("Текущая профессия: {}", dp.class_name(class))),
            TextFont { font_size: FontSize::Px(12.0), ..default() },
            TextColor(Color::srgb(0.85, 0.82, 0.75)),
            Node { position_type: PositionType::Absolute, left: Val::Px(16.0), top: Val::Px(40.0), ..default() },
            ChildOf(root),
        ));
        for (i, id) in options.iter().enumerate() {
            let color = if i == ui.selected { Color::srgb(1.0, 0.85, 0.3) } else { Color::WHITE };
            crate::l2ui::spawn_button(&mut commands, &mut ua, root, Vec2::new(30.0, 64.0 + i as f32 * 30.0), Vec2::new(200.0, 27.0), dp.class_name(*id), color, "ClassChangeWnd", &format!("class{i}"));
        }
        crate::l2ui::spawn_button(&mut commands, &mut ua, root, Vec2::new(size.x - 100.0, size.y - 36.0), Vec2::new(84.0, 27.0), "Позже", Color::WHITE, "ClassChangeWnd", "later");
        return;
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
