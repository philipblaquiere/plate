//! The connect pass: each service's networks, once its parts are placed.
//!
//! A network (`connect`) joins parts (or tagged fixtures) back to a source, one network per
//! scope (a panel per flat). Terminals are taken in order, room by room from the source out, and
//! cut into groups of at most `max` (circuits). A network is drawn one of two ways:
//!
//! - `'arcs`, the electrical convention: an arc from part to part in a group and a home run
//!   toward the source, numbered. Each wall part must still be on a wall it can run in.
//! - `'routed`: along the walls it can run in (a cavity at least `via.cavity` deep) and across the
//!   ceilings of the spaces `via.ceilings` names, by the shortest way, each run checked against
//!   `limit`.

use std::cmp::Ordering;
use std::collections::BinaryHeap;

use crate::geometry::{interior_point, point_segment_distance, Point};
use crate::layout::{Element, ElementKind, RunInfo};
use crate::plan::{tag_of, Selector};
use crate::service::{Draw, Network};

use super::fit::{Instance, Issues, Model, Placed};
use super::num;

/// Something a network joins: a placed part, or a fixture.
#[derive(Debug, Clone)]
struct Node {
    at: Point,
    room: String,
    label: String,
    part: Option<usize>,
}

pub fn run(
    model: &Model,
    instances: &[Instance],
    placed: &mut [Placed],
    elements: &mut Vec<Element>,
    issues: &mut Issues,
) {
    let mut runs: Vec<Element> = Vec::new();
    for inst in instances {
        for (ni, net) in inst.def.connect.iter().enumerate() {
            let pointer = format!("{}/connect/{ni}", inst.def.source.clone().unwrap_or_default());
            let job = Job { model, inst, ni, net, pointer: &pointer };
            job.connect(placed, elements, &mut runs, issues);
        }
    }
    elements.extend(runs);
}

/// The parts and fixtures in this instance a selector names.
fn nodes(
    model: &Model,
    inst: &Instance,
    sel: &Selector,
    placed: &[Placed],
    elements: &[Element],
) -> Vec<Node> {
    let mut out = Vec::new();
    for item in sel.items() {
        if let Some(tag) = tag_of(item) {
            for room in model.rooms.iter().filter(|r| inst.covers(&r.path)) {
                for f in &room.fixtures {
                    let Some(info) = &elements[f.element].space else { continue };
                    if crate::plan::has_tag(&info.tags, info.kind, tag) {
                        let at = Point::new((f.back.0.x + f.back.1.x) / 2.0, (f.back.0.y + f.back.1.y) / 2.0);
                        out.push(Node { at, room: room.path.clone(), label: info.label.clone(), part: None });
                    }
                }
            }
            continue;
        }
        let (service, part) = item.split_once('.').unwrap_or((inst.service.as_str(), item));
        for (k, p) in placed.iter().enumerate() {
            if p.service == service && p.part == part && inst.covers(&p.room) {
                out.push(Node {
                    at: p.at,
                    room: p.room.clone(),
                    label: p.part.replace('_', " "),
                    part: Some(k),
                });
            }
        }
    }
    out
}

struct Job<'a> {
    model: &'a Model,
    inst: &'a Instance,
    ni: usize,
    net: &'a Network,
    pointer: &'a str,
}

