//! Particle effect definitions: Lineage effects are `Emitter` subclasses
//! whose emitters (SpriteEmitter, MeshEmitter, ...) are stored as subobjects
//! of the class, each with tagged properties.

use crate::props::Properties;
use crate::{ObjectRef, Package, Reader};

#[derive(Debug, Clone, PartialEq)]
pub enum EmitterKind {
    Sprite,
    Mesh,
    Beam,
    Trail,
    /// Spark and anything else, not rendered.
    Other(String),
}

/// How a beam finds its end (`EBeamEndPointType`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BeamEnd {
    /// Where the particle would be after its lifetime (velocity + acceleration).
    Velocity,
    /// Along the start velocity, `BeamDistanceRange` long.
    Distance,
    /// A random `BeamEndPoints` offset, relative to the emitter.
    Offset,
    /// The actor named by `BeamEndPoints[].ActorTag`; here, the effect's target.
    Actor,
    /// Like `Offset`, stopped by the first wall (not traced here).
    TraceOffset,
    /// `BeamEndPoints` offsets as absolute world locations.
    OffsetAsAbsolute,
    /// Like `Distance`, changed at runtime by script.
    DynamicDistance,
}

impl BeamEnd {
    fn from_byte(b: u8) -> Self {
        match b {
            1 => Self::Distance,
            2 => Self::Offset,
            3 => Self::Actor,
            4 => Self::TraceOffset,
            5 => Self::OffsetAsAbsolute,
            6 => Self::DynamicDistance,
            _ => Self::Velocity,
        }
    }
}

#[derive(Debug, Clone)]
pub struct BeamEndPoint {
    pub offset: [[f32; 2]; 3],
    pub weight: f32,
}

/// `BeamEmitter` settings. Each particle is one beam.
#[derive(Debug, Clone)]
pub struct BeamDef {
    pub end: BeamEnd,
    pub distance: [f32; 2],
    pub end_points: Vec<BeamEndPoint>,
    /// Texture repeats along the beam / across it.
    pub u_scale: f32,
    pub v_scale: f32,
    /// Sheets rotated around the beam axis; 0 or 1 = one sheet.
    pub sheets: u32,
    /// Large bends: control points and their random offset per axis.
    pub lf_points: u32,
    pub lf_noise: [[f32; 2]; 3],
    /// Small bends: polyline points and their random offset per axis.
    pub hf_points: u32,
    pub hf_noise: [[f32; 2]; 3],
    /// Noise may move the end point too.
    pub noise_moves_end: bool,
}

/// `ETrailShadeType`: how brightness varies along a trail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrailShade {
    None,
    RandomStatic,
    RandomDynamic,
    /// Bright at the head, fading to the tail.
    Linear,
    /// Each point fades over `PointLifeTime`.
    PointLife,
}

/// `TrailEmitter` settings. Each particle drags a ribbon of past positions.
#[derive(Debug, Clone)]
pub struct TrailDef {
    pub shade: TrailShade,
    /// `PTTL_FollowEmitter`: the head stays on the emitter instead of moving
    /// with the particle's velocity.
    pub follow_emitter: bool,
    /// Lineage `AttachEmitterIndex`: the head follows the newest particle of
    /// that emitter (an index into the effect's emitters).
    pub attach: Option<usize>,
    /// `CoordinateSystem`; Lineage adds 4, used by trails behind flying effects.
    pub coordinate_system: u8,
    pub max_points: u32,
    /// A new point is added once the head moved this far (UU).
    pub distance_threshold: f32,
    /// Two crossed sheets instead of one camera-facing ribbon.
    pub crossed_sheets: bool,
    /// Seconds a point lives; 0 = until pushed out by `max_points`.
    pub point_life: f32,
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
    pub beam: Option<BeamDef>,
    pub trail: Option<TrailDef>,
    /// `Disabled`: only a script would switch it on.
    pub disabled: bool,
}

