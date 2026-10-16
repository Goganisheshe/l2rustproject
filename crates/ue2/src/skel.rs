//! USkeletalMesh decoding (Lineage II flavour of UE2).
//!
//! We read the full-resolution source data (points, wedges, triangles,
//! influences) when present. Newer Lineage packages leave those empty and keep
//! the geometry only in LOD 0 as "Lineage wedges" (position, normal, UV and
//! four bone weights per wedge); we rebuild the mesh from that instead.
//! A few keep their soft-skinned part only in LOD 0's `SkinningData` stream
//! (see `decode_skinning`).

use crate::props::Properties;
use crate::{Error, ObjectRef, Package, Reader};

#[derive(Debug, Clone)]
pub struct Bone {
    pub name: String,
    /// Parent bone index; the root bone points at itself (0).
    pub parent: usize,
    /// Local rotation (x, y, z, w) and translation relative to the parent.
    pub rotation: [f32; 4],
    pub translation: [f32; 3],
}

#[derive(Debug, Clone)]
pub struct SkinSection {
    pub material: usize,
    pub first_index: u32,
    pub num_indices: u32,
}

#[derive(Debug, Clone, Default)]
pub struct SkeletalMesh {
    pub name: String,
    pub bones: Vec<Bone>,
    /// Per-vertex data, already split into wedges (one entry per unique UV).
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    /// Up to four (bone index, weight) pairs per vertex; weights sum to 1.
    pub joints: Vec<[u16; 4]>,
    pub weights: Vec<[f32; 4]>,
    pub indices: Vec<u32>,
    pub sections: Vec<SkinSection>,
    /// Material per section index (`ULodMesh::Textures`).
    pub materials: Vec<ObjectRef>,
    /// Mesh-to-actor transform from the mesh properties.
    pub scale: [f32; 3],
    pub origin: [f32; 3],
    /// Pitch, yaw, roll in Unreal rotation units (65536 = 360 degrees).
    pub rot_origin: [i32; 3],
    pub animation: ObjectRef,
}

fn vec3(r: &mut Reader) -> Result<[f32; 3], Error> {
    Ok([r.f32()?, r.f32()?, r.f32()?])
}

fn count(r: &mut Reader, elem_size: usize) -> Result<usize, Error> {
    let n = r.compact()?;
    if n < 0 || (n as usize).saturating_mul(elem_size.max(1)) > r.remaining() {
        return Err(Error::Format(format!("bad array length {n} at {:#x}", r.pos)));
    }
    Ok(n as usize)
}

fn skip_array(r: &mut Reader, elem_size: usize) -> Result<usize, Error> {
    let n = count(r, elem_size)?;
    r.skip(n * elem_size)?;
    Ok(n)
}

/// TLazyArray: `i32 skip_pos` followed by a normal array.
fn lazy(r: &mut Reader) -> Result<(), Error> {
    r.i32()?;
    Ok(())
}

/// `FSkelMeshSection` of a LOD model.
#[derive(Debug, Clone)]
pub struct Section {
    pub material: u16,
    pub min_stream_index: u16,
    pub min_wedge: u16,
    pub max_wedge: u16,
    pub num_stream_indices: u16,
    pub bone: u16,
    pub unknown: u16,
    pub first_face: u16,
    pub num_faces: u16,
    /// Section bone index -> mesh bone index (Lineage only).
    pub bone_map: Vec<i32>,
}

fn read_sections(r: &mut Reader, lineage_map: bool) -> Result<Vec<Section>, Error> {
    let n = count(r, 18)?;
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        let material = r.u16()?;
        let min_stream_index = r.u16()?;
        let min_wedge = r.u16()?;
        let max_wedge = r.u16()?;
        let num_stream_indices = r.u16()?;
        let bone = r.u16()?;
        let unknown = r.u16()?;
        let first_face = r.u16()?;
        let num_faces = r.u16()?;
        let mut bone_map = Vec::new();
        if lineage_map {
            let m = count(r, 4)?;
            for _ in 0..m {
                bone_map.push(r.i32()?);
            }
        }
        out.push(Section {
            material,
            min_stream_index,
            min_wedge,
            max_wedge,
            num_stream_indices,
            bone,
            unknown,
            first_face,
            num_faces,
            bone_map,
        });
    }
    Ok(out)
}

