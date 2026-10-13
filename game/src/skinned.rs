//! Skinned, animated Lineage II models (SkeletalMesh + MeshAnimation).
//!
//! Each model instance gets its own joint entities; a small sampler drives
//! them from the original animation tracks.

use std::collections::HashMap;
use std::sync::Arc;

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::DynamicSkinnedMeshBounds;
use bevy::mesh::skinning::{SkinnedMesh, SkinnedMeshInverseBindposes};
use bevy::mesh::{Indices, PrimitiveTopology, VertexAttributeValues};
use bevy::prelude::*;

use crate::l2assets::{L2Library, UU_TO_M};

pub struct SkinnedPlugin;

impl Plugin for SkinnedPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ModelCache>().add_systems(PostUpdate, animate_models.before(TransformSystems::Propagate));
    }
}

/// How stored bone quaternions are interpreted. UE2/ActorX keep child bone
/// rotations conjugated relative to the root; `L2_QUAT_MODE` lets us flip
/// that while validating against the original client.
fn quat_mode() -> u32 {
    std::env::var("L2_QUAT_MODE").ok().and_then(|v| v.parse().ok()).unwrap_or(0)
}

fn ue_pos(p: [f32; 3]) -> Vec3 {
    Vec3::new(p[0], p[2], p[1]) * UU_TO_M
}

/// Stored UE2 bone rotation -> Bevy local rotation.
fn ue_quat(q: [f32; 4], root: bool, mode: u32) -> Quat {
    let conj = match mode {
        0 => !root,
        1 => true,
        2 => false,
        _ => root,
    };
    let (x, y, z, w) = if conj { (-q[0], -q[1], -q[2], q[3]) } else { (q[0], q[1], q[2], q[3]) };
    // Mirror Y<->Z: rotation axes are pseudo-vectors, so they also flip sign.
    Quat::from_xyzw(-x, -z, -y, w).normalize()
}

struct Key {
    times: Vec<f32>,
    rot: Vec<Quat>,
    pos: Vec<Vec3>,
}

pub struct Clip {
    pub name: String,
    pub frames: f32,
    pub rate: f32,
    /// Per mesh bone, the matching track (if the animation drives it).
    tracks: Vec<Option<Key>>,
}

pub struct Model {
    parts: Vec<(Handle<Mesh>, Handle<StandardMaterial>)>,
    bindposes: Handle<SkinnedMeshInverseBindposes>,
    rest: Vec<(usize, Vec3, Quat)>,
    pub clips: Vec<Clip>,
    /// Mesh-to-actor transform plus a lift so the feet touch y = 0.
    actor: Transform,
    /// Bind-pose height in meters.
    pub height: f32,
}

impl Model {
    pub fn clip(&self, name: &str) -> Option<usize> {
        self.clips.iter().position(|c| c.name.eq_ignore_ascii_case(name))
    }
}

#[derive(Resource, Default)]
pub struct ModelCache(HashMap<String, Option<Arc<Model>>>);

#[derive(Component)]
pub struct Animated {
    pub model: Arc<Model>,
    pub clip: usize,
    pub time: f32,
    pub speed: f32,
    /// When false the clip holds its last frame (e.g. death).
    pub looping: bool,
    joints: Vec<Entity>,
}

impl Animated {
    /// Switches to the first clip found among `names` (case-insensitive).
    pub fn play(&mut self, names: &[&str], looping: bool) {
        if let Some(i) = names.iter().find_map(|n| self.model.clip(n)) {
            if i != self.clip || self.looping != looping {
                self.clip = i;
                self.time = 0.0;
                self.looping = looping;
            }
        }
    }
}

impl ModelCache {
    /// Loads `package:mesh` once; `anim` overrides the mesh's default animation.
    pub fn load(
        &mut self,
        lib: &mut L2Library,
        package: &str,
        mesh_name: &str,
        anim: Option<(&str, &str)>,
        meshes: &mut Assets<Mesh>,
        materials: &mut Assets<StandardMaterial>,
        images: &mut Assets<Image>,
        bindposes: &mut Assets<SkinnedMeshInverseBindposes>,
    ) -> Option<Arc<Model>> {
        let key = format!("{package}:{mesh_name}");
        if let Some(m) = self.0.get(&key) {
            return m.clone();
        }
        let model = build_model(lib, package, mesh_name, anim, meshes, materials, images, bindposes).map(Arc::new);
        if model.is_none() {
            warn!("could not load skeletal mesh {key}");
        }
        self.0.insert(key, model.clone());
        model
    }
}

