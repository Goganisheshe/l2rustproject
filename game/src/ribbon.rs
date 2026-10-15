//! Ribbon geometry for beam and trail emitters: a strip of quads along a
//! polyline, either turned toward the camera or as fixed sheets rotated around
//! the line. Plain math on `[f32; 3]`, so it can be tested without Bevy.

pub type V3 = [f32; 3];

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn scale(a: V3, k: f32) -> V3 {
    [a[0] * k, a[1] * k, a[2] * k]
}

fn dot(a: V3, b: V3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn length(a: V3) -> f32 {
    dot(a, a).sqrt()
}

fn normalize(a: V3) -> Option<V3> {
    let l = length(a);
    (l > 1e-6).then(|| scale(a, 1.0 / l))
}

/// Any unit vector perpendicular to `dir` (a unit vector).
fn perpendicular(dir: V3) -> V3 {
    let up = if dir[1].abs() < 0.9 { [0.0, 1.0, 0.0] } else { [1.0, 0.0, 0.0] };
    normalize(cross(dir, up)).unwrap_or([1.0, 0.0, 0.0])
}

/// Removes the part of `v` along the unit vector `dir`.
pub fn reject(v: V3, dir: V3) -> V3 {
    sub(v, scale(dir, dot(v, dir)))
}

/// How the strip's width is oriented.
#[derive(Debug, Clone, Copy)]
pub enum Facing {
    /// One strip turned toward the camera at this position.
    Camera(V3),
    /// This many strips rotated evenly around the line (1 = one flat sheet).
    Sheets(u32),
}

#[derive(Debug, Default, Clone)]
pub struct RibbonMesh {
    pub positions: Vec<V3>,
    pub uvs: Vec<[f32; 2]>,
    pub colors: Vec<[f32; 4]>,
    pub indices: Vec<u32>,
}

/// One point of the line.
#[derive(Debug, Clone, Copy)]
pub struct RibbonPoint {
    pub pos: V3,
    pub half_width: f32,
    /// Texture U at this point.
    pub u: f32,
    /// Brightness / opacity 0..1.
    pub shade: f32,
}

/// Unit tangents of the polyline (central differences, one-sided at the ends).
fn tangents(points: &[RibbonPoint]) -> Vec<V3> {
    let n = points.len();
    let overall = normalize(sub(points[n - 1].pos, points[0].pos)).unwrap_or([1.0, 0.0, 0.0]);
    (0..n)
        .map(|i| {
            let a = points[i.saturating_sub(1)].pos;
            let b = points[(i + 1).min(n - 1)].pos;
            normalize(sub(b, a)).unwrap_or(overall)
        })
        .collect()
}

/// Builds the strip(s). `v_scale` is the texture V across the width.
pub fn build(points: &[RibbonPoint], facing: Facing, v_scale: f32) -> RibbonMesh {
    let mut out = RibbonMesh::default();
    if points.len() < 2 {
        return out;
    }
    let tangents = tangents(points);
    let overall = normalize(sub(points[points.len() - 1].pos, points[0].pos)).unwrap_or(tangents[0]);
    // Sheets share one frame around the overall direction so they don't twist.
    let base = perpendicular(overall);
    let sheets = match facing {
        Facing::Camera(_) => 1,
        Facing::Sheets(n) => n.max(1),
    };
    for sheet in 0..sheets {
        let first = out.positions.len() as u32;
        let angle = std::f32::consts::PI * sheet as f32 / sheets as f32;
        let sheet_dir = add(scale(base, angle.cos()), scale(cross(overall, base), angle.sin()));
        let mut last_side = sheet_dir;
        for (p, t) in points.iter().zip(&tangents) {
            let side = match facing {
                Facing::Camera(cam) => normalize(cross(*t, sub(cam, p.pos))).unwrap_or(last_side),
                // Keep the sheet perpendicular to the local direction.
                Facing::Sheets(_) => normalize(reject(sheet_dir, *t)).unwrap_or(last_side),
            };
            last_side = side;
            let w = scale(side, p.half_width);
            out.positions.push(add(p.pos, w));
            out.positions.push(sub(p.pos, w));
            out.uvs.push([p.u, 0.0]);
            out.uvs.push([p.u, v_scale]);
            out.colors.push([p.shade; 4]);
            out.colors.push([p.shade; 4]);
        }
        for i in 0..points.len() as u32 - 1 {
            let a = first + i * 2;
            out.indices.extend_from_slice(&[a, a + 2, a + 1, a + 1, a + 2, a + 3]);
        }
    }
    out
}

/// Texture U along the line: cumulative length scaled so the whole line spans
/// `repeats` texture widths.
pub fn u_by_length(points: &[V3], repeats: f32) -> Vec<f32> {
    let mut acc = vec![0.0; points.len()];
    for i in 1..points.len() {
        acc[i] = acc[i - 1] + length(sub(points[i], points[i - 1]));
    }
    let total = acc.last().copied().unwrap_or(0.0);
    acc.iter().map(|d| if total > 0.0 { d / total * repeats } else { 0.0 }).collect()
}

/// Beam noise as a function of the position along the beam `t` (0..1):
/// `lf` holds the large-bend offsets at evenly spaced control points and is
/// interpolated smoothly between them.
pub fn sample_lf(lf: &[V3], t: f32) -> V3 {
    match lf.len() {
        0 => [0.0; 3],
        1 => lf[0],
        n => {
            let x = t.clamp(0.0, 1.0) * (n - 1) as f32;
            let i = (x.floor() as usize).min(n - 2);
            let k = x - i as f32;
            let k = k * k * (3.0 - 2.0 * k);
            add(scale(lf[i], 1.0 - k), scale(lf[i + 1], k))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pt(x: f32) -> RibbonPoint {
        RibbonPoint { pos: [x, 0.0, 0.0], half_width: 0.5, u: x, shade: 1.0 }
    }

    #[test]
    fn camera_facing_strip_is_perpendicular_to_line_and_view() {
        let m = build(&[pt(0.0), pt(1.0), pt(2.0)], Facing::Camera([1.0, 0.0, 10.0]), 1.0);
        assert_eq!(m.positions.len(), 6);
        assert_eq!(m.indices.len(), 12);
        // Line along X, camera along Z: the width must lie along Y.
        for (a, b) in m.positions.iter().step_by(2).zip(m.positions.iter().skip(1).step_by(2)) {
            let w = sub(*a, *b);
            assert!((length(w) - 1.0).abs() < 1e-5);
            assert!(w[0].abs() < 1e-5 && w[2].abs() < 1e-5);
        }
    }

    #[test]
    fn sheets_are_rotated_around_the_line() {
        let m = build(&[pt(0.0), pt(4.0)], Facing::Sheets(2), 1.0);
        assert_eq!(m.positions.len(), 8);
        assert_eq!(m.indices.len(), 12);
        let w0 = sub(m.positions[0], m.positions[1]);
        let w1 = sub(m.positions[4], m.positions[5]);
        assert!(dot(w0, w1).abs() < 1e-5, "two sheets should be at 90 degrees");
        assert!(dot(w0, [1.0, 0.0, 0.0]).abs() < 1e-5);
    }

    #[test]
    fn uvs_and_indices_stay_in_range() {
        let m = build(&[pt(0.0), pt(1.0), pt(3.0)], Facing::Sheets(3), 2.0);
        assert!(m.indices.iter().all(|&i| (i as usize) < m.positions.len()));
        assert_eq!(m.uvs[1], [0.0, 2.0]);
        assert_eq!(m.uvs[4], [3.0, 0.0]);
    }

    #[test]
    fn too_short_or_degenerate_lines() {
        assert!(build(&[pt(0.0)], Facing::Sheets(1), 1.0).positions.is_empty());
        // All points in one place: still finite geometry.
        let m = build(&[pt(0.0), pt(0.0)], Facing::Camera([0.0, 0.0, 0.0]), 1.0);
        assert!(m.positions.iter().flatten().all(|c| c.is_finite()));
    }

    #[test]
    fn u_follows_length() {
        let u = u_by_length(&[[0.0; 3], [1.0, 0.0, 0.0], [4.0, 0.0, 0.0]], 2.0);
        assert_eq!(u, vec![0.0, 0.5, 2.0]);
        assert_eq!(u_by_length(&[[0.0; 3], [0.0; 3]], 1.0), vec![0.0, 0.0]);
    }

    #[test]
    fn lf_noise_interpolates_between_controls() {
        let lf = [[0.0; 3], [0.0, 2.0, 0.0], [0.0; 3]];
        assert_eq!(sample_lf(&lf, 0.0), [0.0; 3]);
        assert_eq!(sample_lf(&lf, 0.5), [0.0, 2.0, 0.0]);
        assert_eq!(sample_lf(&lf, 1.0), [0.0; 3]);
        let q = sample_lf(&lf, 0.25)[1];
        assert!(q > 0.0 && q < 2.0);
    }

    #[test]
    fn reject_removes_the_parallel_part() {
        assert_eq!(reject([3.0, 2.0, 0.0], [1.0, 0.0, 0.0]), [0.0, 2.0, 0.0]);
    }
}