fn read_u16_array(r: &mut Reader) -> Result<Vec<u16>, Error> {
    let n = count(r, 2)?;
    (0..n).map(|_| r.u16()).collect()
}

pub fn read_skeletal_mesh(pkg: &Package, export: usize) -> Result<SkeletalMesh, Error> {
    let Parsed { mut mesh, lod0, source } = parse(pkg, export)?;
    // Some source data is broken (NaN points in FKamael_m000_l_ad00); LOD 0
    // has the same geometry then.
    let source_ok = source.points.iter().flatten().all(|c| c.is_finite());
    if !source.wedges.is_empty() && !source.tris.is_empty() && (source_ok || lod0.is_none()) {
        build_from_source(&mut mesh, &source.points, &source.wedges, &source.tris, &source.influences);
    } else if let Some(lod) = lod0 {
        build_from_lod(&mut mesh, &lod)?;
    } else {
        return Err(Error::Format(format!("{}: no geometry", mesh.name)));
    }
    Ok(mesh)
}

/// Raw LOD 0 of a skeletal mesh, for inspecting meshes whose soft-skinned
/// part lives only in `SkinningData` (see `l2tool skin`).
#[derive(Debug, Clone)]
pub struct LodDump {
    pub name: String,
    pub bones: Vec<String>,
    pub version: i32,
    pub num_lods: usize,
    pub num_soft_wedges: i32,
    pub skinning: Vec<u32>,
    /// `FSkinPoint`: position and packed normal.
    pub skin_points: Vec<([f32; 3], u32)>,
    pub soft: Vec<Section>,
    pub rigid: Vec<Section>,
    pub soft_indices: Vec<u16>,
    pub rigid_indices: Vec<u16>,
    pub rigid_verts: usize,
    pub lineage_wedges: usize,
    /// Full-resolution source arrays (points, wedges, faces, influences).
    pub source_counts: [usize; 4],
}

impl LodDump {
    /// Soft sections exist but neither Lineage wedges nor source data hold
    /// their vertices: the geometry is only in `SkinningData`.
    pub fn soft_only_in_skinning(&self) -> bool {
        !self.soft.is_empty()
            && self.lineage_wedges == 0
            && (self.source_counts[1] == 0 || self.source_counts[2] == 0)
    }
}

pub fn read_lod_dump(pkg: &Package, export: usize) -> Result<LodDump, Error> {
    let Parsed { mesh, lod0, source } = parse(pkg, export)?;
    let lod = lod0.ok_or_else(|| Error::Format(format!("{}: no LOD models", mesh.name)))?;
    Ok(LodDump {
        name: mesh.name,
        bones: mesh.bones.into_iter().map(|b| b.name).collect(),
        version: source.version,
        num_lods: source.num_lods,
        num_soft_wedges: lod.num_soft_wedges,
        skinning: lod.skinning,
        skin_points: lod.skin_points,
        soft: lod.soft,
        rigid: lod.rigid,
        soft_indices: lod.soft_indices,
        rigid_indices: lod.rigid_indices,
        rigid_verts: lod.rigid_verts.len(),
        lineage_wedges: lod.lineage_wedges.len(),
        source_counts: [source.points.len(), source.wedges.len(), source.tris.len(), source.influences.len()],
    })
}

/// Full-resolution source data that follows the LOD models.
#[derive(Default)]
struct Source {
    version: i32,
    num_lods: usize,
    points: Vec<[f32; 3]>,
    wedges: Vec<(u16, [f32; 2])>,
    tris: Vec<([u16; 3], u8)>,
    influences: Vec<(f32, u16, u16)>,
}

struct Parsed {
    mesh: SkeletalMesh,
    lod0: Option<Lod0>,
    source: Source,
}

