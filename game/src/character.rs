//! Player characters assembled from the client's per-race body parts.
//!
//! A Lineage player model is not one mesh: the face, two hair pieces and four
//! armor pieces (upper, lower, gloves, boots) are separate skeletal meshes on
//! a shared skeleton. Faces and hair have no material in the mesh; the client
//! picks textures at runtime, so we name them explicitly.

use crate::l2assets::L2Library;

pub struct CharClass {
    /// Display name in menus.
    pub name: &'static str,
    /// Animation package (.ukx) holding the meshes and the animation set.
    pub package: &'static str,
    /// Mesh/texture name prefix, e.g. `MFighter`.
    pub prefix: &'static str,
    /// Starting one-handed weapon from `lineageweapons.ukx`.
    pub weapon: &'static str,
}

pub const CLASSES: &[CharClass] = &[
    CharClass { name: "Human Fighter (M)", package: "fighter", prefix: "MFighter", weapon: "falchion_m00_wp" },
    CharClass { name: "Human Fighter (F)", package: "fighter", prefix: "FFighter", weapon: "falchion_m00_wp" },
    CharClass { name: "Human Mystic (M)", package: "magic", prefix: "MMagic", weapon: "apprentices_rod_m00_wp" },
    CharClass { name: "Human Mystic (F)", package: "magic", prefix: "FMagic", weapon: "apprentices_rod_m00_wp" },
    CharClass { name: "Elf (M)", package: "elf", prefix: "MElf", weapon: "knife_m00_wp" },
    CharClass { name: "Elf (F)", package: "elf", prefix: "FElf", weapon: "knife_m00_wp" },
    CharClass { name: "Dark Elf (M)", package: "darkelf", prefix: "MDarkElf", weapon: "knife_m00_wp" },
    CharClass { name: "Dark Elf (F)", package: "darkelf", prefix: "FDarkElf", weapon: "knife_m00_wp" },
    CharClass { name: "Orc Fighter (M)", package: "orc", prefix: "MOrc", weapon: "tomahawk_m00_wp" },
    CharClass { name: "Orc Fighter (F)", package: "orc", prefix: "FOrc", weapon: "tomahawk_m00_wp" },
    CharClass { name: "Orc Mystic (M)", package: "shaman", prefix: "MShaman", weapon: "apprentices_rod_m00_wp" },
    CharClass { name: "Orc Mystic (F)", package: "shaman", prefix: "FShaman", weapon: "apprentices_rod_m00_wp" },
    CharClass { name: "Dwarf (M)", package: "dwarf", prefix: "MDwarf", weapon: "club_m00_wp" },
    CharClass { name: "Dwarf (F)", package: "dwarf", prefix: "FDwarf", weapon: "club_m00_wp" },
    CharClass { name: "Kamael (M)", package: "kamael", prefix: "MKamael", weapon: "falchion_m00_wp" },
    CharClass { name: "Kamael (F)", package: "kamael", prefix: "FKamael", weapon: "falchion_m00_wp" },
];

/// Weapon spec for `L2Library::rigid_mesh`: weapon meshes carry no material,
/// their texture is `<name>_t00_wp` in `lineageweaponstex`.
pub fn weapon_spec(class: &CharClass) -> String {
    let base = class.weapon.trim_end_matches("_m00_wp");
    format!("{}@lineageweaponstex/{base}_t00_wp", class.weapon)
}

/// Starter armor set id (`m001` = the novice outfit).
const ARMOR: &str = "m001";

/// Finds a class by prefix or display name (case-insensitive).
pub fn find_class(key: &str) -> Option<usize> {
    CLASSES.iter().position(|c| c.prefix.eq_ignore_ascii_case(key) || c.name.eq_ignore_ascii_case(key))
}

/// Builds the multi-part mesh spec understood by `ModelCache::load`.
pub fn part_spec(lib: &mut L2Library, class: &CharClass) -> String {
    let p = class.prefix;
    let texture_pkg = p.to_ascii_lowercase();
    let mut parts = vec![format!("{p}_m000_f@lineagefacetex/{p}_m000_t00_f")];
    for hair in ["ah", "bh"] {
        let mesh = format!("{p}_m000_m00_{hair}");
        let exists = lib.lib.as_mut().and_then(|l| l.find(class.package, &mesh)).is_some();
        if exists {
            parts.push(format!("{mesh}@{texture_pkg}/{p}_m000_t00_m00_{hair}"));
        }
    }
    for slot in ["u", "l", "g", "b"] {
        // Only the human fighter's novice set embeds its material; for the
        // rest the client picks a texture variant `<prefix>_<set>_tNN_<slot>`.
        let mesh = format!("{p}_{ARMOR}_{slot}");
        match lowest_variant(lib, &texture_pkg, &format!("{p}_{ARMOR}_t"), &format!("_{slot}")) {
            Some(mat) => parts.push(format!("{mesh}@{texture_pkg}/{mat}")),
            None => parts.push(mesh),
        }
    }
    parts.join("+")
}

/// Export named `<head><number><tail>` with the smallest number.
fn lowest_variant(lib: &mut L2Library, package: &str, head: &str, tail: &str) -> Option<String> {
    let pkg = lib.lib.as_mut()?.package(package)?;
    let (head, tail) = (head.to_ascii_lowercase(), tail.to_ascii_lowercase());
    pkg.exports
        .iter()
        .filter_map(|e| {
            let name = pkg.name(e.object_name);
            let lower = name.to_ascii_lowercase();
            let num = lower.strip_prefix(&head)?.strip_suffix(&tail)?;
            Some((num.parse::<u32>().ok()?, name.to_string()))
        })
        .min()
        .map(|(_, n)| n)
}
