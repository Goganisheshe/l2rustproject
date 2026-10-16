//! `l2tool skin <client> <out dir> [name filter]`
//!
//! Dumps the skeletal meshes whose soft-skinned geometry is stored only in
//! `FStaticLODModel::SkinningData` (no Lineage wedges, no source wedges).
//! The stream is decoded by `ue2::skel::decode_skinning` (its layout is
//! described there); this dump is for checking it on new client versions.
//!
//! For every such mesh the output folder gets `<mesh>.txt` (counts, sections,
//! the head of every array) and the raw arrays: `<mesh>.skinning.bin` (u32
//! LE), `<mesh>.points.bin` (3 f32 + packed normal u32), and
//! `<mesh>.soft_indices.bin` (u16 LE). `summary.txt` lists them all. With a
//! filter, every skeletal mesh whose name contains it is dumped instead.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use ue2::skel::{LodDump, Section};
use ue2::{ObjectRef, Package};

const EXTENSIONS: [&str; 3] = ["ukx", "u", "usx"];
const DWORDS_SHOWN: usize = 768;

/// Package files in the client root and its first-level folders.
fn packages(client: &Path) -> Result<Vec<PathBuf>> {
    let mut dirs = vec![client.to_path_buf()];
    for entry in std::fs::read_dir(client).with_context(|| client.display().to_string())? {
        let path = entry?.path();
        if path.is_dir() {
            dirs.push(path);
        }
    }
    let mut out = Vec::new();
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
            if EXTENSIONS.contains(&ext.as_str()) {
                out.push(path);
            }
        }
    }
    out.sort();
    Ok(out)
}

pub fn run(client: &str, out: &str, filter: Option<&str>) -> Result<()> {
    let out = Path::new(out);
    std::fs::create_dir_all(out)?;
    let filter = filter.map(str::to_ascii_lowercase);
    let mut summary = String::new();
    let (mut total, mut dumped, mut failed) = (0usize, 0usize, 0usize);
    for path in packages(Path::new(client))? {
        let pkg = match Package::open(&path) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("{}: {e}", path.display());
                continue;
            }
        };
        for (i, e) in pkg.exports.iter().enumerate() {
            if pkg.class_name(e) != "SkeletalMesh" {
                continue;
            }
            total += 1;
            let full = pkg.full_name(ObjectRef::Export(i));
            let wanted = match &filter {
                Some(f) => full.to_ascii_lowercase().contains(f.as_str()),
                None => true,
            };
            if !wanted {
                continue;
            }
            let dump = match ue2::skel::read_lod_dump(&pkg, i) {
                Ok(d) => d,
                Err(err) => {
                    eprintln!("{}: {full}: {err}", path.display());
                    let _ = writeln!(summary, "FAILED  {full}  ({}): {err}", path.display());
                    failed += 1;
                    continue;
                }
            };
            if filter.is_none() && !dump.soft_only_in_skinning() {
                continue;
            }
            let stem = file_stem(&full);
            write_dump(out, &stem, &path, &dump)?;
            let _ = writeln!(summary, "{}", summary_line(&dump, &path));
            dumped += 1;
        }
    }
    let _ = writeln!(summary, "\n{total} skeletal meshes, {dumped} dumped, {failed} failed to parse");
    std::fs::write(out.join("summary.txt"), &summary)?;
    print!("{summary}");
    println!("written to {}", out.display());
    Ok(())
}

fn file_stem(name: &str) -> String {
    name.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '_' }).collect()
}

fn summary_line(d: &LodDump, path: &Path) -> String {
    format!(
        "{}  bones {}  soft wedges {}  skin points {}  skinning {} dwords ({:.3}/wedge, {:.3}/point)  soft idx {}  rigid verts {}  ({})",
        d.name,
        d.bones.len(),
        d.num_soft_wedges,
        d.skin_points.len(),
        d.skinning.len(),
        ratio(d.skinning.len(), d.num_soft_wedges.max(0) as usize),
        ratio(d.skinning.len(), d.skin_points.len()),
        d.soft_indices.len(),
        d.rigid_verts,
        path.display()
    )
}

fn ratio(a: usize, b: usize) -> f64 {
    if b == 0 {
        0.0
    } else {
        a as f64 / b as f64
    }
}

