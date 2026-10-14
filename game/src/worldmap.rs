//! World map (M) and radar (top right), drawn with the client's own map
//! art: the nine `Minimap.worldmapN` textures of `l2font-ru.utx` form a
//! 3072 px square, mapped to the world at 200 Unreal units per pixel.

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use crate::l2assets::L2Library;
use crate::mmo::{Hero, Phase};
use crate::streaming::bevy_to_ue;

/// Part of the 3072 px canvas that holds the map.
const CROP: (u32, u32, u32, u32) = (1024, 0, 1900, 2620);
/// Radar zoom (screen px per map px) and size.
const RADAR_ZOOM: f32 = 2.5;
const RADAR_SIZE: f32 = 180.0;

#[derive(Resource)]
struct MapArt {
    image: Handle<Image>,
}

/// Map pixel (in the cropped image) of a world position.
fn map_px(ue: [f32; 3]) -> Vec2 {
    Vec2::new(1160.0 - CROP.0 as f32 + (ue[0] + 131072.0) / 200.0, (ue[1] + 262144.0) / 200.0 - CROP.1 as f32)
}

#[derive(Component)]
struct WorldMapRoot;

#[derive(Component)]
struct WorldMapMarker;

#[derive(Component)]
struct RadarImage;

pub struct WorldMapPlugin;

impl Plugin for WorldMapPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(Phase::InWorld), build_map)
            .add_systems(Update, (world_map, radar).run_if(in_state(Phase::InWorld)).run_if(resource_exists::<MapArt>));
    }
}

fn build_map(mut commands: Commands, mut lib: ResMut<L2Library>, mut images: ResMut<Assets<Image>>) {
    let Some(l) = lib.lib.as_mut() else { return };
    let (cw, ch) = (CROP.2, CROP.3);
    let mut data = vec![0u8; (cw * ch * 4) as usize];
    let mut found = 0;
    for i in 0..9u32 {
        let Some(h) = l.find("l2font-ru", &format!("worldmap{}", i + 1)) else { continue };
        let Ok(t) = ue2::texture::read_texture(&h.package, h.export) else { continue };
        found += 1;
        let (ox, oy) = ((i % 3) * 1024, (i / 3) * 1024);
        for y in 0..t.height {
            let cy = (oy + y) as i64 - CROP.1 as i64;
            if cy < 0 || cy >= ch as i64 {
                continue;
            }
            for x in 0..t.width {
                let cx = (ox + x) as i64 - CROP.0 as i64;
                if cx < 0 || cx >= cw as i64 {
                    continue;
                }
                let s = ((y * t.width + x) * 4) as usize;
                let d = ((cy as u32 * cw + cx as u32) * 4) as usize;
                data[d..d + 4].copy_from_slice(&t.rgba[s..s + 4]);
            }
        }
    }
    if found == 0 {
        warn!("world map textures not found");
        return;
    }
    let image = Image::new(
        Extent3d { width: cw, height: ch, depth_or_array_layers: 1 },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    let image = images.add(image);
    // Radar: a clipped window onto the scaled map, centred on the hero.
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                right: Val::Px(12.0),
                top: Val::Px(12.0),
                width: Val::Px(RADAR_SIZE),
                height: Val::Px(RADAR_SIZE),
                overflow: Overflow::clip(),
                border: UiRect::all(Val::Px(2.0)),
                ..default()
            },
            BorderColor::all(Color::srgb(0.45, 0.4, 0.3)),
            BackgroundColor(Color::srgb(0.1, 0.1, 0.1)),
        ))
        .with_children(|p| {
            p.spawn((
                RadarImage,
                ImageNode::new(image.clone()),
                Node {
                    position_type: PositionType::Absolute,
                    width: Val::Px(cw as f32 * RADAR_ZOOM),
                    height: Val::Px(ch as f32 * RADAR_ZOOM),
                    ..default()
                },
            ));
            p.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(RADAR_SIZE / 2.0 - 4.0),
                    top: Val::Px(RADAR_SIZE / 2.0 - 4.0),
                    width: Val::Px(8.0),
                    height: Val::Px(8.0),
                    ..default()
                },
                BackgroundColor(Color::srgb(1.0, 0.25, 0.2)),
            ));
        });
    commands.insert_resource(MapArt { image });
}

fn radar(hero: Single<&Transform, With<Hero>>, mut img: Query<&mut Node, With<RadarImage>>) {
    let p = map_px(bevy_to_ue(hero.translation)) * RADAR_ZOOM;
    for mut n in &mut img {
        n.left = Val::Px(RADAR_SIZE / 2.0 - 2.0 - p.x);
        n.top = Val::Px(RADAR_SIZE / 2.0 - 2.0 - p.y);
    }
}

#[allow(clippy::too_many_arguments)]
fn world_map(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    gamepads: Query<&Gamepad>,
    art: Res<MapArt>,
    hero: Single<&Transform, With<Hero>>,
    roots: Query<Entity, With<WorldMapRoot>>,
    mut marker: Query<&mut Node, With<WorldMapMarker>>,
    mut env_opened: Local<bool>,
) {
    let mut toggle = keys.just_pressed(KeyCode::KeyM) || gamepads.iter().any(|g| g.just_pressed(GamepadButton::LeftThumb));
    if !*env_opened && std::env::var_os("L2_MAP").is_some() {
        *env_opened = true;
        toggle = true;
    }
    let open = !roots.is_empty();
    if (toggle && open) || (open && keys.just_pressed(KeyCode::Escape)) {
        for e in &roots {
            commands.entity(e).despawn();
        }
        return;
    }
    let (cw, ch) = (CROP.2 as f32, CROP.3 as f32);
    let p = map_px(bevy_to_ue(hero.translation));
    if toggle && !open {
        commands
            .spawn((
                WorldMapRoot,
                Node {
                    position_type: PositionType::Absolute,
                    width: Val::Percent(100.0),
                    height: Val::Percent(100.0),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    ..default()
                },
                BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.6)),
            ))
            .with_children(|p0| {
                p0.spawn((
                    ImageNode::new(art.image.clone()),
                    Node { height: Val::Percent(92.0), aspect_ratio: Some(cw / ch), ..default() },
                ))
                .with_children(|m| {
                    m.spawn((
                        WorldMapMarker,
                        Node {
                            position_type: PositionType::Absolute,
                            left: Val::Percent(p.x / cw * 100.0),
                            top: Val::Percent(p.y / ch * 100.0),
                            width: Val::Px(10.0),
                            height: Val::Px(10.0),
                            margin: UiRect::all(Val::Px(-5.0)),
                            ..default()
                        },
                        BackgroundColor(Color::srgb(1.0, 0.2, 0.15)),
                    ));
                });
            });
        return;
    }
    for mut n in &mut marker {
        n.left = Val::Percent(p.x / cw * 100.0);
        n.top = Val::Percent(p.y / ch * 100.0);
    }
}
