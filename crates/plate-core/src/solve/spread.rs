//! Spreading parts over a floor or ceiling: how many, and where in each zone of a room.
//!
//! A room is cut into zones (rectangles, biggest first; see [`crate::geometry::zones`]). With a
//! spacing, each zone gets a grid at most that far apart and about half that from its sides,
//! which in a zone too narrow for two rows is a line. With a count, the count is shared between
//! the zones by area, so every light lights about as much floor, and each zone lays out its share
//! as evenly as it can. Pure: no rooms, no parts, just zones in and points out.

use crate::geometry::{Point, Zone};

/// How the parts are arranged in each zone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Arrange {
    /// A grid in each zone; a zone too narrow for two rows gets a line.
    #[default]
    Grid,
    /// One row along each zone.
    Line,
    /// One in the middle of each zone.
    Centre,
}

/// What was asked: a count, a spacing, or an area each (in the zones' units).
#[derive(Debug, Clone, Copy, Default)]
pub struct Ask {
    pub exactly: Option<u32>,
    pub min: Option<u32>,
    pub max: Option<u32>,
    pub spacing: Option<f64>,
    pub per: Option<f64>,
    pub arrange: Arrange,
    /// Closest any part may be to a zone's side, where its share leaves room for that.
    pub margin: f64,
}

/// Where the parts go, biggest zone first.
pub fn spread(zones: &[Zone], ask: &Ask) -> Vec<Point> {
    if zones.is_empty() {
        return Vec::new();
    }
    if ask.arrange == Arrange::Centre {
        let n = ask.exactly.unwrap_or(zones.len() as u32).min(zones.len() as u32) as usize;
        return zones.iter().take(n).map(|z| z.centre()).collect();
    }
    let clamp = |n: u32| {
        let n = ask.min.map_or(n, |m| n.max(m));
        ask.max.map_or(n, |m| n.min(m))
    };
    if let (None, Some(s)) = (ask.exactly, ask.spacing.filter(|s| *s > 0.0)) {
        let grids: Vec<(usize, usize)> = zones.iter().map(|z| by_spacing(z, s, ask.arrange)).collect();
        let total: usize = grids.iter().map(|(a, b)| a * b).sum();
        if clamp(total as u32) as usize == total {
            return zones
                .iter()
                .zip(&grids)
                .flat_map(|(z, &(cols, rows))| grid(z, cols, rows, ask.margin))
                .collect();
        }
        return count(zones, clamp(total as u32) as usize, ask);
    }
    let area: f64 = zones.iter().map(|z| z.area()).sum();
    let n = match (ask.exactly, ask.per.filter(|a| *a > 0.0)) {
        (Some(n), _) => n,
        (None, Some(per)) => clamp(((area / per) - 1e-9).ceil().max(1.0) as u32),
        (None, None) => clamp(1),
    };
    count(zones, n as usize, ask)
}

/// Columns and rows that keep parts at most `s` apart and about `s / 2` from the sides.
fn by_spacing(z: &Zone, s: f64, arrange: Arrange) -> (usize, usize) {
    let n = |len: f64| ((len / s) - 1e-9).ceil().max(1.0) as usize;
    (n(z.w), if arrange == Arrange::Line { 1 } else { n(z.h) })
}

/// Exactly `n`, shared between the zones by area.
fn count(zones: &[Zone], n: usize, ask: &Ask) -> Vec<Point> {
    if n == 0 {
        return Vec::new();
    }
    // Fewer than zones: one each in the biggest.
    if n <= zones.len() {
        return zones.iter().take(n).map(|z| z.centre()).collect();
    }
    let shares = shares(zones, n);
    zones
        .iter()
        .zip(shares)
        .flat_map(|(z, k)| layout(z, k, ask.arrange == Arrange::Line, ask.margin))
        .collect()
}

/// `n` (at least one per zone) shared by area: largest remainders, then evened out so no zone
/// has a light that lights much more floor than another's.
pub fn shares(zones: &[Zone], n: usize) -> Vec<usize> {
    let area: f64 = zones.iter().map(|z| z.area()).sum::<f64>().max(1e-12);
    let quota: Vec<f64> = zones.iter().map(|z| n as f64 * z.area() / area).collect();
    let mut k: Vec<usize> = quota.iter().map(|q| (q.floor() as usize).max(1)).collect();
    // Too many (every zone has at least one): take from the zone whose parts light least.
    while k.iter().sum::<usize>() > n {
        let i = (0..k.len())
            .filter(|&i| k[i] > 1)
            .min_by(|&a, &b| (zones[a].area() / k[a] as f64).total_cmp(&(zones[b].area() / k[b] as f64)))
            .expect("more than one somewhere");
        k[i] -= 1;
    }
    // Too few: give to the zone whose parts light most.
    while k.iter().sum::<usize>() < n {
        let i = (0..k.len())
            .max_by(|&a, &b| {
                let (fa, fb) = (zones[a].area() / k[a] as f64, zones[b].area() / k[b] as f64);
                fa.total_cmp(&fb).then(b.cmp(&a))
            })
            .unwrap();
        k[i] += 1;
    }
    k
}

