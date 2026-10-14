//! Items from the datapack: templates with stats, starting equipment per
//! class, monster drop lists, the hero's inventory and equipment, and an
//! inventory window (I / gamepad Select).

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use bevy::prelude::*;
use rand::Rng;

use crate::input::PlayerInput;
use crate::l2assets::L2Library;
use crate::mmo::{Hero, Phase};
use crate::rpg::{HeroStats, Log};

const DEFAULT_DATAPACK: &str = "G:/visery/l2-datapack/classic";
pub const ADENA: u32 = 57;

#[derive(Debug, Clone)]
pub struct ItemTemplate {
    pub id: u32,
    pub name: String,
    /// Weapon, Armor, EtcItem.
    pub kind: String,
    pub icon: String,
    /// rhand, lrhand, chest, legs, head, gloves, feet, neck, ...
    pub bodypart: String,
    /// SWORD, BLUNT, DAGGER, BOW, DUAL... for weapons.
    pub weapon_type: String,
    /// Skills cast when the item is used (scrolls, potions).
    pub skills: Vec<u32>,
    /// Shop price in Adena.
    pub price: u64,
    /// Crystal grade (NONE, D, C, B, A, S).
    pub grade: String,
    /// Shots a weapon burns per attack / per spell.
    pub soulshots: u64,
    pub spiritshots: u64,
    /// SoulShots, SpiritShot or BlessedSpiritShot for shot items.
    pub handler: String,
    pub stats: HashMap<String, f32>,
}

impl ItemTemplate {
    pub fn stat(&self, name: &str) -> f32 {
        self.stats.get(name).copied().unwrap_or(0.0)
    }
    pub fn equippable(&self) -> bool {
        !self.bodypart.is_empty() && self.kind != "EtcItem"
    }
    /// Equipment slot; two-handed weapons use the right-hand slot.
    pub fn slot(&self) -> &str {
        match self.bodypart.as_str() {
            "lrhand" => "rhand",
            "alldress" => "chest",
            "rear;lear" => "ear",
            "rfinger;lfinger" => "finger",
            b => b,
        }
    }
}

#[derive(Debug, Clone)]
pub struct DropEntry {
    pub item: u32,
    pub min: u64,
    pub max: u64,
    /// Percent.
    pub chance: f32,
}

#[derive(Debug, Clone)]
pub enum DropRule {
    Single(DropEntry),
    /// Roll `chance`, then pick one entry weighted by its chance.
    Group { chance: f32, items: Vec<DropEntry> },
}

#[derive(Resource, Default)]
pub struct ItemData {
    pub items: HashMap<u32, ItemTemplate>,
    /// classId -> (item, count, equipped).
    pub initial: HashMap<u32, Vec<(u32, u64, bool)>>,
    pub drops: HashMap<u32, Vec<DropRule>>,
}

#[derive(Component, Default)]
pub struct Inventory {
    /// item id -> count.
    pub items: BTreeMap<u32, u64>,
    /// slot -> item id.
    pub equipped: BTreeMap<String, u32>,
    /// Shots set to auto-use (right-click in the client's inventory).
    pub auto_shots: std::collections::BTreeSet<u32>,
}

/// The Classic universal shots work with any weapon grade.
const UNIVERSAL_SHOTS: [u32; 2] = [91927, 91928];

/// Burns the auto-use shot for the equipped weapon: returns the damage
/// multiplier (soulshot x2 for attacks and physical skills; spiritshot x2,
/// blessed x2.2 for magic), or 1 without a matching charged shot.
pub fn use_shot(inv: &mut Inventory, data: &ItemData, magic: bool) -> f32 {
    let Some(weapon) = inv.equipped.get("rhand").and_then(|w| data.items.get(w)) else { return 1.0 };
    let need = if magic { weapon.spiritshots } else { weapon.soulshots }.max(1);
    let pick = inv.auto_shots.iter().copied().find(|id| {
        let Some(t) = data.items.get(id) else { return false };
        let kind_ok = if magic { t.handler == "SpiritShot" || t.handler == "BlessedSpiritShot" } else { t.handler == "SoulShots" };
        kind_ok && (UNIVERSAL_SHOTS.contains(id) || t.grade == weapon.grade) && inv.items.get(id).copied().unwrap_or(0) >= need
    });
    let Some(id) = pick else { return 1.0 };
    let left = inv.items.get_mut(&id).unwrap();
    *left -= need;
    if *left == 0 {
        inv.items.remove(&id);
        inv.auto_shots.remove(&id);
    }
    match data.items.get(&id).map(|t| t.handler.as_str()) {
        Some("BlessedSpiritShot") => 2.2,
        _ => 2.0,
    }
}

