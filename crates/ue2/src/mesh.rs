//! UStaticMesh decoding.
//!
//! Layout (UE2, ArVer >= 112): properties, UPrimitive bounds, sections,
//! bounds again, then vertex/color/alpha/UV/index streams. Collision data
//! after the streams is ignored.

use crate::props::{Properties, Value};
use crate::{Error, ObjectRef, Package, Reader};

#[derive(Debug, Clone)]
pub struct Section {
    pub first_index: u32,
    pub num_faces: u32,
    /// Material object reference from the mesh's `Materials` property.
    pub material: ObjectRef,
}

#[derive(Debug, Clone, Default)]
pub struct StaticMesh {
    pub name: String,
    /// Unreal coordinates: X forward, Y right, Z up, 1 unit ~ 1 inch-ish.
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    pub colors: Vec<[u8; 4]>,
    pub indices: Vec<u32>,
    pub sections: Vec<Section>,
}

fn vec3(r: &mut Reader) -> Result<[f32; 3], Error> {
    Ok([r.f32()?, r.f32()?, r.f32()?])
}

fn skip_box(r: &mut Reader) -> Result<(), Error> {
    r.skip(25)
}

fn array_len(r: &mut Reader) -> Result<usize, Error> {
    let n = r.compact()?;
    if n < 0 || n as usize > r.remaining() {
        return Err(Error::Format(format!("bad array length {n} at {}", r.pos)));
    }
    Ok(n as usize)
}

/// Reads the `Materials` array property: each element is a tagged struct
/// whose `Material` field is an object reference.
pub fn material_list(pkg: &Package, props: &Properties) -> Result<Vec<ObjectRef>, Error> {
    let Some(Value::Raw { data, .. }) = props.get(pkg, "Materials") else {
        return Ok(Vec::new());
    };
    let mut r = Reader::new(data);
    let n = array_len(&mut r)?;
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        let elem = Properties::read(pkg, &mut r)?;
        out.push(elem.object(pkg, "Material").map(ObjectRef::from_raw).unwrap_or(ObjectRef::Null));
    }
    Ok(out)
}

pub fn read_static_mesh(pkg: &Package, export: usize) -> Result<StaticMesh, Error> {
    let data = pkg.export_data(export);
    let mut r = Reader::new(data);
    let props = Properties::read(pkg, &mut r)?;
    let materials = material_list(pkg, &props)?;

    skip_box(&mut r)?; // UPrimitive::BoundingBox
    r.skip(16)?; // UPrimitive::BoundingSphere

    let n = array_len(&mut r)?;
    let mut sections = Vec::with_capacity(n);
    for i in 0..n {
        r.u32()?;
        let first_index = r.u16()? as u32;
        r.u16()?; // first vertex
        r.u16()?; // last vertex
        r.u16()?;
        let num_faces = r.u16()? as u32;
        let material = materials.get(i).copied().unwrap_or(ObjectRef::Null);
        sections.push(Section { first_index, num_faces, material });
    }
    skip_box(&mut r)?;

    let n = array_len(&mut r)?;
    let mut positions = Vec::with_capacity(n);
    let mut normals = Vec::with_capacity(n);
    for _ in 0..n {
        positions.push(vec3(&mut r)?);
        normals.push(vec3(&mut r)?);
    }
    r.i32()?; // revision

    let mut colors = Vec::new();
    for stream in 0..2 {
        let n = array_len(&mut r)?;
        let raw = r.bytes(n * 4)?;
        if stream == 0 {
            colors = raw.chunks_exact(4).map(|c| [c[2], c[1], c[0], c[3]]).collect();
        }
        r.i32()?;
    }

    let n_streams = array_len(&mut r)?;
    let mut uvs = Vec::new();
    for s in 0..n_streams {
        let n = array_len(&mut r)?;
        let raw = r.bytes(n * 8)?;
        if s == 0 {
            uvs = raw
                .chunks_exact(8)
                .map(|c| {
                    [
                        f32::from_le_bytes(c[0..4].try_into().unwrap()),
                        f32::from_le_bytes(c[4..8].try_into().unwrap()),
                    ]
                })
                .collect();
        }
        r.i32()?;
        r.i32()?;
    }

    let n = array_len(&mut r)?;
    let raw = r.bytes(n * 2)?;
    let indices = raw.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]]) as u32).collect();

    if uvs.len() != positions.len() {
        uvs.resize(positions.len(), [0.0, 0.0]);
    }
    Ok(StaticMesh {
        name: pkg.full_name(ObjectRef::Export(export)),
        positions,
        normals,
        uvs,
        colors,
        indices,
        sections,
    })
}
