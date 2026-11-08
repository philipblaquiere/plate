//! Regions: the spaces of one level joined by open edges, with no wall between them.
//!
//! Found after a level is placed, from its ports: two spaces join when a port between them is
//! `'open` and at least `passage` long (a door's width: a corner touch isn't a way through).
//! Circulation never joins a region (a hall stays a hall), and fixtures don't either. A space
//! joined to nothing isn't a region: placements that count per region treat it as its own.
//!
//! Regions never change a solve. They're drawn (an outline, a label, an area), exported as one
//! closed polyline, and they're what `per = 'region` counts.

use std::collections::BTreeMap;

use crate::geometry::{contains, Point};
use crate::plan::SpaceKind;

use super::outline;
use super::ports::OPEN;
use super::Placed;

/// One region of a level.
#[derive(Debug, Clone, PartialEq)]
pub struct Found {
    /// Indexes into the level's placed spaces, biggest first.
    pub members: Vec<usize>,
    /// The union of their shapes: outer loop (counter-clockwise), then holes.
    pub outer: Vec<Point>,
    pub holes: Vec<Vec<Point>>,
}

/// The regions of a level. `kind` says what each placed space is.
pub fn find(placed: &[Placed], kind: impl Fn(usize) -> SpaceKind, passage: f64) -> Vec<Found> {
    let n = placed.len();
    let joins = |i: usize| !matches!(kind(i), SpaceKind::Circulation | SpaceKind::Fixture);
    let index = |name: &str| placed.iter().position(|p| p.name == name);
    // Open edge seen from each side: each side sees its own stretch, the longer will do.
    let mut open: BTreeMap<(usize, usize), f64> = BTreeMap::new();
    for (i, p) in placed.iter().enumerate().filter(|(i, _)| joins(*i)) {
        for port in p.ports.iter().filter(|s| s.has(OPEN)) {
            if let Some(j) = port.other.as_deref().and_then(index).filter(|&j| j != i && joins(j)) {
                *open.entry((i, j)).or_default() += port.len();
            }
        }
    }
    let mut parent: Vec<usize> = (0..n).collect();
    fn root(parent: &mut [usize], mut i: usize) -> usize {
        while parent[i] != i {
            parent[i] = parent[parent[i]];
            i = parent[i];
        }
        i
    }
    for (&(i, j), &len) in &open {
        let other = open.get(&(j, i)).copied().unwrap_or(0.0);
        if len.max(other) >= passage - 1e-9 {
            let (a, b) = (root(&mut parent, i), root(&mut parent, j));
            if a != b {
                parent[a.max(b)] = a.min(b);
            }
        }
    }
    let mut groups: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for i in (0..n).filter(|&i| joins(i)) {
        let r = root(&mut parent, i);
        groups.entry(r).or_default().push(i);
    }
    let area = |i: usize| {
        let p = &placed[i];
        crate::geometry::signed_area(&p.polygon).abs()
            - p.holes.iter().map(|h| crate::geometry::signed_area(h).abs()).sum::<f64>()
    };
    groups
        .into_values()
        .filter(|g| g.len() > 1)
        .map(|mut members| {
            members.sort_by(|&a, &b| area(b).total_cmp(&area(a)).then(a.cmp(&b)));
            let (outer, holes) = union(members.iter().map(|&i| &placed[i]));
            Found { members, outer, holes }
        })
        .collect()
}