impl Job<'_> {
    fn connect(
        &self,
        placed: &mut [Placed],
        elements: &[Element],
        runs: &mut Vec<Element>,
        issues: &mut Issues,
    ) {
        let (model, inst, net, pointer) = (self.model, self.inst, self.net, self.pointer);
        let units = model.units;
        let m = 1000.0 / units.to_mm();
        let scope = inst.space.clone().unwrap_or_default();
        let sources = nodes(model, inst, &net.from, placed, elements);
        let terminals = nodes(model, inst, &net.to, placed, elements);
        if terminals.is_empty() {
            return;
        }
        let Some(source) = sources.first().cloned() else {
            issues.add(
                pointer,
                format!("{}: nothing to connect to, no {} here", inst.service, net.from.items().join(" or ")),
                &scope,
                Some("Place the source part (a `one` placement), or tag the fixture it starts from.".into()),
            );
            return;
        };
        let network = net.label.clone().unwrap_or_else(|| inst.service.clone());
        let need = net.via.cavity.map_or(0.0, |c| c.0.to(units));

        // Wall parts must be on a wall the network can run in.
        if need > 0.0 {
            for t in &terminals {
                let Some((r, f)) = t.part.and_then(|k| placed[k].face) else { continue };
                let face = &model.rooms[r].faces[f];
                if face.cavity + 1e-9 < need {
                    let wall = face
                        .wall
                        .and_then(|w| elements[w].wall.as_ref())
                        .map_or_else(|| "it".to_owned(), |w| w.label.clone());
                    issues.add(
                        pointer,
                        format!("a {} is on a wall it can't run in ({wall} has a cavity of {}; it needs {})", t.label, num(face.cavity), num(need)),
                        &t.room,
                        Some("Give that wall's assembly a cavity, or keep the part off it (`wall.cavity` on a `one` placement).".into()),
                    );
                }
            }
        }

        // Terminals in order: from the source out, room by room.
        let mut order: Vec<usize> = Vec::new();
        let mut left: Vec<usize> = (0..terminals.len()).collect();
        let (mut at, mut room) = (source.at, source.room.clone());
        while !left.is_empty() {
            let pos = left
                .iter()
                .enumerate()
                .map(|(pos, &t)| {
                    let away = if terminals[t].room == room { 0.0 } else { 1000.0 * m };
                    (pos, terminals[t].at.distance(at) + away)
                })
                .min_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)))
                .map_or(0, |x| x.0);
            let t = left.remove(pos);
            at = terminals[t].at;
            room = terminals[t].room.clone();
            order.push(t);
        }
        let size = net.max.map_or(order.len(), |n| n.max(1) as usize).max(1);
        let groups: Vec<Vec<usize>> = order.chunks(size).map(|c| c.to_vec()).collect();
        let grouped = net.max.is_some();
        if grouped {
            for (g, group) in groups.iter().enumerate() {
                for &t in group {
                    if let Some(k) = terminals[t].part {
                        placed[k].group = Some(format!("{}", g + 1));
                    }
                }
            }
        }
        let mut push = |points: Vec<Point>, group: Option<String>, home: bool, draw: &'static str| {
            let mut e = Element::new(
                format!("run:{}:{}:{scope}#{}", inst.service, self.ni, runs.len()),
                ElementKind::Run,
                points,
                false,
            );
            e.source = Some(pointer.to_owned());
            e.run =
                Some(RunInfo { service: inst.service.clone(), network: network.clone(), group, home, draw });
            runs.push(e);
        };

        match net.draw {
            Draw::Arcs => {
                for (g, group) in groups.iter().enumerate() {
                    let label = grouped.then(|| format!("{}", g + 1));
                    for pair in group.windows(2) {
                        push(arc(terminals[pair[0]].at, terminals[pair[1]].at), label.clone(), false, "arcs");
                    }
                    // A home run, toward the source.
                    let first = terminals[group[0]].at;
                    let d = first.distance(source.at);
                    if d > 1e-9 {
                        let len = (0.6 * m).min(d * 0.5);
                        let u = Point::new((source.at.x - first.x) / d, (source.at.y - first.y) / d);
                        push(
                            vec![first, Point::new(first.x + u.x * len, first.y + u.y * len)],
                            label,
                            true,
                            "arcs",
                        );
                    }
                }
            }
            Draw::Routed => {
                // Every source starts a run: each terminal is joined to its nearest one.
                let graph = Graph::build(model, inst, net, &sources, &terminals, elements, need);
                let limit = net.limit.map(|l| l.0.to(units));
                let (dist, prev) = graph.shortest(&graph.sources);
                for (g, group) in groups.iter().enumerate() {
                    let label = grouped.then(|| format!("{}", g + 1));
                    for &t in group {
                        let node = graph.terminals[t];
                        let what = &terminals[t].label;
                        if !dist[node].is_finite() {
                            let hint = match net.via.ceilings {
                                Some(_) => "No wall it can run in leads there: give the walls on the way a cavity deep enough.",
                                None => "No wall it can run in leads there: give the walls on the way a cavity, or let it cross ceilings (`via.ceilings`).",
                            };
                            issues.add(
                                pointer,
                                format!("can't reach a {what} from the {}", source.label),
                                &terminals[t].room,
                                Some(hint.into()),
                            );
                            continue;
                        }
                        if let Some(limit) = limit.filter(|l| dist[node] > l + 1e-9) {
                            issues.add(
                                pointer,
                                format!("a {what} is {} from the {} (at most {})", num(dist[node]), source.label, num(limit)),
                                &terminals[t].room,
                                Some(format!("Bring the {what} closer to the {} (a claim such as `plate.near`), or allow a longer run.", source.label)),
                            );
                        }
                        let mut path = vec![graph.points[node]];
                        let mut n = node;
                        while let Some(p) = prev[n] {
                            path.push(graph.points[p]);
                            n = p;
                        }
                        path.dedup_by(|a, b| a.distance(*b) < 1e-9);
                        path.reverse();
                        push(path, label.clone(), false, "routed");
                    }
                }
            }
        }
    }
}

