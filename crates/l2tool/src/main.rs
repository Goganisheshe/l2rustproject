use anyhow::{bail, Context, Result};
use ue2::Package;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("info") => info(&args[2..]),
        Some("names") => { let p = Package::open(&args[2])?; for (i, n) in p.names.iter().enumerate() { println!("{i:4} {n}"); } Ok(()) }
        Some("textures") => textures(&args[2], &args[3]),
        Some("scan") => scan(&args[2..]),
        Some("hex") => hex(&args[2], args[3].parse()?, args.get(4).map(|s| s.parse()).transpose()?.unwrap_or(128)),
        _ => bail!("usage: l2tool info <pkg>... | hex <pkg> <export> [n]"),
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
                if class == "Texture" {
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