pub struct ItemsPlugin;

impl Plugin for ItemsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ItemData>()
            .init_resource::<UseQueue>()
            .init_resource::<InventoryUi>()
            .add_systems(PreStartup, load_items)
            .add_systems(
                Update,
                (give_starting_items, apply_equipment, hero_appearance, inventory_window, use_items).chain().run_if(in_state(Phase::InWorld)),
            );
    }
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

fn load_items(mut data: ResMut<ItemData>) {
    let root = std::env::var_os("L2_DATAPACK").map(PathBuf::from).unwrap_or_else(|| DEFAULT_DATAPACK.into());
    let mut files = Vec::new();
    xml_files(&root.join("stats/items"), &mut files);
    for f in files {
        let Ok(src) = std::fs::read_to_string(&f) else { continue };
        let Ok(doc) = roxmltree::Document::parse(&src) else { continue };
        for n in doc.root_element().children().filter(|c| c.has_tag_name("item")) {
            let Some(id) = attr::<u32>(n, "id") else { continue };
            let set = |name: &str| {
                n.children()
                    .find(|c| c.has_tag_name("set") && c.attribute("name") == Some(name))
                    .and_then(|c| c.attribute("val"))
                    .unwrap_or("")
                    .to_string()
            };
            let stats = n
                .children()
                .find(|c| c.has_tag_name("stats"))
                .map(|s| {
                    s.children()
                        .filter(|c| c.has_tag_name("stat"))
                        .filter_map(|c| Some((c.attribute("type")?.to_string(), c.text()?.trim().parse().ok()?)))
                        .collect()
                })
                .unwrap_or_default();
            data.items.insert(
                id,
                ItemTemplate {
                    id,
                    name: n.attribute("name").unwrap_or("").to_string(),
                    kind: n.attribute("type").unwrap_or("EtcItem").to_string(),
                    icon: set("icon"),
                    bodypart: set("bodypart"),
                    weapon_type: set("weapon_type"),
                    price: set("price").parse().unwrap_or(0),
                    grade: Some(set("crystal_type")).filter(|g| !g.is_empty()).unwrap_or_else(|| "NONE".into()),
                    soulshots: set("soulshots").parse().unwrap_or(0),
                    spiritshots: set("spiritshots").parse().unwrap_or(0),
                    handler: set("handler"),
                    skills: n
                        .children()
                        .filter(|c| c.has_tag_name("skills"))
                        .flat_map(|s| s.children().filter(|c| c.has_tag_name("skill")))
                        .filter_map(|c| attr(c, "id"))
                        .collect(),
                    stats,
                },
            );
        }
    }
    if let Ok(src) = std::fs::read_to_string(root.join("stats/players/initialEquipment.xml")) {
        if let Ok(doc) = roxmltree::Document::parse(&src) {
            for e in doc.root_element().children().filter(|c| c.has_tag_name("equipment")) {
                let Some(class) = attr::<u32>(e, "classId") else { continue };
                let list = e
                    .children()
                    .filter(|c| c.has_tag_name("item"))
                    .filter_map(|c| Some((attr(c, "id")?, attr(c, "count").unwrap_or(1), c.attribute("equipped") == Some("true"))))
                    .collect();
                data.initial.insert(class, list);
            }
        }
    }
    let entry = |c: roxmltree::Node| -> Option<DropEntry> {
        Some(DropEntry {
            item: attr(c, "id")?,
            min: attr(c, "min").unwrap_or(1),
            max: attr(c, "max").unwrap_or(1),
            chance: attr(c, "chance").unwrap_or(100.0),
        })
    };
    let mut files = Vec::new();
    xml_files(&root.join("stats/npcs"), &mut files);
    for f in files {
        let Ok(src) = std::fs::read_to_string(&f) else { continue };
        let Ok(doc) = roxmltree::Document::parse(&src) else { continue };
        for n in doc.root_element().children().filter(|c| c.has_tag_name("npc")) {
            let Some(id) = attr::<u32>(n, "id") else { continue };
            let Some(drop) = n.descendants().find(|c| c.has_tag_name("drop")) else { continue };
            let mut rules = Vec::new();
            for c in drop.children() {
                if c.has_tag_name("item") {
                    if let Some(e) = entry(c) {
                        rules.push(DropRule::Single(e));
                    }
                } else if c.has_tag_name("group") {
                    let items = c.children().filter(|i| i.has_tag_name("item")).filter_map(entry).collect();
                    rules.push(DropRule::Group { chance: attr(c, "chance").unwrap_or(100.0), items });
                }
            }
            data.drops.insert(id, rules);
        }
    }
    info!("items: {} templates, {} starting kits, {} drop lists", data.items.len(), data.initial.len(), data.drops.len());
}

