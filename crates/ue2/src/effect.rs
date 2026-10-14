//! Particle effect definitions: Lineage effects are `Emitter` subclasses
//! whose emitters (SpriteEmitter, MeshEmitter, ...) are stored as subobjects
//! of the class, each with tagged properties.

use crate::props::Properties;
use crate::{ObjectRef, Package, Reader};

#[derive(Debug, Clone, PartialEq)]
pub enum EmitterKind {
    Sprite,
    Mesh,
    /// Beam, trail, spark... not rendered yet.
    Other(String),
}

#[derive(Debug, Clone)]
pub struct EmitterDef {
    pub name: String,
    pub kind: EmitterKind,
    pub texture: ObjectRef,
    pub static_mesh: ObjectRef,
    pub max_particles: u32,
    /// Keep emitting forever (`RespawnDeadParticles`).
    pub respawn: bool,
    /// Particles per second.
    pub rate: f32,
    pub initial_delay: [f32; 2],
    pub lifetime: [f32; 2],
    /// Unreal units for sprites, scale factors for meshes.
    pub start_size: [[f32; 2]; 3],
    pub uniform_size: bool,
    /// (relative time, size multiplier), used when non-empty.
    pub size_scale: Vec<(f32, f32)>,
    /// (relative time, RGBA 0..1), used when non-empty.
    pub color_scale: Vec<(f32, [f32; 4])>,
    pub color_multiplier: [[f32; 2]; 3],
    pub fade_in_end: Option<f32>,
    pub fade_out_start: Option<f32>,
    pub velocity: [[f32; 2]; 3],
    pub acceleration: [f32; 3],
    pub location_offset: [f32; 3],
    pub location_range: [[f32; 2]; 3],
    pub spin: bool,
    pub start_spin: [[f32; 2]; 3],
    pub spin_rate: [[f32; 2]; 3],
    pub subdivisions: (u32, u32),
    pub subdivision_range: (u32, u32),
    /// EParticleDrawStyle: 0 regular, 1 alpha blend, 2 modulated, 3 translucent,
    /// 4 alpha modulate, 5 darken, 6 brighten.
    pub draw_style: u8,
    /// Meshes keep the actor's orientation (`UseRotationFrom`).
    pub rotation_from_actor: bool,
}

/// Finds the effect class `class_name` in `pkg` and reads its emitters.
pub fn read_effect(pkg: &Package, class_name: &str) -> Option<Vec<EmitterDef>> {
    let class = (0..pkg.exports.len()).find(|&i| {
        let e = &pkg.exports[i];
        pkg.class_name(e) == "Class" && pkg.name(e.object_name).eq_ignore_ascii_case(class_name)
    })?;
    let mut out = Vec::new();
    for (i, e) in pkg.exports.iter().enumerate() {
        if e.package != ObjectRef::Export(class) {
            continue;
        }
        let class = pkg.class_name(e);
        if !class.ends_with("Emitter") {
            continue;
        }
        let mut r = Reader::new(pkg.export_data(i));
        let Ok(p) = Properties::read(pkg, &mut r) else { continue };
        out.push(emitter(pkg, class, &p, pkg.name(e.object_name)));
    }
    Some(out)
}

fn emitter(pkg: &Package, class: &str, p: &Properties, object: &str) -> EmitterDef {
    let kind = match class {
        "SpriteEmitter" => EmitterKind::Sprite,
        "MeshEmitter" => EmitterKind::Mesh,
        c => EmitterKind::Other(c.to_string()),
    };
    let f = |n: &str, d: f32| p.float(pkg, n).unwrap_or(d);
    let b = |n: &str, d: bool| p.bool(pkg, n).unwrap_or(d);
    let rv = |n: &str, d: [[f32; 2]; 3]| p.range_vector(pkg, n).unwrap_or(d);
    let zero = [[0.0; 2]; 3];

    let max_particles = p.int(pkg, "MaxParticles").unwrap_or(10).max(0) as u32;
    let lifetime = p.range(pkg, "LifetimeRange").unwrap_or([4.0, 4.0]);
    // Engine default: spread MaxParticles evenly over one lifetime.
    let rate = if b("AutomaticInitialSpawning", true) {
        max_particles as f32 / ((lifetime[0] + lifetime[1]) * 0.5).max(0.01)
    } else {
        f("InitialParticlesPerSecond", 0.0)
    };
    let size_scale = if b("UseSizeScale", false) {
        p.struct_array(pkg, "SizeScale")
            .iter()
            .map(|s| (s.float(pkg, "RelativeTime").unwrap_or(0.0), s.float(pkg, "RelativeSize").unwrap_or(1.0)))
            .collect()
    } else {
        Vec::new()
    };
    let color_scale = if b("UseColorScale", false) {
        p.struct_array(pkg, "ColorScale")
            .iter()
            .map(|s| {
                let c = s.raw(pkg, "Color").filter(|c| c.len() >= 4).map(|c| {
                    [c[2] as f32 / 255.0, c[1] as f32 / 255.0, c[0] as f32 / 255.0, c[3] as f32 / 255.0]
                });
                (s.float(pkg, "RelativeTime").unwrap_or(0.0), c.unwrap_or([1.0; 4]))
            })
            .collect()
    } else {
        Vec::new()
    };
    EmitterDef {
        name: p
            .get(pkg, "Name")
            .and_then(|v| match v {
                crate::props::Value::Str(s) => Some(s.clone()),
                _ => None,
            })
            .unwrap_or_else(|| object.to_string()),
        kind,
        texture: p.object(pkg, "Texture").map(ObjectRef::from_raw).unwrap_or_default(),
        static_mesh: p.object(pkg, "StaticMesh").map(ObjectRef::from_raw).unwrap_or_default(),
        max_particles,
        respawn: b("RespawnDeadParticles", true),
        rate,
        initial_delay: p.range(pkg, "InitialDelayRange").unwrap_or([0.0, 0.0]),
        lifetime,
        start_size: rv("StartSizeRange", [[100.0, 100.0]; 3]),
        uniform_size: b("UniformSize", false),
        size_scale,
        color_scale,
        color_multiplier: rv("ColorMultiplierRange", [[1.0, 1.0]; 3]),
        fade_in_end: b("FadeIn", false).then(|| f("FadeInEndTime", 0.0)),
        fade_out_start: b("FadeOut", false).then(|| f("FadeOutStartTime", 0.0)),
        velocity: rv("StartVelocityRange", zero),
        acceleration: p.vector(pkg, "Acceleration").unwrap_or([0.0; 3]),
        location_offset: p.vector(pkg, "StartLocationOffset").unwrap_or([0.0; 3]),
        location_range: rv("StartLocationRange", zero),
        spin: b("SpinParticles", false),
        start_spin: rv("StartSpinRange", zero),
        spin_rate: rv("SpinsPerSecondRange", zero),
        subdivisions: (
            p.int(pkg, "TextureUSubdivisions").unwrap_or(0).max(0) as u32,
            p.int(pkg, "TextureVSubdivisions").unwrap_or(0).max(0) as u32,
        ),
        subdivision_range: (
            p.int(pkg, "SubdivisionStart").unwrap_or(0).max(0) as u32,
            p.int(pkg, "SubdivisionEnd").unwrap_or(0).max(0) as u32,
        ),
        draw_style: p.byte(pkg, "DrawStyle").unwrap_or(3),
        rotation_from_actor: p.byte(pkg, "UseRotationFrom").unwrap_or(0) == 1,
    }
}
