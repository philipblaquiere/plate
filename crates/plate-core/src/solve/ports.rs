//! Ports: labelled parts of a slot's boundary (`entrance`, `facade`, `wet`, …).
//!
//! A parent level assigns ports to every space it places, from geometry: the part of a space's
//! wall on the parent's facade is `facade`, the wall against a corridor is `entrance`, a wall
//! shared with a sibling of the same type is `party`, and so on. The child plan only ever talks
//! about ports, never about its parent's spaces, which is what makes plans portable.

use serde::Serialize;

use super::frame::Transform;
use crate::geometry::Point;
use crate::plan::role_matches;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PortSeg {
    pub roles: Vec<String>,
    pub a: Point,
    pub b: Point,
    /// The sibling on the other side, if any (not part of a typical's identity).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub other: Option<String>,
}

impl PortSeg {
    pub fn new(roles: Vec<String>, a: Point, b: Point) -> PortSeg {
        PortSeg { roles, a, b, other: None }
    }

    pub fn len(&self) -> f64 {
        self.a.distance(self.b)
    }

    pub fn has(&self, wanted: &str) -> bool {
        self.roles.iter().any(|r| role_matches(r, wanted))
    }

    pub fn transformed(&self, t: &Transform) -> PortSeg {
        PortSeg {
            roles: self.roles.clone(),
            a: t.apply(self.a),
            b: t.apply(self.b),
            other: self.other.clone(),
        }
    }
}

/// If segments p–q and r–s lie on the same line (within `tol`), the part of p–q they share, as
/// parameters `(t0, t1)` along p–q.
pub fn collinear_overlap(p: Point, q: Point, r: Point, s: Point, tol: f64) -> Option<(f64, f64)> {
    let (dx, dy) = (q.x - p.x, q.y - p.y);
    let len2 = dx * dx + dy * dy;
    if len2 <= tol * tol {
        return None;
    }
    let len = len2.sqrt();
    let off = |o: Point| ((o.x - p.x) * dy - (o.y - p.y) * dx).abs() / len;
    if off(r) > tol || off(s) > tol {
        return None;
    }
    let t = |o: Point| ((o.x - p.x) * dx + (o.y - p.y) * dy) / len2;
    let (t0, t1) = (t(r).min(t(s)).max(0.0), t(r).max(t(s)).min(1.0));
    (t1 - t0 > tol / len).then_some((t0, t1))
}

fn lerp(p: Point, q: Point, t: f64) -> Point {
    Point::new(p.x + (q.x - p.x) * t, p.y + (q.y - p.y) * t)
}

/// Something a wall can be against: a port of the slot, or a sibling's wall.
pub struct Source<'a> {
    pub segments: &'a [(Point, Point)],
    pub roles: Vec<String>,
    pub other: Option<String>,
    /// Counts as "something is here" for `exterior` (siblings do; slot ports don't).
    pub occupies: bool,
}

/// Ports along the given walls (in the level's frame): every overlap with a source, plus
/// `exterior` on wall parts nothing occupies (used where a level has no boundary of its own,
/// such as a spine in open space).
pub fn ports_along(
    walls: &[(Point, Point)],
    sources: &[Source],
    exterior: Option<&str>,
    tol: f64,
) -> Vec<PortSeg> {
    let mut out = Vec::new();
    for &(p, q) in walls {
        let mut occupied: Vec<(f64, f64)> = Vec::new();
        for source in sources {
            for &(r, s) in source.segments {
                if let Some((t0, t1)) = collinear_overlap(p, q, r, s, tol) {
                    out.push(PortSeg {
                        roles: source.roles.clone(),
                        a: lerp(p, q, t0),
                        b: lerp(p, q, t1),
                        other: source.other.clone(),
                    });
                    if source.occupies {
                        occupied.push((t0, t1));
                    }
                }
            }
        }
        if let Some(role) = exterior {
            for (t0, t1) in gaps(&mut occupied, tol / p.distance(q).max(tol)) {
                out.push(PortSeg::new(vec![role.to_owned()], lerp(p, q, t0), lerp(p, q, t1)));
            }
        }
    }
    // Merge identical roles on touching, collinear pieces would be nice; not needed for solving.
    out.retain(|s| s.len() > tol && !s.roles.is_empty());
    out
}

