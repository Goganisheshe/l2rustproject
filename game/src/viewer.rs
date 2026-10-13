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
        app.insert_resource(Spec(self.spec.clone())).add_systems(Startup, setup).add_systems(Update, orbit);
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
    let parts: Vec<&str> = spec.0.split(':').collect();
    let (pkg, mesh, clip) = (parts[0], parts.get(1).copied().unwrap_or(""), parts.get(2).copied().unwrap_or("wait"));
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
        spawn_model(&mut commands, &model, root, clip);
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