fn parse(pkg: &Package, export: usize) -> Result<Parsed, Error> {
    let data = pkg.export_data(export);
    let mut r = Reader::new(data);
    Properties::read(pkg, &mut r)?;
    let ver = pkg.version;
    let lic = pkg.licensee;

    // UPrimitive
    r.skip(25 + 16)?;

    // ULodMesh
    let version = r.i32()?;
    r.i32()?; // vertex count
    if ver >= 133 {
        skip_array(&mut r, 12)?;
    } else {
        skip_array(&mut r, 4)?;
    }
    if version <= 1 {
        skip_array(&mut r, 38)?;
    }
    let n = count(&mut r, 1)?;
    let mut materials = Vec::with_capacity(n);
    for _ in 0..n {
        materials.push(ObjectRef::from_raw(r.compact()?));
    }
    let scale = vec3(&mut r)?;
    let origin = vec3(&mut r)?;
    let rot_origin = [r.i32()?, r.i32()?, r.i32()?];
    if version <= 1 {
        skip_array(&mut r, 2)?;
    }
    skip_array(&mut r, 2)?; // FaceLevel
    skip_array(&mut r, 8)?; // Faces
    skip_array(&mut r, 2)?; // CollapseWedgeThus
    skip_array(&mut r, 10)?; // Wedges
    skip_array(&mut r, 8)?; // Materials
    r.skip(4 * 6)?; // MeshScaleMax, LODHysteresis, LODStrength, LODMinVerts, LODMorph, LODZDisplace
    if version >= 3 {
        r.i32()?; // HasImpostor
        r.compact()?; // SpriteMaterial
        r.skip(12 + 12 + 12 + 4 + 12)?;
    }
    if version >= 4 {
        r.f32()?;
    }
    if version >= 5 {
        r.i32()?;
        if version >= 6 {
            r.u8()?;
        }
    }

    // USkeletalMesh
    skip_array(&mut r, 12)?; // Points2
    let n = count(&mut r, 1)?;
    let mut bones = Vec::with_capacity(n);
    for _ in 0..n {
        let name = pkg.name(r.compact()? as usize).to_string();
        r.u32()?; // flags
        let q = [r.f32()?, r.f32()?, r.f32()?, r.f32()?];
        let t = vec3(&mut r)?;
        r.skip(4 + 12)?; // length, size
        r.i32()?; // num children
        let parent = r.i32()?.max(0) as usize;
        bones.push(Bone { name, parent, rotation: q, translation: t });
    }
    let animation = ObjectRef::from_raw(r.compact()?);
    r.i32()?; // skeletal depth
    let n = count(&mut r, 1)?; // WeightIndices
    for _ in 0..n {
        skip_array(&mut r, 2)?;
        r.i32()?;
    }
    skip_array(&mut r, 4)?; // BoneInfluences
    let n = count(&mut r, 1)?; // AttachAliases
    for _ in 0..n {
        r.compact()?;
    }
    let n = count(&mut r, 1)?; // AttachBoneNames
    for _ in 0..n {
        r.compact()?;
    }
    skip_array(&mut r, 48)?; // AttachCoords

    let mesh = SkeletalMesh {
        name: pkg.full_name(ObjectRef::Export(export)),
        bones,
        materials,
        scale,
        origin,
        rot_origin,
        animation,
        ..Default::default()
    };
    if version <= 1 {
        return Err(Error::Format(format!("{}: old skeletal mesh version {version}", mesh.name)));
    }

    // LODModels: only LOD 0 is kept (Lineage wedges / rigid stream)
    let lineage = lic >= 0x1C;
    let n_lods = count(&mut r, 1)?;
    let mut lod0: Option<Lod0> = None;
    for lod in 0..n_lods {
        // SkinningData and SkinPoints are kept for LOD 0 only.
        let (skinning, skin_points) = if lod == 0 {
            let n = count(&mut r, 4)?;
            let skinning: Vec<u32> = (0..n).map(|_| r.u32()).collect::<Result<_, _>>()?;
            let n = count(&mut r, 16)?;
            let mut points = Vec::with_capacity(n);
            for _ in 0..n {
                points.push((vec3(&mut r)?, r.u32()?));
            }
            (skinning, points)
        } else {
            skip_array(&mut r, 4)?;
            skip_array(&mut r, 16)?;
            (Vec::new(), Vec::new())
        };
        let num_soft_wedges = r.i32()?;
        let soft = read_sections(&mut r, lineage)?;
        let rigid = read_sections(&mut r, lineage)?;
        let soft_indices = read_u16_array(&mut r)?;
        r.i32()?;
        let rigid_indices = read_u16_array(&mut r)?;
        r.i32()?;
        r.skip(12)?; // vertex stream header
        let n = count(&mut r, 32)?;
        let mut rigid_verts = Vec::with_capacity(n);
        for _ in 0..n {
            rigid_verts.push((vec3(&mut r)?, vec3(&mut r)?, [r.f32()?, r.f32()?]));
        }
        lazy(&mut r)?;
        skip_array(&mut r, 8)?; // VertInfluences
        lazy(&mut r)?;
        skip_array(&mut r, 10)?; // Wedges
        lazy(&mut r)?;
        skip_array(&mut r, 8)?; // Faces
        lazy(&mut r)?;
        skip_array(&mut r, 12)?; // Points
        r.skip(4 * 6)?; // LODDistanceFactor .. f118
        let mut lineage_wedges = Vec::new();
        if lineage {
            r.i32()?; // UseNewWedges
            let n = count(&mut r, 52)?;
            for _ in 0..n {
                let p = vec3(&mut r)?;
                let nrm = vec3(&mut r)?;
                let uv = [r.f32()?, r.f32()?];
                let b = r.bytes(4)?;
                let bones = [b[0], b[1], b[2], b[3]];
                let w = [r.f32()?, r.f32()?, r.f32()?, r.f32()?];
                lineage_wedges.push(LineageWedge { p, n: nrm, uv, bones, w });
            }
        }
        if lod == 0 {
            lod0 = Some(Lod0 {
                soft,
                rigid,
                soft_indices,
                rigid_indices,
                rigid_verts,
                lineage_wedges,
                skinning,
                skin_points,
                num_soft_wedges,
            });
        }
    }

    // Full-resolution source data.
    r.compact()?; // f224
    lazy(&mut r)?;
    let n = count(&mut r, 12)?;
    let mut points = Vec::with_capacity(n);
    for _ in 0..n {
        points.push(vec3(&mut r)?);
    }
    lazy(&mut r)?;
    let n = count(&mut r, 10)?;
    let mut wedges = Vec::with_capacity(n);
    for _ in 0..n {
        let v = r.u16()?;
        wedges.push((v, [r.f32()?, r.f32()?]));
    }
    lazy(&mut r)?;
    let n = count(&mut r, 12)?;
    let mut tris = Vec::with_capacity(n);
    for _ in 0..n {
        let w = [r.u16()?, r.u16()?, r.u16()?];
        let mat = r.u8()?;
        r.u8()?;
        r.u32()?;
        tris.push((w, mat));
    }
    lazy(&mut r)?;
    let n = count(&mut r, 8)?;
    let mut influences = Vec::with_capacity(n);
    for _ in 0..n {
        let w = r.f32()?;
        let p = r.u16()?;
        let b = r.u16()?;
        influences.push((w, p, b));
    }

    let source = Source { version, num_lods: n_lods, points, wedges, tris, influences };
    Ok(Parsed { mesh, lod0, source })
}