/// An arc from `a` to `b`, bowed to the left, as a polyline.
fn arc(a: Point, b: Point) -> Vec<Point> {
    let d = a.distance(b);
    if d <= 1e-9 {
        return vec![a, b];
    }
    let mid = Point::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0);
    let n = Point::new(-(b.y - a.y) / d, (b.x - a.x) / d);
    let c = Point::new(mid.x + n.x * d * 0.2, mid.y + n.y * d * 0.2);
    (0..=12)
        .map(|k| {
            let t = k as f64 / 12.0;
            let (u, v, w) = ((1.0 - t) * (1.0 - t), 2.0 * t * (1.0 - t), t * t);
            Point::new(u * a.x + v * c.x + w * b.x, u * a.y + v * c.y + w * b.y)
        })
        .collect()
}

/// Where a network can run, as a graph.
struct Graph {
    points: Vec<Point>,
    edges: Vec<Vec<(usize, f64)>>,
    sources: Vec<usize>,
    terminals: Vec<usize>,
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

impl Graph {
    fn node(&mut self, p: Point, tol: f64) -> usize {
        if let Some(i) = self.points.iter().position(|q| q.distance(p) < tol) {
            return i;
        }
        self.points.push(p);
        self.edges.push(Vec::new());
        self.points.len() - 1
    }

    fn link(&mut self, a: usize, b: usize) {
        if a == b || self.edges[a].iter().any(|e| e.0 == b) {
            return;
        }
        let d = self.points[a].distance(self.points[b]);
        self.edges[a].push((b, d));
        self.edges[b].push((a, d));
    }

