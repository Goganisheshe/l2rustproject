//! Server-side game data, imported from the L2J Mobius datapack of the same
//! chronicle (Salvation): NPC templates, spawns, player class templates and
//! the experience table. Point `L2_DATAPACK` at its `dist/game/data` folder.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use bevy::prelude::*;
use rand::Rng;

const DEFAULT_DATAPACK: &str = "G:/visery/l2-datapack/salvation";

#[derive(Debug, Clone)]
pub struct NpcTemplate {
    pub id: u32,
    pub level: u32,
    /// Monster, Folk, RaidBoss, GrandBoss, Guard, Merchant, Teleporter...
    pub kind: String,
    pub name: String,
    pub title: String,
    pub hp: f32,
    pub mp: f32,
    pub patk: f32,
    pub matk: f32,
    pub pdef: f32,
    pub mdef: f32,
    pub atk_speed: f32,
    /// Unreal units.
    pub atk_range: f32,
    pub run_speed: f32,
    pub walk_speed: f32,
    pub exp: u64,
    pub sp: u64,
    /// 0 = passive.
    pub aggro_range: f32,
    pub radius: f32,
    pub height: f32,
}

impl NpcTemplate {
    pub fn is_monster(&self) -> bool {
        matches!(self.kind.as_str(), "Monster" | "RaidBoss" | "GrandBoss" | "FeedableBeast" | "FriendlyMob")
    }
}

#[derive(Debug, Clone)]
pub struct SpawnPoint {
    pub npc: u32,
    /// Unreal units.
    pub pos: [f32; 3],
    pub heading: i32,
    pub respawn: f32,
}

#[derive(Debug, Clone, Default)]
pub struct LevelGain {
    pub hp: f32,
    pub mp: f32,
    pub cp: f32,
    pub hp_regen: f32,
    pub mp_regen: f32,
}

#[derive(Debug, Clone, Default)]
pub struct PlayerTemplate {
    pub class_id: u32,
    pub creation_points: Vec<[f32; 3]>,
    pub str_: f32,
    pub dex: f32,
    pub con: f32,
    pub int: f32,
    pub wit: f32,
    pub men: f32,
    pub patk: f32,
    pub matk: f32,
    pub pdef: f32,
    pub mdef: f32,
    pub patk_speed: f32,
    pub matk_speed: f32,
    pub atk_range: f32,
    pub run_speed: f32,
    pub walk_speed: f32,
    pub levels: Vec<LevelGain>,
}

#[derive(Resource, Default)]
pub struct Datapack {
    pub npcs: HashMap<u32, NpcTemplate>,
    pub spawns: Vec<SpawnPoint>,
    /// Spawn indices per map tile `(x, y)`.
    pub spawns_by_tile: HashMap<(i32, i32), Vec<usize>>,
    /// Starting class templates by file stem (`HumanFighter`, `ElvenMystic`...).
    pub classes: HashMap<String, PlayerTemplate>,
    /// Total experience needed to reach each level (index = level).
    pub exp: Vec<u64>,
}

impl Datapack {
    /// Level for a total amount of experience.
    pub fn level_for(&self, exp: u64) -> u32 {
        let mut lvl = 1;
        for (l, &need) in self.exp.iter().enumerate().skip(1) {
            if exp >= need {
                lvl = l as u32;
            }
        }
        lvl.max(1)
    }
}

pub struct DatapackPlugin;

impl Plugin for DatapackPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Datapack>().add_systems(PreStartup, load_datapack);
    }
}

fn load_datapack(mut dp: ResMut<Datapack>) {
    let root = std::env::var_os("L2_DATAPACK").map(PathBuf::from).unwrap_or_else(|| DEFAULT_DATAPACK.into());
    let start = std::time::Instant::now();
    *dp = load(&root);
    info!(
        "datapack {}: {} npcs, {} spawns, {} classes, max level {} ({:.1}s)",
        root.display(),
        dp.npcs.len(),
        dp.spawns.len(),
        dp.classes.len(),
        dp.exp.len().saturating_sub(1),
        start.elapsed().as_secs_f32()
    );
}

fn xml_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            xml_files(&p, out);
        } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("xml")) {
            out.push(p);
        }
    }
}

fn attr<T: std::str::FromStr>(n: roxmltree::Node, name: &str) -> Option<T> {
    n.attribute(name)?.parse().ok()
}

fn child<'a, 'i>(n: roxmltree::Node<'a, 'i>, name: &str) -> Option<roxmltree::Node<'a, 'i>> {
    n.children().find(|c| c.has_tag_name(name))
}

fn text<T: std::str::FromStr>(n: roxmltree::Node, name: &str) -> Option<T> {
    child(n, name)?.text()?.trim().parse().ok()
}

