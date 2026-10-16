//! Where a skill's client effects go. Each `L2EffectEmitter` of a skill stage
//! (`gamedata::FxSpec`) names an effect class and places it on the caster or
//! the target: at the collision cylinder (offsets count radii and half
//! heights), on a hand or arm bone, following the pawn (`AM_Trail`) or left
//! where it was spawned. A shot spawned on the caster flies to the target
//! over the skill's `FlyingTime`.

use bevy::prelude::*;

use crate::effects::{EffectSpawn, Flight};
use crate::gamedata::FxSpec;
use crate::l2assets::UU_TO_M;

/// A character an effect can sit on.
#[derive(Clone, Copy)]
pub struct Pawn {
    /// Root entity (feet, facing).
    pub entity: Entity,
    /// The skeletal model, for hand and arm bones.
    pub actor: Option<Entity>,
    pub feet: Vec3,
    pub rotation: Quat,
    /// Collision cylinder, meters.
    pub radius: f32,
    pub half_height: f32,
}

impl Pawn {
    pub fn center(&self) -> Vec3 {
        self.feet + Vec3::Y * self.half_height
    }
}

/// Bones of `AttachOn` 2–5 (right / left hand, right / left arm).
fn bones(attach: u8) -> &'static [&'static str] {
    match attach {
        2 => &["weapon_r_bone", "bip01_r_hand", "bip01 r hand"],
        3 => &["weapon_l_bone", "bip01_l_hand", "bip01 l hand"],
        4 => &["bip01_r_forearm", "bip01 r forearm"],
        5 => &["bip01_l_forearm", "bip01 l forearm"],
        _ => &[],
    }
}

/// Facing from `from` toward `to` on the ground plane.
pub fn facing(from: Vec3, to: Vec3) -> Option<Quat> {
    let way = (to - from).with_y(0.0);
    (way.length_squared() > 1e-6).then(|| Quat::from_rotation_y((-way.z).atan2(way.x)))
}

/// How to spawn the effect of `spec` cast by `caster` on `target`.
/// `bone_of` finds a bone joint of a model entity.
pub fn place(spec: &FxSpec, caster: &Pawn, target: Option<&Pawn>, bone_of: &dyn Fn(Entity, &[&str]) -> Option<Entity>) -> EffectSpawn {
    let host = if spec.on_target { target.unwrap_or(caster) } else { caster };
    // Effects on someone else face the way the skill came.
    let rotation = match target {
        Some(t) if spec.on_target && t.entity != caster.entity => facing(caster.feet, t.feet).unwrap_or(host.rotation),
        _ => host.rotation,
    };
    let o = spec.offset;
    let local = if spec.cylinder {
        Vec3::new(o[0] * host.radius, o[2] * host.half_height, o[1] * host.radius)
    } else {
        Vec3::new(o[0], o[2], o[1]) * UU_TO_M
    };
    let from_feet = Vec3::Y * host.half_height + local;
    let at = Transform::from_translation(host.feet + rotation * from_feet).with_rotation(rotation);
    match spec.attach {
        2..=5 => match host.actor.and_then(|a| bone_of(a, bones(spec.attach))) {
            Some(bone) => EffectSpawn { follow: Some(bone), follow_offset: Vec3::new(o[0], o[2], o[1]) * UU_TO_M, ..EffectSpawn::at(at) },
            None => EffectSpawn::at(at),
        },
        // AM_Trail: rides along with the pawn.
        9 => EffectSpawn { follow: Some(host.entity), follow_offset: from_feet, ..EffectSpawn::at(at) },
        _ => EffectSpawn::at(at),
    }
}

/// Like `place` for a shot; one spawned on the caster flies to the target's
/// chest in `flying_time` seconds.
pub fn place_shot(
    spec: &FxSpec,
    caster: &Pawn,
    target: Option<&Pawn>,
    flying_time: f32,
    bone_of: &dyn Fn(Entity, &[&str]) -> Option<Entity>,
) -> EffectSpawn {
    let mut s = place(spec, caster, target, bone_of);
    if let Some(t) = target.filter(|t| is_projectile(spec, caster, Some(t), flying_time)) {
        // A projectile leaves from the caster's chest, not a moving bone.
        s.follow = None;
        if let Some(r) = facing(caster.feet, t.feet) {
            s.transform.rotation = r;
        }
        s.flight = Some(Flight { target: t.entity, aim: Vec3::Y * t.half_height, time: flying_time });
    }
    s
}

/// A shot that flies: spawned on the caster, with a flying time and someone
/// else to fly to.
pub fn is_projectile(spec: &FxSpec, caster: &Pawn, target: Option<&Pawn>, flying_time: f32) -> bool {
    !spec.on_target && flying_time > 0.0 && target.is_some_and(|t| t.entity != caster.entity)
}