struct LineageWedge {
    p: [f32; 3],
    n: [f32; 3],
    uv: [f32; 2],
    bones: [u8; 4],
    w: [f32; 4],
}

struct Lod0 {
    soft: Vec<Section>,
    rigid: Vec<Section>,
    soft_indices: Vec<u16>,
    rigid_indices: Vec<u16>,
    rigid_verts: Vec<([f32; 3], [f32; 3], [f32; 2])>,
    lineage_wedges: Vec<LineageWedge>,
    skinning: Vec<u32>,
    skin_points: Vec<([f32; 3], u32)>,
    num_soft_wedges: i32,
}

/// Keeps the four heaviest influences and normalizes them.
fn pack_weights(list: &mut Vec<(f32, u16)>) -> ([u16; 4], [f32; 4]) {
    list.sort_by(|a, b| b.0.total_cmp(&a.0));
    list.truncate(4);
    let sum: f32 = list.iter().map(|x| x.0).sum();
    let mut j = [0u16; 4];
    let mut w = [0f32; 4];
    for (i, (wt, b)) in list.iter().enumerate() {
        j[i] = *b;
        w[i] = if sum > 0.0 { wt / sum } else { 0.0 };
    }
    if sum <= 0.0 {
        w[0] = 1.0;
    }
    (j, w)
}

