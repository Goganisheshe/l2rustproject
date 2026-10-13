use anyhow::{bail, Context, Result};
use ue2::Package;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("info") => info(&args[2..]),
        Some("names") => { let p = Package::open(&args[2])?; for (i, n) in p.names.iter().enumerate() { println!("{i:4} {n}"); } Ok(()) }
        Some("textures") => textures(&args[2], &args[3]),
        Some("scan") => scan(&args[2..]),
        Some("obj") => obj(&args[2], &args[3], &args[4]),
        Some("materials") => materials(&args[2], &args[3], &args[4]),
        Some("hex") => hex(&args[2], args[3].parse()?, args.get(4).map(|s| s.parse()).transpose()?.unwrap_or(128)),
        _ => bail!(
            "usage: l2tool info <pkg>... | names <pkg> | textures <pkg> <dir> | scan <dir>... | \
             obj <pkg> <mesh> <out.obj> | materials <client> <pkg> <mesh> | hex <pkg> <export> [n]"
        ),
    }
}

fn info(paths: &[String]) -> Result<()> {
    for p in paths {
        let pkg = Package::open(p).with_context(|| p.clone())?;
        println!(
            "{p}: ver {} lic {} names {} imports {} exports {}",
            pkg.version, pkg.licensee, pkg.names.len(), pkg.imports.len(), pkg.exports.len()
        );
        for (i, e) in pkg.exports.iter().enumerate() {
            println!(
                "  [{i:4}] {:<20} {:<40} size {:>8} off {:>9} flags {:08x}",
                pkg.class_name(e), pkg.full_name(ue2::ObjectRef::Export(i)), e.serial_size, e.serial_offset, e.flags
            );
        }
    }
    Ok(())
}

fn textures(path: &str, out: &str) -> Result<()> {
    let pkg = Package::open(path)?;
    let dir = std::path::Path::new(out).join(&pkg.name);
    std::fs::create_dir_all(&dir)?;
    let (mut ok, mut fail) = (0, 0);
    for i in pkg.exports_of_class("Texture").collect::<Vec<_>>() {
        match ue2::texture::read_texture(&pkg, i) {
            Ok(t) => {
                let file = dir.join(format!("{}.png", t.name));
                image::save_buffer(file, &t.rgba, t.width, t.height, image::ColorType::Rgba8)?;
                ok += 1;
            }
            Err(e) => {
                eprintln!("  {e}");
                fail += 1;
            }
        }
    }
    println!("{}: {ok} textures exported, {fail} failed", pkg.name);
    Ok(())
}

/// Opens every package under the given folders, tallies export classes and
/// test-decodes all textures.
fn scan(dirs: &[String]) -> Result<()> {
    let mut classes: std::collections::BTreeMap<String, usize> = Default::default();
    let (mut pkgs, mut bad_pkgs, mut tex_ok, mut tex_fail) = (0, 0, 0, 0);
    let (mut mesh_ok, mut mesh_fail) = (0, 0);
    let (mut skel_ok, mut skel_fail) = (0, 0);
    let (mut anim_ok, mut anim_fail) = (0, 0);
    let skip_tex = std::env::var_os("SCAN_NO_TEX").is_some();
    let mut versions: std::collections::BTreeMap<(u16, u16), usize> = Default::default();
    for d in dirs {
        for entry in std::fs::read_dir(d)? {
            let path = entry?.path();
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
            if !matches!(ext.as_str(), "u" | "ukx" | "utx" | "usx" | "unr" | "uax" | "uix" | "ugx") {
                continue;
            }
            let pkg = match Package::open(&path) {
                Ok(p) => p,
                Err(e) => {
                    eprintln!("{}: {e}", path.display());
                    bad_pkgs += 1;
                    continue;
                }
            };
            pkgs += 1;
            *versions.entry((pkg.version, pkg.licensee)).or_default() += 1;
            for (i, e) in pkg.exports.iter().enumerate() {
                let class = pkg.class_name(e).to_string();
                if class == "StaticMesh" {
                    match ue2::mesh::read_static_mesh(&pkg, i) {
                        Ok(m) if m.indices.iter().all(|&x| (x as usize) < m.positions.len())
                            && m.sections.iter().all(|s| ((s.first_index + s.num_faces * 3) as usize) <= m.indices.len()) => mesh_ok += 1,
                        Ok(m) => {
                            if mesh_fail < 20 { eprintln!("{}: {} inconsistent", path.display(), m.name); }
                            mesh_fail += 1;
                        }
                        Err(err) => {
                            if mesh_fail < 20 { eprintln!("{}: {}: {err}", path.display(), pkg.full_name(ue2::ObjectRef::Export(i))); }
                            mesh_fail += 1;
                        }
                    }
                }
                if class == "SkeletalMesh" {
                    match ue2::skel::read_skeletal_mesh(&pkg, i) {
                        Ok(m) if m.indices.iter().all(|&x| (x as usize) < m.positions.len())
                            && m.joints.iter().flatten().all(|&j| (j as usize) < m.bones.len().max(1)) && !m.indices.is_empty() => skel_ok += 1,
                        Ok(m) => {
                            if skel_fail < 30 { eprintln!("{}: {} inconsistent ({} verts, {} idx, {} bones)", path.display(), m.name, m.positions.len(), m.indices.len(), m.bones.len()); }
                            skel_fail += 1;
                        }
                        Err(err) => {
                            if skel_fail < 30 { eprintln!("{} [{}/{}]: {}: {err}", path.display(), pkg.version, pkg.licensee, pkg.full_name(ue2::ObjectRef::Export(i))); }
                            skel_fail += 1;
                        }
                    }
                }
                if class == "MeshAnimation" {
                    match ue2::anim::read_animation(&pkg, i) {
                        Ok(a) if a.sequences.iter().all(|s| s.tracks.len() == a.bones.len()) && !a.sequences.is_empty() => anim_ok += 1,
                        Ok(a) => {
                            if anim_fail < 30 { eprintln!("{}: {} inconsistent ({} seqs, {} bones)", path.display(), a.name, a.sequences.len(), a.bones.len()); }
                            anim_fail += 1;
                        }
                        Err(err) => {
                            if anim_fail < 30 { eprintln!("{} [{}/{}]: {}: {err}", path.display(), pkg.version, pkg.licensee, pkg.full_name(ue2::ObjectRef::Export(i))); }
                            anim_fail += 1;
                        }
                    }
                }
                if class == "Texture" && !skip_tex {
                    match ue2::texture::read_texture(&pkg, i) {
                        Ok(_) => tex_ok += 1,
                        Err(err) => {
                            if tex_fail < 20 {
                                eprintln!("{}: {err}", path.display());
                            }
                            tex_fail += 1;
                        }
                    }
                }
                *classes.entry(class).or_default() += 1;
            }
        }
    }
    println!("packages: {pkgs} ok, {bad_pkgs} failed; versions {versions:?}");
    println!("textures: {tex_ok} decoded, {tex_fail} failed");
    println!("static meshes: {mesh_ok} decoded, {mesh_fail} failed");
    println!("skeletal meshes: {skel_ok} decoded, {skel_fail} failed");
    println!("animations: {anim_ok} decoded, {anim_fail} failed");
    let mut top: Vec<_> = classes.into_iter().collect();
    top.sort_by(|a, b| b.1.cmp(&a.1));
    for (c, n) in top.iter().take(40) {
        println!("  {n:7} {c}");
    }
    Ok(())
}