/// Finds the effect class `class_name` in `pkg` and reads its emitters.
pub fn read_effect(pkg: &Package, class_name: &str) -> Option<Vec<EmitterDef>> {
    let class = (0..pkg.exports.len()).find(|&i| {
        let e = &pkg.exports[i];
        pkg.class_name(e) == "Class" && pkg.name(e.object_name).eq_ignore_ascii_case(class_name)
    })?;
    let is_emitter = |i: usize| {
        let e = &pkg.exports[i];
        e.package == ObjectRef::Export(class) && pkg.class_name(e).ends_with("Emitter")
    };
    // Emitter indices (`AttachEmitterIndex`, ...) follow the class's
    // `Emitters` array, which is not always the export order.
    let listed: Vec<usize> = crate::props::class_defaults(pkg, class)
        .map(|d| d.object_array(pkg, "Emitters"))
        .unwrap_or_default()
        .into_iter()
        .map_while(|r| match r {
            ObjectRef::Export(i) if i < pkg.exports.len() && is_emitter(i) => Some(i),
            _ => None,
        })
        .collect();
    let exported: Vec<usize> = (0..pkg.exports.len()).filter(|&i| is_emitter(i)).collect();
    let order = if !listed.is_empty() && listed.len() == exported.len() { listed } else { exported };
    let mut out: Vec<EmitterDef> = order
        .iter()
        .map(|&i| {
            let e = &pkg.exports[i];
            let mut r = Reader::new(pkg.export_data(i));
            match Properties::read(pkg, &mut r) {
                Ok(p) => emitter(pkg, pkg.class_name(e), &p, pkg.name(e.object_name)),
                // Keep the slot so later indices still line up.
                Err(_) => EmitterDef { disabled: true, ..emitter(pkg, pkg.class_name(e), &Properties::default(), pkg.name(e.object_name)) },
            }
        })
        .collect();
    let n = out.len();
    for (k, d) in out.iter_mut().enumerate() {
        if let Some(t) = d.trail.as_mut() {
            // Pointing at itself or past the end means no attachment.
            t.attach = t.attach.filter(|&a| a != k && a < n);
            // Lineage's CoordinateSystem 4 on a free trail: it follows the effect.
            t.follow_emitter |= t.attach.is_none() && t.coordinate_system == 4;
        }
    }
    Some(out)
}

fn emitter(pkg: &Package, class: &str, p: &Properties, object: &str) -> EmitterDef {
    let kind = match class {
        "SpriteEmitter" => EmitterKind::Sprite,
        "MeshEmitter" => EmitterKind::Mesh,
        "BeamEmitter" => EmitterKind::Beam,
        "TrailEmitter" => EmitterKind::Trail,
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
    let beam = (kind == EmitterKind::Beam).then(|| beam(pkg, p));
    let trail = (kind == EmitterKind::Trail).then(|| trail(pkg, p));
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
        beam,
        trail,
        disabled: b("Disabled", false),
    }
}

fn beam(pkg: &Package, p: &Properties) -> BeamDef {
    let zero = [[0.0; 2]; 3];
    let end_points = p
        .struct_array(pkg, "BeamEndPoints")
        .iter()
        .map(|s| BeamEndPoint {
            offset: s.range_vector(pkg, "Offset").unwrap_or(zero),
            weight: s.float(pkg, "Weight").unwrap_or(1.0),
        })
        .collect();
    let count = |n: &str, d: i32| p.int(pkg, n).unwrap_or(d).max(0) as u32;
    BeamDef {
        end: BeamEnd::from_byte(p.byte(pkg, "DetermineEndPointBy").unwrap_or(0)),
        distance: p.range(pkg, "BeamDistanceRange").unwrap_or([500.0, 500.0]),
        end_points,
        u_scale: p.float(pkg, "BeamTextureUScale").unwrap_or(1.0),
        v_scale: p.float(pkg, "BeamTextureVScale").unwrap_or(1.0),
        sheets: count("RotatingSheets", 0),
        lf_points: count("LowFrequencyPoints", 3),
        lf_noise: p.range_vector(pkg, "LowFrequencyNoiseRange").unwrap_or(zero),
        hf_points: count("HighFrequencyPoints", 10),
        hf_noise: p.range_vector(pkg, "HighFrequencyNoiseRange").unwrap_or(zero),
        noise_moves_end: p.bool(pkg, "NoiseDeterminesEndPoint").unwrap_or(false),
    }
}

fn trail(pkg: &Package, p: &Properties) -> TrailDef {
    let shade = match p.byte(pkg, "TrailShadeType").unwrap_or(0) {
        1 => TrailShade::RandomStatic,
        2 => TrailShade::RandomDynamic,
        3 => TrailShade::Linear,
        4 => TrailShade::PointLife,
        _ => TrailShade::None,
    };
    TrailDef {
        shade,
        follow_emitter: p.byte(pkg, "TrailLocation").unwrap_or(0) == 1,
        attach: p.int(pkg, "AttachEmitterIndex").filter(|&a| a >= 0).map(|a| a as usize),
        coordinate_system: p.byte(pkg, "CoordinateSystem").unwrap_or(1),
        max_points: p.int(pkg, "MaxPointsPerTrail").unwrap_or(100).max(2) as u32,
        distance_threshold: p.float(pkg, "DistanceThreshold").unwrap_or(2.0).max(0.0),
        crossed_sheets: p.bool(pkg, "UseCrossedSheets").unwrap_or(false),
        point_life: p.float(pkg, "PointLifeTime").unwrap_or(0.0).max(0.0),
    }
}
