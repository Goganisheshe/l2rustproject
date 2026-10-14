//! Level BSP (`UModel`): the brush geometry towns and dungeons are built of.
//!
//! Layout as found in Lineage II maps (UE2 build): UPrimitive bounds, then
//! Vectors, Points, Nodes, Surfs, Verts arrays. Each node with vertices is a
//! convex polygon on one surface; the surface gives material, flags and the
//! texture axes for UVs.

use crate::props::read_object;
use crate::{Error, ObjectRef, Package, Reader};

pub const PF_INVISIBLE: u32 = 0x0000_0001;
pub const PF_MASKED: u32 = 0x0000_0002;
pub const PF_TRANSLUCENT: u32 = 0x0000_0004;
pub const PF_NOT_SOLID: u32 = 0x0000_0008;
pub const PF_FAKE_BACKDROP: u32 = 0x0000_0080;
pub const PF_TWO_SIDED: u32 = 0x0000_0100;
pub const PF_PORTAL: u32 = 0x0400_0000;

#[derive(Debug, Clone)]
pub struct BspPoly {
    /// Unreal units.
    pub verts: Vec<[f32; 3]>,
    pub material: ObjectRef,
    pub flags: u32,
    pub base: [f32; 3],
    pub normal: [f32; 3],
    pub tex_u: [f32; 3],
    pub tex_v: [f32; 3],
}

impl BspPoly {
    pub fn visible(&self) -> bool {
        self.flags & (PF_INVISIBLE | PF_FAKE_BACKDROP | PF_PORTAL) == 0
    }

    pub fn solid(&self) -> bool {
        self.flags & (PF_NOT_SOLID | PF_PORTAL | PF_FAKE_BACKDROP) == 0
    }

    /// Texel coordinates of a vertex (divide by the texture size for UVs).
    pub fn texel(&self, p: [f32; 3]) -> [f32; 2] {
        let d = [p[0] - self.base[0], p[1] - self.base[1], p[2] - self.base[2]];
        let dot = |a: [f32; 3]| d[0] * a[0] + d[1] * a[1] + d[2] * a[2];
        [dot(self.tex_u), dot(self.tex_v)]
    }
}

fn vec3(r: &mut Reader) -> Result<[f32; 3], Error> {
    Ok([r.f32()?, r.f32()?, r.f32()?])
}

fn vec_array(r: &mut Reader) -> Result<Vec<[f32; 3]>, Error> {
    let n = r.compact()?.max(0) as usize;
    if n * 12 > r.remaining() {
        return Err(Error::Format("bad vector array".into()));
    }
    (0..n).map(|_| vec3(r)).collect()
}

/// The level's main BSP: the largest `Model` export of a map package.
pub fn level_model(pkg: &Package) -> Option<usize> {
    (0..pkg.exports.len())
        .filter(|&i| pkg.class_name(&pkg.exports[i]) == "Model")
        .max_by_key(|&i| pkg.exports[i].serial_size)
}

pub fn read_bsp(pkg: &Package, export: usize) -> Result<Vec<BspPoly>, Error> {
    let (_, start) = read_object(pkg, export)?;
    let data = pkg.export_data(export);
    let mut r = Reader::at(data, start);
    r.skip(25 + 16)?; // UPrimitive bounds
    let vectors = vec_array(&mut r)?;
    let points = vec_array(&mut r)?;

    struct Node {
        vert_pool: usize,
        surf: usize,
        num_verts: usize,
    }
    let nn = r.compact()?.max(0) as usize;
    let mut nodes = Vec::with_capacity(nn);
    for _ in 0..nn {
        r.skip(16 + 8 + 1)?; // plane, zone mask, flags
        let vert_pool = r.compact()?.max(0) as usize;
        let surf = r.compact()?.max(0) as usize;
        for _ in 0..5 {
            r.compact()?; // back, front, plane, collision bound, render bound
        }
        r.skip(16 + 16 + 2)?; // bound sphere, unknown, zones
        let num_verts = r.u8()? as usize;
        r.skip(8 + 12)?; // leaves, section/lightmap data
        nodes.push(Node { vert_pool, surf, num_verts });
    }

    struct Surf {
        material: ObjectRef,
        flags: u32,
        base: usize,
        normal: usize,
        tex_u: usize,
        tex_v: usize,
    }
    let ns = r.compact()?.max(0) as usize;
    let mut surfs = Vec::with_capacity(ns);
    for _ in 0..ns {
        let material = ObjectRef::from_raw(r.compact()?);
        let flags = r.u32()?;
        let base = r.compact()?.max(0) as usize;
        let normal = r.compact()?.max(0) as usize;
        let tex_u = r.compact()?.max(0) as usize;
        let tex_v = r.compact()?.max(0) as usize;
        r.compact()?; // brush poly
        r.compact()?; // actor
        r.skip(16 + 4 + 4)?; // plane, lightmap scale, lightmap index
        surfs.push(Surf { material, flags, base, normal, tex_u, tex_v });
    }

    let nv = r.compact()?.max(0) as usize;
    let mut verts = Vec::with_capacity(nv);
    for _ in 0..nv {
        verts.push(r.compact()?.max(0) as usize);
        r.compact()?; // side
    }

    let get = |a: &[[f32; 3]], i: usize| a.get(i).copied().unwrap_or([0.0; 3]);
    let mut out = Vec::new();
    for n in &nodes {
        if n.num_verts < 3 {
            continue;
        }
        let Some(s) = surfs.get(n.surf) else { continue };
        let vs: Vec<[f32; 3]> =
            (0..n.num_verts).filter_map(|k| verts.get(n.vert_pool + k).map(|&p| get(&points, p))).collect();
        if vs.len() < 3 {
            continue;
        }
        out.push(BspPoly {
            verts: vs,
            material: s.material,
            flags: s.flags,
            base: get(&points, s.base),
            normal: get(&vectors, s.normal),
            tex_u: get(&vectors, s.tex_u),
            tex_v: get(&vectors, s.tex_v),
        });
    }
    Ok(out)
}
