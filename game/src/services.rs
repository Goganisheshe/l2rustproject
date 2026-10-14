//! NPC services reached from dialog bypasses: merchant buy/sell lists
//! (`buylists/`), multisell exchanges (`multisell/`) and gatekeeper
//! teleports (`TeleportListData.xml`, `teleporters/`). Each returns an HTML
//! page whose links come back here.

use std::path::Path;

use crate::items::ADENA;
use crate::quest::Player;

const PAGE: usize = 10;

/// Teleports are free up to this level, as on Classic servers.
const FREE_TELEPORT_LEVEL: u32 = 40;

fn doc_items(path: &Path) -> Option<String> {
    std::fs::read_to_string(path).ok()
}

fn attr<T: std::str::FromStr>(n: roxmltree::Node, name: &str) -> Option<T> {
    n.attribute(name)?.parse().ok()
}

fn name(p: &Player, id: u32) -> String {
    p.items.items.get(&id).map_or(format!("item {id}"), |t| t.name.clone())
}

fn pager(cmd: &str, page: usize, total: usize) -> String {
    let mut s = String::new();
    if page > 0 {
        s.push_str(&format!("<Button action=\"bypass -h {cmd} {}\">&lt; Previous</Button>", page - 1));
    }
    if (page + 1) * PAGE < total {
        s.push_str(&format!("<Button action=\"bypass -h {cmd} {}\">Next &gt;</Button>", page + 1));
    }
    s
}

fn header(p: &Player, msg: &str) -> String {
    let mut s = String::from("<html><body>");
    if !msg.is_empty() {
        s.push_str(&format!("<font color=\"LEVEL\">{msg}</font><br>"));
    }
    s.push_str(&format!("Adena: {}<br>", p.count(ADENA)));
    s
}

/// `buylists/<id>.xml`: (item, price).
fn buylist(root: &Path, list: u32) -> Vec<(u32, u64)> {
    let Some(src) = doc_items(&root.join(format!("buylists/{list}.xml"))) else { return Vec::new() };
    let Ok(doc) = roxmltree::Document::parse(&src) else { return Vec::new() };
    doc.descendants()
        .filter(|n| n.has_tag_name("item"))
        .filter_map(|n| Some((attr(n, "id")?, attr(n, "price").unwrap_or(0))))
        .collect()
}

pub fn buy_page(root: &Path, p: &Player, list: u32, page: usize, msg: &str) -> String {
    let items = buylist(root, list);
    let mut s = header(p, msg);
    for &(id, price) in items.iter().skip(page * PAGE).take(PAGE) {
        s.push_str(&format!("<Button action=\"bypass -h shop_buy {list} {id} 1 {page}\">{} - {price} a</Button>", name(p, id)));
        let stackable = p.items.items.get(&id).is_some_and(|t| t.kind == "EtcItem");
        if stackable {
            s.push_str(&format!("<Button action=\"bypass -h shop_buy {list} {id} 100 {page}\">  x100 - {} a</Button>", price * 100));
        }
    }
    s.push_str(&pager(&format!("shop_list {list}"), page, items.len()));
    s.push_str(&format!("<Button action=\"bypass -h shop_sell {list} 0\">Sell items</Button></body></html>"));
    s
}

pub fn buy(root: &Path, p: &mut Player, list: u32, item: u32, count: u64, page: usize) -> String {
    let Some(&(_, price)) = buylist(root, list).iter().find(|(id, _)| *id == item) else {
        return buy_page(root, p, list, page, "That item is not sold here.");
    };
    let cost = price * count;
    if p.count(ADENA) < cost {
        return buy_page(root, p, list, page, "You do not have enough Adena.");
    }
    p.take(ADENA, cost as i64);
    p.give(item, count);
    let msg = format!("Bought {} x{count}", name(p, item));
    buy_page(root, p, list, page, &msg)
}