fn build_from_source(
    mesh: &mut SkeletalMesh,
    points: &[[f32; 3]],
    wedges: &[(u16, [f32; 2])],
    tris: &[([u16; 3], u8)],
    influences: &[(f32, u16, u16)],
) {
    let mut per_point: Vec<Vec<(f32, u16)>> = vec![Vec::new(); points.len()];
    for &(w, p, b) in influences {
        if let Some(v) = per_point.get_mut(p as usize) {
            v.push((w, b));
        }
    }
    if std::env::var_os("L2_SKEL_DEBUG").is_some() {
        let empty = per_point.iter().filter(|v| v.is_empty()).count();
        eprintln!("source mesh: {} points, {} without influences, {} influences", points.len(), empty, influences.len());
    }
    let packed: Vec<_> = per_point.iter_mut().map(pack_weights).collect();

    for &(p, uv) in wedges {
        let p = p as usize;
        mesh.positions.push(points.get(p).copied().unwrap_or_default());
        mesh.uvs.push(uv);
        let (j, w) = packed.get(p).copied().unwrap_or(([0; 4], [1.0, 0.0, 0.0, 0.0]));
        mesh.joints.push(j);
        mesh.weights.push(w);
    }
    // Group triangles by material into sections.
    let max_mat = tris.iter().map(|t| t.1 as usize).max().unwrap_or(0);
    for mat in 0..=max_mat {
        let first = mesh.indices.len() as u32;
        for (w, m) in tris {
            if *m as usize == mat {
                mesh.indices.extend(w.iter().map(|&x| x as u32));
            }
        }
        let n = mesh.indices.len() as u32 - first;
        if n > 0 {
            mesh.sections.push(SkinSection { material: mat, first_index: first, num_indices: n });
        }
    }
    mesh.normals = compute_normals(&mesh.positions, &mesh.indices);
}

