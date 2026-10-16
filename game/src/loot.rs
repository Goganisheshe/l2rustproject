//! Drops on the ground, as in the client: killed monsters scatter their drop
//! list around the corpse with each item's drop model (`*grp.dat`). Click an
//! item, or press Z / gamepad Y, to walk over and pick it up. `L2_AUTOLOOT=1`
//! puts drops straight into the inventory instead.

use std::collections::HashMap;

use bevy::prelude::*;
use rand::Rng;

use crate::gamedata::GameData;
use crate::items::{roll_drops, Inventory, ItemData, ADENA};
use crate::l2assets::{L2Library, MeshPart};
use crate::mmo::{Hero, MoveTarget, Phase};
use crate::rpg::{HeroStats, Log, Npc, NpcState};

/// Seconds a drop stays on the ground.
const LIFETIME: f32 = 300.0;
/// Pick-up reach in meters.
const REACH: f32 = 1.2;

#[derive(Component)]
pub struct GroundItem {
    pub item: u32,
    pub count: u64,
    age: f32,
}

/// The item the hero is walking to.
#[derive(Resource, Default)]
struct Pickup(Option<Entity>);

/// Drop meshes per item id (None: no model, a placeholder is used).
#[derive(Resource, Default)]
struct DropModels(HashMap<u32, Option<Vec<MeshPart>>>);

pub struct LootPlugin;

impl Plugin for LootPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Pickup>()
            .init_resource::<DropModels>()
            .add_systems(Update, (drop_loot, choose_pickup.after(crate::rpg::pick_target), pick_up, expire).chain().run_if(in_state(Phase::InWorld)));
    }
}

fn drop_model(
    item: u32,
    gd: &GameData,
    lib: &mut L2Library,
    models: &mut DropModels,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
) -> Option<Vec<MeshPart>> {
    models
        .0
        .entry(item)
        .or_insert_with(|| {
            let d = gd.drops.get(&item)?;
            let (pkg, name) = d.mesh.split_once('.')?;
            let spec = match d.textures.first().and_then(|t| t.split_once('.')) {
                Some((tp, tn)) => format!("{name}@{tp}/{}", tn.rsplit('.').next().unwrap_or(tn)),
                None => name.to_string(),
            };
            lib.rigid_mesh(pkg, &spec, meshes, materials, images)
        })
        .clone()
}

/// Rolls the drop list of every monster that just died and scatters it.
#[allow(clippy::too_many_arguments)]
fn drop_loot(
    mut commands: Commands,
    data: Res<ItemData>,
    gd: Res<GameData>,
    mut lib: ResMut<L2Library>,
    mut models: ResMut<DropModels>,
    mut assets: (ResMut<Assets<Mesh>>, ResMut<Assets<StandardMaterial>>, ResMut<Assets<Image>>),
    mut log: ResMut<Log>,
    mut inv: Single<&mut Inventory, With<Hero>>,
    mut npcs: Query<(&mut Npc, &Transform)>,
    mut placeholder: Local<Option<(Handle<Mesh>, Handle<StandardMaterial>)>>,
) {
    let auto = std::env::var_os("L2_AUTOLOOT").is_some()
        || (std::env::var_os("L2_AUTOFIGHT").is_some() && std::env::var_os("L2_GROUNDLOOT").is_none());
    let mut rng = rand::rng();
    let (meshes, materials, images) = &mut assets;
    for (mut npc, tr) in &mut npcs {
        if npc.state != NpcState::Dead || npc.looted {
            continue;
        }
        npc.looted = true;
        let Some(rules) = data.drops.get(&npc.template.id) else { continue };
        for (item, count) in roll_drops(rules, &mut rng) {
            if auto {
                *inv.items.entry(item).or_default() += count;
                log.push(pickup_line(&data, item, count));
                continue;
            }
            let a = rng.random_range(0.0..std::f32::consts::TAU);
            let r = rng.random_range(0.3..1.2);
            let pos = tr.translation + Vec3::new(a.cos() * r, 0.05, a.sin() * r);
            let mut rot = Quat::from_rotation_y(rng.random_range(0.0..std::f32::consts::TAU));
            // Weapons are modeled upright; lay them down.
            if gd.weapons.contains_key(&item) {
                rot *= Quat::from_rotation_z(std::f32::consts::FRAC_PI_2);
            }
            let e = commands
                .spawn((GroundItem { item, count, age: 0.0 }, Transform::from_translation(pos).with_rotation(rot), Visibility::default()))
                .id();
            match drop_model(item, &gd, &mut lib, &mut models, meshes, materials, images) {
                Some(parts) => {
                    for p in parts {
                        commands.spawn((Mesh3d(p.mesh), MeshMaterial3d(p.material), ChildOf(e)));
                    }
                }
                None => {
                    let (m, mat) = placeholder
                        .get_or_insert_with(|| {
                            (
                                meshes.add(Cuboid::new(0.15, 0.15, 0.15)),
                                materials.add(StandardMaterial { base_color: Color::srgb(0.8, 0.7, 0.3), ..default() }),
                            )
                        })
                        .clone();
                    commands.spawn((Mesh3d(m), MeshMaterial3d(mat), Transform::from_xyz(0.0, 0.08, 0.0), ChildOf(e)));
                }
            }
        }
    }
}