/// The outline of the union of some spaces' shapes, on a grid of their own coordinates.
fn union<'a>(spaces: impl Iterator<Item = &'a Placed> + Clone) -> (Vec<Point>, Vec<Vec<Point>>) {
    let mut xs: Vec<f64> = Vec::new();
    let mut ys: Vec<f64> = Vec::new();
    for p in spaces.clone() {
        for q in p.polygon.iter().chain(p.holes.iter().flatten()) {
            xs.push(q.x);
            ys.push(q.y);
        }
    }
    let tidy = |v: &mut Vec<f64>| {
        v.sort_by(f64::total_cmp);
        v.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
    };
    tidy(&mut xs);
    tidy(&mut ys);
    if xs.len() < 2 || ys.len() < 2 {
        return (Vec::new(), Vec::new());
    }
    let (nx, ny) = (xs.len() as i32 - 1, ys.len() as i32 - 1);
    let filled = |i: i32, j: i32| {
        let c = Point::new(
            (xs[i as usize] + xs[i as usize + 1]) / 2.0,
            (ys[j as usize] + ys[j as usize + 1]) / 2.0,
        );
        spaces.clone().any(|p| contains(&p.polygon, c) && !p.holes.iter().any(|h| contains(h, c)))
    };
    let at = |(i, j): (i32, i32)| Point::new(xs[i as usize], ys[j as usize]);
    let mut found = outline::regions(nx, ny, filled);
    // Joined by open edges, the spaces are one piece; keep the biggest should rounding split it.
    found.sort_by_key(|r| -r.cells);
    let Some(r) = found.into_iter().next() else { return (Vec::new(), Vec::new()) };
    (
        r.outer.into_iter().map(at).collect(),
        r.holes.into_iter().map(|h| h.into_iter().map(at).collect()).collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::solve::frame::Transform;
    use crate::solve::ports::PortSeg;

    /// An open stretch of a space's edge: the neighbour, and the stretch's ends in the space's frame.
    type Open<'a> = (&'a str, (f64, f64), (f64, f64));

    fn rect(name: &str, x0: f64, y0: f64, x1: f64, y1: f64, open: &[Open]) -> Placed {
        let polygon = vec![Point::new(x0, y0), Point::new(x1, y0), Point::new(x1, y1), Point::new(x0, y1)];
        let ports = open
            .iter()
            .map(|(other, a, b)| PortSeg {
                a: Point::new(a.0, a.1),
                b: Point::new(b.0, b.1),
                roles: vec![OPEN.to_owned()],
                other: Some((*other).to_owned()),
            })
            .collect();
        Placed {
            name: name.into(),
            type_key: name.into(),
            polygon,
            rect: Some((x1 - x0, y1 - y0, Transform::IDENTITY)),
            ports,
            fixture: None,
            holes: Vec::new(),
        }
    }

    #[test]
    fn joins_spaces_by_open_edges_long_enough() {
        // a | b open along 4; b | c open along only 0.5; d alone.
        let placed = vec![
            rect("a", 0.0, 0.0, 4.0, 4.0, &[("b", (4.0, 0.0), (4.0, 4.0))]),
            rect("b", 4.0, 0.0, 7.0, 4.0, &[("a", (0.0, 0.0), (0.0, 4.0)), ("c", (0.0, 4.0), (0.5, 4.0))]),
            rect("c", 4.0, 4.0, 7.0, 6.0, &[("b", (0.0, 0.0), (0.5, 0.0))]),
            rect("d", 0.0, 4.0, 4.0, 6.0, &[]),
        ];
        let found = find(&placed, |_| SpaceKind::Zone, 0.9);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].members, vec![0, 1]); // biggest first
        let area = crate::geometry::signed_area(&found[0].outer).abs();
        assert!((area - 28.0).abs() < 1e-9, "{area}");
        assert_eq!(found[0].outer.len(), 4, "one rectangle: {:?}", found[0].outer);
    }

    #[test]
    fn circulation_never_joins() {
        let placed = vec![
            rect("living", 0.0, 0.0, 4.0, 4.0, &[("hall", (4.0, 0.0), (4.0, 4.0))]),
            rect("hall", 4.0, 0.0, 6.0, 4.0, &[("living", (0.0, 0.0), (0.0, 4.0))]),
        ];
        let kind = |i: usize| if i == 1 { SpaceKind::Circulation } else { SpaceKind::Zone };
        assert!(find(&placed, kind, 0.9).is_empty());
    }

    #[test]
    fn an_l_of_three_has_one_outline() {
        let placed = vec![
            rect("a", 0.0, 0.0, 4.0, 3.0, &[("b", (4.0, 0.0), (4.0, 3.0)), ("c", (0.0, 3.0), (4.0, 3.0))]),
            rect("b", 4.0, 0.0, 6.0, 3.0, &[("a", (0.0, 0.0), (0.0, 3.0))]),
            rect("c", 0.0, 3.0, 4.0, 5.0, &[("a", (0.0, 0.0), (4.0, 0.0))]),
        ];
        let found = find(&placed, |_| SpaceKind::Room, 0.9);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].outer.len(), 6, "an L: {:?}", found[0].outer);
        assert!(found[0].holes.is_empty());
    }
}