fn build_from_lod(mesh: &mut SkeletalMesh, lod: &Lod0) -> Result<(), Error> {
    if lod.lineage_wedges.is_empty() && !lod.soft.is_empty() && !lod.skinning.is_empty() {
        add_skinning_wedges(mesh, lod)?;
    }
    // Soft (skinned) wedges come first, rigid stream vertices after them.
    let soft_count = lod.lineage_wedges.len();
    // Each soft wedge needs its section's bone map; find it through the faces.
    let mut wedge_section = vec![usize::MAX; soft_count];
    for (si, s) in lod.soft.iter().enumerate() {
        for i in 0..s.num_faces as usize * 3 {
            if let Some(&w) = lod.soft_indices.get(s.first_face as usize * 3 + i) {
                if let Some(slot) = wedge_section.get_mut(w as usize) {
                    *slot = si;
                }
            }
        }
    }
    for (i, lw) in lod.lineage_wedges.iter().enumerate() {
        mesh.positions.push(lw.p);
        let len = (lw.n[0] * lw.n[0] + lw.n[1] * lw.n[1] + lw.n[2] * lw.n[2]).sqrt().max(1e-6);
        mesh.normals.push(lw.n.map(|c| c / len));
        mesh.uvs.push(lw.uv);
        let map = lod.soft.get(wedge_section[i]).map(|s| s.bone_map.as_slice()).unwrap_or(&[]);
        let mut list: Vec<(f32, u16)> = (0..4)
            .filter(|&k| lw.bones[k] != 255 && lw.w[k] > 1e-6)
            .map(|k| (lw.w[k], map.get(lw.bones[k] as usize).copied().unwrap_or(0).max(0) as u16))
            .collect();
        if std::env::var_os("L2_SKEL_DEBUG").is_some() {
            let miss = (0..4).any(|k| lw.bones[k] != 255 && lw.w[k] > 1e-6 && map.get(lw.bones[k] as usize).is_none());
            if miss || list.is_empty() {
                eprintln!("wedge {i}: section {} bones {:?} w {:?} map len {}", wedge_section[i] as isize, lw.bones, lw.w, map.len());
            }
        }
        let (j, w) = pack_weights(&mut list);
        mesh.joints.push(j);
        mesh.weights.push(w);
    }
    for s in &lod.soft {
        let first = mesh.indices.len() as u32;
        let start = s.first_face as usize * 3;
        let end = start + s.num_faces as usize * 3;
        let idx = lod.soft_indices.get(start..end).ok_or_else(|| Error::Format("soft section out of range".into()))?;
        mesh.indices.extend(idx.iter().map(|&x| x as u32));
        mesh.sections.push(SkinSection { material: s.material as usize, first_index: first, num_indices: idx.len() as u32 });
    }
    // Wedges decoded from SkinningData carry no normals; derive them from the
    // soft triangles (the only ones so far).
    if mesh.normals.len() < mesh.positions.len() {
        mesh.normals = compute_normals(&mesh.positions, &mesh.indices);
    }

    if std::env::var_os("L2_SKEL_DEBUG").is_some() {
        for s in &lod.rigid {
            eprintln!("rigid section: bone {} faces {} map {:?}", s.bone, s.num_faces, s.bone_map);
        }
        for s in &lod.soft {
            eprintln!("soft section: bone {} faces {} map len {}", s.bone, s.num_faces, s.bone_map.len());
        }
        eprintln!("lineage wedges {}, rigid verts {}", lod.lineage_wedges.len(), lod.rigid_verts.len());
    }
    // Rigid sections: every vertex follows one bone.
    let base = mesh.positions.len() as u32;
    let mut rigid_bone = vec![0u16; lod.rigid_verts.len()];
    for s in &lod.rigid {
        let start = s.first_face as usize * 3;
        for i in 0..s.num_faces as usize * 3 {
            if let Some(&w) = lod.rigid_indices.get(start + i) {
                if let Some(slot) = rigid_bone.get_mut(w as usize) {
                    *slot = s.bone;
                }
            }
        }
    }
    for (i, (p, n, uv)) in lod.rigid_verts.iter().enumerate() {
        mesh.positions.push(*p);
        let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().max(1e-6);
        mesh.normals.push(n.map(|c| c / len));
        mesh.uvs.push(*uv);
        mesh.joints.push([rigid_bone[i], 0, 0, 0]);
        mesh.weights.push([1.0, 0.0, 0.0, 0.0]);
    }
    for s in &lod.rigid {
        let first = mesh.indices.len() as u32;
        let start = s.first_face as usize * 3;
        let end = start + s.num_faces as usize * 3;
        let idx = lod.rigid_indices.get(start..end).ok_or_else(|| Error::Format("rigid section out of range".into()))?;
        mesh.indices.extend(idx.iter().map(|&x| x as u32 + base));
        mesh.sections.push(SkinSection { material: s.material as usize, first_index: first, num_indices: idx.len() as u32 });
    }
    Ok(())
}

/// One soft wedge decoded from `SkinningData`.
#[derive(Debug, Clone, PartialEq)]
struct SkinWedge {
    /// Index into `SkinPoints`.
    point: usize,
    uv: [f32; 2],
    /// (mesh bone index, weight), 1 to 4 entries.
    influences: Vec<(u16, f32)>,
}