fn sellable(p: &Player) -> Vec<(u32, u64, u64)> {
    p.inv
        .items
        .iter()
        .filter(|(id, _)| **id != ADENA && !p.inv.equipped.values().any(|e| e == *id))
        .filter_map(|(id, n)| {
            let price = p.items.items.get(id)?.price / 2;
            (price > 0).then_some((*id, *n, price))
        })
        .collect()
}

pub fn sell_page(p: &Player, list: u32, page: usize, msg: &str) -> String {
    let items = sellable(p);
    let mut s = header(p, msg);
    if items.is_empty() {
        s.push_str("You have nothing to sell.<br>");
    }
    for &(id, n, price) in items.iter().skip(page * PAGE).take(PAGE) {
        s.push_str(&format!(
            "<Button action=\"bypass -h shop_sellitem {list} {id} {page}\">{}{} - {} a</Button>",
            name(p, id),
            if n > 1 { format!(" x{n}") } else { String::new() },
            price * n
        ));
    }
    s.push_str(&pager(&format!("shop_sell {list}"), page, items.len()));
    s.push_str(&format!("<Button action=\"bypass -h shop_list {list} 0\">Back to buying</Button></body></html>"));
    s
}

/// Sells the whole stack.
pub fn sell(p: &mut Player, list: u32, item: u32, page: usize) -> String {
    let Some(&(_, n, price)) = sellable(p).iter().find(|(id, _, _)| *id == item) else {
        return sell_page(p, list, page, "");
    };
    p.take(item, -1);
    p.give(ADENA, n * price);
    let msg = format!("Sold {} x{n}", name(p, item));
    sell_page(p, list, page, &msg)
}

/// One multisell entry: ingredients and products.
struct Exchange {
    ingredients: Vec<(u32, u64)>,
    products: Vec<(u32, u64)>,
}

fn multisell(root: &Path, list: u32) -> Vec<Exchange> {
    let Some(src) = doc_items(&root.join(format!("multisell/{list}.xml"))) else { return Vec::new() };
    let Ok(doc) = roxmltree::Document::parse(&src) else { return Vec::new() };
    doc.root_element()
        .children()
        .filter(|n| n.has_tag_name("item"))
        .map(|n| {
            let pick = |tag: &str| {
                n.children()
                    .filter(|c| c.has_tag_name(tag))
                    .filter_map(|c| Some((attr(c, "id")?, attr(c, "count").unwrap_or(1))))
                    .collect()
            };
            Exchange { ingredients: pick("ingredient"), products: pick("production") }
        })
        .collect()
}

pub fn multisell_page(root: &Path, p: &Player, list: u32, page: usize, msg: &str) -> String {
    let entries = multisell(root, list);
    let mut s = header(p, msg);
    let list_names = |v: &[(u32, u64)]| v.iter().map(|(id, n)| format!("{}{}", name(p, *id), if *n > 1 { format!(" x{n}") } else { String::new() })).collect::<Vec<_>>().join(", ");
    for (i, e) in entries.iter().enumerate().skip(page * PAGE).take(PAGE) {
        s.push_str(&format!(
            "<Button action=\"bypass -h ms_exchange {list} {i} {page}\">{}</Button><font color=\"808080\">for {}</font><br>",
            list_names(&e.products),
            list_names(&e.ingredients)
        ));
    }
    s.push_str(&pager(&format!("ms_list {list}"), page, entries.len()));
    s.push_str("</body></html>");
    s
}

pub fn exchange(root: &Path, p: &mut Player, list: u32, index: usize, page: usize) -> String {
    let entries = multisell(root, list);
    let Some(e) = entries.get(index) else { return multisell_page(root, p, list, page, "") };
    if e.ingredients.iter().any(|&(id, n)| p.count(id) < n) {
        return multisell_page(root, p, list, page, "You do not have the required items.");
    }
    for &(id, n) in &e.ingredients {
        p.take(id, n as i64);
    }
    for &(id, n) in &e.products {
        p.give(id, n);
    }
    multisell_page(root, p, list, page, "Exchange complete.")
}