/// `k` parts in a zone, in rows along it: the arrangement whose spacing is closest to square in
/// its least square row. When `k` doesn't divide into equal rows, the middle rows take one more.
pub fn layout(z: &Zone, k: usize, line: bool, margin: f64) -> Vec<Point> {
    if k == 0 {
        return Vec::new();
    }
    let ratio = |a: f64, b: f64| a.max(b) / a.min(b).max(1e-12);
    let mut best: Option<(f64, usize)> = None;
    for rows in 1..=(if line { 1 } else { k }) {
        let (base, extra) = (k / rows, k % rows);
        if base == 0 {
            continue;
        }
        // Each row's spacing along against the spacing between rows: a row of one in a wide
        // room is as bad as its emptiness (3 as a line beats a triangle).
        let pv = z.h / rows as f64;
        let mut score = ratio(z.w / base as f64, pv);
        if extra > 0 {
            score = score.max(ratio(z.w / (base + 1) as f64, pv)) + 0.3;
        }
        if best.is_none_or(|b| score < b.0 - 1e-9) {
            best = Some((score, rows));
        }
    }
    let rows = best.map_or(1, |b| b.1);
    let base = k / rows;
    let extra = k % rows;
    // The rows nearest the middle take the extras.
    let mut order: Vec<usize> = (0..rows).collect();
    let mid = (rows as f64 - 1.0) / 2.0;
    order.sort_by(|&a, &b| (a as f64 - mid).abs().total_cmp(&(b as f64 - mid).abs()).then(a.cmp(&b)));
    let mut per_row = vec![base; rows];
    for &r in order.iter().take(extra) {
        per_row[r] += 1;
    }
    let ts = positions(z.h, rows, margin);
    let mut out = Vec::with_capacity(k);
    for (r, &t) in ts.iter().enumerate() {
        for s in positions(z.w, per_row[r], margin) {
            out.push(z.at(s, t));
        }
    }
    out
}

/// A full grid of `cols` × `rows`.
fn grid(z: &Zone, cols: usize, rows: usize, margin: f64) -> Vec<Point> {
    let ts = positions(z.h, rows, margin);
    let ss = positions(z.w, cols, margin);
    ts.iter().flat_map(|&t| ss.iter().map(move |&s| z.at(s, t))).collect()
}

/// `n` places along a length: each in the middle of an equal share, or, where that's closer
/// than `margin` to the ends, spread evenly between `margin` from each end.
pub fn positions(len: f64, n: usize, margin: f64) -> Vec<f64> {
    if n == 0 {
        return Vec::new();
    }
    if n == 1 {
        return vec![len / 2.0];
    }
    let p = len / n as f64;
    if p / 2.0 + 1e-9 >= margin || len <= 2.0 * margin {
        return (0..n).map(|i| p * (i as f64 + 0.5)).collect();
    }
    let inner = len - 2.0 * margin;
    (0..n).map(|i| margin + inner * i as f64 / (n - 1) as f64).collect()
}

