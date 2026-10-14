//! Lineage II client data tables (`system/*.dat`).
//!
//! Files start with the UTF-16 header `Lineage2Ver413`, followed by 128-byte
//! RSA blocks and a 20-byte tail. Each block decrypts (public key) to a
//! length byte plus payload; the joined payload is a 4-byte size and a zlib
//! stream. Strings in newer tables are indices into `L2GameDataName.dat`.
//!
//! Table layouts follow the community L2ClientDat definitions for this
//! client: skillgrp = "Salvation", npcgrp = "GrandCrusade", skillname and
//! animationcombo = "Helios".

use std::collections::HashMap;
use std::io::Read;
use std::path::Path;

use num_bigint::BigUint;

use crate::{Error, Reader};

/// Public modulus used by Lineage2Ver413 files (exponent 0x1d).
const RSA_413_MODULUS: &str = "75b4d6de5c016544068a1acf125869f43d2e09fc55b8b1e289556daf9b8757635593446288b3653da1ce91c87bb1a5c18f16323495c55d7d72c0890a83f69bfd1fd9434eb1c02f3e4679edfa43309319070129c267c85604d87bb65bae205de3707af1d2108881abb567c3b3d069ae67c3a4c6a3aa93d26413d4c66094ae2039";
const RSA_413_EXPONENT: u32 = 0x1d;

/// Decrypts and decompresses a `Lineage2Ver41x` file.
pub fn decrypt_dat(data: &[u8]) -> Result<Vec<u8>, Error> {
    if data.len() < 28 + 20 + 128 {
        return Err(Error::Format("dat file too short".into()));
    }
    let n = BigUint::parse_bytes(RSA_413_MODULUS.as_bytes(), 16).unwrap();
    let e = BigUint::from(RSA_413_EXPONENT);
    let body = &data[28..data.len() - 20];
    let mut joined = Vec::with_capacity(body.len());
    for block in body.chunks_exact(128) {
        let m = BigUint::from_bytes_be(block).modpow(&e, &n).to_bytes_be();
        let mut padded = vec![0u8; 128usize.saturating_sub(m.len())];
        padded.extend_from_slice(&m);
        let size = padded[3] as usize;
        let pad = (4 - size % 4) % 4;
        if size + pad > 124 {
            return Err(Error::Format("bad RSA block (wrong key?)".into()));
        }
        joined.extend_from_slice(&padded[128 - size - pad..128 - pad]);
    }
    if joined.len() < 4 {
        return Err(Error::Format("empty dat".into()));
    }
    let expected = u32::from_le_bytes(joined[..4].try_into().unwrap()) as usize;
    let mut out = Vec::with_capacity(expected);
    flate2::read::ZlibDecoder::new(&joined[4..])
        .read_to_end(&mut out)
        .map_err(|e| Error::Format(format!("zlib: {e}")))?;
    Ok(out)
}

pub fn read_dat(path: impl AsRef<Path>) -> Result<Vec<u8>, Error> {
    decrypt_dat(&std::fs::read(path)?)
}

/// String table from `L2GameDataName.dat`.
pub struct GameDataNames(pub Vec<String>);

impl GameDataNames {
    pub fn parse(data: &[u8]) -> Result<Self, Error> {
        let mut r = Reader::new(data);
        let n = r.u32()? as usize;
        let mut v = Vec::with_capacity(n);
        for _ in 0..n {
            v.push(unicode(&mut r)?);
        }
        Ok(Self(v))
    }

    pub fn get(&self, i: u32) -> &str {
        self.0.get(i as usize).map(String::as_str).unwrap_or("")
    }
}

/// `UNICODE`: u32 byte length + UTF-16LE.
fn unicode(r: &mut Reader) -> Result<String, Error> {
    let n = r.u32()? as usize;
    let b = r.bytes(n)?;
    let units: Vec<u16> = b.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
    Ok(String::from_utf16_lossy(&units))
}

