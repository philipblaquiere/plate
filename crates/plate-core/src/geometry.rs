use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl Point {
    pub const fn new(x: f64, y: f64) -> Self {
        Point { x, y }
    }

    pub fn distance(self, other: Point) -> f64 {
        (other.x - self.x).hypot(other.y - self.y)
    }
}

/// A closed polygon. The last point connects back to the first; it is not repeated.
/// Points are stored counter-clockwise (y up), the convention CAD tools expect.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Polygon {
    pub points: Vec<Point>,
}

impl Polygon {
    pub fn new(points: Vec<Point>) -> Self {
        let mut polygon = Polygon { points };
        if polygon.signed_area() < 0.0 {
            polygon.points.reverse();
        }
        polygon
    }

    /// Axis-aligned rectangle with its lower-left corner at `origin`.
    pub fn rectangle(origin: Point, width: f64, depth: f64) -> Self {
        let Point { x, y } = origin;
        Polygon::new(vec![
            Point::new(x, y),
            Point::new(x + width, y),
            Point::new(x + width, y + depth),
            Point::new(x, y + depth),
        ])
    }

    /// Shoelace formula; positive when counter-clockwise.
    pub fn signed_area(&self) -> f64 {
        self.edges().map(|(a, b)| a.x * b.y - b.x * a.y).sum::<f64>() / 2.0
    }

    pub fn area(&self) -> f64 {
        self.signed_area().abs()
    }

    pub fn perimeter(&self) -> f64 {
        self.edges().map(|(a, b)| a.distance(b)).sum()
    }

    /// Consecutive point pairs, including the closing edge.
    pub fn edges(&self) -> impl Iterator<Item = (Point, Point)> + '_ {
        let n = self.points.len();
        (0..n).map(move |i| (self.points[i], self.points[(i + 1) % n]))
    }

    pub fn bounds(&self) -> Bounds {
        Bounds::from_points(&self.points)
    }
}

/// Length of an open polyline (or closed, when `closed`).
pub fn path_length(points: &[Point], closed: bool) -> f64 {
    let n = points.len();
    let segments = if closed { n } else { n.saturating_sub(1) };
    (0..segments).map(|i| points[i].distance(points[(i + 1) % n])).sum()
}

/// Signed area of a closed polyline in its given order (positive when counter-clockwise).
pub fn signed_area(points: &[Point]) -> f64 {
    let n = points.len();
    (0..n).map(|i| points[i].x * points[(i + 1) % n].y - points[(i + 1) % n].x * points[i].y).sum::<f64>()
        / 2.0
}

/// Whether segments `ab` and `cd` cross or come within `tolerance` of each other.
pub fn segments_cross(a: Point, b: Point, c: Point, d: Point, tolerance: f64) -> bool {
    fn orient(p: Point, q: Point, r: Point) -> f64 {
        (q.x - p.x) * (r.y - p.y) - (q.y - p.y) * (r.x - p.x)
    }
    let (o1, o2, o3, o4) = (orient(a, b, c), orient(a, b, d), orient(c, d, a), orient(c, d, b));
    if o1 * o2 < 0.0 && o3 * o4 < 0.0 {
        return true;
    }
    [
        point_segment_distance(c, a, b),
        point_segment_distance(d, a, b),
        point_segment_distance(a, c, d),
        point_segment_distance(b, c, d),
    ]
    .into_iter()
    .any(|dist| dist <= tolerance)
}

/// Is `p` inside the closed polygon (even-odd)?
pub fn contains(polygon: &[Point], p: Point) -> bool {
    let mut inside = false;
    let n = polygon.len();
    for i in 0..n {
        let (a, b) = (polygon[i], polygon[(i + 1) % n]);
        if (a.y > p.y) != (b.y > p.y) {
            let x = a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x);
            if p.x < x {
                inside = !inside;
            }
        }
    }
    inside
}

/// A point well inside a polygon with holes, as far from its edges as a coarse search finds: where
/// a label goes when the middle of the bounds may be outside (an L, a ring).
pub fn interior_point(outer: &[Point], holes: &[Vec<Point>]) -> Point {
    let b = Bounds::from_points(outer);
    let mut best = (f64::MIN, Point::new((b.min_x + b.max_x) / 2.0, (b.min_y + b.max_y) / 2.0));
    let steps = 32;
    let edges = |l: &[Point]| (0..l.len()).map(|k| (l[k], l[(k + 1) % l.len()])).collect::<Vec<_>>();
    let mut all = edges(outer);
    for h in holes {
        all.extend(edges(h));
    }
    for j in 0..steps {
        for i in 0..steps {
            let p = Point::new(
                b.min_x + (i as f64 + 0.5) / steps as f64 * b.width(),
                b.min_y + (j as f64 + 0.5) / steps as f64 * b.height(),
            );
            if !contains(outer, p) || holes.iter().any(|h| contains(h, p)) {
                continue;
            }
            let d = all.iter().map(|&(a, c)| point_segment_distance(p, a, c)).fold(f64::INFINITY, f64::min);
            if d > best.0 {
                best = (d, p);
            }
        }
    }
    best.1
}