    fn build(
        model: &Model,
        inst: &Instance,
        net: &Network,
        sources: &[Node],
        terminals: &[Node],
        elements: &[Element],
        need: f64,
    ) -> Graph {
        let tol = model.units.tolerance() * 100.0;
        let mut g =
            Graph { points: Vec::new(), edges: Vec::new(), sources: Vec::new(), terminals: Vec::new() };
        // Walls it can run in, in this scope, with their thickness.
        let segments: Vec<(Point, Point, f64)> = model
            .walls
            .iter()
            .filter_map(|&w| elements[w].wall.as_ref())
            .filter(|w| w.cavity + 1e-9 >= need)
            .filter(|w| w.between.iter().any(|n| !n.starts_with('\'') && inst.covers(n)))
            .map(|w| (w.line[0], w.line[1], w.thickness))
            .collect();
        let mut stops: Vec<Vec<f64>> = vec![vec![0.0, 1.0]; segments.len()];
        let mut extra: Vec<(Point, Point)> = Vec::new();
        for (k, &(a, b, _)) in segments.iter().enumerate() {
            for p in [a, b] {
                // Walls that meet here (one's end on another's line).
                let mut met = false;
                for (j, &(c, d, _)) in segments.iter().enumerate() {
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
                        .map(|(j, &(c, d, t))| (j, project(p, c, d), t))
                        .filter(|(_, (q, _), t)| q.distance(p) <= t / 2.0 + tol * 10.0)
                        .min_by(|x, y| x.1 .0.distance(p).total_cmp(&y.1 .0.distance(p)));
                    if let Some((j, (q, s), _)) = near {
                        stops[j].push(s);
                        extra.push((p, q));
                    }
                }
            }
        }
        // Where each thing joins: the nearest point on a wall it can run in.
        let things: Vec<Point> = sources.iter().chain(terminals).map(|t| t.at).collect();
        let joins: Vec<Option<Point>> = things
            .iter()
            .map(|&p| {
                segments
                    .iter()
                    .enumerate()
                    .map(|(k, &(a, b, _))| (k, project(p, a, b)))
                    .min_by(|x, y| x.1 .0.distance(p).total_cmp(&y.1 .0.distance(p)))
                    .map(|(k, (q, s))| {
                        stops[k].push(s);
                        q
                    })
            })
            .collect();
        for (k, &(a, b, _)) in segments.iter().enumerate() {
            let len = a.distance(b);
            if len <= tol {
                continue;
            }
            let mut s = std::mem::take(&mut stops[k]);
            s.sort_by(f64::total_cmp);
            s.dedup_by(|x, y| (*x - *y).abs() * len < tol);
            let at = |t: f64| Point::new(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t);
            for pair in s.windows(2) {
                let (p, q) = (g.node(at(pair[0]), tol), g.node(at(pair[1]), tol));
                g.link(p, q);
            }
        }
        for (p, q) in extra {
            let (p, q) = (g.node(p, tol), g.node(q, tol));
            g.link(p, q);
        }
        // Ceilings it can cross: a hub in the middle of each such space, joined to the walls round it.
        if let Some(ceilings) = &net.via.ceilings {
            for room in model.rooms.iter().filter(|r| inst.covers(&r.path)) {
                if !ceilings.matches_tagged(&room.name, &room.key, &room.tags, room.kind) {
                    continue;
                }
                let reach = room
                    .faces
                    .iter()
                    .filter_map(|f| f.wall.and_then(|w| elements[w].wall.as_ref()).map(|w| w.thickness))
                    .fold(0.0, f64::max)
                    + tol * 10.0;
                let wall_nodes = g.points.len();
                let hub = g.node(interior_point(&room.loops[0], &room.loops[1..]), tol);
                let around: Vec<usize> = (0..wall_nodes)
                    .filter(|&i| {
                        room.loops.iter().any(|l| {
                            (0..l.len()).any(|k| {
                                point_segment_distance(g.points[i], l[k], l[(k + 1) % l.len()]) <= reach
                            })
                        })
                    })
                    .collect();
                for i in around {
                    g.link(hub, i);
                }
            }
        }
        // The source and terminals, joined to their nearest wall.
        let mut ids = Vec::new();
        for (p, j) in things.iter().zip(&joins) {
            let id = g.node(*p, tol);
            if let Some(q) = j {
                let on = g.node(*q, tol);
                g.link(id, on);
            }
            ids.push(id);
        }
        g.sources = ids[..sources.len()].to_vec();
        g.terminals = ids[sources.len()..].to_vec();
        g
    }

    fn shortest(&self, from: &[usize]) -> (Vec<f64>, Vec<Option<usize>>) {
        let n = self.points.len();
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
            for &(v, w) in &self.edges[u] {
                let nd = d + w;
                if nd < dist[v] - 1e-12 {
                    dist[v] = nd;
                    prev[v] = Some(u);
                    heap.push(Item(nd, v));
                }
            }
        }
        (dist, prev)
    }
}
