//! Skill visuals, NPC models and equipment models from the client's data tables.
//!
//! A skill is assembled from three sources:
//! - `skillgrp.dat`: timings, range, power, magic flag and an animation combo
//!   code (`E`, `S`, `SMB01`...);
//! - `animationcombo.dat`: combo code -> pawn animation variables
//!   (`CastMidAnimName`, `CastEndAnimName`, `MagicShotAnimName`), which name
//!   clips on the character (`CastMid_MMagic`, `SpAtk01_1HS_MFighter`...);
//! - `lineageskilleffect.u`: class `l2_<id>_skill` with casting, shot and
//!   explosion emitters pointing at effect classes, plus the flying time.
//!
//! Monsters come from `npcgrp.dat`: mesh, per-slot textures and collision size.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use bevy::prelude::*;
use ue2::dat;
use ue2::props::{class_defaults, Properties};
use ue2::{ObjectRef, Package, Reader};

use crate::l2assets::{L2Library, UU_TO_M};

const DEFAULT_CLIENT_DIR: &str = "G:/visery/l2-client";

/// One visual effect of a skill stage.
#[derive(Debug, Clone)]
pub struct FxSpec {
    /// Effect class name, e.g. `el_wind_strike_ca`.
    pub class: String,
    /// Spawn on the target instead of the caster.
    pub on_target: bool,
    /// Attached to a body part (follows the caster); otherwise free-flying.
    pub attached: bool,
    /// Offset in the caster's frame, meters (X forward).
    pub offset: Vec3,
}

#[derive(Debug, Clone)]
pub struct SkillDef {
    pub id: u32,
    pub name: String,
    /// Icon texture as `package.object`, e.g. `icon.skill1177`.
    pub icon: String,
    pub magic: bool,
    /// |effect_point| from skillgrp.
    pub power: f32,
    /// Cast range in meters.
    pub range: f32,
    pub hit_time: f32,
    pub reuse: f32,
    /// Clip base names in play order (from the animation combo).
    pub anims: Vec<String>,
    /// First clip loops while casting.
    pub looped: bool,
    pub casting: Vec<FxSpec>,
    pub shot: Vec<FxSpec>,
    pub explosion: Vec<FxSpec>,
    /// Projectile flight time in seconds; 0 = instant.
    pub flying_time: f32,
}

#[derive(Resource, Default)]
pub struct GameData {
    /// Every npcgrp entry's model: id -> (package, model spec with textures).
    pub npc_models: HashMap<u32, (String, String)>,
    /// Client-side visuals (animations, effects, icon) for every skill id.
    pub visuals: HashMap<u32, Arc<SkillDef>>,
    /// Weapon models by item id (weapongrp).
    pub weapons: HashMap<u32, dat::WeaponGrp>,
    /// Armor models per race by item id (armorgrp).
    pub armors: HashMap<u32, dat::ArmorGrp>,
}

pub struct GameDataPlugin;

impl Plugin for GameDataPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<GameData>().add_systems(PreStartup, load_game_data);
    }
}

fn load_game_data(mut data: ResMut<GameData>, mut lib: ResMut<L2Library>) {
    let root = std::env::var_os("L2_CLIENT_DIR").map(PathBuf::from).unwrap_or_else(|| DEFAULT_CLIENT_DIR.into());
    match load(&root, &mut lib) {
        Ok(d) => {
            info!("game data: {} skill visuals, {} npc models", d.visuals.len(), d.npc_models.len());
            *data = d;
        }
        Err(e) => warn!("could not load client data tables: {e}"),
    }
}