/// Rolls a monster's drop list.
pub fn roll_drops(rules: &[DropRule], rng: &mut impl Rng) -> Vec<(u32, u64)> {
    let mut out = Vec::new();
    let count = |e: &DropEntry, rng: &mut dyn FnMut(u64, u64) -> u64| if e.max > e.min { rng(e.min, e.max) } else { e.min };
    let mut range = |a: u64, b: u64| -> u64 { a + (rand::random::<u64>() % (b - a + 1)) };
    for r in rules {
        match r {
            DropRule::Single(e) => {
                if rng.random_range(0.0..100.0) < e.chance {
                    out.push((e.item, count(e, &mut range)));
                }
            }
            DropRule::Group { chance, items } => {
                if rng.random_range(0.0..100.0) >= *chance || items.is_empty() {
                    continue;
                }
                let total: f32 = items.iter().map(|i| i.chance).sum();
                let mut pick = rng.random_range(0.0..total.max(0.001));
                for e in items {
                    pick -= e.chance;
                    if pick <= 0.0 {
                        out.push((e.item, count(e, &mut range)));
                        break;
                    }
                }
            }
        }
    }
    out
}

pub fn give_starting_items(
    mut commands: Commands,
    data: Res<ItemData>,
    dp: Res<crate::datapack::Datapack>,
    hero: Single<(Entity, &HeroStats), (With<Hero>, Without<Inventory>)>,
) {
    let (e, stats) = hero.into_inner();
    let mut inv = Inventory::default();
    // The kit belongs to the starting class (the root of the class tree).
    let mut class = stats.template.class_id;
    while let Some((_, Some(parent))) = dp.class_list.get(&class) {
        class = *parent;
    }
    for &(item, count, equipped) in data.initial.get(&class).into_iter().flatten() {
        *inv.items.entry(item).or_default() += count;
        if let (true, Some(t)) = (equipped, data.items.get(&item)) {
            inv.equipped.insert(t.slot().to_string(), item);
        }
    }
    // L2_AUTOSHOTS=1 turns on every shot of the kit (tests).
    if std::env::var_os("L2_AUTOSHOTS").is_some() {
        inv.items.insert(91927, 1000);
        inv.items.insert(91928, 1000);
        inv.auto_shots = inv.items.keys().copied().filter(|i| data.items.get(i).is_some_and(|t| t.handler.contains("Shot"))).collect();
    }
    commands.entity(e).insert(inv);
}

/// Feeds equipped item stats into the hero's stats when equipment changes.
fn apply_equipment(data: Res<ItemData>, hero: Single<(&Inventory, &mut HeroStats), (With<Hero>, Changed<Inventory>)>) {
    let (inv, mut stats) = hero.into_inner();
    let (mut patk, mut matk, mut pdef, mut mdef, mut spd) = (0.0, 0.0, 0.0, 0.0, 0.0);
    for item in inv.equipped.values() {
        let Some(t) = data.items.get(item) else { continue };
        patk += t.stat("pAtk");
        matk += t.stat("mAtk");
        pdef += t.stat("pDef");
        mdef += t.stat("mDef");
        if t.kind == "Weapon" {
            spd = t.stat("pAtkSpd");
        }
    }
    stats.equip = crate::rpg::Equipment { patk, matk, pdef, mdef, atk_speed: spd };
    stats.recalc();
}

