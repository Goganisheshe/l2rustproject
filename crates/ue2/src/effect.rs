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

/// Lineage `ControlPoints` entry of a curved beam (`bApllyBezierCurve`).
#[derive(Debug, Clone)]
pub struct BeamControlPoint {
    /// Where along the beam (0..1) the point sits.
    pub ratio: f32,
    /// Fixed offset, in a frame along the beam (X forward).
    pub pivot: [f32; 3],
    /// The point swings by up to this much (same frame) ...
    pub oscillation: [f32; 3],
    /// ... this many times a second.
    pub frequency: f32,
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
    /// Lineage curved beams: a Bezier curve through swinging control points.
    pub bezier: bool,
    pub control_points: Vec<BeamControlPoint>,
    /// Lineage `RenderingDensity`: points along a curved beam.
    pub density: u32,
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
    /// Alpha multiplier of every particle (`Opacity`).
    pub opacity: f32,
    /// `UseDirectionAs` (EParticleDirectionUsage): 0 camera-facing, 1 up
    /// along the velocity, 2 right, 3 facing along the velocity, 4 lying in
    /// the plane of `projection_normal`, 5 up and normal...
    pub direction: u8,
    /// Plane normal of `direction` 4 / 5 (Unreal axes, effect frame).
    pub projection_normal: [f32; 3],
    /// Extra passes through the color / size curves over a lifetime.
    pub color_repeats: f32,
    pub size_repeats: f32,
    /// `StartLocationShape`: 0 box (`location_range`), 1 sphere, 2 polar.
    pub location_shape: u8,
    pub sphere_radius: [f32; 2],
    /// Polar start: X azimuth and Y polar angle in degrees, Z radius.
    pub polar: [[f32; 2]; 3],
    /// `GetVelocityDirectionFrom`: 0 none, 1 away from the emitter, 2 toward
    /// it (both scale the start velocity per axis), 3 add `radial_velocity`.
    pub velocity_from: u8,
    pub radial_velocity: [f32; 2],
    /// Fraction of the velocity lost per second, per axis.
    pub velocity_loss: [[f32; 2]; 3],
    /// Chance per axis that a particle spins counter-clockwise.
    pub spin_ccw: [f32; 3],
    /// Pick a random atlas cell; otherwise the cells play over the lifetime.
    pub random_subdivision: bool,
    /// Particles orbit `revolution_center` (effect frame) at these
    /// revolutions per second per axis.
    pub revolution: Option<([[f32; 2]; 3], [[f32; 2]; 3])>,
    /// `CoordinateSystem`: 0 world space; 1 (the default) and Lineage's 3, 5,
    /// 6, 7 move and turn with the effect; Lineage's 4 moves with it.
    pub coordinate_system: u8,
    /// Meshes keep their own material's blending (`UseMeshBlendMode`).
    pub mesh_blend: bool,
    /// Particles start at the newest particle of this emitter (an index into
    /// the effect's emitters, `AddLocationFromOtherEmitter`).
    pub add_location_from: Option<usize>,
}

/// Rotation of an effect class (`Rotation` default property), pitch, yaw,
/// roll; the effect's emitters turn with it.
pub fn effect_rotation(pkg: &Package, class_name: &str) -> [i32; 3] {
    (0..pkg.exports.len())
        .find(|&i| {
            let e = &pkg.exports[i];
            pkg.class_name(e) == "Class" && pkg.name(e.object_name).eq_ignore_ascii_case(class_name)
        })
        .and_then(|i| crate::props::class_defaults(pkg, i))
        .and_then(|d| d.rotator(pkg, "Rotation"))
        .unwrap_or([0; 3])
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
        d.add_location_from = d.add_location_from.filter(|&a| a != k && a < n);
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
    let mesh = kind == EmitterKind::Mesh;
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
        // MeshEmitter's default is a scale of 1, sprites' 100 UU.
        start_size: rv("StartSizeRange", if mesh { [[1.0, 1.0]; 3] } else { [[100.0, 100.0]; 3] }),
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
        opacity: f("Opacity", 1.0),
        direction: p.byte(pkg, "UseDirectionAs").unwrap_or(0),
        projection_normal: p.vector(pkg, "ProjectionNormal").unwrap_or([0.0, 0.0, 1.0]),
        color_repeats: f("ColorScaleRepeats", 0.0).max(0.0),
        size_repeats: f("SizeScaleRepeats", 0.0).max(0.0),
        location_shape: p.byte(pkg, "StartLocationShape").unwrap_or(0),
        sphere_radius: p.range(pkg, "SphereRadiusRange").unwrap_or([0.0, 0.0]),
        polar: rv("StartLocationPolarRange", zero),
        velocity_from: p.byte(pkg, "GetVelocityDirectionFrom").unwrap_or(0),
        radial_velocity: p.range(pkg, "StartVelocityRadialRange").unwrap_or([0.0, 0.0]),
        velocity_loss: rv("VelocityLossRange", zero),
        spin_ccw: p.vector(pkg, "SpinCCWorCW").unwrap_or([0.5; 3]),
        random_subdivision: b("UseRandomSubdivision", false),
        revolution: b("UseRevolution", false).then(|| (rv("RevolutionCenterOffsetRange", zero), rv("RevolutionsPerSecondRange", zero))),
        coordinate_system: p.byte(pkg, "CoordinateSystem").unwrap_or(1),
        mesh_blend: b("UseMeshBlendMode", true),
        add_location_from: p.int(pkg, "AddLocationFromOtherEmitter").filter(|&a| a >= 0).map(|a| a as usize),
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
        bezier: p.bool(pkg, "bApllyBezierCurve").unwrap_or(false),
        control_points: p
            .struct_array(pkg, "ControlPoints")
            .iter()
            .map(|c| BeamControlPoint {
                ratio: c.float(pkg, "locationratio").unwrap_or(0.5),
                pivot: c.vector(pkg, "pivotLocation").unwrap_or([0.0; 3]),
                oscillation: c.vector(pkg, "oscillationRange").unwrap_or([0.0; 3]),
                frequency: c.float(pkg, "Frequency").unwrap_or(0.0),
            })
            .collect(),
        density: count("RenderingDensity", 2),
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