/// A rectangle inside a shape, in the shape's own frame: corner `origin`, sides `w` along `u`
/// (the longer one) and `h` along `v`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Zone {
    pub origin: Point,
    pub u: Point,
    pub v: Point,
    pub w: f64,
    pub h: f64,
}

impl Zone {
    /// The point `s` along it and `t` across it.
    pub fn at(&self, s: f64, t: f64) -> Point {
        Point::new(self.origin.x + self.u.x * s + self.v.x * t, self.origin.y + self.u.y * s + self.v.y * t)
    }

    pub fn area(&self) -> f64 {
        self.w * self.h
    }

    pub fn centre(&self) -> Point {
        self.at(self.w / 2.0, self.h / 2.0)
    }
}

/// A shape cut into rectangles, biggest first: a rectangle is one zone, an L two (its body and
/// its arm). Each zone is the biggest rectangle left in the shape; cutting stops at pieces
/// narrower than `min_side` or smaller than a tenth of the shape. Rectangles line up with the
/// shape's longest edge, so a turned room is cut along its own walls.
pub fn zones(outer: &[Point], holes: &[Vec<Point>], min_side: f64) -> Vec<Zone> {
    let n = outer.len();
    if n < 3 {
        return Vec::new();
    }
    // The frame: along the longest edge.
    let (mut best, mut u) = (0.0, Point::new(1.0, 0.0));
    for k in 0..n {
        let (a, b) = (outer[k], outer[(k + 1) % n]);
        let l = a.distance(b);
        if l > best + 1e-9 {
            best = l;
            u = Point::new((b.x - a.x) / l, (b.y - a.y) / l);
        }
    }
    let v = Point::new(-u.y, u.x);
    let to = |p: &Point| Point::new(p.x * u.x + p.y * u.y, p.x * v.x + p.y * v.y);
    let outer_f: Vec<Point> = outer.iter().map(to).collect();
    let holes_f: Vec<Vec<Point>> = holes.iter().map(|h| h.iter().map(to).collect()).collect();
    let b = Bounds::from_points(&outer_f);
    let span = b.width().max(b.height());
    if span <= 0.0 {
        return Vec::new();
    }
    let tol = span * 1e-9;
    // Grid lines through every corner; a shape with slanted edges also gets a fine even grid.
    let mut xs: Vec<f64> = Vec::new();
    let mut ys: Vec<f64> = Vec::new();
    let mut slanted = false;
    for l in std::iter::once(&outer_f).chain(holes_f.iter()) {
        for k in 0..l.len() {
            let (a, c) = (l[k], l[(k + 1) % l.len()]);
            xs.push(a.x);
            ys.push(a.y);
            if (a.x - c.x).abs() > tol && (a.y - c.y).abs() > tol {
                slanted = true;
            }
        }
    }
    if slanted {
        let step = span / 48.0;
        let mut x = b.min_x;
        while x < b.max_x {
            xs.push(x);
            x += step;
        }
        let mut y = b.min_y;
        while y < b.max_y {
            ys.push(y);
            y += step;
        }
        xs.push(b.max_x);
        ys.push(b.max_y);
    }
    let clean = |v: &mut Vec<f64>| {
        v.sort_by(f64::total_cmp);
        v.dedup_by(|a, b| (*a - *b).abs() <= tol * 1e3);
    };
    clean(&mut xs);
    clean(&mut ys);
    if xs.len() < 2 || ys.len() < 2 {
        return Vec::new();
    }
    let (nx, ny) = (xs.len() - 1, ys.len() - 1);
    let mut free = vec![vec![false; ny]; nx];
    let mut total = 0.0;
    for (i, col) in free.iter_mut().enumerate() {
        for (j, cell) in col.iter_mut().enumerate() {
            let c = Point::new((xs[i] + xs[i + 1]) / 2.0, (ys[j] + ys[j + 1]) / 2.0);
            if contains(&outer_f, c) && !holes_f.iter().any(|h| contains(h, c)) {
                *cell = true;
                total += (xs[i + 1] - xs[i]) * (ys[j + 1] - ys[j]);
            }
        }
    }
    let mut out = Vec::new();
    while out.len() < 8 {
        // The biggest rectangle of free cells: per row, how far up each column is free, then
        // every run of columns.
        let mut tall = vec![0usize; nx];
        let mut found: Option<(f64, usize, usize, usize, usize)> = None;
        for j in 0..ny {
            for i in 0..nx {
                tall[i] = if free[i][j] { tall[i] + 1 } else { 0 };
            }
            for i0 in 0..nx {
                let mut rows = usize::MAX;
                for i1 in i0..nx {
                    rows = rows.min(tall[i1]);
                    if rows == 0 {
                        break;
                    }
                    let (w, h) = (xs[i1 + 1] - xs[i0], ys[j + 1] - ys[j + 1 - rows]);
                    if found.is_none_or(|f| w * h > f.0 + tol * span) {
                        found = Some((w * h, i0, i1, j + 1 - rows, j));
                    }
                }
            }
        }
        let Some((area, i0, i1, j0, j1)) = found else { break };
        let (w, h) = (xs[i1 + 1] - xs[i0], ys[j1 + 1] - ys[j0]);
        if w.min(h) < min_side - tol || (!out.is_empty() && area < total * 0.1) {
            break;
        }
        for col in free.iter_mut().take(i1 + 1).skip(i0) {
            for cell in col.iter_mut().take(j1 + 1).skip(j0) {
                *cell = false;
            }
        }
        let corner = Point::new(xs[i0], ys[j0]);
        let origin = Point::new(corner.x * u.x + corner.y * v.x, corner.x * u.y + corner.y * v.y);
        out.push(if w >= h { Zone { origin, u, v, w, h } } else { Zone { origin, u: v, v: u, w: h, h: w } });
    }
    out
}