/// Animation stance suffix (`wait_<stance>`) for the equipped weapon.
pub fn stance(weapon: Option<&ItemTemplate>) -> &'static str {
    let Some(w) = weapon else { return "Hand" };
    match w.weapon_type.as_str() {
        "BOW" => "Bow",
        "CROSSBOW" | "TWOHANDCROSSBOW" => "Bowgun",
        "POLE" => "Pole",
        "DUAL" | "DUALDAGGER" | "DUALBLUNT" => "Dual",
        "FIST" | "DUALFIST" => "Hand",
        _ if w.bodypart == "lrhand" => "2HS",
        _ => "1HS",
    }
}

/// Rebuilds the hero's model when equipment changes: armor pieces come from
/// armorgrp for the hero's race, weapons and shields from weapongrp.
#[allow(clippy::type_complexity)]
fn hero_appearance(
    mut commands: Commands,
    data: Res<ItemData>,
    gd: Res<crate::gamedata::GameData>,
    mut lib: ResMut<L2Library>,
    mut cache: ResMut<crate::skinned::ModelCache>,
    mut assets: (
        ResMut<Assets<Mesh>>,
        ResMut<Assets<StandardMaterial>>,
        ResMut<Assets<Image>>,
        ResMut<Assets<bevy::mesh::skinning::SkinnedMeshInverseBindposes>>,
    ),
    hero: Single<(Entity, &mut Hero, &Inventory), Changed<Inventory>>,
    mut last: Local<Option<BTreeMap<String, u32>>>,
) {
    use crate::character::{armor_parts, armor_slot, equipment_spec, weapon_part, CLASSES};
    use crate::skinned::{spawn_model, Attachment};
    let (he, mut hero, inv) = hero.into_inner();
    // Picking up loot changes the inventory too; only equipment matters here.
    if last.as_ref() == Some(&inv.equipped) {
        return;
    }
    *last = Some(inv.equipped.clone());
    let class = &CLASSES[hero.class];
    let race = armor_slot(class);
    let mut armor: HashMap<char, String> = HashMap::new();
    for slot in ["chest", "legs", "gloves", "feet", "onepiece"] {
        let Some(item) = inv.equipped.get(slot) else { continue };
        let Some(model) = gd.armors.get(item).and_then(|a| a.models.get(race)) else { continue };
        let parts = armor_parts(model);
        let full = matches!(data.items.get(item).map(|t| t.bodypart.as_str()), Some("onepiece" | "alldress"));
        if full && !parts.iter().any(|(c, _)| *c == 'l') {
            armor.insert('l', String::new()); // the robe covers the legs
        }
        for (c, part) in parts {
            armor.insert(c, part);
        }
    }
    let spec = equipment_spec(&mut lib, class, &armor);

    let weapon_item = inv.equipped.get("rhand").copied();
    let weapon = weapon_item.and_then(|i| data.items.get(&i));
    let (meshes, materials, images, bindposes) = &mut assets;
    let mut attachments: Vec<Attachment> = Vec::new();
    let mut attach = |bone: &'static str, item: u32, index: usize, lib: &mut L2Library| {
        let Some(w) = gd.weapons.get(&item) else { return };
        let Some(mesh) = w.meshes.get(index) else { return };
        let Some((pkg, part)) = weapon_part(mesh, w.textures.get(index)) else { return };
        if let Some(parts) = lib.rigid_mesh(&pkg, &part, meshes, materials, images) {
            attachments.push(Attachment { bone, parts });
        }
    };
    let stance = stance(weapon);
    if let Some(item) = weapon_item {
        match stance {
            "Bow" | "Bowgun" => attach("Weapon_L_Bone", item, 0, &mut lib),
            "Dual" => {
                attach("Weapon_R_Bone", item, 0, &mut lib);
                attach("Weapon_L_Bone", item, 1, &mut lib);
            }
            _ => attach("Weapon_R_Bone", item, 0, &mut lib),
        }
    }
    if let Some(&shield) = inv.equipped.get("lhand") {
        attach("Shield_L_Bone", shield, 0, &mut lib);
    }
    let Some(model) = cache.load(&mut lib, class.package, &spec, None, meshes, materials, images, bindposes) else {
        return;
    };
    commands.entity(hero.actor).despawn();
    hero.actor = spawn_model(&mut commands, &model, he, &format!("wait_{stance}"), &attachments);
    hero.stance = stance;
}