pub fn load(root: &Path) -> Datapack {
    let mut dp = Datapack::default();

    // NPC templates.
    let mut files = Vec::new();
    xml_files(&root.join("stats/npcs"), &mut files);
    for f in files {
        let Ok(src) = std::fs::read_to_string(&f) else { continue };
        let Ok(doc) = roxmltree::Document::parse(&src) else { continue };
        for n in doc.root_element().children().filter(|c| c.has_tag_name("npc")) {
            let Some(id) = attr::<u32>(n, "id") else { continue };
            let stats = child(n, "stats");
            let vitals = stats.and_then(|s| child(s, "vitals"));
            let attack = stats.and_then(|s| child(s, "attack"));
            let defence = stats.and_then(|s| child(s, "defence"));
            let speed = stats.and_then(|s| child(s, "speed"));
            let g = |n: Option<roxmltree::Node>, a: &str, d: f32| n.and_then(|n| attr::<f32>(n, a)).unwrap_or(d);
            let acquire = child(n, "acquire");
            let collision = child(n, "collision");
            dp.npcs.insert(
                id,
                NpcTemplate {
                    id,
                    level: attr(n, "level").unwrap_or(1),
                    kind: n.attribute("type").unwrap_or("Folk").to_string(),
                    name: n.attribute("name").unwrap_or("").to_string(),
                    title: n.attribute("title").unwrap_or("").to_string(),
                    hp: g(vitals, "hp", 100.0),
                    mp: g(vitals, "mp", 50.0),
                    patk: g(attack, "physical", 10.0),
                    matk: g(attack, "magical", 10.0),
                    pdef: g(defence, "physical", 40.0),
                    mdef: g(defence, "magical", 30.0),
                    atk_speed: g(attack, "attackSpeed", 253.0),
                    atk_range: g(attack, "range", 40.0),
                    run_speed: g(speed.and_then(|s| child(s, "run")), "ground", 80.0),
                    walk_speed: g(speed.and_then(|s| child(s, "walk")), "ground", 40.0),
                    exp: acquire.and_then(|a| attr(a, "exp")).unwrap_or(0),
                    sp: acquire.and_then(|a| attr(a, "sp")).unwrap_or(0),
                    aggro_range: g(child(n, "ai"), "aggroRange", 0.0),
                    radius: g(collision.and_then(|c| child(c, "radius")), "normal", 10.0),
                    height: g(collision.and_then(|c| child(c, "height")), "normal", 20.0),
                },
            );
        }
    }

    // Spawns: fixed points and territories (random points in a polygon).
    let mut files = Vec::new();
    xml_files(&root.join("spawns"), &mut files);
    let mut rng = rand::rng();
    for f in files {
        if f.components().any(|c| c.as_os_str() == "old") {
            continue;
        }
        let Ok(src) = std::fs::read_to_string(&f) else { continue };
        let Ok(doc) = roxmltree::Document::parse(&src) else { continue };
        for spawn in doc.descendants().filter(|c| c.has_tag_name("spawn")) {
            let territories: Vec<(Vec<[f32; 2]>, f32, f32)> = spawn
                .descendants()
                .filter(|c| c.has_tag_name("territory"))
                .map(|t| {
                    let nodes = t
                        .children()
                        .filter(|c| c.has_tag_name("node"))
                        .filter_map(|c| Some([attr(c, "x")?, attr(c, "y")?]))
                        .collect();
                    (nodes, attr(t, "minZ").unwrap_or(0.0), attr(t, "maxZ").unwrap_or(0.0))
                })
                .collect();
            for n in spawn.descendants().filter(|c| c.has_tag_name("npc")) {
                let Some(npc) = attr::<u32>(n, "id") else { continue };
                let respawn = n.attribute("respawnTime").map(parse_duration).unwrap_or(60.0);
                if let (Some(x), Some(y)) = (attr::<f32>(n, "x"), attr::<f32>(n, "y")) {
                    dp.spawns.push(SpawnPoint {
                        npc,
                        pos: [x, y, attr(n, "z").unwrap_or(0.0)],
                        heading: attr(n, "heading").unwrap_or(0),
                        respawn,
                    });
                } else if let Some((poly, min_z, max_z)) = territories.first() {
                    let count: u32 = attr(n, "count").unwrap_or(1);
                    for _ in 0..count {
                        if let Some([x, y]) = random_point_in(poly, &mut rng) {
                            dp.spawns.push(SpawnPoint {
                                npc,
                                pos: [x, y, (min_z + max_z) * 0.5],
                                heading: rng.random_range(0..65536),
                                respawn,
                            });
                        }
                    }
                }
            }
        }
    }
    for (i, s) in dp.spawns.iter().enumerate() {
        let tile = crate::streaming::tile_of(s.pos[0], s.pos[1]);
        dp.spawns_by_tile.entry(tile).or_default().push(i);
    }

    // Starting class templates.
    let mut files = Vec::new();
    xml_files(&root.join("stats/players/templates/StartingClass"), &mut files);
    for f in files {
        let Ok(src) = std::fs::read_to_string(&f) else { continue };
        let Ok(doc) = roxmltree::Document::parse(&src) else { continue };
        let r = doc.root_element();
        let Some(sd) = child(r, "staticData") else { continue };
        let sum = |name: &str| -> f32 {
            child(sd, name).map(|n| n.children().filter_map(|c| c.text()?.trim().parse::<f32>().ok()).sum()).unwrap_or(0.0)
        };
        let mut t = PlayerTemplate {
            class_id: text(r, "classId").unwrap_or(0),
            str_: text(sd, "baseSTR").unwrap_or(40.0),
            dex: text(sd, "baseDEX").unwrap_or(30.0),
            con: text(sd, "baseCON").unwrap_or(40.0),
            int: text(sd, "baseINT").unwrap_or(30.0),
            wit: text(sd, "baseWIT").unwrap_or(30.0),
            men: text(sd, "baseMEN").unwrap_or(30.0),
            patk: text(sd, "basePAtk").unwrap_or(4.0),
            matk: text(sd, "baseMAtk").unwrap_or(6.0),
            pdef: sum("basePDef"),
            mdef: sum("baseMDef"),
            patk_speed: text(sd, "basePAtkSpd").unwrap_or(300.0),
            matk_speed: text(sd, "baseMAtkSpd").unwrap_or(333.0),
            atk_range: text(sd, "baseAtkRange").unwrap_or(20.0),
            run_speed: child(sd, "baseMoveSpd").and_then(|m| text(m, "run")).unwrap_or(120.0),
            walk_speed: child(sd, "baseMoveSpd").and_then(|m| text(m, "walk")).unwrap_or(80.0),
            ..default()
        };
        if let Some(cp) = child(sd, "creationPoints") {
            t.creation_points = cp
                .children()
                .filter(|c| c.has_tag_name("node"))
                .filter_map(|c| Some([attr(c, "x")?, attr(c, "y")?, attr(c, "z")?]))
                .collect();
        }
        if let Some(lv) = child(r, "lvlUpgainData") {
            for l in lv.children().filter(|c| c.has_tag_name("level")) {
                t.levels.push(LevelGain {
                    hp: text(l, "hp").unwrap_or(100.0),
                    mp: text(l, "mp").unwrap_or(50.0),
                    cp: text(l, "cp").unwrap_or(50.0),
                    hp_regen: text(l, "hpRegen").unwrap_or(2.0),
                    mp_regen: text(l, "mpRegen").unwrap_or(1.0),
                });
            }
        }
        let stem = f.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        dp.classes.insert(stem, t);
    }

    // Experience table.
    if let Ok(src) = std::fs::read_to_string(root.join("stats/players/experience.xml")) {
        if let Ok(doc) = roxmltree::Document::parse(&src) {
            let mut exp = vec![0u64; 2];
            for e in doc.root_element().children().filter(|c| c.has_tag_name("experience")) {
                let (Some(l), Some(t)) = (attr::<usize>(e, "level"), attr::<u64>(e, "tolevel")) else { continue };
                if exp.len() <= l {
                    exp.resize(l + 1, 0);
                }
                exp[l] = t;
            }
            dp.exp = exp;
        }
    }
    dp
}