pub fn point_segment_distance(p: Point, a: Point, b: Point) -> f64 {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let len2 = dx * dx + dy * dy;
    let t = if len2 == 0.0 { 0.0 } else { (((p.x - a.x) * dx + (p.y - a.y) * dy) / len2).clamp(0.0, 1.0) };
    p.distance(Point::new(a.x + t * dx, a.y + t * dy))
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Bounds {
    pub min_x: f64,
    pub min_y: f64,
    pub max_x: f64,
    pub max_y: f64,
}

impl Bounds {
    pub const EMPTY: Bounds = Bounds { min_x: 0.0, min_y: 0.0, max_x: 0.0, max_y: 0.0 };

    pub fn from_points(points: &[Point]) -> Self {
        let mut iter = points.iter();
        let Some(first) = iter.next() else { return Bounds::EMPTY };
        iter.fold(Bounds { min_x: first.x, min_y: first.y, max_x: first.x, max_y: first.y }, |b, p| Bounds {
            min_x: b.min_x.min(p.x),
            min_y: b.min_y.min(p.y),
            max_x: b.max_x.max(p.x),
            max_y: b.max_y.max(p.y),
        })
    }

    pub fn union(self, other: Bounds) -> Bounds {
        Bounds {
            min_x: self.min_x.min(other.min_x),
            min_y: self.min_y.min(other.min_y),
            max_x: self.max_x.max(other.max_x),
            max_y: self.max_y.max(other.max_y),
        }
    }

    pub fn width(&self) -> f64 {
        self.max_x - self.min_x
    }

    pub fn height(&self) -> f64 {
        self.max_y - self.min_y
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn square_measurements() {
        let sq = Polygon::rectangle(Point::new(0.0, 0.0), 30.0, 30.0);
        assert_eq!(sq.area(), 900.0);
        assert_eq!(sq.perimeter(), 120.0);
        assert!(sq.signed_area() > 0.0);
    }

    #[test]
    fn crossing_and_touching_segments() {
        let p = Point::new;
        assert!(segments_cross(p(0.0, 0.0), p(10.0, 10.0), p(10.0, 0.0), p(0.0, 10.0), 1e-9));
        assert!(segments_cross(p(0.0, 0.0), p(10.0, 0.0), p(5.0, 0.0), p(5.0, 5.0), 1e-9)); // T junction
        assert!(!segments_cross(p(0.0, 0.0), p(10.0, 0.0), p(0.0, 1.0), p(10.0, 1.0), 1e-9));
        assert_eq!(path_length(&[p(0.0, 0.0), p(3.0, 4.0)], false), 5.0);
        assert_eq!(signed_area(&[p(0.0, 0.0), p(1.0, 0.0), p(1.0, 1.0)]), 0.5);
    }

    #[test]
    fn clockwise_input_is_normalised() {
        let cw = Polygon::new(vec![
            Point::new(0.0, 0.0),
            Point::new(0.0, 1.0),
            Point::new(1.0, 1.0),
            Point::new(1.0, 0.0),
        ]);
        assert!(cw.signed_area() > 0.0);
    }

    fn pts(v: &[(f64, f64)]) -> Vec<Point> {
        v.iter().map(|&(x, y)| Point::new(x, y)).collect()
    }

    fn inside_all(z: &Zone, outer: &[Point]) -> bool {
        // Corners pulled in a hair: the zone lies in the shape.
        let e = 1e-6;
        [(e, e), (z.w - e, e), (z.w - e, z.h - e), (e, z.h - e), (z.w / 2.0, z.h / 2.0)]
            .iter()
            .all(|&(s, t)| contains(outer, z.at(s, t)))
    }

    #[test]
    fn a_rectangle_is_one_zone() {
        let r = pts(&[(0.0, 0.0), (5.0, 0.0), (5.0, 3.0), (0.0, 3.0)]);
        let z = zones(&r, &[], 0.6);
        assert_eq!(z.len(), 1);
        assert!((z[0].w - 5.0).abs() < 1e-9 && (z[0].h - 3.0).abs() < 1e-9);
        let c = z[0].centre();
        assert!((c.x - 2.5).abs() < 1e-9 && (c.y - 1.5).abs() < 1e-9);
    }

    #[test]
    fn an_l_is_its_body_and_its_arm() {
        // A 2 m wide leg up the left, 7 m tall; a 3.5 m arm along the bottom, 2.7 m deep.
        let l = pts(&[(0.0, 0.0), (5.5, 0.0), (5.5, 2.7), (2.0, 2.7), (2.0, 7.0), (0.0, 7.0)]);
        let z = zones(&l, &[], 0.6);
        assert_eq!(z.len(), 2, "{z:?}");
        assert!(z[0].area() >= z[1].area());
        for zone in &z {
            assert!(inside_all(zone, &l), "{zone:?}");
        }
        let covered: f64 = z.iter().map(|z| z.area()).sum();
        assert!((covered - (5.5 * 2.7 + 2.0 * 4.3)).abs() < 1e-6);
        // Neither centre is near the inside corner (2, 2.7).
        for zone in &z {
            assert!(zone.centre().distance(Point::new(2.0, 2.7)) > 0.9);
        }
    }

    #[test]
    fn a_u_is_three_zones_and_a_hole_is_cut_round() {
        let u = pts(&[
            (0.0, 0.0),
            (6.0, 0.0),
            (6.0, 5.0),
            (4.0, 5.0),
            (4.0, 2.0),
            (2.0, 2.0),
            (2.0, 5.0),
            (0.0, 5.0),
        ]);
        assert_eq!(zones(&u, &[], 0.6).len(), 3);
        let outer = pts(&[(0.0, 0.0), (6.0, 0.0), (6.0, 6.0), (0.0, 6.0)]);
        let hole = pts(&[(2.0, 2.0), (2.0, 4.0), (4.0, 4.0), (4.0, 2.0)]);
        let z = zones(&outer, std::slice::from_ref(&hole), 0.6);
        assert_eq!(z.len(), 4, "{z:?}");
        for zone in &z {
            assert!(!contains(&hole, zone.centre()));
        }
    }

    #[test]
    fn slivers_get_no_zone() {
        // A 4 × 5 room with a 0.3 m jog along one side.
        let r = pts(&[(0.0, 0.0), (4.0, 0.0), (4.0, 4.7), (3.7, 4.7), (3.7, 5.0), (0.0, 5.0)]);
        let z = zones(&r, &[], 0.6);
        assert_eq!(z.len(), 1, "{z:?}");
        // A tiny alcove (a tenth of the room or less) gets none either.
        let a = pts(&[(0.0, 0.0), (6.0, 0.0), (6.0, 5.0), (1.5, 5.0), (1.5, 5.8), (0.0, 5.8)]);
        assert_eq!(zones(&a, &[], 0.6).len(), 1);
    }

    #[test]
    fn a_turned_room_is_cut_along_its_walls() {
        let (c, s) = (30f64.to_radians().cos(), 30f64.to_radians().sin());
        let turn = |x: f64, y: f64| Point::new(x * c - y * s, x * s + y * c);
        let r = vec![turn(0.0, 0.0), turn(8.0, 0.0), turn(8.0, 1.5), turn(0.0, 1.5)];
        let z = zones(&r, &[], 0.6);
        assert_eq!(z.len(), 1, "{z:?}");
        assert!((z[0].w - 8.0).abs() < 1e-6 && (z[0].h - 1.5).abs() < 1e-6, "{z:?}");
        assert!(inside_all(&z[0], &r));
    }
}