fn load(root: &std::path::Path, lib: &mut L2Library) -> Result<GameData, ue2::Error> {
    let sys = root.join("system");
    let names = dat::GameDataNames::parse(&dat::read_dat(sys.join("l2gamedataname.dat"))?)?;
    let skillgrp = dat::parse_skillgrp(&dat::read_dat(sys.join("skillgrp.dat"))?, &names)?;
    let skill_names = dat::parse_skillname(&dat::read_dat(sys.join("skillname-e.dat")).or_else(|_| {
        dat::read_dat(sys.join("skillname-ru.dat"))
    })?)?;
    let combos = dat::parse_animationcombo(&dat::read_dat(sys.join("animationcombo.dat"))?, &names)?;
    let npcs = dat::parse_npcgrp(&dat::read_dat(sys.join("npcgrp.dat"))?, &names)?;
    let weapons = dat::parse_weapongrp(&dat::read_dat(sys.join("weapongrp.dat"))?, &names)?;
    let armors = dat::parse_armorgrp(&dat::read_dat(sys.join("armorgrp.dat"))?, &names)?;

    let effects = lib.lib.as_mut().and_then(|l| l.package("lineageskilleffect"));
    let visuals = effects.map(|p| skill_visuals(&p)).unwrap_or_default();

    // Every skill's client visuals (any operate type).
    let mut all_visuals = HashMap::new();
    for s in skillgrp.iter().filter(|s| s.level == 1 && s.sublevel == 0) {
        let combo = s.animations.first().and_then(|c| combos.get(c));
        let v = visuals.get(&s.id).cloned().unwrap_or_default();
        let def = Arc::new(SkillDef {
            id: s.id,
            name: skill_names.get(&(s.id, 1)).cloned().unwrap_or_default(),
            icon: s.icon.clone(),
            magic: s.is_magic == 1,
            power: (s.effect_point as f32).abs(),
            range: s.cast_range as f32 * UU_TO_M,
            hit_time: s.hit_time,
            reuse: s.reuse_delay,
            anims: combo.map(|c| c.anims.iter().map(|a| clip_base(a)).collect()).unwrap_or_default(),
            looped: combo.is_some_and(|c| c.looped),
            casting: v.casting.clone(),
            shot: v.shot.clone(),
            explosion: v.explosion.clone(),
            flying_time: v.flying_time,
        });
        all_visuals.insert(s.id, def.clone());
    }

    let mut npc_models = HashMap::new();
    for n in &npcs {
        let Some((pkg, mesh)) = n.mesh.split_once('.') else { continue };
        let textures: Vec<String> = n
            .textures
            .iter()
            .filter_map(|t| {
                let (p, rest) = t.split_once('.')?;
                Some(format!("{p}/{}", rest.rsplit('.').next().unwrap_or(rest)))
            })
            .collect();
        let spec = if textures.is_empty() { mesh.to_string() } else { format!("{mesh}@{}", textures.join("|")) };
        npc_models.insert(n.id, (pkg.to_string(), spec));
    }

    let weapons = weapons.into_iter().map(|w| (w.id, w)).collect();
    let armors = armors.into_iter().map(|a| (a.id, a)).collect();
    Ok(GameData { npc_models, visuals: all_visuals, weapons, armors })
}

/// `CastMidAnimName` -> `CastMid`.
fn clip_base(var: &str) -> String {
    let lower = var.to_ascii_lowercase();
    match lower.rfind("animname") {
        Some(i) if i + 8 == lower.len() => var[..i].to_string(),
        _ => var.to_string(),
    }
}

#[derive(Default, Clone)]
struct Visual {
    casting: Vec<FxSpec>,
    shot: Vec<FxSpec>,
    explosion: Vec<FxSpec>,
    flying_time: f32,
}

/// Reads every `l2_<id>_skill` class in lineageskilleffect.u.
fn skill_visuals(pkg: &Arc<Package>) -> HashMap<u32, Visual> {
    let mut out = HashMap::new();
    for (i, e) in pkg.exports.iter().enumerate() {
        if pkg.class_name(e) != "Class" {
            continue;
        }
        let name = pkg.name(e.object_name);
        let Some(id) = name.strip_prefix("l2_").and_then(|s| s.strip_suffix("_skill")).and_then(|s| s.parse().ok())
        else {
            continue;
        };
        let Some(p) = class_defaults(pkg, i) else { continue };
        let specs = |field: &str| -> Vec<FxSpec> {
            p.object_array(pkg, field).into_iter().filter_map(|r| fx_spec(pkg, r)).collect()
        };
        let v = Visual {
            casting: specs("CastingAction"),
            shot: specs("ShotAction"),
            explosion: specs("ExplosionAction"),
            flying_time: p.float(pkg, "FlyingTime").unwrap_or(0.0),
        };
        out.insert(id, v);
    }
    out
}

fn fx_spec(pkg: &Package, r: ObjectRef) -> Option<FxSpec> {
    let ObjectRef::Export(i) = r else { return None };
    let mut rd = Reader::new(pkg.export_data(i));
    let p = Properties::read(pkg, &mut rd).ok()?;
    let class = pkg.object_name(p.object(pkg, "EffectClass").map(ObjectRef::from_raw)?).to_string();
    if class == "None" {
        return None;
    }
    let o = p.vector(pkg, "offset").unwrap_or([0.0; 3]);
    Some(FxSpec {
        class,
        on_target: p.bool(pkg, "bSpawnOnTarget").unwrap_or(false),
        attached: p.byte(pkg, "AttachOn").is_some(),
        offset: Vec3::new(o[0], o[2], o[1]) * UU_TO_M,
    })
}