/// Decodes `FStaticLODModel::SkinningData`, the software-skinning stream:
/// one record per soft wedge, in wedge order (the order `SoftIndices` uses),
/// ended by `0xFFFFFFFF`. Worked out from the client's own meshes that carry
/// both this stream and Lineage wedges.
///
/// - A new point: 1..=4 influence words, then U and V as `f32`. Each word
///   holds the bone index * 6 in bits 0-11 and the weight * 65535 in bits
///   12-27; the first word's bits 28-29 count the words after it, and its
///   bit 31 puts the point into a cache of shared points. Points are numbered
///   in the order they appear here, which is their order in `SkinPoints`.
/// - A shared point (top nibble `0xF`): its cache slot * 6 in the low bits,
///   then U and V. These are the wedges along UV seams.
///
/// The `* 6` are offsets into the engine's per-bone and per-vertex float
/// buffers (a transformed position and normal per cached point).
fn decode_skinning(data: &[u32], num_points: usize, num_bones: usize) -> Result<Vec<SkinWedge>, Error> {
    let bad = |what: &str, at: usize| Error::Format(format!("SkinningData: {what} at word {at}"));
    let mut out = Vec::new();
    let mut cache: Vec<usize> = Vec::new();
    let mut next_point = 0usize;
    let mut i = 0;
    while let Some(&first) = data.get(i) {
        if first == 0xffff_ffff {
            break;
        }
        let at = i;
        let (point, influences) = if first >> 28 == 0xf {
            let slot = (first & 0x0fff_ffff) as usize;
            if slot % 6 != 0 {
                return Err(bad("unaligned shared point", at));
            }
            let point = *cache.get(slot / 6).ok_or_else(|| bad("unknown shared point", at))?;
            i += 1;
            (point, Vec::new())
        } else {
            let n = 1 + ((first >> 28) & 3) as usize;
            let words = data.get(i..i + n).ok_or_else(|| bad("truncated influences", at))?;
            let mut influences = Vec::with_capacity(n);
            for w in words {
                let bone = (w & 0xfff) as usize;
                if bone % 6 != 0 || bone / 6 >= num_bones.max(1) {
                    return Err(bad("bad bone", at));
                }
                influences.push(((bone / 6) as u16, ((w >> 12) & 0xffff) as f32 / 65535.0));
            }
            if first >> 31 == 1 {
                cache.push(next_point);
            }
            i += n;
            next_point += 1;
            (next_point - 1, influences)
        };
        if point >= num_points {
            return Err(bad("point out of range", at));
        }
        let uv = data.get(i..i + 2).ok_or_else(|| bad("truncated UV", at))?;
        out.push(SkinWedge { point, uv: [f32::from_bits(uv[0]), f32::from_bits(uv[1])], influences });
        i += 2;
    }
    Ok(out)
}

/// Soft wedges from `SkinningData` for meshes without Lineage wedges.
fn add_skinning_wedges(mesh: &mut SkeletalMesh, lod: &Lod0) -> Result<(), Error> {
    let wedges = decode_skinning(&lod.skinning, lod.skin_points.len(), mesh.bones.len())
        .map_err(|e| Error::Format(format!("{}: {e}", mesh.name)))?;
    if wedges.len() != lod.num_soft_wedges.max(0) as usize {
        return Err(Error::Format(format!(
            "{}: SkinningData has {} wedges, NumSoftWedges is {}",
            mesh.name,
            wedges.len(),
            lod.num_soft_wedges
        )));
    }
    // Shared points copy the influences of the point they share.
    let mut by_point: Vec<Option<([u16; 4], [f32; 4])>> = vec![None; lod.skin_points.len()];
    for w in wedges.iter().filter(|w| !w.influences.is_empty()) {
        let mut list: Vec<(f32, u16)> = w.influences.iter().map(|&(b, wt)| (wt, b)).collect();
        by_point[w.point] = Some(pack_weights(&mut list));
    }
    for w in &wedges {
        mesh.positions.push(lod.skin_points[w.point].0);
        mesh.uvs.push(w.uv);
        let (j, wt) = by_point[w.point].unwrap_or(([0; 4], [1.0, 0.0, 0.0, 0.0]));
        mesh.joints.push(j);
        mesh.weights.push(wt);
    }
    Ok(())
}