fn hex(path: &str, idx: usize, n: usize) -> Result<()> {
    let pkg = Package::open(path)?;
    let d = pkg.export_data(idx);
    for (row, chunk) in d[..n.min(d.len())].chunks(16).enumerate() {
        let hex: Vec<String> = chunk.iter().map(|b| format!("{b:02x}")).collect();
        let asc: String = chunk.iter().map(|&b| if (32..127).contains(&b) { b as char } else { '.' }).collect();
        println!("{:06x}  {:<48} {asc}", row * 16, hex.join(" "));
    }
    Ok(())
}

/// Writes a static mesh as Wavefront OBJ (Unreal axes, Z up).
fn obj(path: &str, name: &str, out: &str) -> Result<()> {
    use std::io::Write;
    let pkg = Package::open(path)?;
    let idx = pkg.find_export(name).with_context(|| format!("no export {name}"))?;
    let m = ue2::mesh::read_static_mesh(&pkg, idx)?;
    let mut f = std::io::BufWriter::new(std::fs::File::create(out)?);
    for p in &m.positions {
        writeln!(f, "v {} {} {}", p[0], p[1], p[2])?;
    }
    for uv in &m.uvs {
        writeln!(f, "vt {} {}", uv[0], 1.0 - uv[1])?;
    }
    for n in &m.normals {
        writeln!(f, "vn {} {} {}", n[0], n[1], n[2])?;
    }
    for (si, s) in m.sections.iter().enumerate() {
        writeln!(f, "g section{si}_{}", pkg.object_name(s.material))?;
        let tris = &m.indices[s.first_index as usize..(s.first_index + s.num_faces * 3) as usize];
        for t in tris.chunks_exact(3) {
            let (a, b, c) = (t[0] + 1, t[1] + 1, t[2] + 1);
            writeln!(f, "f {a}/{a}/{a} {b}/{b}/{b} {c}/{c}/{c}")?;
        }
    }
    println!("{}: {} verts, {} tris, {} sections", m.name, m.positions.len(), m.indices.len() / 3, m.sections.len());
    Ok(())
}

/// Shows how each section material of a static mesh resolves to a texture.
fn materials(client: &str, package: &str, mesh: &str) -> Result<()> {
    let mut lib = ue2::library::Library::new(client)?;
    let h = lib.find(package, mesh).with_context(|| format!("no {package}:{mesh}"))?;
    let m = ue2::mesh::read_static_mesh(&h.package, h.export)?;
    for (i, s) in m.sections.iter().enumerate() {
        let full = h.package.full_name(s.material);
        match lib.resolve(&h.package, s.material) {
            Some(mat) => {
                let tex = lib.diffuse_texture(&mat);
                println!(
                    "section {i}: {full} -> {}.{} ({}) -> {:?}",
                    mat.package.name,
                    mat.name(),
                    mat.class(),
                    tex.map(|t| (t.name, t.width, t.height, t.masked, t.alpha))
                );
            }
            None => println!("section {i}: {full} -> unresolved"),
        }
    }
    Ok(())
}