#[allow(clippy::too_many_arguments)]
fn build_model(
    lib: &mut L2Library,
    package: &str,
    mesh_name: &str,
    anim: Option<(&str, &str)>,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
    bindposes: &mut Assets<SkinnedMeshInverseBindposes>,
) -> Option<Model> {
    let l = lib.lib.as_mut()?;
    let h = l.find(package, mesh_name)?;
    let sm = match ue2::skel::read_skeletal_mesh(&h.package, h.export) {
        Ok(m) => m,
        Err(e) => {
            warn!("{package}:{mesh_name}: {e}");
            return None;
        }
    };
    let mode = quat_mode();

    // Rest pose in Bevy space.
    let rest: Vec<(usize, Vec3, Quat)> = sm
        .bones
        .iter()
        .enumerate()
        .map(|(i, b)| (if i == 0 { 0 } else { b.parent }, ue_pos(b.translation), ue_quat(b.rotation, i == 0, mode)))
        .collect();
    let mut global: Vec<Mat4> = Vec::with_capacity(rest.len());
    for (i, (parent, t, r)) in rest.iter().enumerate() {
        let local = Mat4::from_rotation_translation(*r, *t);
        global.push(if i == 0 { local } else { global[*parent] * local });
    }
    let inverse: Vec<Mat4> = global.iter().map(|m| m.inverse()).collect();

    let positions: Vec<[f32; 3]> = sm.positions.iter().map(|&p| ue_pos(p).to_array()).collect();
    let normals: Vec<[f32; 3]> = sm.normals.iter().map(|&n| Vec3::new(n[0], n[2], n[1]).to_array()).collect();

    // Actor transform: RotOrigin yaw and MeshOrigin. Unreal yaw turns +X toward +Y,
    // which after the Y/Z mirror is a negative turn about Bevy's Y axis.
    let yaw = -(sm.rot_origin[1] as f32) * std::f32::consts::TAU / 65536.0;
    if sm.rot_origin[0] != 0 || sm.rot_origin[2] != 0 {
        debug!("{mesh_name}: ignoring pitch/roll {:?}", sm.rot_origin);
    }
    let scale = Vec3::new(sm.scale[0], sm.scale[2], sm.scale[1]);
    let rot = Quat::from_rotation_y(yaw);
    let origin = ue_pos(sm.origin);
    let mut actor = Transform { translation: rot * (scale * -origin), rotation: rot, scale };
    let min_y = positions
        .iter()
        .map(|p| actor.transform_point(Vec3::from_array(*p)).y)
        .fold(f32::INFINITY, f32::min);
    let max_y = positions
        .iter()
        .map(|p| actor.transform_point(Vec3::from_array(*p)).y)
        .fold(f32::NEG_INFINITY, f32::max);
    if min_y.is_finite() {
        actor.translation.y -= min_y;
    }
    let height = max_y - min_y;

    let mut parts = Vec::new();
    let pkg = h.package.clone();
    for s in &sm.sections {
        let range = s.first_index as usize..(s.first_index + s.num_indices) as usize;
        let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD);
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions.clone());
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals.clone());
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, sm.uvs.clone());
        mesh.insert_attribute(Mesh::ATTRIBUTE_JOINT_INDEX, VertexAttributeValues::Uint16x4(sm.joints.clone()));
        mesh.insert_attribute(Mesh::ATTRIBUTE_JOINT_WEIGHT, sm.weights.clone());
        mesh.insert_indices(Indices::U32(sm.indices[range].to_vec()));
        let Ok(mesh) = mesh.with_generated_skinned_mesh_bounds() else { continue };
        let mat_ref = sm.materials.get(s.material).copied().unwrap_or_default();
        let material = lib.material(&pkg, mat_ref, materials, images);
        parts.push((meshes.add(mesh), material));
    }

    // Animation: explicit override, else the mesh's own reference.
    let l = lib.lib.as_mut()?;
    let anim_handle = match anim {
        Some((p, a)) => l.find(p, a),
        None => l.resolve(&pkg, sm.animation),
    };
    let mut clips = Vec::new();
    if let Some(ah) = anim_handle {
        match ue2::anim::read_animation(&ah.package, ah.export) {
            Ok(a) => {
                let by_name: HashMap<String, usize> =
                    a.bones.iter().enumerate().map(|(i, n)| (n.to_ascii_lowercase(), i)).collect();
                let map: Vec<Option<usize>> =
                    sm.bones.iter().map(|b| by_name.get(&b.name.to_ascii_lowercase()).copied()).collect();
                for seq in &a.sequences {
                    let tracks = map
                        .iter()
                        .enumerate()
                        .map(|(bone, ti)| {
                            let t = seq.tracks.get((*ti)?)?;
                            Some(Key {
                                times: t.times.clone(),
                                rot: t.rotations.iter().map(|&q| ue_quat(q, bone == 0, mode)).collect(),
                                pos: t.translations.iter().map(|&p| ue_pos(p)).collect(),
                            })
                        })
                        .collect();
                    clips.push(Clip { name: seq.name.clone(), frames: seq.num_frames as f32, rate: seq.rate, tracks });
                }
            }
            Err(e) => warn!("animation for {mesh_name}: {e}"),
        }
    }
    info!(
        "model {package}:{mesh_name}: {} verts, {} bones, {} clips, {height:.2} m tall",
        sm.positions.len(),
        sm.bones.len(),
        clips.len()
    );

    Some(Model { parts, bindposes: bindposes.add(SkinnedMeshInverseBindposes::from(inverse)), rest, clips, actor, height })
}

