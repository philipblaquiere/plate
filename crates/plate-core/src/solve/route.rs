//! Where a network's runs can go, as a graph.
//!
//! Runs live in wall cavities and across ceilings, never across a floor. A wall a network can
//! run in (a cavity at least `via.cavity` deep, and an assembly that lets runs through) has two
//! bands when the network can also cross ceilings:
//!
//! - **low**, at outlet height: broken at doors, since nothing runs through a doorway;
//! - **high**, just under the ceiling: it passes over door and window headers.
//!
//! **Risers** join the bands anywhere along a wall, and the high band joins the **ceilings** of
//! the rooms the network may cross (`via.ceilings`), where runs go square to the room (along
//! the joists, then across). A network that can't cross ceilings (a drain) has the low band
//! only, unbroken: it runs in the walls and under the doors, as drains do below the floor.
//!
//! Costs are lengths, plus a little for each riser, so runs look as an electrician wires:
//! receptacles stay low in the walls, lights are reached through the ceiling, and a run goes up
//! and over a door rather than round the room.

use std::cmp::Ordering;
use std::collections::BinaryHeap;

use crate::assembly::Cross;
use crate::geometry::{contains, point_segment_distance, Point};
use crate::layout::{Element, ElementKind};
use crate::service::Via;

use super::fit::{Instance, Model};

/// Where a piece of a run goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Along {
    /// In a wall, low.
    Wall,
    /// In a wall, high: under the ceiling, over doors.
    Header,
    /// Up or down a wall, between the bands.
    Riser,
    /// Across a ceiling.
    Ceiling,
    /// From a band or a ceiling into a part.
    Drop,
}

