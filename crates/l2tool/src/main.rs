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
        Some("bones") => skel_bones(&args[2], &args[3], &args[4]),
        Some("alpha") => mat_alpha(&args[2], &args[3], &args[4]),
        Some("props") => dump_props(&args[2], &args[3]),
        Some("effect") => dump_effect(&args[2], &args[3]),
        Some("defaults") => class_defaults(&args[2], &args[3]),
        Some("dat") => dat_summary(&args[2]),
        Some("xdat") => {
            let x = ue2::xdat::parse(&std::fs::read(&args[2])?)?;
            println!("{:?} bytes; {} shortcuts, {} windows, {} wnd pos, {} fonts, {} styles", x.consumed, x.shortcuts.len(), x.windows.len(), x.wnd_def_pos.len(), x.fonts.len(), x.styles.len());
            fn dump(e: &ue2::xdat::Element, depth: usize, max: usize) {
                if depth > max {
                    return;
                }
                println!("{}{} [{}] {}x{} at {:?} {},{} tex {}", "  ".repeat(depth), e.name(), e.class, e.int("width"), e.int("height"), e.str("relativeTo"), e.float("x"), e.float("y"), [e.str("file"), e.str("backTex"), e.str("normalTex"), e.str("foreTex")].concat());
                for c in e.children() {
                    dump(c, depth + 1, max);
                }
            }
            let filter = args.get(3).cloned().unwrap_or_default();
            for w in &x.windows {
                if filter.is_empty() {
                    println!("{} ({} children)", w.name(), w.children().len());
                } else if w.name().eq_ignore_ascii_case(&filter) {
                    dump(w, 0, 6);
                }
            }
            Ok(())
        }
        Some("datdump") => Ok(std::fs::write(&args[3], ue2::dat::read_dat(&args[2])?)?),
        Some("map") => map_info(&args[2], &args[3]),
        Some("bsp") => bsp_probe(&args[2], &args[3]),
        Some("bspinfo") => {
            let pkg = Package::open(&args[2])?;
            let m = ue2::bsp::level_model(&pkg).context("no model")?;
            let polys = ue2::bsp::read_bsp(&pkg, m)?;
            let vis = polys.iter().filter(|p| p.visible()).count();
            println!("{} polys ({} visible) in {}", polys.len(), vis, pkg.full_name(ue2::ObjectRef::Export(m)));
            Ok(())
        }
        Some("height") => terrain_height(&args[2], &args[3], args[4].parse()?, args[5].parse()?),
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
    let refs: Vec<ue2::ObjectRef> = if h.class().eq_ignore_ascii_case("SkeletalMesh") {
        let m = ue2::skel::read_skeletal_mesh(&h.package, h.export)?;
        println!("{} sections: {:?}", m.name, m.sections.iter().map(|s| (s.material, s.num_indices / 3)).collect::<Vec<_>>());
        m.materials
    } else {
        ue2::mesh::read_static_mesh(&h.package, h.export)?.sections.iter().map(|s| s.material).collect()
    };
    for (i, mref) in refs.iter().enumerate() {
        let full = h.package.full_name(*mref);
        match lib.resolve(&h.package, *mref) {
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

/// Dumps bones and per-bone vertex counts of a skeletal mesh.
pub fn skel_bones(client: &str, package: &str, mesh: &str) -> Result<()> {
    let mut lib = ue2::library::Library::new(client)?;
    let h = lib.find(package, mesh).with_context(|| format!("no {package}:{mesh}"))?;
    let m = ue2::skel::read_skeletal_mesh(&h.package, h.export)?;
    let mut mn = [f32::MAX; 3];
    let mut mx = [f32::MIN; 3];
    for p in &m.positions {
        for k in 0..3 {
            mn[k] = mn[k].min(p[k]);
            mx[k] = mx[k].max(p[k]);
        }
    }
    println!(
        "{} verts, {} tris, bounds {mn:?}..{mx:?}, scale {:?} origin {:?} rot {:?}",
        m.positions.len(),
        m.indices.len() / 3,
        m.scale,
        m.origin,
        m.rot_origin
    );
    let mut used = vec![0usize; m.bones.len()];
    for (j, w) in m.joints.iter().zip(&m.weights) {
        for k in 0..4 {
            if w[k] > 0.0 {
                used[j[k] as usize] += 1;
            }
        }
    }
    for (i, b) in m.bones.iter().enumerate() {
        println!("{i:3} {:<28} parent {:3} verts {:4} t {:?}", b.name, b.parent, used[i], b.translation);
    }
    Ok(())
}

/// Prints alpha statistics of a material's diffuse and opacity textures.
pub fn mat_alpha(client: &str, package: &str, material: &str) -> Result<()> {
    let mut lib = ue2::library::Library::new(client)?;
    let h = lib.find(package, material).with_context(|| format!("no {package}:{material}"))?;
    let mut r = ue2::Reader::new(h.package.export_data(h.export));
    let props = ue2::props::Properties::read(&h.package, &mut r)?;
    for p in &props.0 {
        println!("  prop {} = {:?}", h.package.name(p.name), p.value);
    }
    let stats = |t: &ue2::texture::Texture| {
        let px = (t.rgba.len() / 4).max(1);
        let clear = t.rgba.chunks_exact(4).filter(|p| p[3] < 128).count();
        let dark = t.rgba.chunks_exact(4).filter(|p| (p[0] as u32 + p[1] as u32 + p[2] as u32) < 3 * 128).count();
        format!("{} {:?} alpha<128: {}%, rgb dark: {}%", t.name, t.format, clear * 100 / px, dark * 100 / px)
    };
    for field in ["Diffuse", "Opacity", "Material"] {
        if let Some(o) = props.object(&h.package, field).filter(|&o| o != 0) {
            if let Some(m) = lib.resolve(&h.package, ue2::ObjectRef::from_raw(o)) {
                if let Some(t) = lib.diffuse_texture(&m) {
                    println!("{field}: {}", stats(&t));
                }
            }
        }
    }
    Ok(())
}

/// Dumps the tagged properties of one export (`Outer.Name` or name).
pub fn dump_props(path: &str, object: &str) -> Result<()> {
    let pkg = Package::open(path)?;
    let idx = pkg.find_export(object).with_context(|| format!("no export {object}"))?;
    let (props, end) = ue2::props::read_object(&pkg, idx)?;
    let remaining = pkg.export_data(idx).len() - end;
    println!("{} ({}), {} bytes after properties", object, pkg.class_name(&pkg.exports[idx]), remaining);
    for p in &props.0 {
        let v = match &p.value {
            ue2::props::Value::Object(o) => format!("Object {}", pkg.full_name(ue2::ObjectRef::from_raw(*o))),
            ue2::props::Value::Name(n) => format!("Name {}", pkg.name(*n)),
            ue2::props::Value::Raw { kind, name, data } => {
                let floats: Vec<String> = data
                    .chunks_exact(4)
                    .map(|c| format!("{:.3}", f32::from_le_bytes(c.try_into().unwrap())))
                    .collect();
                format!(
                    "Raw kind {kind} struct {} [{}] floats({})",
                    name.map(|n| pkg.name(n).to_string()).unwrap_or_default(),
                    data.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(" "),
                    floats.join(", ")
                )
            }
            v => format!("{v:?}"),
        };
        println!("  {}[{}] = {v}", pkg.name(p.name), p.index);
    }
    Ok(())
}

/// Prints the decoded emitters of an effect class.
pub fn dump_effect(path: &str, class: &str) -> Result<()> {
    let pkg = Package::open(path)?;
    let emitters = ue2::effect::read_effect(&pkg, class).with_context(|| format!("no effect {class}"))?;
    for e in emitters {
        println!(
            "{} {:?} tex={} mesh={} max={} respawn={} rate={} life={:?} size={:?} style={} vel={:?} acc={:?} sizescale={:?} colors={}",
            e.name,
            e.kind,
            pkg.full_name(e.texture),
            pkg.full_name(e.static_mesh),
            e.max_particles,
            e.respawn,
            e.rate,
            e.lifetime,
            e.start_size,
            e.draw_style,
            e.velocity,
            e.acceleration,
            e.size_scale,
            e.color_scale.len()
        );
    }
    Ok(())
}

/// Finds a class's default properties by scanning for an offset where a
/// tagged property stream parses cleanly up to the end of the export.
pub fn class_defaults(path: &str, class: &str) -> Result<()> {
    let pkg = Package::open(path)?;
    let idx = (0..pkg.exports.len())
        .find(|&i| pkg.class_name(&pkg.exports[i]) == "Class" && pkg.name(pkg.exports[i].object_name).eq_ignore_ascii_case(class))
        .with_context(|| format!("no class {class}"))?;
    let data = pkg.export_data(idx);
    for start in 0..data.len() {
        let mut r = ue2::Reader::at(data, start);
        if let Ok(p) = ue2::props::Properties::read(&pkg, &mut r) {
            if r.remaining() == 0 && !p.0.is_empty() {
                println!("defaults at {start}/{}", data.len());
                for prop in &p.0 {
                    let v = match &prop.value {
                        ue2::props::Value::Object(o) => pkg.full_name(ue2::ObjectRef::from_raw(*o)),
                        ue2::props::Value::Raw { data, .. } => format!("raw {} bytes {:02x?}", data.len(), &data[..data.len().min(48)]),
                        v => format!("{v:?}"),
                    };
                    println!("  {}[{}] = {v}", pkg.name(prop.name), prop.index);
                }
                return Ok(());
            }
        }
    }
    println!("no defaults found");
    Ok(())
}

/// Decodes the client data tables and prints a summary.
pub fn dat_summary(client: &str) -> Result<()> {
    use ue2::dat::*;
    let sys = std::path::Path::new(client).join("system");
    let names = GameDataNames::parse(&read_dat(sys.join("l2gamedataname.dat"))?)?;
    println!("names: {}", names.0.len());
    let skills = parse_skillgrp(&read_dat(sys.join("skillgrp.dat"))?, &names)?;
    println!("skillgrp: {} entries", skills.len());
    let skill_names = parse_skillname(&read_dat(sys.join("skillname-ru.dat"))?)?;
    println!("skillname: {} entries", skill_names.len());
    let combos = parse_animationcombo(&read_dat(sys.join("animationcombo.dat"))?, &names)?;
    println!("animationcombo: {} entries", combos.len());
    let npcs = parse_npcgrp(&read_dat(sys.join("npcgrp.dat"))?, &names)?;
    println!("npcgrp: {} entries", npcs.len());
    let weapons = parse_weapongrp(&read_dat(sys.join("weapongrp.dat"))?, &names)?;
    println!("weapongrp: {} entries", weapons.len());
    let etc = parse_etcitemgrp(&read_dat(sys.join("etcitemgrp.dat"))?, &names)?;
    println!("etcitemgrp: {} entries", etc.len());
    for (id, d) in etc.iter().filter(|e| matches!(e.0, 57 | 1060 | 736 | 1835)) {
        println!("  etc {id}: {d:?}");
    }
    let armors = parse_armorgrp(&read_dat(sys.join("armorgrp.dat"))?, &names)?;
    println!("armorgrp: {} entries", armors.len());
    let mut parts: std::collections::BTreeMap<u8, (usize, String)> = Default::default();
    for a in &armors {
        let e = parts.entry(a.body_part).or_insert((0, format!("{} {:?}", a.id, a.models[0].meshes)));
        e.0 += 1;
    }
    for (p, (n, ex)) in parts {
        println!("  part {p}: {n} e.g. {ex}");
    }
    for a in armors.iter().filter(|a| matches!(a.id, 1146 | 1147 | 21 | 28 | 425 | 461)) {
        println!("  armor {} part {} type {} hide {:x}: {:?}", a.id, a.body_part, a.armor_type, a.hide_mask, &a.models[..2]);
    }
    for w in weapons.iter().filter(|w| matches!(w.id, 1 | 2369 | 6 | 177 | 2 | 10)) {
        println!("  weapon {:?}", w);
    }
    for s in skills.iter().filter(|s| matches!(s.id, 3 | 1177 | 1011) && s.level == 1) {
        println!(
            "  skill {} {:?}: anims {:?} -> {:?}, visual {}, icon {}, ep {}, magic {}, op {}, hit {}",
            s.id,
            skill_names.get(&(s.id, 1)),
            s.animations,
            s.animations.iter().map(|a| combos.get(a).map(|c| c.anims.clone())).collect::<Vec<_>>(),
            s.visual_effect,
            s.icon,
            s.effect_point,
            s.is_magic,
            s.operate_type,
            s.hit_time
        );
    }
    for n in npcs.iter().filter(|n| n.mesh.to_ascii_lowercase().contains("gremlin")).take(3) {
        println!("  npc {:?}", n);
    }
    Ok(())
}

/// Summarizes a map tile: actors and terrain layers.
pub fn map_info(client: &str, map: &str) -> Result<()> {
    let mut lib = ue2::library::Library::new(client)?;
    let pkg = lib.package(map).with_context(|| format!("no map {map}"))?;
    let tile = ue2::world::read_map(&pkg)?;
    println!("{} static mesh actors, {} terrains", tile.actors.len(), tile.terrains.len());
    let mut resolved = 0;
    for a in &tile.actors {
        if lib.resolve(&pkg, a.mesh).is_some() {
            resolved += 1;
        }
    }
    println!("meshes resolved: {resolved}/{}", tile.actors.len());
    // L2_NEAR="x,y,radius" lists actors around a point instead of the first three.
    let near: Option<Vec<f32>> = std::env::var("L2_NEAR").ok().map(|v| v.split(',').filter_map(|x| x.parse().ok()).collect());
    let shown: Vec<_> = match &near {
        Some(n) if n.len() == 3 => tile
            .actors
            .iter()
            .filter(|a| ((a.location[0] - n[0]).powi(2) + (a.location[1] - n[1]).powi(2)).sqrt() < n[2])
            .collect(),
        _ => tile.actors.iter().take(3).collect(),
    };
    for a in shown {
        println!("  {} at {:?} rot {:?} scale {:?}", pkg.full_name(a.mesh), a.location, a.rotation, a.scale);
    }
    for t in &tile.terrains {
        let hm = lib.resolve(&pkg, t.heightmap).with_context(|| "heightmap")?;
        let h = ue2::world::read_heightmap(&hm.package, hm.export)?;
        println!(
            "terrain at {:?} scale {:?}, heightmap {}x{} ({}..{}), {} layers",
            t.location,
            t.scale,
            h.width,
            h.height,
            h.data.iter().min().unwrap_or(&0),
            h.data.iter().max().unwrap_or(&0),
            t.layers.len()
        );
        for l in &t.layers {
            println!("  layer {} alpha {} u{} v{}", pkg.full_name(l.texture), pkg.full_name(l.alpha_map), l.uscale, l.vscale);
        }
    }
    Ok(())
}

/// Samples a tile's terrain height at Unreal (x, y) with both row orders.
pub fn terrain_height(client: &str, map: &str, x: f32, y: f32) -> Result<()> {
    let mut lib = ue2::library::Library::new(client)?;
    let pkg = lib.package(map).with_context(|| format!("no map {map}"))?;
    let tile = ue2::world::read_map(&pkg)?;
    for t in &tile.terrains {
        let hm = lib.resolve(&pkg, t.heightmap).with_context(|| "heightmap")?;
        let h = ue2::world::read_heightmap(&hm.package, hm.export)?;
        let fi = (x - t.location[0]) / t.scale[0] + h.width as f32 / 2.0;
        let fj = (y - t.location[1]) / t.scale[1] + h.height as f32 / 2.0;
        let (i, j) = (fi.round().clamp(0.0, 255.0) as usize, fj.round().clamp(0.0, 255.0) as usize);
        let z = |v: u16| t.location[2] + (v as f32 - 32768.0) * t.scale[2] / 256.0;
        println!(
            "cell ({i},{j}): z={:.0}; flipped rows z={:.0}; flipped cols z={:.0}",
            z(h.data[j * h.width + i]),
            z(h.data[(h.height - 1 - j) * h.width + i]),
            z(h.data[j * h.width + (h.width - 1 - i)])
        );
    }
    Ok(())
}

/// Probes the level BSP model layout.
pub fn bsp_probe(path: &str, object: &str) -> Result<()> {
    let pkg = Package::open(path)?;
    let idx = pkg.find_export(object).with_context(|| format!("no export {object}"))?;
    let (_, start) = ue2::props::read_object(&pkg, idx)?;
    let data = pkg.export_data(idx);
    let mut r = ue2::Reader::at(data, start);
    r.skip(25 + 16)?;
    let nv = r.compact()? as usize;
    r.skip(nv * 12)?;
    let np = r.compact()? as usize;
    r.skip(np * 12)?;
    let nn = r.compact()? as usize;
    println!("vectors {nv}, points {np}, nodes {nn} at {:#x}", r.pos);
    let mut max_vp = 0;
    let mut total_verts = 0usize;
    for _ in 0..nn {
        r.skip(16 + 8 + 1)?;
        let vp = r.compact()?;
        for _ in 0..6 {
            r.compact()?;
        }
        r.skip(16 + 16 + 2)?;
        let nverts = r.u8()? as usize;
        r.skip(8 + 12)?;
        max_vp = max_vp.max(vp as usize + nverts);
        total_verts += nverts;
    }
    let ns = r.compact()?;
    println!("after nodes at {:#x}: max vertpool {max_vp}, total node verts {total_verts}, surfs {ns}", r.pos);
    for _ in 0..ns {
        r.compact()?;
        r.u32()?;
        for _ in 0..6 {
            r.compact()?;
        }
        r.skip(16 + 4 + 4)?;
    }
    let nverts = r.compact()?;
    println!("after surfs at {:#x}: verts {nverts}", r.pos);
    let base = r.pos;
    for row in data[base..base + 192].chunks(16) {
        println!("{}", row.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(" "));
    }
    Ok(())
}