fn write_dump(out: &Path, stem: &str, path: &Path, d: &LodDump) -> Result<()> {
    std::fs::write(out.join(format!("{stem}.txt")), report(d, path))?;
    let bytes: Vec<u8> = d.skinning.iter().flat_map(|v| v.to_le_bytes()).collect();
    std::fs::write(out.join(format!("{stem}.skinning.bin")), bytes)?;
    let mut bytes = Vec::with_capacity(d.skin_points.len() * 16);
    for (p, n) in &d.skin_points {
        for c in p {
            bytes.extend_from_slice(&c.to_le_bytes());
        }
        bytes.extend_from_slice(&n.to_le_bytes());
    }
    std::fs::write(out.join(format!("{stem}.points.bin")), bytes)?;
    let bytes: Vec<u8> = d.soft_indices.iter().flat_map(|v| v.to_le_bytes()).collect();
    std::fs::write(out.join(format!("{stem}.soft_indices.bin")), bytes)?;
    Ok(())
}

fn section_line(s: &Section) -> String {
    format!(
        "mat {} min_stream {} wedges {}..={} stream_idx {} bone {} unk {} faces {}+{} map[{}] {:?}",
        s.material,
        s.min_stream_index,
        s.min_wedge,
        s.max_wedge,
        s.num_stream_indices,
        s.bone,
        s.unknown,
        s.first_face,
        s.num_faces,
        s.bone_map.len(),
        s.bone_map
    )
}

/// A dword read as f32, when it looks like an ordinary float.
fn as_float(v: u32) -> String {
    let f = f32::from_bits(v);
    if f.is_finite() && (f == 0.0 || (1e-6..1e7).contains(&f.abs())) {
        format!("{f:.6}")
    } else {
        String::new()
    }
}

fn report(d: &LodDump, path: &Path) -> String {
    let mut s = String::new();
    let _ = writeln!(s, "{}", d.name);
    let _ = writeln!(s, "package {}", path.display());
    let _ = writeln!(s, "ULodMesh version {}, {} LOD models", d.version, d.num_lods);
    let _ = writeln!(s, "{} bones: {}", d.bones.len(), d.bones.join(", "));
    let _ = writeln!(s, "NumSoftWedges {}", d.num_soft_wedges);
    let _ = writeln!(s, "SkinPoints {}", d.skin_points.len());
    let _ = writeln!(
        s,
        "SkinningData {} dwords = {:.4} per soft wedge, {:.4} per skin point",
        d.skinning.len(),
        ratio(d.skinning.len(), d.num_soft_wedges.max(0) as usize),
        ratio(d.skinning.len(), d.skin_points.len())
    );
    let (min, max) = d.soft_indices.iter().fold((u16::MAX, 0u16), |(a, b), &x| (a.min(x), b.max(x)));
    let _ = writeln!(s, "SoftIndices {} (min {min}, max {max})", d.soft_indices.len());
    let _ = writeln!(s, "RigidIndices {}, rigid stream verts {}", d.rigid_indices.len(), d.rigid_verts);
    let _ = writeln!(s, "LineageWedges {}", d.lineage_wedges);
    let [p, w, f, i] = d.source_counts;
    let _ = writeln!(s, "source: points {p}, wedges {w}, faces {f}, influences {i}");
    let _ = writeln!(s, "\nsoft sections:");
    for sec in &d.soft {
        let _ = writeln!(s, "  {}", section_line(sec));
    }
    let _ = writeln!(s, "rigid sections:");
    for sec in &d.rigid {
        let _ = writeln!(s, "  {}", section_line(sec));
    }
    let _ = writeln!(s, "\nfirst soft indices:");
    for chunk in d.soft_indices.chunks(24).take(4) {
        let _ = writeln!(s, "  {chunk:?}");
    }
    let _ = writeln!(s, "\nfirst skin points (x, y, z, packed normal):");
    for (k, (p, n)) in d.skin_points.iter().take(24).enumerate() {
        let _ = writeln!(s, "  {k:5} {:12.4} {:12.4} {:12.4}  {n:08x}", p[0], p[1], p[2]);
    }
    let _ = writeln!(s, "\nSkinningData (index, hex, u32, as f32):");
    for (k, &v) in d.skinning.iter().take(DWORDS_SHOWN).enumerate() {
        let _ = writeln!(s, "  {k:6}  {v:08x}  {v:>10}  {}", as_float(v));
    }
    if d.skinning.len() > DWORDS_SHOWN {
        let _ = writeln!(s, "  ... {} more in the .skinning.bin file", d.skinning.len() - DWORDS_SHOWN);
    }
    s
}