/// `ASCF`: UE compact length; positive = single-byte (cp1251 here), negative = UTF-16.
fn ascf(r: &mut Reader) -> Result<String, Error> {
    let n = r.compact()?;
    if n >= 0 {
        let b = r.bytes(n as usize)?;
        let b = b.strip_suffix(&[0]).unwrap_or(b);
        Ok(b.iter().map(|&c| cp1251(c)).collect())
    } else {
        let b = r.bytes((-n) as usize * 2)?;
        let units: Vec<u16> =
            b.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).take_while(|&u| u != 0).collect();
        Ok(String::from_utf16_lossy(&units))
    }
}

fn cp1251(c: u8) -> char {
    match c {
        0xC0..=0xFF => char::from_u32(0x410 + (c as u32 - 0xC0)).unwrap(),
        0xA8 => 'Ё',
        0xB8 => 'ё',
        _ => c as char,
    }
}

fn i16(r: &mut Reader) -> Result<i16, Error> {
    Ok(r.u16()? as i16)
}

#[derive(Debug, Clone)]
pub struct SkillGrp {
    pub id: u32,
    pub level: u32,
    pub sublevel: i16,
    pub operate_type: u8,
    pub magic_type: u8,
    pub mp_consume: i16,
    /// Unreal units.
    pub cast_range: u32,
    pub cast_style: u8,
    pub hit_time: f32,
    pub cool_time: f32,
    pub reuse_delay: f32,
    /// Negative for offensive skills.
    pub effect_point: i32,
    pub is_magic: u8,
    /// Animation combo codes (see `AnimationCombo`).
    pub animations: Vec<String>,
    pub visual_effect: String,
    pub icon: String,
    pub debuff: u8,
}

pub fn parse_skillgrp(data: &[u8], names: &GameDataNames) -> Result<Vec<SkillGrp>, Error> {
    let mut r = Reader::new(data);
    let n = r.u32()? as usize;
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        let id = r.u16()? as u32;
        let level = r.u8()? as u32;
        let sublevel = i16(&mut r)?;
        let _icon_type = r.u8()?;
        let magic_type = r.u8()?;
        let operate_type = r.u8()?;
        let mp_consume = i16(&mut r)?;
        let cast_range = r.u32()?;
        let cast_style = r.u8()?;
        let hit_time = r.f32()?;
        let cool_time = r.f32()?;
        let reuse_delay = r.f32()?;
        let effect_point = r.i32()?;
        let is_magic = r.u8()?;
        let _origin = i16(&mut r)?;
        let _double = r.u8()?;
        let na = r.u32()? as usize;
        let mut animations = Vec::with_capacity(na);
        for _ in 0..na {
            animations.push(anim_code(names.get(r.u32()?)));
        }
        let visual_effect = names.get(r.u32()?).to_string();
        let icon = names.get(r.u32()?).to_string();
        let _icon_panel = r.u32()?;
        let debuff = r.u8()?;
        let _resist = r.u8()?;
        let _ench_level = r.u8()?;
        let _ench_icon = r.u32()?;
        let _hp = i16(&mut r)?;
        r.u8()?;
        r.u8()?;
        out.push(SkillGrp {
            id,
            level,
            sublevel,
            operate_type,
            magic_type,
            mp_consume,
            cast_range,
            cast_style,
            hit_time,
            cool_time,
            reuse_delay,
            effect_point,
            is_magic,
            animations,
            visual_effect,
            icon,
            debuff,
        });
    }
    Ok(out)
}

/// Some single-letter combo codes share a string-table slot with an
/// unrelated "L2Day - X" entry; the code is the part after the dash.
fn anim_code(s: &str) -> String {
    match s.strip_prefix("L2Day - ") {
        Some(code) => code.to_string(),
        None => s.to_string(),
    }
}