/// The closest two points are to each other (infinite for fewer than two).
pub fn closest(points: &[Point]) -> f64 {
    let mut d = f64::INFINITY;
    for i in 0..points.len() {
        for j in i + 1..points.len() {
            d = d.min(points[i].distance(points[j]));
        }
    }
    d
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::zones;

    fn rect(w: f64, h: f64) -> Zone {
        Zone { origin: Point::new(0.0, 0.0), u: Point::new(1.0, 0.0), v: Point::new(0.0, 1.0), w, h }
    }

    fn l_shape() -> Vec<Zone> {
        // A 6 × 5 body and a 1.6 m wide, 4 m arm.
        let p = |x, y| Point::new(x, y);
        zones(&[p(0.0, 0.0), p(6.0, 0.0), p(6.0, 9.0), p(4.4, 9.0), p(4.4, 5.0), p(0.0, 5.0)], &[], 0.6)
    }

    #[test]
    fn nothing_asked_is_one_in_the_middle_of_the_biggest_zone() {
        let z = l_shape();
        assert_eq!(z.len(), 2);
        let pts = spread(&z, &Ask::default());
        assert_eq!(pts.len(), 1);
        assert!(pts[0].distance(z[0].centre()) < 1e-9);
    }

    #[test]
    fn a_spacing_is_a_grid_and_a_line_in_the_narrow_part() {
        let z = l_shape();
        let pts = spread(&z, &Ask { spacing: Some(2.0), margin: 0.3, ..Ask::default() });
        // The body (6 × 5): 3 × 3. The arm (4 × 1.6): a line of 2.
        assert_eq!(pts.len(), 9 + 2, "{pts:?}");
        let arm: Vec<_> = pts.iter().filter(|p| p.y > 5.0).collect();
        assert_eq!(arm.len(), 2);
        assert!(arm.iter().all(|p| (p.x - 5.2).abs() < 1e-9), "a line down the middle of the arm: {arm:?}");
        assert!(closest(&pts) <= 2.0 + 1e-9);
    }

    #[test]
    fn a_count_is_exact_and_shared_by_area() {
        let z = l_shape();
        for n in [2, 3, 5, 7, 10, 13] {
            let pts = spread(&z, &Ask { exactly: Some(n), margin: 0.3, ..Ask::default() });
            assert_eq!(pts.len(), n as usize);
        }
        // 7 over 30 m² and 6.4 m²: about 6 and 1, at least one each.
        let k = shares(&z, 7);
        assert_eq!(k.iter().sum::<usize>(), 7);
        assert!(k[0] >= 5 && k[1] >= 1, "{k:?}");
    }

    #[test]
    fn rows_come_out_even_and_close_to_square() {
        // 6 in a 6 × 4 room: 3 × 2.
        let pts = layout(&rect(6.0, 4.0), 6, false, 0.3);
        let mut ys: Vec<i64> = pts.iter().map(|p| (p.y * 1000.0).round() as i64).collect();
        ys.dedup();
        assert_eq!(ys.len(), 2, "{pts:?}");
        // 7 in a square: three rows, 2-3-2.
        let pts = layout(&rect(5.0, 5.0), 7, false, 0.3);
        let row = |y: f64| pts.iter().filter(|p| (p.y - y).abs() < 1e-6).count();
        assert_eq!((row(5.0 / 6.0), row(2.5), row(25.0 / 6.0)), (2, 3, 2), "{pts:?}");
        // 3 in a room a bit longer than wide: a line, not a triangle.
        for (w, h) in [(6.5, 4.7), (6.5, 5.45)] {
            let pts = layout(&rect(w, h), 3, false, 0.3);
            assert!(pts.iter().all(|p| (p.y - h / 2.0).abs() < 1e-9), "{pts:?}");
        }
        // 3 over a long thin zone: a line.
        let pts = layout(&rect(2.4, 1.0), 3, false, 0.0);
        assert!(pts.iter().all(|p| (p.y - 0.5).abs() < 1e-9));
    }

    #[test]
    fn margins_hold_when_shares_are_small() {
        // 4 in a row along 2 m: shares of 0.5 m put the ends 0.25 m from the walls.
        assert_eq!(positions(2.0, 4, 0.0), vec![0.25, 0.75, 1.25, 1.75]);
        let p = positions(2.0, 4, 0.4);
        assert!((p[0] - 0.4).abs() < 1e-9 && (p[3] - 1.6).abs() < 1e-9, "{p:?}");
    }

    #[test]
    fn a_range_bounds_what_a_spacing_gives() {
        let z = vec![rect(6.0, 4.0)];
        let by = |min, max| spread(&z, &Ask { spacing: Some(1.0), min, max, ..Ask::default() }).len();
        assert_eq!(by(None, None), 24);
        assert_eq!(by(None, Some(8)), 8);
        assert_eq!(spread(&z, &Ask { min: Some(3), ..Ask::default() }).len(), 3);
    }

    #[test]
    fn line_and_centre() {
        let z = l_shape();
        let line = spread(&z, &Ask { spacing: Some(2.0), arrange: Arrange::Line, ..Ask::default() });
        assert_eq!(line.len(), 3 + 2);
        let centre = spread(&z, &Ask { arrange: Arrange::Centre, ..Ask::default() });
        assert_eq!(centre, vec![z[0].centre(), z[1].centre()]);
    }
}