/// Classic global teleport list: (id, name, position, price).
fn teleport_list(root: &Path) -> Vec<(u32, String, [f32; 3], u64)> {
    let Some(src) = doc_items(&root.join("TeleportListData.xml")) else { return Vec::new() };
    src.lines()
        .filter_map(|l| {
            let l = l.trim();
            let num = |k: &str| -> Option<f64> {
                let i = l.find(&format!(" {k}=\""))? + k.len() + 3;
                l[i..].split('"').next()?.parse().ok()
            };
            if !l.starts_with("<teleport ") {
                return None;
            }
            let name = l.split("<!--").nth(1).map(|c| c.trim_end_matches("-->").trim().to_string()).unwrap_or_default();
            Some((num("id")? as u32, name, [num("x")? as f32, num("y")? as f32, num("z")? as f32], num("price").unwrap_or(0.0) as u64))
        })
        .collect()
}

fn fee(p: &Player, price: u64) -> u64 {
    if p.level() <= FREE_TELEPORT_LEVEL { 0 } else { price }
}

pub fn teleport_page(root: &Path, p: &Player, page: usize, msg: &str) -> String {
    let mut list = teleport_list(root);
    list.sort_by(|a, b| a.1.cmp(&b.1));
    let mut s = header(p, msg);
    for (id, name, _, price) in list.iter().skip(page * PAGE).take(PAGE) {
        let fee = fee(p, *price);
        let cost = if fee == 0 { "free".to_string() } else { format!("{fee} a") };
        s.push_str(&format!("<Button action=\"bypass -h tele_go {id} {page}\">{name} - {cost}</Button>"));
    }
    s.push_str(&pager("tele_list", page, list.len()));
    s.push_str("</body></html>");
    s
}

/// Returns a page only when the teleport failed.
pub fn teleport(root: &Path, p: &mut Player, id: u32, page: usize) -> Option<String> {
    let list = teleport_list(root);
    let (_, _, pos, price) = list.iter().find(|t| t.0 == id)?;
    let fee = fee(p, *price);
    if p.count(ADENA) < fee {
        return Some(teleport_page(root, p, page, "You do not have enough Adena."));
    }
    p.take(ADENA, fee as i64);
    p.tele(pos[0], pos[1], pos[2]);
    None
}

/// `npc_<obj>_teleport <type> <index>`: an NPC's own teleport from `teleporters/`.
pub fn npc_teleport(root: &Path, p: &mut Player, npc: u32, kind: &str, index: usize) -> bool {
    let mut files = Vec::new();
    fn walk(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            let path = e.path();
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().is_some_and(|x| x == "xml") {
                out.push(path);
            }
        }
    }
    walk(&root.join("teleporters"), &mut files);
    for f in files {
        let Some(src) = doc_items(&f) else { continue };
        if !src.contains(&format!("<npc id=\"{npc}\"")) {
            continue;
        }
        let Ok(doc) = roxmltree::Document::parse(&src) else { continue };
        let Some(n) = doc.descendants().find(|n| n.has_tag_name("npc") && attr::<u32>(*n, "id") == Some(npc)) else { continue };
        let Some(list) = n
            .children()
            .filter(|t| t.has_tag_name("teleport"))
            .find(|t| t.attribute("type") == Some(kind) || t.attribute("name") == Some(kind))
        else {
            continue;
        };
        let Some(loc) = list.children().filter(|l| l.has_tag_name("location")).nth(index) else { continue };
        let (Some(x), Some(y), Some(z)) = (attr::<f32>(loc, "x"), attr::<f32>(loc, "y"), attr::<f32>(loc, "z")) else { continue };
        let fee = fee(p, attr(loc, "feeCount").unwrap_or(0));
        if p.count(ADENA) < fee {
            p.log.push("You do not have enough Adena.");
            return true;
        }
        p.take(ADENA, fee as i64);
        p.tele(x, y, z);
        return true;
    }
    false
}
