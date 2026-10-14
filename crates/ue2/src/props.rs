//! UE2 tagged property stream (the part every UObject serializes first).

use crate::{Error, Package, Reader};

#[derive(Debug, Clone)]
pub enum Value {
    Byte(u8),
    Int(i32),
    Bool(bool),
    Float(f32),
    Object(i32),
    Name(usize),
    Str(String),
    /// Struct, array, vector etc. kept as raw bytes; `name` is the struct name if any.
    Raw { kind: u8, name: Option<usize>, data: Vec<u8> },
}

#[derive(Debug, Clone)]
pub struct Property {
    pub name: usize,
    pub index: u32,
    pub value: Value,
}

#[derive(Debug, Default, Clone)]
pub struct Properties(pub Vec<Property>);

impl Properties {
    /// Reads tagged properties until the `None` terminator.
    pub fn read(pkg: &Package, r: &mut Reader) -> Result<Self, Error> {
        let mut out = Vec::new();
        loop {
            let name = r.compact()? as usize;
            if pkg.name(name).eq_ignore_ascii_case("None") {
                break;
            }
            let info = r.u8()?;
            let kind = info & 0x0F;
            let array_bit = info & 0x80 != 0;
            let struct_name = if kind == 10 { Some(r.compact()? as usize) } else { None };
            let size = match (info >> 4) & 7 {
                0 => 1,
                1 => 2,
                2 => 4,
                3 => 12,
                4 => 16,
                5 => r.u8()? as usize,
                6 => r.u16()? as usize,
                _ => r.u32()? as usize,
            };
            let index = if array_bit && kind != 3 {
                let b = r.u8()? as u32;
                if b & 0x80 == 0 {
                    b
                } else if b & 0xC0 == 0x80 {
                    ((b & 0x7F) << 8) | r.u8()? as u32
                } else {
                    let rest = r.bytes(3)?;
                    ((b & 0x3F) << 24) | (rest[0] as u32) << 16 | (rest[1] as u32) << 8 | rest[2] as u32
                }
            } else {
                0
            };
            let raw = r.bytes(size)?;
            let mut v = Reader::new(raw);
            let value = match kind {
                1 if size == 1 => Value::Byte(v.u8()?),
                2 if size == 4 => Value::Int(v.i32()?),
                3 => Value::Bool(array_bit),
                4 if size == 4 => Value::Float(v.f32()?),
                5 | 8 => Value::Object(v.compact()?),
                6 => Value::Name(v.compact()? as usize),
                13 => Value::Str(v.fstring()?),
                _ => Value::Raw { kind, name: struct_name, data: raw.to_vec() },
            };
            out.push(Property { name, index, value });
        }
        Ok(Properties(out))
    }

