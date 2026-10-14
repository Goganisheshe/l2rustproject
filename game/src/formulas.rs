//! Lineage II combat formulas, after L2J Mobius `Formulas` and the stat
//! functions: base stat bonuses from `stats/statBonus.xml`, the level
//! modifier, accuracy / evasion hit rolls, critical hits and damage.

use std::path::Path;
use std::sync::OnceLock;

use rand::Rng;

/// Bonus multipliers per base stat value (index = stat value).
#[derive(Default, Debug)]
pub struct StatBonus {
    pub str_: Vec<f32>,
    pub int: Vec<f32>,
    pub con: Vec<f32>,
    pub men: Vec<f32>,
    pub dex: Vec<f32>,
    pub wit: Vec<f32>,
}

static BONUS: OnceLock<StatBonus> = OnceLock::new();

pub fn load_bonus(root: &Path) {
    let mut b = StatBonus::default();
    if let Ok(src) = std::fs::read_to_string(root.join("stats/statBonus.xml")) {
        if let Ok(doc) = roxmltree::Document::parse(&src) {
            for table in doc.root_element().children().filter(|c| c.is_element()) {
                let mut v = Vec::new();
                for s in table.children().filter(|c| c.has_tag_name("stat")) {
                    let (Some(i), Some(x)) = (s.attribute("value").and_then(|x| x.parse::<usize>().ok()), s.attribute("bonus").and_then(|x| x.parse::<f32>().ok())) else {
                        continue;
                    };
                    if v.len() <= i {
                        v.resize(i + 1, 1.0);
                    }
                    v[i] = x;
                }
                match table.tag_name().name() {
                    "STR" => b.str_ = v,
                    "INT" => b.int = v,
                    "CON" => b.con = v,
                    "MEN" => b.men = v,
                    "DEX" => b.dex = v,
                    "WIT" => b.wit = v,
                    _ => {}
                }
            }
        }
    }
    let _ = BONUS.set(b);
}

fn lookup(t: &[f32], v: f32) -> f32 {
    if t.is_empty() {
        return 1.0;
    }
    t[(v.max(0.0) as usize).min(t.len() - 1)]
}

pub fn str_bonus(v: f32) -> f32 {
    BONUS.get().map_or(1.0, |b| lookup(&b.str_, v))
}
pub fn int_bonus(v: f32) -> f32 {
    BONUS.get().map_or(1.0, |b| lookup(&b.int, v))
}
pub fn con_bonus(v: f32) -> f32 {
    BONUS.get().map_or(1.0, |b| lookup(&b.con, v))
}
pub fn men_bonus(v: f32) -> f32 {
    BONUS.get().map_or(1.0, |b| lookup(&b.men, v))
}
pub fn dex_bonus(v: f32) -> f32 {
    BONUS.get().map_or(1.0, |b| lookup(&b.dex, v))
}
pub fn wit_bonus(v: f32) -> f32 {
    BONUS.get().map_or(1.0, |b| lookup(&b.wit, v))
}

/// (level + 89) / 100.
pub fn level_mod(level: u32) -> f32 {
    (level as f32 + 89.0) / 100.0
}

/// Accuracy and evasion from DEX: sqrt(DEX) * 6 + level.
pub fn dex_accuracy(dex: f32, level: u32) -> f32 {
    dex.max(0.0).sqrt() * 6.0 + level as f32
}

/// Combat numbers of one side of a fight (final values).
#[derive(Clone, Copy, Debug, Default)]
pub struct Combat {
    pub patk: f32,
    pub pdef: f32,
    pub matk: f32,
    pub mdef: f32,
    pub accuracy: f32,
    pub evasion: f32,
    /// Critical rate in per mille.
    pub crit: f32,
    /// Weapon damage spread in percent.
    pub random: f32,
}

/// Hit roll: (80 + 2 * (accuracy - evasion))%, kept within 20..98%.
pub fn hits(attacker: &Combat, defender: &Combat, rng: &mut impl Rng) -> bool {
    let chance = ((80.0 + 2.0 * (attacker.accuracy - defender.evasion)) * 10.0).clamp(200.0, 980.0);
    rng.random_range(0.0..1000.0) < chance
}

/// Physical hit: 77 * P.Atk (+ skill power) * shot / P.Def, spread by the
/// weapon's random damage, doubled on a critical. `None` is a miss.
pub fn physical(attacker: &Combat, defender: &Combat, power: f32, shot: f32, skill: bool, rng: &mut impl Rng) -> Option<(f32, bool)> {
    if !skill && !hits(attacker, defender, rng) {
        return None;
    }
    let crit = rng.random_range(0.0..1000.0) < attacker.crit.min(500.0);
    let spread = 1.0 + rng.random_range(-attacker.random..=attacker.random) / 100.0;
    let mut dmg = 77.0 * (attacker.patk + power) * shot / defender.pdef.max(1.0) * spread;
    if crit {
        dmg *= 2.0;
    }
    Some((dmg.max(1.0), crit))
}

/// Magic damage: 91 * sqrt(M.Atk) * power / M.Def (spiritshots scale it).
pub fn magical(attacker: &Combat, defender: &Combat, power: f32, shot: f32, rng: &mut impl Rng) -> f32 {
    (91.0 * attacker.matk.max(1.0).sqrt() * power / defender.mdef.max(1.0) * shot * rng.random_range(0.95..1.05)).max(1.0)
}