fn pickup_line(data: &ItemData, item: u32, count: u64) -> String {
    let name = data.items.get(&item).map_or(format!("item {item}"), |t| t.name.clone());
    if item == ADENA {
        format!("You have obtained {count} Adena.")
    } else if count > 1 {
        format!("You have obtained {name} x{count}.")
    } else {
        format!("You have obtained {name}.")
    }
}

/// Click on an item, or Z / gamepad Y for the nearest one, picks a target.
#[allow(clippy::too_many_arguments)]
fn choose_pickup(
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    gamepads: Query<&Gamepad, With<crate::input::ActivePad>>,
    windows: Query<&Window>,
    camera: Single<(&Camera, &GlobalTransform)>,
    hover: Res<bevy::picking::hover::HoverMap>,
    ui_nodes: Query<(), With<Node>>,
    mut pickup: ResMut<Pickup>,
    mut move_to: ResMut<MoveTarget>,
    hero: Single<(&Transform, &mut HeroStats), With<Hero>>,
    items: Query<(Entity, &Transform), With<GroundItem>>,
) {
    let (hero_tr, mut stats) = hero.into_inner();
    let over_ui = hover.values().any(|m| m.keys().any(|e| ui_nodes.contains(*e)));
    let mut chosen = None;
    if mouse.just_pressed(MouseButton::Left) && !over_ui {
        let (cam, cam_tr) = camera.into_inner();
        let cursor = windows.iter().next().and_then(|w| w.cursor_position());
        if let Some(ray) = cursor.and_then(|c| cam.viewport_to_world(cam_tr, c).ok()) {
            chosen = items
                .iter()
                .filter_map(|(e, t)| {
                    let to = t.translation + Vec3::Y * 0.1 - ray.origin;
                    let along = to.dot(*ray.direction);
                    (along > 0.0 && (to - *ray.direction * along).length() < 0.35).then_some((e, along))
                })
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(e, _)| e);
        }
    }
    let key = keys.just_pressed(KeyCode::KeyZ)
        || gamepads.iter().any(|g| {
            g.just_pressed(GamepadButton::North) && !g.pressed(GamepadButton::LeftTrigger) && !g.pressed(GamepadButton::LeftTrigger2)
        });
    if key {
        chosen = items
            .iter()
            .map(|(e, t)| (e, t.translation.distance(hero_tr.translation)))
            .filter(|(_, d)| *d < 15.0)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(e, _)| e);
    }
    if let Some(e) = chosen {
        pickup.0 = Some(e);
        stats.attacking = false;
        if let Ok((_, t)) = items.get(e) {
            move_to.0 = Some(t.translation);
        }
    }
}

fn pick_up(
    mut commands: Commands,
    data: Res<ItemData>,
    mut pickup: ResMut<Pickup>,
    mut log: ResMut<Log>,
    move_to: Res<MoveTarget>,
    hero: Single<(&Transform, &mut Inventory), With<Hero>>,
    items: Query<(&GroundItem, &Transform)>,
) {
    let Some(e) = pickup.0 else { return };
    let Ok((g, t)) = items.get(e) else {
        pickup.0 = None;
        return;
    };
    let (hero_tr, mut inv) = hero.into_inner();
    if hero_tr.translation.with_y(0.0).distance(t.translation.with_y(0.0)) <= REACH {
        *inv.items.entry(g.item).or_default() += g.count;
        log.push(pickup_line(&data, g.item, g.count));
        commands.entity(e).despawn();
        pickup.0 = None;
    } else if move_to.0.is_none() {
        // The walk was cancelled (keys, a fight, a wall).
        pickup.0 = None;
    }
}

fn expire(mut commands: Commands, time: Res<Time>, mut items: Query<(Entity, &mut GroundItem)>) {
    for (e, mut g) in &mut items {
        g.age += time.delta_secs();
        if g.age > LIFETIME {
            commands.entity(e).despawn();
        }
    }
}