/// English skill names by (id, level) from `SkillName-*.dat`.
pub fn parse_skillname(data: &[u8]) -> Result<HashMap<(u32, u32), String>, Error> {
    let mut r = Reader::new(data);
    let n = r.compact()?.max(0) as usize;
    let mut text = HashMap::with_capacity(n);
    for _ in 0..n {
        let t = ascf(&mut r)?;
        let idx = r.u32()?;
        text.insert(idx, t);
    }
    let m = r.u32()? as usize;
    let mut out = HashMap::with_capacity(m);
    for _ in 0..m {
        let id = r.u32()?;
        let level = r.u16()? as u32;
        r.skip(2 + 4 + 2 + 2)?; // sublevel, prev skill
        let name = r.u32()?;
        r.skip(4 * 6)?;
        if let Some(t) = text.get(&name) {
            out.entry((id, level)).or_insert_with(|| t.clone());
        }
    }
    Ok(out)
}

/// Animation combo: code -> up to three animation variable names
/// (e.g. `CastMidAnimName`, `CastEndAnimName`, `MagicShotAnimName`).
#[derive(Debug, Clone)]
pub struct AnimationCombo {
    pub anims: Vec<String>,
    /// First animation loops for the cast time.
    pub looped: bool,
}

pub fn parse_animationcombo(data: &[u8], names: &GameDataNames) -> Result<HashMap<String, AnimationCombo>, Error> {
    let mut r = Reader::new(data);
    let n = r.u32()? as usize;
    let mut out = HashMap::with_capacity(n);
    for _ in 0..n {
        let code = anim_code(names.get(r.u32()?));
        let anims: Vec<String> = (0..3)
            .map(|_| ascf(&mut r))
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .filter(|a| !a.eq_ignore_ascii_case("none") && !a.is_empty())
            .collect();
        let looped = r.u32()? == 1;
        out.insert(code, AnimationCombo { anims, looped });
    }
    Ok(out)
}

#[derive(Debug, Clone)]
pub struct NpcGrp {
    pub id: u32,
    /// Pawn class, e.g. `LineageMonster.gremlin`.
    pub class_name: String,
    /// Skeletal mesh, e.g. `LineageMonsters.gremlin_m00`.
    pub mesh: String,
    /// Materials per mesh section, overriding the mesh's own.
    pub textures: Vec<String>,
    pub speed: f32,
    pub draw_scale: f32,
    pub collision_radius: f32,
    pub collision_height: f32,
    pub hp: f64,
    pub npc_type: i16,
}

pub fn parse_npcgrp(data: &[u8], names: &GameDataNames) -> Result<Vec<NpcGrp>, Error> {
    let mut r = Reader::new(data);
    let n = r.u32()? as usize;
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        let id = r.u16()? as u32;
        let class_name = names.get(r.u32()?).to_string();
        let mesh = names.get(r.u32()?).to_string();
        let nt = r.compact()?.max(0) as usize;
        let mut textures = Vec::with_capacity(nt);
        for _ in 0..nt {
            textures.push(names.get(r.u32()?).to_string());
        }
        let n2 = r.u32()? as usize;
        r.skip(n2 * 4)?;
        let np = r.compact()?.max(0) as usize;
        r.skip(np * 2)?;
        let speed = r.f32()?;
        for _ in 0..3 {
            let k = r.compact()?.max(0) as usize; // attack / defense / damage sounds
            r.skip(k * 4)?;
        }
        let k = r.compact()?.max(0) as usize; // deco effects
        r.skip(k * 8)?;
        let k = r.compact()?.max(0) as usize; // quests
        r.skip(k * 3)?;
        r.u32()?; // attack effect
        r.skip(5)?; // sound vol/radius/random, social, hp showable
        let k = r.u32()? as usize; // dialog sounds
        r.skip(k * 4)?;
        r.skip(4)?; // silhouette, summon sort/max/grade
        let draw_scale = r.f32()?;
        r.f32()?; // zoom-in cam
        r.u32()?; // icon
        r.u8()?;
        r.skip(4)?; // ground high/low
        let collision_radius = r.f32()?;
        r.f32()?;
        let collision_height = r.f32()?;
        r.f32()?;
        r.skip(12)?; // equipment slots
        let hp = f64::from_le_bytes(r.bytes(8)?.try_into().unwrap());
        r.skip(8)?; // mp
        let npc_type = i16(&mut r)?;
        out.push(NpcGrp {
            id,
            class_name,
            mesh,
            textures,
            speed,
            draw_scale,
            collision_radius,
            collision_height,
            hp,
            npc_type,
        });
    }
    Ok(out)
}