impl Along {
    pub fn name(self) -> &'static str {
        match self {
            Along::Wall => "wall",
            Along::Header => "header",
            Along::Riser => "riser",
            Along::Ceiling => "ceiling",
            Along::Drop => "drop",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Edge {
    pub to: usize,
    /// What the shortest way minimises.
    pub cost: f64,
    /// How much cable it takes: its length in plan, plus any height it climbs.
    pub length: f64,
    pub along: Along,
    /// The corner of a ceiling run that goes square to its room.
    pub bend: Option<Point>,
    /// The room whose ceiling it crosses.
    pub room: Option<usize>,
}

/// A node: a point, and what it's on.
#[derive(Debug, Clone, Copy)]
pub struct Node {
    pub at: Point,
    /// 0: low band, 1: high band, 2: a thing joined (a part, a fixture).
    pub level: u8,
    /// The wall it's in (an element index), for nodes in a band.
    pub wall: Option<usize>,
}

/// Something to join: where it is, what it's against, how high.
#[derive(Debug, Clone, Copy)]
pub struct Spot {
    pub at: Point,
    /// The face it's on (room index, face index).
    pub face: Option<(usize, usize)>,
    pub room: Option<usize>,
    /// Height above the floor, for wall parts.
    pub above: Option<f64>,
    /// On the ceiling.
    pub ceiling: bool,
}

pub struct Route {
    pub nodes: Vec<Node>,
    pub edges: Vec<Vec<Edge>>,
    /// The node of each spot passed to [`Route::build`], in order.
    pub spots: Vec<usize>,
    /// Whether it has two bands and ceilings.
    pub tall: bool,
}

/// Extra cost of a riser, so runs don't climb for nothing.
const RISER_MM: f64 = 300.0;
/// The low band's height above the floor, and the high band's below the ceiling.
const LOW_MM: f64 = 300.0;
const HIGH_MM: f64 = 150.0;

impl Route {
    fn node(&mut self, at: Point, level: u8, wall: Option<usize>, tol: f64) -> usize {
        if level < 2 {
            if let Some(i) = self.nodes.iter().position(|n| n.level == level && n.at.distance(at) < tol) {
                return i;
            }
        }
        self.nodes.push(Node { at, level, wall });
        self.edges.push(Vec::new());
        self.nodes.len() - 1
    }

    #[allow(clippy::too_many_arguments)]
    fn link(
        &mut self,
        a: usize,
        b: usize,
        cost: f64,
        length: f64,
        along: Along,
        bend: Option<Point>,
        room: Option<usize>,
    ) {
        if a == b || self.edges[a].iter().any(|e| e.to == b && e.along == along) {
            return;
        }
        self.edges[a].push(Edge { to: b, cost, length, along, bend, room });
        self.edges[b].push(Edge { to: a, cost, length, along, bend, room });
    }

    /// The low and high bands' heights.
    pub fn bands(model: &Model) -> (f64, f64) {
        let mm = model.units.to_mm();
        let low = LOW_MM / mm;
        (low, (model.height - HIGH_MM / mm).max(low * 2.0))
    }

    /// The graph for one network in one scope, with `spots` joined to it.
    pub fn build(model: &Model, inst: &Instance, via: &Via, elements: &[Element], spots: &[Spot]) -> Route {
        let units = model.units;
        let tol = units.tolerance() * 100.0;
        let mm = units.to_mm();
        let need = via.cavity.map_or(0.0, |c| c.0.to(units));
        let tall = via.ceilings.is_some();
        let (low_h, high_h) = Route::bands(model);
        let mut g = Route { nodes: Vec::new(), edges: Vec::new(), spots: Vec::new(), tall };

        // Walls it can run in, in this scope: (a, b, thickness, element).
        let segments: Vec<(Point, Point, f64, usize)> = model
            .walls
            .iter()
            .filter_map(|&e| elements[e].wall.as_ref().map(|w| (e, w)))
            .filter(|(_, w)| w.cavity + 1e-9 >= need && w.cross != Cross::No.name())
            .filter(|(_, w)| w.between.iter().any(|n| !n.starts_with('\'') && inst.covers(n)))
            .map(|(e, w)| (w.line[0], w.line[1], w.thickness, e))
            .collect();
        let mut stops: Vec<Vec<f64>> = vec![vec![0.0, 1.0]; segments.len()];
        let mut extra: Vec<(Point, Point)> = Vec::new();
        for (k, &(a, b, ..)) in segments.iter().enumerate() {
            for p in [a, b] {
                // Walls that meet here (one's end on another's line).
                let mut met = false;
                for (j, &(c, d, ..)) in segments.iter().enumerate() {
                    if j != k && point_segment_distance(p, c, d) < tol {
                        stops[j].push(project(p, c, d).1);
                        met = true;
                    }
                }
                // A wall inside a space stops at the face of the wall around it, half that
                // wall's thickness short of its centreline: join it across.
                if !met {
                    let near = segments
                        .iter()
                        .enumerate()
                        .filter(|(j, _)| *j != k)
                        .map(|(j, &(c, d, t, _))| (j, project(p, c, d), t))
                        .filter(|(_, (q, _), t)| q.distance(p) <= t / 2.0 + tol * 10.0)
                        .min_by(|x, y| x.1 .0.distance(p).total_cmp(&y.1 .0.distance(p)));
                    if let Some((j, (q, s), _)) = near {
                        stops[j].push(s);
                        extra.push((p, q));
                    }
                }
            }
        }
        // Doorways along each wall: the low band breaks there.
        let mut doors: Vec<Vec<(f64, f64)>> = vec![Vec::new(); segments.len()];
        if tall {
            for e in elements.iter().filter(|e| e.kind == ElementKind::Door && e.points.len() >= 2) {
                let (p, q) = (e.points[0], e.points[1]);
                for (k, &(a, b, t, _)) in segments.iter().enumerate() {
                    let reach = t / 2.0 + tol * 10.0;
                    if point_segment_distance(p, a, b) > reach || point_segment_distance(q, a, b) > reach {
                        continue;
                    }
                    let (s0, s1) = (project(p, a, b).1, project(q, a, b).1);
                    let (s0, s1) = (s0.min(s1), s0.max(s1));
                    if (s1 - s0) * a.distance(b) > tol {
                        doors[k].push((s0, s1));
                        stops[k].push(s0);
                        stops[k].push(s1);
                    }
                }
            }
        }
        // Where each spot joins a wall: its own wall, or (off the walls) the nearest wall of its
        // room. Ceiling spots join ceilings instead, when there are some.
        let joins: Vec<Option<(usize, Point)>> = spots
            .iter()
            .map(|t| {
                if t.ceiling && tall {
                    return None;
                }
                let may_join = |wall: usize| match (t.face, t.room) {
                    (Some((r, f)), _) => model.rooms[r].faces[f].wall == Some(wall),
                    (None, Some(r)) => model.rooms[r].faces.iter().any(|f| f.wall == Some(wall)),
                    (None, None) => false,
                };
                segments
                    .iter()
                    .enumerate()
                    .filter(|(_, s)| may_join(s.3))
                    .map(|(k, &(a, b, ..))| (k, project(t.at, a, b)))
                    .min_by(|x, y| x.1 .0.distance(t.at).total_cmp(&y.1 .0.distance(t.at)))
                    .map(|(k, (q, s))| {
                        stops[k].push(s);
                        (k, q)
                    })
            })
            .collect();

        // The bands.
        for (k, &(a, b, _, e)) in segments.iter().enumerate() {
            let len = a.distance(b);
            if len <= tol {
                continue;
            }
            let mut s = std::mem::take(&mut stops[k]);
            s.sort_by(f64::total_cmp);
            s.dedup_by(|x, y| (*x - *y).abs() * len < tol);
            let at = |t: f64| Point::new(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t);
            let lows: Vec<usize> = s.iter().map(|&t| g.node(at(t), 0, Some(e), tol)).collect();
            for (i, pair) in s.windows(2).enumerate() {
                let mid = (pair[0] + pair[1]) / 2.0;
                if doors[k].iter().any(|&(d0, d1)| mid > d0 && mid < d1) {
                    continue;
                }
                let d = (pair[1] - pair[0]) * len;
                g.link(lows[i], lows[i + 1], d, d, Along::Wall, None, None);
            }
            if tall {
                let highs: Vec<usize> = s.iter().map(|&t| g.node(at(t), 1, Some(e), tol)).collect();
                for (i, pair) in s.windows(2).enumerate() {
                    let d = (pair[1] - pair[0]) * len;
                    g.link(highs[i], highs[i + 1], d, d, Along::Header, None, None);
                }
                let climb = high_h - low_h;
                for (l, h) in lows.iter().zip(&highs) {
                    g.link(*l, *h, climb + RISER_MM / mm, climb, Along::Riser, None, None);
                }
            }
        }
        for (p, q) in extra {
            for level in if tall { 0..2 } else { 0..1 } {
                let (p, q) = (g.node(p, level, None, tol), g.node(q, level, None, tol));
                let d = g.nodes[p].at.distance(g.nodes[q].at);
                let along = if level == 0 { Along::Wall } else { Along::Header };
                g.link(p, q, d, d, along, None, None);
            }
        }

        // The spots.
        let mut ids = Vec::new();
        for (t, j) in spots.iter().zip(&joins) {
            let id = g.node(t.at, 2, None, tol);
            if let Some((k, q)) = *j {
                let flat = t.at.distance(q);
                let above = t.above.unwrap_or(low_h);
                let low = g.node(q, 0, Some(segments[k].3), tol);
                let d = flat + (above - low_h).abs();
                g.link(id, low, d, d, Along::Drop, None, None);
                if tall {
                    let high = g.node(q, 1, Some(segments[k].3), tol);
                    let d = flat + (high_h - above).abs();
                    g.link(id, high, d, d, Along::Drop, None, None);
                }
            }
            ids.push(id);
        }

        // Ceilings: the high band round each room, and the ceiling parts in it.
        if let Some(ceilings) = &via.ceilings {
            for (r, room) in model.rooms.iter().enumerate().filter(|(_, r)| inst.covers(&r.path)) {
                let allowed = ceilings.matches_tagged(&room.name, &room.key, &room.tags, room.kind);
                let here: Vec<usize> = spots
                    .iter()
                    .enumerate()
                    .filter(|(_, t)| t.ceiling && t.room == Some(r))
                    .map(|(i, _)| ids[i])
                    .collect();
                if !allowed && here.is_empty() {
                    continue;
                }
                let reach = room
                    .faces
                    .iter()
                    .filter_map(|f| f.wall.and_then(|w| elements[w].wall.as_ref()).map(|w| w.thickness))
                    .fold(0.0, f64::max)
                    + tol * 10.0;
                let near_room = |p: Point| {
                    room.loops.iter().any(|l| {
                        (0..l.len()).any(|k| point_segment_distance(p, l[k], l[(k + 1) % l.len()]) <= reach)
                    })
                };
                let around: Vec<usize> = (0..g.nodes.len())
                    .filter(|&i| g.nodes[i].level == 1 && near_room(g.nodes[i].at))
                    .collect();
                // The room's axes: along its longest side, and across.
                let outer = &room.loops[0];
                let (mut u, mut best) = (Point::new(1.0, 0.0), 0.0);
                for k in 0..outer.len() {
                    let (a, b) = (outer[k], outer[(k + 1) % outer.len()]);
                    let l = a.distance(b);
                    if l > best {
                        best = l;
                        u = Point::new((b.x - a.x) / l, (b.y - a.y) / l);
                    }
                }
                let inside = |p: Point| {
                    near_room(p) || (contains(outer, p) && !room.loops[1..].iter().any(|h| contains(h, p)))
                };
                let square = |a: Point, b: Point| -> Option<(f64, Option<Point>)> {
                    let (dx, dy) = (b.x - a.x, b.y - a.y);
                    let du = dx * u.x + dy * u.y;
                    let dv = -dx * u.y + dy * u.x;
                    let d = du.abs() + dv.abs();
                    if du.abs() <= tol || dv.abs() <= tol {
                        return Some((d, None));
                    }
                    let c1 = Point::new(a.x + u.x * du, a.y + u.y * du);
                    let c2 = Point::new(a.x - u.y * dv, a.y + u.x * dv);
                    [c1, c2].into_iter().find(|&c| inside(c)).map(|c| (d, Some(c)))
                };
                let mut pairs: Vec<(usize, usize)> = Vec::new();
                for (i, &a) in here.iter().enumerate() {
                    for &b in &here[i + 1..] {
                        pairs.push((a, b));
                    }
                    for &b in &around {
                        pairs.push((a, b));
                    }
                }
                if allowed {
                    for (i, &a) in around.iter().enumerate() {
                        for &b in &around[i + 1..] {
                            // Not along one wall: that's the band's job.
                            if g.nodes[a].wall.is_some() && g.nodes[a].wall == g.nodes[b].wall {
                                continue;
                            }
                            pairs.push((a, b));
                        }
                    }
                }
                for (a, b) in pairs {
                    if let Some((d, bend)) = square(g.nodes[a].at, g.nodes[b].at) {
                        g.link(a, b, d, d, Along::Ceiling, bend, Some(r));
                    }
                }
            }
        }
        g.spots = ids;
        g
    }

    /// Shortest ways from `from`: distances, and the way back (previous node, edge index).
    pub fn shortest(&self, from: &[usize]) -> (Vec<f64>, Vec<Option<(usize, usize)>>) {
        let n = self.nodes.len();
        let mut dist = vec![f64::INFINITY; n];
        let mut prev = vec![None; n];
        let mut heap = BinaryHeap::new();
        for &f in from {
            dist[f] = 0.0;
            heap.push(Item(0.0, f));
        }
        while let Some(Item(d, u)) = heap.pop() {
            if d > dist[u] {
                continue;
            }
            for (k, e) in self.edges[u].iter().enumerate() {
                let nd = d + e.cost;
                if nd < dist[e.to] - 1e-12 {
                    dist[e.to] = nd;
                    prev[e.to] = Some((u, k));
                    heap.push(Item(nd, e.to));
                }
            }
        }
        (dist, prev)
    }

    /// The way from a source of `prev` to `to`, as (node, edge taken to reach the next node).
    pub fn path(&self, prev: &[Option<(usize, usize)>], to: usize) -> Vec<(usize, Edge)> {
        let mut out = Vec::new();
        let mut n = to;
        while let Some((p, k)) = prev[n] {
            out.push((p, self.edges[p][k]));
            n = p;
        }
        out.reverse();
        out
    }
}

#[derive(PartialEq)]
struct Item(f64, usize);

impl Eq for Item {}

impl PartialOrd for Item {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Item {
    fn cmp(&self, other: &Self) -> Ordering {
        other.0.total_cmp(&self.0).then(other.1.cmp(&self.1))
    }
}

/// The nearest point of segment a–b to p, and how far along it.
fn project(p: Point, a: Point, b: Point) -> (Point, f64) {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let len2 = (dx * dx + dy * dy).max(1e-18);
    let t = (((p.x - a.x) * dx + (p.y - a.y) * dy) / len2).clamp(0.0, 1.0);
    (Point::new(a.x + dx * t, a.y + dy * t), t)
}

/// Routes on a pinned flat (`testkit::pinned_flat`).
#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::project;
    use crate::geometry::Point;
    use crate::layout::ElementKind;
    use crate::solve::testkit::*;

    /// Pieces of routed runs as (along, a, b).
    fn pieces(s: &crate::solve::Solution) -> Vec<(&'static str, Point, Point)> {
        runs(s, "routed")
            .iter()
            .flat_map(|r| {
                let along = r.run.as_ref().unwrap().along.unwrap_or("?");
                r.points.windows(2).map(move |w| (along, w[0], w[1]))
            })
            .collect()
    }

    #[test]
    fn runs_low_in_a_wall_never_pass_through_a_doorway() {
        let s = run(pinned_flat_with(json!({ "power": power(), "plumbing": plumbing() })));
        assert_met(&s);
        let doors = of_kind(&s, ElementKind::Door);
        assert_eq!(doors.len(), 3);
        for (along, a, b) in pieces(&s).into_iter().filter(|p| p.0 == "wall") {
            let steps = (a.distance(b) / 0.02).ceil().max(1.0) as usize;
            for k in 0..=steps {
                let t = k as f64 / steps as f64;
                let p = Point::new(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t);
                for d in &doors {
                    let (q, u) = project(p, d.points[0], d.points[1]);
                    // Inside the opening (not at its jambs) and within a wall's thickness of it.
                    assert!(
                        !(u > 0.02 && u < 0.98 && q.distance(p) < 0.2),
                        "a {along} run passes through {} at {p:?}",
                        d.id
                    );
                }
            }
        }
    }

    #[test]
    fn every_piece_says_where_it_goes() {
        let s = run(pinned_flat_with(json!({ "power": power() })));
        for (along, a, b) in pieces(&s) {
            assert!(["wall", "header", "ceiling", "drop"].contains(&along), "{along}: {a:?} → {b:?}");
        }
        // Power may cross the hall's ceiling, so at least one piece does or goes over a door.
        assert!(pieces(&s).iter().any(|p| p.0 == "ceiling" || p.0 == "header"));
    }

    #[test]
    fn routing_is_the_same_every_time() {
        let a = run(pinned_flat_with(json!({ "power": power(), "plumbing": plumbing() })));
        let b = run(pinned_flat_with(json!({ "power": power(), "plumbing": plumbing() })));
        assert_eq!(pieces(&a), pieces(&b));
    }
}
