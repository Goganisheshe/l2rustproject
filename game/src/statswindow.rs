//! Character status window (Alt+T / gamepad Back + Y), like the client's:
//! level, vitals, combat values and base stats.

use bevy::prelude::*;

use crate::datapack::Datapack;
use crate::mmo::{Hero, Phase};
use crate::rpg::HeroStats;

#[derive(Component)]
struct StatusRoot;

pub struct StatusWindowPlugin;

impl Plugin for StatusWindowPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, status_window.run_if(in_state(Phase::InWorld)));
    }
}

fn status_window(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    dp: Res<Datapack>,
    stats: Single<&HeroStats, With<Hero>>,
    mut text: Query<&mut Text, With<StatusRoot>>,
    roots: Query<Entity, With<StatusRoot>>,
    mut open: Local<bool>,
) {
    let alt = keys.pressed(KeyCode::AltLeft) || keys.pressed(KeyCode::AltRight);
    if (alt && keys.just_pressed(KeyCode::KeyT)) || (std::env::var_os("L2_STATUS").is_some() && roots.is_empty() && !*open) {
        *open = !*open;
        if !*open {
            for e in &roots {
                commands.entity(e).despawn();
            }
        } else {
            commands.spawn((
                StatusRoot,
                Text::new(""),
                TextFont { font_size: FontSize::Px(15.0), ..default() },
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(470.0),
                    top: Val::Px(150.0),
                    padding: UiRect::all(Val::Px(10.0)),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.04, 0.04, 0.07, 0.9)),
            ));
        }
    }
    if !*open {
        return;
    }
    let s = *stats;
    let t = &s.template;
    let body = format!(
        "{}  Lv {}\n\
         HP {:.0} / {:.0}    MP {:.0} / {:.0}\n\
         SP {}\n\n\
         P. Atk.   {:>6.0}    M. Atk.   {:>6.0}\n\
         P. Def.   {:>6.0}    M. Def.   {:>6.0}\n\
         Accuracy  {:>6.0}    Evasion   {:>6.0}\n\
         Critical  {:>6.0}    Speed     {:>6.0}\n\
         Atk. Spd. {:>6.0}    Casting   {:>6.0}\n\n\
         STR {:.0}  DEX {:.0}  CON {:.0}\n\
         INT {:.0}  WIT {:.0}  MEN {:.0}",
        dp.class_name(t.class_id),
        s.level,
        s.hp,
        s.max_hp,
        s.mp,
        s.max_mp,
        s.sp,
        s.patk,
        s.matk,
        s.pdef,
        s.mdef,
        s.accuracy,
        s.evasion,
        s.crit,
        s.run_speed,
        s.patk_speed,
        s.matk_speed,
        t.str_,
        t.dex,
        t.con,
        t.int,
        t.wit,
        t.men
    );
    for mut tx in &mut text {
        if tx.0 != body {
            tx.0 = body.clone();
        }
    }
}