/// The role of an edge with no wall: a zone's side facing open floor.
pub const OPEN: &str = "open";

/// The role of a port where a door opens into the space: the door itself, not the whole wall.
pub const DOOR: &str = "door";

/// The role of a port where a window is: the window itself, not the whole wall.
pub const WINDOW: &str = "window";

/// The role of a port where an opening (a gap with no door) is: the gap itself.
pub const OPENING: &str = "opening";

/// Is this a port of a gap in a wall (a door, a window, an opening) rather than of the wall?
pub fn is_gap(roles: &[String]) -> bool {
    roles.iter().all(|r| r == DOOR || r == WINDOW || r == OPENING)
}

/// `door` ports: the part of each door (a segment in the level's frame) that lies on one of
/// `walls`. Fixtures inside keep the door's swing clear.
pub fn door_ports(walls: &[(Point, Point)], doors: &[(Point, Point)], tol: f64) -> Vec<PortSeg> {
    opening_ports(walls, doors, DOOR, tol)
}

/// `window` ports: the part of each window that lies on one of `walls`, so rules inside can keep
/// things away from it (`plate.apart "bed" 'window`).
pub fn window_ports(walls: &[(Point, Point)], windows: &[(Point, Point)], tol: f64) -> Vec<PortSeg> {
    opening_ports(walls, windows, WINDOW, tol)
}

/// `opening` ports: the part of each opening (a gap with no door) that lies on one of `walls`.
pub fn gap_ports(walls: &[(Point, Point)], gaps: &[(Point, Point)], tol: f64) -> Vec<PortSeg> {
    opening_ports(walls, gaps, OPENING, tol)
}

fn opening_ports(
    walls: &[(Point, Point)],
    openings: &[(Point, Point)],
    role: &str,
    tol: f64,
) -> Vec<PortSeg> {
    let source = Source { segments: openings, roles: vec![role.to_owned()], other: None, occupies: false };
    ports_along(walls, std::slice::from_ref(&source), None, tol)
}

/// The parts of [0, 1] not covered by `intervals`.
fn gaps(intervals: &mut [(f64, f64)], eps: f64) -> Vec<(f64, f64)> {
    intervals.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut out = Vec::new();
    let mut at = 0.0;
    for &(a, b) in intervals.iter() {
        if a > at + eps {
            out.push((at, a));
        }
        at = f64::max(at, b);
    }
    if at < 1.0 - eps {
        out.push((at, 1.0));
    }
    out
}

/// Total length of ports with `role` that a wall p–q shares.
pub fn shared_with_role(p: Point, q: Point, ports: &[PortSeg], role: &str, tol: f64) -> f64 {
    let len = p.distance(q);
    ports
        .iter()
        .filter(|s| s.has(role))
        .filter_map(|s| collinear_overlap(p, q, s.a, s.b, tol))
        .map(|(t0, t1)| (t1 - t0) * len)
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlap_of_collinear_segments() {
        let o = collinear_overlap(
            Point::new(0.0, 0.0),
            Point::new(10.0, 0.0),
            Point::new(12.0, 0.0),
            Point::new(4.0, 0.0),
            1e-6,
        );
        assert_eq!(o, Some((0.4, 1.0)));
        assert!(collinear_overlap(
            Point::new(0.0, 0.0),
            Point::new(10.0, 0.0),
            Point::new(0.0, 1.0),
            Point::new(4.0, 1.0),
            1e-6
        )
        .is_none());
    }

    #[test]
    fn exterior_fills_the_gaps() {
        let wall = [(Point::new(0.0, 0.0), Point::new(10.0, 0.0))];
        let neighbour = [(Point::new(2.0, 0.0), Point::new(5.0, 0.0))];
        let sources = [Source {
            segments: &neighbour,
            roles: vec!["party".into()],
            other: Some("room_2".into()),
            occupies: true,
        }];
        let ports = ports_along(&wall, &sources, Some("facade"), 1e-6);
        let facade: f64 = ports.iter().filter(|p| p.has("facade")).map(PortSeg::len).sum();
        assert!((facade - 7.0).abs() < 1e-9);
        assert!(ports.iter().any(|p| p.has("party") && p.other.as_deref() == Some("room_2")));
    }
}