#[derive(Debug, Clone)]
pub struct WeaponGrp {
    pub id: u32,
    /// Static meshes, e.g. `LineageWeapons.small_sword_m00_wp`; two for dual weapons.
    pub meshes: Vec<String>,
    pub textures: Vec<String>,
    pub body_part: u8,
    pub handness: u8,
    pub weapon_type: u8,
    pub drop: ItemDrop,
}

/// Parses `weapongrp.dat` (L2ClientDat "Helios" layout, used by Salvation clients).
pub fn parse_weapongrp(data: &[u8], names: &GameDataNames) -> Result<Vec<WeaponGrp>, Error> {
    let mut r = Reader::new(data);
    let n = r.u32()? as usize;
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        let (id, drop) = item_head(&mut r, names)?;
        r.skip(20)?; // icons
        r.skip(4)?; // durability, weight
        r.skip(2)?; // material, crystallizable
        let q = r.u8()? as usize;
        r.skip(q * 2)?;
        r.skip(2)?; // color, attribution
        r.skip(2)?; // property params
        r.skip(8)?; // icon panel, drop sound type
        r.u8()?; // inventory type
        let body_part = r.u8()?;
        let handness = r.u8()?;
        let m = r.u8()? as usize;
        let mut meshes = Vec::with_capacity(m);
        for _ in 0..m {
            meshes.push(names.get(r.u32()?).to_string());
        }
        r.skip(m)?;
        let t = r.u8()? as usize;
        let mut textures = Vec::with_capacity(t);
        for _ in 0..t {
            textures.push(names.get(r.u32()?).to_string());
        }
        let s = r.u8()? as usize;
        r.skip(s * 4)?;
        r.skip(12)?; // drop / equip sound, effect
        r.u8()?; // random damage
        let weapon_type = r.u8()?;
        r.skip(4)?; // grade, mp, soulshots, spiritshots
        r.skip(2)?; // curvature
        r.skip(3)?; // unk, hero, magic
        r.skip(4)?; // ertheia fist scale
        r.skip(2)?;
        let e = r.u8()? as usize;
        r.skip(e * (4 + 4 * 21) + 4)?;
        r.skip(6)?; // variation effect types
        let v = r.u8()? as usize;
        r.skip(v * 4)?;
        r.skip(2)?; // ensoul slots
        out.push(WeaponGrp { id, meshes, textures, body_part, handness, weapon_type, drop });
    }
    Ok(out)
}