/// Items used from the inventory, applied by `use_items`.
#[derive(Resource, Default)]
pub struct UseQueue(pub Vec<u32>);

/// Uses an item: consumes one and applies its skills' effects (teleport and
/// escape scrolls, healing potions).
#[allow(clippy::too_many_arguments)]
fn use_items(
    mut uses: ResMut<UseQueue>,
    data: Res<ItemData>,
    book: Res<crate::datapack::SkillBook>,
    mut log: ResMut<Log>,
    mut streamer: ResMut<crate::streaming::TileStreamer>,
    hero: Single<(&mut Transform, &mut HeroStats, &mut Inventory), With<Hero>>,
) {
    if uses.0.is_empty() {
        return;
    }
    let (mut tr, mut stats, mut inv) = hero.into_inner();
    for id in std::mem::take(&mut uses.0) {
        let Some(t) = data.items.get(&id) else { continue };
        if inv.items.get(&id).copied().unwrap_or(0) == 0 || stats.hp <= 0.0 {
            continue;
        }
        let mut used = false;
        for def in t.skills.iter().filter_map(|s| book.skills.get(s)) {
            for e in &def.effects {
                match e.name.as_str() {
                    "Teleport" => {
                        if let Some(p) = e.pos {
                            let pos = crate::streaming::ue_to_bevy(p);
                            tr.translation = pos;
                            streamer.request_around(pos);
                            used = true;
                        }
                    }
                    "Escape" => {
                        let pos = stats.respawn;
                        tr.translation = pos;
                        streamer.request_around(pos);
                        used = true;
                    }
                    "Heal" | "HealOverTime" | "Hp" => {
                        let amount = e.power.at(1).max(e.amount.at(1));
                        let amount = if e.name == "HealOverTime" { amount * def.abnormal_time.max(1.0) } else { amount };
                        stats.hp = (stats.hp + amount).min(stats.max_hp);
                        used = true;
                    }
                    "Mp" | "ManaHealOverTime" => {
                        let amount = e.power.at(1).max(e.amount.at(1));
                        stats.mp = (stats.mp + amount).min(stats.max_mp);
                        used = true;
                    }
                    _ => {}
                }
            }
        }
        if used {
            log.push(format!("You used {}", t.name));
            let left = inv.items.get_mut(&id).unwrap();
            *left -= 1;
            if *left == 0 {
                inv.items.remove(&id);
            }
        } else {
            log.push(format!("{} cannot be used yet", t.name));
        }
    }
}

// ---------------------------------------------------------------------------
// Inventory window
// ---------------------------------------------------------------------------

#[derive(Resource, Default)]
struct InventoryUi {
    open: bool,
    selected: usize,
    shown: Option<(usize, usize, u64)>,
}

#[derive(Component)]
struct InventoryRoot;

