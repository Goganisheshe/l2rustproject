//! Model viewer: `L2_MODEL=<package>:<mesh>[:<clip>]` shows one skeletal mesh
//! instead of the game. Used to validate meshes and animations.

use bevy::mesh::skinning::SkinnedMeshInverseBindposes;
use bevy::prelude::*;

use crate::l2assets::L2Library;
use crate::skinned::{spawn_model, ModelCache};

pub struct ViewerPlugin {
    pub spec: String,
}

#[derive(Resource)]
struct Spec(String);

impl Plugin for ViewerPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(Spec(self.spec.clone()))
            .add_systems(Startup, setup)
            .add_systems(Update, (orbit, replay_effect));
    }
}

#[allow(clippy::too_many_arguments)]
fn setup(
    mut commands: Commands,
    spec: Res<Spec>,
    mut lib: ResMut<L2Library>,
    mut cache: ResMut<ModelCache>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut bindposes: ResMut<Assets<SkinnedMeshInverseBindposes>>,
) {
    // `class:<prefix>[:clip]` shows an assembled player character.
    let parts: Vec<&str> = spec.0.split(':').collect();
    let (pkg, mesh, clip) = match (parts[0], parts.get(1).and_then(|k| crate::character::find_class(k))) {
        ("class", Some(i)) => {
            let c = &crate::character::CLASSES[i];
            (c.package.to_string(), crate::character::part_spec(&mut lib, c), parts.get(2).copied().unwrap_or("wait_1HS"))
        }
        _ => (
            parts[0].to_string(),
            parts.get(1).copied().unwrap_or("").to_string(),
            parts.get(2).copied().unwrap_or("wait"),
        ),
    };
    let (pkg, mesh) = (pkg.as_str(), mesh.as_str());
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(20.0, 20.0))),
        MeshMaterial3d(materials.add(Color::srgb(0.3, 0.32, 0.3))),
    ));
    commands.spawn((
        DirectionalLight { illuminance: 8000.0, shadow_maps_enabled: true, ..default() },
        Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -0.9, 0.6, 0.0)),
    ));
    commands.spawn((Camera3d::default(), Transform::from_xyz(0.0, 2.0, 6.0).looking_at(Vec3::Y, Vec3::Y)));
    let root = commands.spawn((Transform::default(), Visibility::default())).id();
    if let Some(model) =
        cache.load(&mut lib, pkg, mesh, None, &mut meshes, &mut materials, &mut images, &mut bindposes)
    {
        let names: Vec<&str> = model.clips.iter().map(|c| c.name.as_str()).collect();
        info!("clips: {}", names.join(", "));
        let weapon = parts.get(1).and_then(|k| crate::character::find_class(k)).filter(|_| parts[0] == "class");
        let attach: Vec<crate::skinned::Attachment> = weapon
            .and_then(|i| {
                let spec = crate::character::weapon_spec(&crate::character::CLASSES[i]);
                lib.rigid_mesh("lineageweapons", &spec, &mut meshes, &mut materials, &mut images)
            })
            .map(|parts| vec![crate::skinned::Attachment { bone: "Weapon_R_Bone", parts }])
            .unwrap_or_default();
        spawn_model(&mut commands, &model, root, clip, &attach);
    }
}

/// Slowly circles the camera; `L2_VIEW_ANGLE` (degrees) freezes it instead.
fn orbit(time: Res<Time>, mut cam: Single<&mut Transform, With<Camera3d>>) {
    let a = std::env::var("L2_VIEW_ANGLE")
        .ok()
        .and_then(|v| v.parse::<f32>().ok())
        .map(f32::to_radians)
        .unwrap_or(time.elapsed_secs() * 0.4);
    let dist = std::env::var("L2_VIEW_DIST").ok().and_then(|v| v.parse().ok()).unwrap_or(5.0);
    let h: f32 = std::env::var("L2_VIEW_HEIGHT").ok().and_then(|v| v.parse().ok()).unwrap_or(1.0);
    cam.translation = Vec3::new(a.sin() * dist, h + dist * 0.15, a.cos() * dist);
    cam.look_at(Vec3::Y * h, Vec3::Y);
}

/// `L2_EFFECT=<class>` replays one effect class at the origin every 1.5 s,
/// sweeping it along +X to mimic a projectile.
#[allow(clippy::too_many_arguments)]
fn replay_effect(
    mut commands: Commands,
    time: Res<Time>,
    mut timer: Local<f32>,
    mut lib: ResMut<L2Library>,
    mut fx: ResMut<crate::effects::EffectCache>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut movers: Query<(Entity, &mut Transform), With<Mover>>,
) {
    for (e, mut t) in &mut movers {
        t.translation.x += 6.0 * time.delta_secs();
        if t.translation.x > 3.0 {
            commands.entity(e).despawn();
        }
    }
    let Ok(class) = std::env::var("L2_EFFECT") else { return };
    *timer -= time.delta_secs();
    if *timer > 0.0 {
        return;
    }
    *timer = 1.5;
    let Some(asset) = fx.load(&mut lib, &class, &mut meshes, &mut materials, &mut images) else { return };
    let start = Transform::from_xyz(-3.0, 1.0, 0.0);
    let mover = commands.spawn((Mover, start, Visibility::default())).id();
    crate::effects::spawn_effect(&mut commands, &asset, start, Some(mover));
}

#[derive(Component)]
struct Mover;
