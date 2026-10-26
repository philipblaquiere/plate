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
}
