//! UTexture decoding (mip 0 to RGBA8).
//!
//! Lineage II inserts version-dependent data between the property block and
//! the mip array (shader source strings, padding). Rather than hard-code every
//! layout we locate the first mip by its self-describing header: a lazy array
//! whose `SkipPos` points exactly past `size` bytes we can predict from the
//! texture format and dimensions.

use crate::props::Properties;
use crate::{Error, ObjectRef, Package, Reader};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    P8,
    Rgba7,
    Rgb16,
    Dxt1,
    Rgb8,
    Rgba8,
    Dxt3,
    Dxt5,
    L8,
    G16,
    Other(u8),
}

impl Format {
    pub fn from_byte(b: u8) -> Self {
        match b {
            0 => Format::P8,
            1 => Format::Rgba7,
            2 => Format::Rgb16,
            3 => Format::Dxt1,
            4 => Format::Rgb8,
            5 => Format::Rgba8,
            7 => Format::Dxt3,
            8 => Format::Dxt5,
            9 => Format::L8,
            10 => Format::G16,
            o => Format::Other(o),
        }
    }

    pub fn data_size(self, w: usize, h: usize) -> Option<usize> {
        let blocks = w.div_ceil(4).max(1) * h.div_ceil(4).max(1);
        Some(match self {
            Format::Dxt1 => blocks * 8,
            Format::Dxt3 | Format::Dxt5 => blocks * 16,
            Format::P8 | Format::L8 => w * h,
            Format::G16 | Format::Rgb16 => w * h * 2,
            Format::Rgb8 => w * h * 3,
            Format::Rgba8 => w * h * 4,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone)]
pub struct Texture {
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub format: Format,
    /// Mip 0 as tightly packed RGBA8.
    pub rgba: Vec<u8>,
    /// `bMasked` / `bAlphaTexture`: alpha channel is meant as opacity.
    pub masked: bool,
    pub alpha: bool,
}

pub fn read_texture(pkg: &Package, export: usize) -> Result<Texture, Error> {
    let e = &pkg.exports[export];
    let data = pkg.export_data(export);
    let mut r = Reader::new(data);
    let props = Properties::read(pkg, &mut r)?;
    let format = Format::from_byte(props.byte(pkg, "Format").unwrap_or(0));
    let w = props.int(pkg, "USize").unwrap_or(0) as usize;
    let h = props.int(pkg, "VSize").unwrap_or(0) as usize;
    let name = pkg.full_name(ObjectRef::Export(export));
    let size = format
        .data_size(w, h)
        .ok_or_else(|| Error::Format(format!("{name}: unsupported format {format:?}")))?;

    let start = find_mip0(data, r.pos, e.serial_offset, size)
        .ok_or_else(|| Error::Format(format!("{name}: mip 0 not found ({w}x{h} {format:?})")))?;
    let pixels = &data[start..start + size];

    let mut rgba = vec![0u8; w * h * 4];
    match format {
        Format::Dxt1 => texpresso::Format::Bc1.decompress(pixels, w, h, &mut rgba),
        Format::Dxt3 => texpresso::Format::Bc2.decompress(pixels, w, h, &mut rgba),
        Format::Dxt5 => texpresso::Format::Bc3.decompress(pixels, w, h, &mut rgba),
        Format::Rgba8 => {
            for (o, i) in rgba.chunks_exact_mut(4).zip(pixels.chunks_exact(4)) {
                o.copy_from_slice(&[i[2], i[1], i[0], i[3]]);
            }
        }
        Format::Rgb8 => {
            for (o, i) in rgba.chunks_exact_mut(4).zip(pixels.chunks_exact(3)) {
                o.copy_from_slice(&[i[2], i[1], i[0], 255]);
            }
        }
        Format::L8 => {
            for (o, &l) in rgba.chunks_exact_mut(4).zip(pixels) {
                o.copy_from_slice(&[l, l, l, 255]);
            }
        }
        Format::G16 | Format::Rgb16 => {
            for (o, i) in rgba.chunks_exact_mut(4).zip(pixels.chunks_exact(2)) {
                let l = i[1];
                o.copy_from_slice(&[l, l, l, 255]);
            }
        }
        Format::P8 => {
            let palette = props
                .object(pkg, "Palette")
                .map(ObjectRef::from_raw)
                .and_then(|p| match p {
                    ObjectRef::Export(i) => read_palette(pkg, i).ok(),
                    _ => None,
                })
                .unwrap_or_else(|| (0..=255).map(|i| [i, i, i, 255]).collect());
            for (o, &idx) in rgba.chunks_exact_mut(4).zip(pixels) {
                o.copy_from_slice(palette.get(idx as usize).unwrap_or(&[0, 0, 0, 255]));
            }
        }
        _ => unreachable!(),
    }
    let flag = |n: &str| matches!(props.get(pkg, n), Some(crate::props::Value::Bool(true)));
    let (masked, alpha) = (flag("bMasked"), flag("bAlphaTexture"));
    Ok(Texture { name, width: w as u32, height: h as u32, format, rgba, masked, alpha })
}

/// Scans for `[i32 skip_pos][compact size][size bytes]` where skip_pos is the
/// absolute offset right after the pixel data.
fn find_mip0(data: &[u8], from: usize, base: usize, size: usize) -> Option<usize> {
    for p in from..data.len().saturating_sub(5) {
        let skip = u32::from_le_bytes(data[p..p + 4].try_into().unwrap()) as usize;
        let mut r = Reader::at(data, p + 4);
        let Ok(n) = r.compact() else { continue };
        if n as usize == size && skip == base + r.pos + size && r.pos + size <= data.len() {
            return Some(r.pos);
        }
    }
    None
}

fn read_palette(pkg: &Package, export: usize) -> Result<Vec<[u8; 4]>, Error> {
    let data = pkg.export_data(export);
    let mut r = Reader::new(data);
    Properties::read(pkg, &mut r)?;
    let n = r.compact()? as usize;
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        let c = r.bytes(4)?;
        out.push([c[2], c[1], c[0], 255]);
    }
    Ok(out)
}

/// Undecoded bytes of mip 0 (e.g. 16-bit heights of a G16 heightmap).
pub fn raw_mip0(pkg: &Package, export: usize) -> Result<Vec<u8>, Error> {
    let e = &pkg.exports[export];
    let data = pkg.export_data(export);
    let mut r = Reader::new(data);
    let props = Properties::read(pkg, &mut r)?;
    let format = Format::from_byte(props.byte(pkg, "Format").unwrap_or(0));
    let w = props.int(pkg, "USize").unwrap_or(0) as usize;
    let h = props.int(pkg, "VSize").unwrap_or(0) as usize;
    let size = format.data_size(w, h).ok_or_else(|| Error::Format(format!("unsupported format {format:?}")))?;
    let start = find_mip0(data, r.pos, e.serial_offset, size).ok_or_else(|| Error::Format("mip 0 not found".into()))?;
    Ok(data[start..start + size].to_vec())
}