fn compute_normals(pos: &[[f32; 3]], idx: &[u32]) -> Vec<[f32; 3]> {
    // Accumulate per position (not per wedge) so UV seams stay smooth.
    use std::collections::HashMap;
    let key = |p: &[f32; 3]| (p[0].to_bits(), p[1].to_bits(), p[2].to_bits());
    let mut acc: HashMap<(u32, u32, u32), [f32; 3]> = HashMap::new();
    for t in idx.chunks_exact(3) {
        let (a, b, c) = (pos[t[0] as usize], pos[t[1] as usize], pos[t[2] as usize]);
        let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
        // Unreal is left-handed: v x u points outward for its winding.
        let n = [v[1] * u[2] - v[2] * u[1], v[2] * u[0] - v[0] * u[2], v[0] * u[1] - v[1] * u[0]];
        for p in [a, b, c] {
            let e = acc.entry(key(&p)).or_insert([0.0; 3]);
            e[0] += n[0];
            e[1] += n[1];
            e[2] += n[2];
        }
    }
    pos.iter()
        .map(|p| {
            let n = acc.get(&key(p)).copied().unwrap_or([0.0, 0.0, 1.0]);
            let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().max(1e-9);
            [n[0] / l, n[1] / l, n[2] / l]
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uv(u: f32, v: f32) -> [u32; 2] {
        [u.to_bits(), v.to_bits()]
    }

    /// Influence word: bone, weight 0..=65535, number of words after it, cache flag.
    fn infl(bone: u32, weight: u32, more: u32, cache: bool) -> u32 {
        (cache as u32) << 31 | more << 28 | weight << 12 | bone * 6
    }

    #[test]
    fn decodes_points_influences_and_shared_points() {
        let mut data = vec![infl(2, 0xffff, 0, false)];
        data.extend(uv(0.0, 0.5));
        // Three influences, cached for later.
        data.extend([infl(1, 0xcf5a, 2, true), infl(9, 0x199a, 0, false), infl(6, 0x170b, 0, false)]);
        data.extend(uv(0.25, 0.75));
        data.extend([infl(3, 0x7fff, 1, true), infl(4, 0x8000, 0, false)]);
        data.extend(uv(1.0, 1.0));
        // Shared: cache slot 1 (point 2), then slot 0 (point 1).
        data.push(0xf000_0000 | 6);
        data.extend(uv(0.5, 0.5));
        data.push(0xf000_0000);
        data.extend(uv(0.125, 0.0));
        data.push(0xffff_ffff);
        let w = decode_skinning(&data, 3, 10).unwrap();
        assert_eq!(w.len(), 5);
        assert_eq!(w.iter().map(|w| w.point).collect::<Vec<_>>(), vec![0, 1, 2, 2, 1]);
        assert_eq!(w[0].influences, vec![(2, 1.0)]);
        assert_eq!(w[1].influences.iter().map(|x| x.0).collect::<Vec<_>>(), vec![1, 9, 6]);
        let sum: f32 = w[1].influences.iter().map(|x| x.1).sum();
        assert!((sum - 1.0).abs() < 1e-3);
        assert_eq!(w[3].uv, [0.5, 0.5]);
        assert!(w[3].influences.is_empty());
    }

    #[test]
    fn rejects_broken_streams() {
        // Shared point before anything was cached.
        assert!(decode_skinning(&[0xf000_0000, 0, 0, 0xffff_ffff], 1, 1).is_err());
        // Bone outside the skeleton.
        assert!(decode_skinning(&[infl(5, 0xffff, 0, false), 0, 0], 1, 2).is_err());
        // More points than SkinPoints.
        assert!(decode_skinning(&[infl(0, 0xffff, 0, false), 0, 0, infl(0, 0xffff, 0, false), 0, 0], 1, 1).is_err());
        // Cut off in the middle of a record.
        assert!(decode_skinning(&[infl(0, 0xffff, 1, false)], 1, 1).is_err());
    }

    #[test]
    fn empty_stream_and_missing_terminator() {
        assert!(decode_skinning(&[0xffff_ffff], 0, 0).unwrap().is_empty());
        assert_eq!(decode_skinning(&[infl(0, 0xffff, 0, false), 0, 0], 1, 1).unwrap().len(), 1);
    }
}