/// One race/gender slot of an armor: mesh and its section textures.
#[derive(Debug, Clone, Default)]
pub struct ArmorModel {
    pub meshes: Vec<String>,
    pub textures: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ArmorGrp {
    pub id: u32,
    pub body_part: u8,
    /// Slots in client order: m/f HumnFigh, DarkElf, Dorf, Elf, HumnMyst, OrcFigh,
    /// OrcMage, Kamael, Ertheia, then NPC.
    pub models: Vec<ArmorModel>,
    pub armor_type: u8,
    pub hide_mask: u16,
    pub drop: ItemDrop,
}

pub const ARMOR_SLOTS: [&str; 19] = [
    "m_HumnFigh", "f_HumnFigh", "m_DarkElf", "f_DarkElf", "m_Dorf", "f_Dorf", "m_Elf", "f_Elf",
    "m_HumnMyst", "f_HumnMyst", "m_OrcFigh", "f_OrcFigh", "m_OrcMage", "f_OrcMage", "m_Kamael",
    "f_Kamael", "m_Ertheia", "f_Ertheia", "NPC",
];

/// Parses `armorgrp.dat` (L2ClientDat "Helios" layout).
pub fn parse_armorgrp(data: &[u8], names: &GameDataNames) -> Result<Vec<ArmorGrp>, Error> {
    let mut r = Reader::new(data);
    let n = r.u32()? as usize;
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        let (id, drop) = item_head(&mut r, names)?;
        r.skip(20 + 4 + 2)?;
        let q = r.u8()? as usize;
        r.skip(q * 2)?;
        r.skip(4 + 8)?;
        r.u8()?; // inventory type
        let body_part = r.u8()?;
        let mut models = Vec::with_capacity(ARMOR_SLOTS.len());
        for _ in 0..ARMOR_SLOTS.len() {
            let m = r.u8()? as usize;
            let mut meshes = Vec::with_capacity(m);
            for _ in 0..m {
                meshes.push(names.get(r.u32()?).to_string());
            }
            let t = r.u8()? as usize;
            let mut textures = Vec::with_capacity(t);
            for _ in 0..t {
                textures.push(names.get(r.u32()?).to_string());
            }
            models.push(ArmorModel { meshes, textures });
            // additional meshes (unused): meshes, per-mesh flags, textures, effect
            let a = r.u8()? as usize;
            r.skip(a * 6)?;
            let b = r.u8()? as usize;
            r.skip(b * 4 + 4)?;
        }
        r.u32()?; // attack effect
        let s = r.u8()? as usize;
        r.skip(s * 4)?;
        r.skip(12)?; // drop/equip sound, unk
        r.u8()?;
        let armor_type = r.u8()?;
        r.u8()?; // grade
        r.skip(2)?; // mp bonus
        let hide_mask = r.u16()?;
        r.skip(3)?;
        out.push(ArmorGrp { id, body_part, models, armor_type, hide_mask, drop });
    }
    Ok(out)
}

/// How an item looks lying on the ground: static mesh and its textures.
#[derive(Debug, Clone, Default)]
pub struct ItemDrop {
    pub mesh: String,
    pub textures: Vec<String>,
}

/// Common start of weapongrp / armorgrp / etcitemgrp records: tag, item id
/// and the drop models (the first one is used).
fn item_head(r: &mut Reader, names: &GameDataNames) -> Result<(u32, ItemDrop), Error> {
    r.u8()?; // tag
    let id = r.u32()?;
    r.skip(4)?; // drop type, anim, radius, height
    let k = r.u8()? as usize;
    let mut drop = ItemDrop::default();
    for i in 0..k {
        let mesh = names.get(r.u32()?).to_string();
        let t = r.u8()? as usize;
        let mut textures = Vec::with_capacity(t);
        for _ in 0..t {
            textures.push(names.get(r.u32()?).to_string());
        }
        if i == 0 {
            drop = ItemDrop { mesh, textures };
        }
    }
    Ok((id, drop))
}

/// Parses `etcitemgrp.dat` (L2ClientDat "Helios" layout): only the drop models.
pub fn parse_etcitemgrp(data: &[u8], names: &GameDataNames) -> Result<Vec<(u32, ItemDrop)>, Error> {
    let mut r = Reader::new(data);
    let n = r.u32()? as usize;
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        let (id, drop) = item_head(&mut r, names)?;
        r.skip(20 + 4 + 2)?; // icons, durability, weight, material, crystallizable
        let q = r.u8()? as usize;
        r.skip(q * 2)?;
        r.skip(4 + 8)?; // color, attribution, params, icon panel, drop sound type
        r.u8()?; // inventory type
        let m = r.u8()? as usize;
        r.skip(m * 4)?;
        let t = r.u8()? as usize;
        r.skip(t * 4)?;
        r.skip(8)?; // drop / equip sound
        r.u8()?; // consume type
        r.u32()?; // etcitem type
        r.u8()?; // grade
        out.push((id, drop));
    }
    Ok(out)
}

/// Parses `sysstring-*.dat`: UI strings by id (window titles, labels).
pub fn parse_sysstring(data: &[u8]) -> Result<HashMap<u32, String>, Error> {
    let mut r = Reader::new(data);
    let n = r.u32()? as usize;
    let mut out = HashMap::with_capacity(n);
    for _ in 0..n {
        let id = r.u32()?;
        let s = ascf(&mut r)?;
        out.insert(id, s);
    }
    Ok(out)
}