#[allow(clippy::too_many_arguments)]
fn inventory_window(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    gamepads: Query<&Gamepad>,
    input: Res<PlayerInput>,
    data: Res<ItemData>,
    mut ui: ResMut<InventoryUi>,
    mut log: ResMut<Log>,
    mut lib: ResMut<L2Library>,
    mut images: ResMut<Assets<Image>>,
    mut icons: Local<HashMap<String, Option<Handle<Image>>>>,
    mut inv: Single<&mut Inventory, With<Hero>>,
    roots: Query<Entity, With<InventoryRoot>>,
    mut uses: ResMut<UseQueue>,
    mut opened_by_env: Local<bool>,
) {
    // L2_INVENTORY=1 opens the window at start (screenshots).
    if !*opened_by_env && std::env::var_os("L2_INVENTORY").is_some() {
        *opened_by_env = true;
        ui.open = true;
    }
    let toggle = keys.just_pressed(KeyCode::KeyI) || gamepads.iter().any(|g| g.just_pressed(GamepadButton::Select));
    if toggle {
        ui.open = !ui.open;
        ui.shown = None;
    }
    if !ui.open {
        for e in &roots {
            commands.entity(e).despawn();
        }
        return;
    }
    let list: Vec<(u32, u64)> = inv.items.iter().map(|(k, v)| (*k, *v)).collect();
    if list.is_empty() {
        return;
    }
    if input.nav != 0 {
        ui.selected = (ui.selected as i32 + input.nav).rem_euclid(list.len() as i32) as usize;
    }
    ui.selected = ui.selected.min(list.len() - 1);
    // Equip / unequip with Enter or gamepad A (not while toggling).
    if input.confirm && !toggle {
        let (id, _) = list[ui.selected];
        if data.items.get(&id).is_some_and(|t| t.handler.contains("Shot")) {
            // Shots toggle auto-use.
            if !inv.auto_shots.remove(&id) {
                inv.auto_shots.insert(id);
                log.push(format!("Automatic use of {} has been activated.", data.items[&id].name));
            } else {
                log.push(format!("Automatic use of {} has been deactivated.", data.items[&id].name));
            }
        } else if data.items.get(&id).is_some_and(|t| !t.equippable() && !t.skills.is_empty()) {
            uses.0.push(id);
        } else if let Some(t) = data.items.get(&id).filter(|t| t.equippable()) {
            let slot = t.slot().to_string();
            if inv.equipped.get(&slot) == Some(&id) {
                inv.equipped.remove(&slot);
                log.push(format!("Unequipped {}", t.name));
            } else {
                inv.equipped.insert(slot, id);
                log.push(format!("Equipped {}", t.name));
            }
        }
        ui.shown = None;
    }
    let state = (ui.selected, list.len(), list.iter().map(|x| x.1).sum::<u64>() + inv.equipped.len() as u64 + 1000 * inv.auto_shots.len() as u64);
    if ui.shown == Some(state) {
        return;
    }
    ui.shown = Some(state);
    for e in &roots {
        commands.entity(e).despawn();
    }
    let equipped: Vec<u32> = inv.equipped.values().copied().collect();
    commands
        .spawn((
            InventoryRoot,
            Node {
                position_type: PositionType::Absolute,
                right: Val::Px(16.0),
                top: Val::Px(80.0),
                width: Val::Px(340.0),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(Val::Px(8.0)),
                row_gap: Val::Px(2.0),
                ..default()
            },
            BackgroundColor(Color::srgba(0.05, 0.05, 0.08, 0.85)),
        ))
        .with_children(|p| {
            let adena = inv.items.get(&ADENA).copied().unwrap_or(0);
            p.spawn((Text::new(format!("Inventory   Adena: {adena}")), TextFont { font_size: FontSize::Px(18.0), ..default() }));
            for (i, (id, count)) in list.iter().enumerate() {
                let t = data.items.get(id);
                let name = t.map_or(format!("item {id}"), |t| t.name.clone());
                let icon_name = t.map(|t| t.icon.clone()).unwrap_or_default();
                let icon = icons
                    .entry(icon_name.clone())
                    .or_insert_with(|| {
                        let (pk, o) = icon_name.split_once('.')?;
                        lib.texture(pk, o, false).map(|img| images.add(img))
                    })
                    .clone();
                let mark = if equipped.contains(id) {
                    " [E]"
                } else if inv.auto_shots.contains(id) {
                    " [auto]"
                } else {
                    ""
                };
                let color = if i == ui.selected { Color::srgb(1.0, 0.85, 0.3) } else { Color::WHITE };
                p.spawn(Node { align_items: AlignItems::Center, column_gap: Val::Px(6.0), ..default() }).with_children(|row| {
                    if let Some(img) = icon {
                        row.spawn((ImageNode::new(img), Node { width: Val::Px(24.0), height: Val::Px(24.0), ..default() }));
                    }
                    row.spawn((
                        Text::new(format!("{name}{}{mark}", if *count > 1 { format!(" x{count}") } else { String::new() })),
                        TextFont { font_size: FontSize::Px(14.0), ..default() },
                        TextColor(color),
                    ));
                });
            }
        });
}
