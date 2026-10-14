//! Static world collision: triangles of collidable static mesh sections
//! (`EnableCollision`), bucketed in a 2D grid over the ground plane.

use std::collections::HashMap;

use bevy::prelude::*;

const CELL: f32 = 4.0;

#[derive(Resource, Default)]
pub struct CollisionWorld {
    tris: Vec<[Vec3; 3]>,
    cells: HashMap<(i32, i32), Vec<u32>>,
}

pub struct Hit {
    pub t: f32,
    pub normal: Vec3,
}

fn cell_of(x: f32, z: f32) -> (i32, i32) {
    ((x / CELL).floor() as i32, (z / CELL).floor() as i32)
}

impl CollisionWorld {
    pub fn add(&mut self, tris: impl IntoIterator<Item = [Vec3; 3]>) {
        for t in tris {
            let idx = self.tris.len() as u32;
            let min = t[0].min(t[1]).min(t[2]);
            let max = t[0].max(t[1]).max(t[2]);
            // Skip degenerate or absurdly large triangles (sky domes etc.).
            if (max - min).xz().length() > 400.0 {
                continue;
            }
            let (c0, c1) = (cell_of(min.x, min.z), cell_of(max.x, max.z));
            for cx in c0.0..=c1.0 {
                for cz in c0.1..=c1.1 {
                    self.cells.entry((cx, cz)).or_default().push(idx);
                }
            }
            self.tris.push(t);
        }
    }

    pub fn len(&self) -> usize {
        self.tris.len()
    }

    /// Nearest hit along `dir` (normalized) within `max` meters.
    pub fn raycast(&self, origin: Vec3, dir: Vec3, max: f32) -> Option<Hit> {
        let end = origin + dir * max;
        let (c0, c1) = (cell_of(origin.x.min(end.x), origin.z.min(end.z)), cell_of(origin.x.max(end.x), origin.z.max(end.z)));
        let mut best: Option<Hit> = None;
        let mut seen = std::collections::HashSet::new();
        for cx in c0.0..=c1.0 {
            for cz in c0.1..=c1.1 {
                let Some(list) = self.cells.get(&(cx, cz)) else { continue };
                for &i in list {
                    if !seen.insert(i) {
                        continue;
                    }
                    let t = &self.tris[i as usize];
                    if let Some(d) = ray_triangle(origin, dir, t) {
                        if d <= max && best.as_ref().is_none_or(|b| d < b.t) {
                            let n = (t[1] - t[0]).cross(t[2] - t[0]).normalize_or_zero();
                            let n = if n.dot(dir) > 0.0 { -n } else { n };
                            best = Some(Hit { t: d, normal: n });
                        }
                    }
                }
            }
        }
        best
    }

    /// Highest walkable surface below `pos + up` within `up + down` meters.
    pub fn ground(&self, pos: Vec3, up: f32, down: f32) -> Option<f32> {
        let origin = pos + Vec3::Y * up;
        self.raycast(origin, Vec3::NEG_Y, up + down)
            .filter(|h| h.normal.y > 0.45)
            .map(|h| origin.y - h.t)
    }
}

/// Möller–Trumbore, double-sided.
fn ray_triangle(o: Vec3, d: Vec3, t: &[Vec3; 3]) -> Option<f32> {
    let e1 = t[1] - t[0];
    let e2 = t[2] - t[0];
    let p = d.cross(e2);
    let det = e1.dot(p);
    if det.abs() < 1e-8 {
        return None;
    }
    let inv = 1.0 / det;
    let s = o - t[0];
    let u = s.dot(p) * inv;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = s.cross(e1);
    let v = d.dot(q) * inv;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let dist = e2.dot(q) * inv;
    (dist > 0.0).then_some(dist)
}