    pub fn get<'a>(&'a self, pkg: &Package, name: &str) -> Option<&'a Value> {
        self.0
            .iter()
            .find(|p| pkg.name(p.name).eq_ignore_ascii_case(name))
            .map(|p| &p.value)
    }

    pub fn int(&self, pkg: &Package, name: &str) -> Option<i32> {
        match self.get(pkg, name)? {
            Value::Int(i) => Some(*i),
            Value::Byte(b) => Some(*b as i32),
            _ => None,
        }
    }

    pub fn byte(&self, pkg: &Package, name: &str) -> Option<u8> {
        match self.get(pkg, name)? {
            Value::Byte(b) => Some(*b),
            _ => None,
        }
    }

    pub fn object(&self, pkg: &Package, name: &str) -> Option<i32> {
        match self.get(pkg, name)? {
            Value::Object(o) => Some(*o),
            _ => None,
        }
    }
    pub fn float(&self, pkg: &Package, name: &str) -> Option<f32> {
        match self.get(pkg, name)? {
            Value::Float(f) => Some(*f),
            Value::Int(i) => Some(*i as f32),
            _ => None,
        }
    }

    pub fn bool(&self, pkg: &Package, name: &str) -> Option<bool> {
        match self.get(pkg, name)? {
            Value::Bool(b) => Some(*b),
            _ => None,
        }
    }

    /// Raw bytes of a struct/array property.
    pub fn raw(&self, pkg: &Package, name: &str) -> Option<&[u8]> {
        match self.get(pkg, name)? {
            Value::Raw { data, .. } => Some(data),
            _ => None,
        }
    }

    /// Non-native structs (Range, RangeVector, ...) are themselves tagged
    /// property streams.
    pub fn nested(&self, pkg: &Package, name: &str) -> Option<Properties> {
        let data = self.raw(pkg, name)?;
        Properties::read(pkg, &mut Reader::new(data)).ok()
    }

    /// Native `Vector` struct: three raw floats.
    pub fn vector(&self, pkg: &Package, name: &str) -> Option<[f32; 3]> {
        let d = self.raw(pkg, name)?;
        if d.len() < 12 {
            return None;
        }
        let f = |i: usize| f32::from_le_bytes(d[i * 4..i * 4 + 4].try_into().unwrap());
        Some([f(0), f(1), f(2)])
    }

    /// `Range { Min, Max }`; missing fields are 0.
    pub fn range(&self, pkg: &Package, name: &str) -> Option<[f32; 2]> {
        let r = self.nested(pkg, name)?;
        Some([r.float(pkg, "Min").unwrap_or(0.0), r.float(pkg, "Max").unwrap_or(0.0)])
    }

    /// `RangeVector { X, Y, Z: Range }`.
    pub fn range_vector(&self, pkg: &Package, name: &str) -> Option<[[f32; 2]; 3]> {
        let r = self.nested(pkg, name)?;
        let axis = |n: &str| r.range(pkg, n).unwrap_or([0.0, 0.0]);
        Some([axis("X"), axis("Y"), axis("Z")])
    }

    /// Dynamic array of tagged structs.
    pub fn struct_array(&self, pkg: &Package, name: &str) -> Vec<Properties> {
        let Some(data) = self.raw(pkg, name) else { return Vec::new() };
        let mut r = Reader::new(data);
        let Ok(n) = r.compact() else { return Vec::new() };
        (0..n.max(0)).map_while(|_| Properties::read(pkg, &mut r).ok()).collect()
    }
}


impl Properties {
    /// Dynamic array of object references (compact count + compact refs).
    pub fn object_array(&self, pkg: &Package, name: &str) -> Vec<crate::ObjectRef> {
        let Some(data) = self.raw(pkg, name) else { return Vec::new() };
        let mut r = Reader::new(data);
        let Ok(n) = r.compact() else { return Vec::new() };
        (0..n.max(0)).map_while(|_| r.compact().ok().map(crate::ObjectRef::from_raw)).collect()
    }
}

/// Default properties of a UClass export. The class body before them
/// (bytecode, state data) is version-specific, so we look for the offset
/// where a tagged property stream parses cleanly to the end of the export.
pub fn class_defaults(pkg: &Package, export: usize) -> Option<Properties> {
    let data = pkg.export_data(export);
    (0..data.len()).find_map(|start| {
        let mut r = Reader::at(data, start);
        let p = Properties::read(pkg, &mut r).ok()?;
        (r.remaining() == 0 && !p.0.is_empty()).then_some(p)
    })
}

/// `RF_HasStack`: the object starts with a script state frame.
pub const RF_HAS_STACK: u32 = 0x0200_0000;

/// Reads an export's tagged properties, skipping the state frame that
/// actors (and other objects with `RF_HasStack`) serialize first.
/// Returns the properties and the offset right after them.
pub fn read_object(pkg: &Package, export: usize) -> Result<(Properties, usize), Error> {
    let data = pkg.export_data(export);
    let mut r = Reader::new(data);
    if pkg.exports.get(export).is_some_and(|e| e.flags & RF_HAS_STACK != 0) {
        let node = r.compact()?;
        r.compact()?; // state node
        r.skip(8)?; // probe mask
        r.i32()?; // latent action
        if node != 0 {
            r.compact()?; // code offset
        }
    }
    let p = Properties::read(pkg, &mut r)?;
    Ok((p, r.pos))
}

impl Properties {
    /// Native `Rotator` struct: pitch, yaw, roll (65536 = full turn).
    pub fn rotator(&self, pkg: &Package, name: &str) -> Option<[i32; 3]> {
        let d = self.raw(pkg, name)?;
        if d.len() < 12 {
            return None;
        }
        let f = |i: usize| i32::from_le_bytes(d[i * 4..i * 4 + 4].try_into().unwrap());
        Some([f(0), f(1), f(2)])
    }
}