/// "60sec", "5min", "12hour", "1day" -> seconds.
fn parse_duration(s: &str) -> f32 {
    let digits: String = s.chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
    let n: f32 = digits.parse().unwrap_or(60.0);
    let unit = &s[digits.len()..];
    match unit {
        u if u.starts_with("min") => n * 60.0,
        u if u.starts_with("hour") => n * 3600.0,
        u if u.starts_with("day") => n * 86400.0,
        u if u.starts_with("ms") => n / 1000.0,
        _ => n,
    }
}

fn random_point_in(poly: &[[f32; 2]], rng: &mut impl Rng) -> Option<[f32; 2]> {
    if poly.len() < 3 {
        return poly.first().copied();
    }
    let (mut min, mut max) = (poly[0], poly[0]);
    for p in poly {
        min = [min[0].min(p[0]), min[1].min(p[1])];
        max = [max[0].max(p[0]), max[1].max(p[1])];
    }
    for _ in 0..30 {
        let p = [rng.random_range(min[0]..=max[0]), rng.random_range(min[1]..=max[1])];
        if inside(poly, p) {
            return Some(p);
        }
    }
    None
}

fn inside(poly: &[[f32; 2]], p: [f32; 2]) -> bool {
    let mut c = false;
    let mut j = poly.len() - 1;
    for i in 0..poly.len() {
        let (a, b) = (poly[i], poly[j]);
        if (a[1] > p[1]) != (b[1] > p[1]) && p[0] < (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0] {
            c = !c;
        }
        j = i;
    }
    c
}