/// Spawns a model instance under `parent` and returns the actor entity.
pub fn spawn_model(commands: &mut Commands, model: &Arc<Model>, parent: Entity, clip: &str) -> Entity {
    let actor = commands.spawn((model.actor, Visibility::default(), ChildOf(parent))).id();
    let mut joints: Vec<Entity> = Vec::with_capacity(model.rest.len());
    for (i, (p, t, r)) in model.rest.iter().enumerate() {
        let parent_e = if i == 0 { actor } else { joints[*p] };
        let e = commands.spawn((Transform::from_translation(*t).with_rotation(*r), ChildOf(parent_e))).id();
        joints.push(e);
    }
    for (mesh, mat) in &model.parts {
        commands.spawn((
            Mesh3d(mesh.clone()),
            MeshMaterial3d(mat.clone()),
            SkinnedMesh { inverse_bindposes: model.bindposes.clone(), joints: joints.clone() },
            DynamicSkinnedMeshBounds,
            ChildOf(actor),
        ));
    }
    let clip_idx = model.clip(clip).unwrap_or(0);
    commands.entity(actor).insert(Animated {
        model: model.clone(),
        clip: clip_idx,
        time: 0.0,
        speed: 1.0,
        looping: true,
        joints,
    });
    actor
}

fn sample(key: &Key, frame: f32, frames: f32) -> (Option<Vec3>, Quat) {
    let n = key.times.len();
    let pick = |v: usize, len: usize| v.min(len.saturating_sub(1));
    if n <= 1 || key.rot.len() <= 1 {
        return (key.pos.first().copied(), key.rot.first().copied().unwrap_or(Quat::IDENTITY));
    }
    let k = key.times.iter().rposition(|&t| t <= frame).unwrap_or(0);
    let (next, t1) = if k + 1 < n { (k + 1, key.times[k + 1]) } else { (0, frames.max(key.times[k] + 1e-3)) };
    let t0 = key.times[k];
    let a = if t1 > t0 { ((frame - t0) / (t1 - t0)).clamp(0.0, 1.0) } else { 0.0 };
    let rot = key.rot[pick(k, key.rot.len())].slerp(key.rot[pick(next, key.rot.len())], a);
    let pos = match key.pos.len() {
        0 => None,
        1 => Some(key.pos[0]),
        len => Some(key.pos[pick(k, len)].lerp(key.pos[pick(next, len)], a)),
    };
    (pos, rot)
}

fn animate_models(time: Res<Time>, mut models: Query<&mut Animated>, mut joints: Query<&mut Transform>) {
    for mut anim in &mut models {
        let model = anim.model.clone();
        let Some(clip) = model.clips.get(anim.clip) else { continue };
        let frames = clip.frames.max(1.0);
        let rate = if clip.rate > 0.0 { clip.rate } else { 30.0 };
        let t = anim.time + time.delta_secs() * rate * anim.speed;
        let t = if anim.looping { t % frames } else { t.min(frames - 0.01) };
        anim.time = t;
        for (bone, key) in clip.tracks.iter().enumerate() {
            let Some(key) = key else { continue };
            let Ok(mut tr) = joints.get_mut(anim.joints[bone]) else { continue };
            let (pos, rot) = sample(key, t, frames);
            tr.rotation = rot;
            if let Some(p) = pos {
                tr.translation = p;
            }
        }
    }
}
