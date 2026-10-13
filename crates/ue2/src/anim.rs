//! UMeshAnimation decoding: bone tracks per animation sequence.

use crate::props::Properties;
use crate::{Error, ObjectRef, Package, Reader};

#[derive(Debug, Clone, Default)]
pub struct Track {
    /// Rotation keys (x, y, z, w) as stored (see `skel::Bone` for conventions).
    pub rotations: Vec<[f32; 4]>,
    /// Translation keys; may be empty (rotation-only track) or a single key.
    pub translations: Vec<[f32; 3]>,
    /// Key times in frames.
    pub times: Vec<f32>,
}

#[derive(Debug, Clone)]
pub struct Sequence {
    pub name: String,
    pub num_frames: u32,
    /// Frames per second.
    pub rate: f32,
    /// One track per entry of `Animation::bones`.
    pub tracks: Vec<Track>,
}

#[derive(Debug, Clone, Default)]
pub struct Animation {
    pub name: String,
    pub bones: Vec<String>,
    pub sequences: Vec<Sequence>,
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

fn read_track(r: &mut Reader) -> Result<Track, Error> {
    r.u32()?; // flags
    let n = count(r, 16)?;
    let mut rotations = Vec::with_capacity(n);
    for _ in 0..n {
        rotations.push([r.f32()?, r.f32()?, r.f32()?, r.f32()?]);
    }
    let n = count(r, 12)?;
    let mut translations = Vec::with_capacity(n);
    for _ in 0..n {
        translations.push(vec3(r)?);
    }
    let n = count(r, 4)?;
    let mut times = Vec::with_capacity(n);
    for _ in 0..n {
        times.push(r.f32()?);
    }
    Ok(Track { rotations, translations, times })
}

struct Motion {
    track_time: f32,
    tracks: Vec<Track>,
}

fn read_motion(r: &mut Reader) -> Result<Motion, Error> {
    vec3(r)?; // root speed
    let track_time = r.f32()?;
    r.i32()?; // start bone
    r.u32()?; // flags
    let n = count(r, 4)?;
    r.skip(n * 4)?; // bone indices
    let n = count(r, 4)?;
    let mut tracks = Vec::with_capacity(n);
    for _ in 0..n {
        tracks.push(read_track(r)?);
    }
    read_track(r)?; // root track
    Ok(Motion { track_time, tracks })
}

pub fn read_animation(pkg: &Package, export: usize) -> Result<Animation, Error> {
    let base = pkg.exports[export].serial_offset;
    let data = pkg.export_data(export);
    let mut r = Reader::new(data);
    Properties::read(pkg, &mut r)?;
    let (ver, lic) = (pkg.version, pkg.licensee);

    r.i32()?; // version
    let n = count(&mut r, 6)?;
    let mut bones = Vec::with_capacity(n);
    for _ in 0..n {
        bones.push(pkg.name(r.compact()? as usize).to_string());
        r.u32()?;
        r.i32()?;
    }

    let mut moves = Vec::new();
    if ver >= 123 && lic >= 0x19 {
        let end = r.i32()? as usize;
        let n = count(&mut r, 4)?;
        for _ in 0..n {
            let next = r.i32()? as usize;
            moves.push(read_motion(&mut r)?);
            r.pos = next.checked_sub(base).ok_or_else(|| Error::Format("bad motion skip".into()))?;
        }
        r.pos = end.checked_sub(base).ok_or_else(|| Error::Format("bad moves skip".into()))?;
    } else {
        let n = count(&mut r, 4)?;
        for _ in 0..n {
            moves.push(read_motion(&mut r)?);
        }
    }

    let n = count(&mut r, 4)?;
    let mut sequences = Vec::with_capacity(n);
    for i in 0..n {
        if ver >= 115 {
            r.f32()?;
        }
        let name = pkg.name(r.compact()? as usize).to_string();
        let groups = count(&mut r, 1)?;
        for _ in 0..groups {
            r.compact()?;
        }
        r.i32()?; // start frame
        let num_frames = r.i32()?.max(0) as u32;
        let notifies = count(&mut r, 5)?;
        for _ in 0..notifies {
            r.f32()?;
            r.compact()?;
            if ver >= 112 {
                r.compact()?;
            }
            if ver >= 131 {
                let len = r.i32()?.max(0) as usize;
                r.skip(len * 2)?;
            }
        }
        let rate = r.f32()?;
        if lic >= 1 {
            r.i32()?;
            r.i32()?;
            if lic >= 2 {
                r.i32()?;
            }
            r.compact()?;
            if lic >= 0x14 {
                r.i32()?;
            }
            if lic >= 0x19 {
                r.i32()?;
            }
            if lic == 0x1A {
                let n = count(&mut r, 8)?;
                r.skip(n * 8)?;
            } else if lic >= 0x1B {
                r.u8()?;
                let n = count(&mut r, 8)?;
                r.skip(n * 8)?;
                let n = count(&mut r, 5)?;
                for _ in 0..n {
                    r.i32()?;
                    let m = count(&mut r, 8)?;
                    r.skip(m * 8)?;
                }
                r.i32()?;
                r.i32()?;
                let n = count(&mut r, 8)?;
                r.skip(n * 8)?;
            }
        }
        let mut tracks = Vec::new();
        if let Some(m) = moves.get_mut(i) {
            tracks = std::mem::take(&mut m.tracks);
            // Key times are in "track time" units; rescale to frames.
            if m.track_time > 0.0 && num_frames > 0 {
                let k = num_frames as f32 / m.track_time;
                for t in &mut tracks {
                    for time in &mut t.times {
                        *time *= k;
                    }
                }
            }
        }
        sequences.push(Sequence { name, num_frames, rate, tracks });
    }

    Ok(Animation { name: pkg.full_name(ObjectRef::Export(export)), bones, sequences })
}
