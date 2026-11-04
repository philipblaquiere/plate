//! The `spine_stack` engine: a corridor along a spine, with spaces stacked on both sides.
//!
//! The spine is the plate's spine (in spine mode) or the slot's long axis. Spaces that `along`
//! names fill bays beside the corridor: fixed counts first, then as many of the open-ended ones
//! as fit, at widths snapped to the grid so repeated bays come out identical (and become one
//! typical). Other spaces (a stair, a lift) take the ends. Each bay gets ports: `entrance` on
//! the corridor, `facade` on the outside, `party` between neighbours of the same type, and
//! `wet` on every other party wall, so bathrooms can go back to back.

use super::build::Foresight;
use super::common::{num, select, Clear, Named, SpaceType};
use super::frame::Transform;
use super::grid;
use super::ports::{collinear_overlap, door_ports, ports_along, window_ports, PortSeg, Source};
use super::{Ctx, Issue, LevelResult, Opening, Placed, Slot};
use crate::geometry::Point;
use crate::layout::ElementKind;
use crate::plan::{Plan, Rule, Sides, SpaceKind};

#[derive(Debug, Clone)]
struct Bay {
    type_index: usize,
    /// Segment index, position along it, side (+1 left, -1 right).
    seg: usize,
    u0: f64,
    side: i8,
    w: f64,
    d: f64,
    /// Position within its run (for alternating wet walls).
    run: usize,
    slot: usize,
}

struct Run {
    seg: usize,
    side: i8,
    start: f64,
    len: f64,
    /// Next free position from the start, and from the end.
    head: f64,
    tail: f64,
}

impl Run {
    fn free(&self) -> f64 {
        (self.len - self.head - self.tail).max(0.0)
    }
}

pub fn solve(plan: &Plan, slot: &Slot, ctx: &Ctx) -> LevelResult {
    let units = ctx.units;
    let mm = units.to_mm();
    let mut types = SpaceType::all(plan, units);
    let mut issues = Vec::new();
    // Bays are sized once, not searched: size each for the walls it will have. Between bays and to
    // the corridor they're known; the back is the plate's edge, at the top level.
    if types.iter().any(|t| t.clear.is_some()) {
        let walls = Foresight::new(plan, units);
        for t in types.iter_mut() {
            let Some(c) = t.clear else { continue };
            let me = (t.key.as_str(), t.key.as_str());
            let role = if t.space.plan.is_some() { "party" } else { "partition" };
            let side = walls.intrusion(&[role.to_owned()], me, Some(me));
            let front = walls.intrusion(&["entrance".to_owned()], me, Some(me));
            let back = if ctx.root && slot.boundary.is_some() {
                let depth = |p: &PortSeg| walls.intrusion(&p.roles, me, None);
                slot.ports.iter().map(depth).fold(0.0, f64::max)
            } else {
                0.0
            };
            let (width, depth) = (
                Clear::gross(c.own_width, c.width, 2.0 * side, 2.0 * side),
                Clear::gross(c.own_depth, c.depth, front + back, front + back),
            );
            t.width = width;
            t.depth = depth;
        }
    }
    // Fixtures stand inside rooms; a corridor stack has nowhere to put them.
    for t in types.iter().filter(|t| t.space.is_fixture()) {
        issues.push(Issue::new(
            format!("/spaces/{}/kind", t.key),
            format!("{} is a fixture: spine_stack can't place fixtures, use the grid engine", t.key),
        ));
    }
    types.retain(|t| !t.space.is_fixture());

    // Which space is the corridor, and what lines up along it.
    let rules = plan.rules();
    let along: Vec<(&crate::plan::Selector, &crate::plan::Selector, Sides)> = rules
        .iter()
        .filter_map(|(_, r)| match r {
            Rule::Along { a, b, sides } => Some((a, b, *sides)),
            _ => None,
        })
        .collect();
    let by_key = |key: &str| types.iter().position(|t| t.key == key);
    let corridor = along
        .first()
        .and_then(|(_, b, _)| b.items().first().and_then(|k| by_key(k)))
        .or_else(|| types.iter().position(|t| t.space.kind == SpaceKind::Circulation));
    let Some(corridor) = corridor else {
        issues.push(Issue::new(
            "/spaces",
            "spine_stack needs a corridor: add a space with kind = 'circulation (and `plate.along` rules)",
        ));
        let mut result = grid::solve(plan, slot, ctx);
        result.issues.extend(issues);
        return result;
    };
    let sides = along.first().map_or(Sides::Both, |(_, _, s)| *s);
    let lined: Vec<usize> = if along.is_empty() {
        (0..types.len()).filter(|&i| i != corridor && types[i].space.kind != SpaceKind::Vertical).collect()
    } else {
        let mut v: Vec<usize> = Vec::new();
        for (a, _, _) in &along {
            for (i, t) in types.iter().enumerate() {
                if i != corridor && a.matches(&t.key, &t.key) && !v.contains(&i) {
                    v.push(i);
                }
            }
        }
        v
    };
    let ends: Vec<usize> = (0..types.len()).filter(|i| *i != corridor && !lined.contains(i)).collect();

    // The spine, and how deep the bays can be.
    let wc = types[corridor].width.target(1800.0 / mm).max(1e-6);
    let (spine, room_for_depth) = match (&slot.spine, &slot.boundary) {
        (Some(s), _) if s.len() >= 2 => (s.clone(), f64::INFINITY),
        (_, Some(b)) => {
            let (min_x, max_x) = b.iter().fold((f64::MAX, f64::MIN), |(a, z), p| (a.min(p.x), z.max(p.x)));
            let (min_y, max_y) = b.iter().fold((f64::MAX, f64::MIN), |(a, z), p| (a.min(p.y), z.max(p.y)));
            let (w, h) = (max_x - min_x, max_y - min_y);
            let deep = if sides == Sides::Both { (h.min(w) - wc) / 2.0 } else { h.min(w) - wc };
            let spine = if w >= h {
                let y = if sides == Sides::Both {
                    (min_y + max_y) / 2.0
                } else if sides == Sides::Left {
                    min_y + wc / 2.0
                } else {
                    max_y - wc / 2.0
                };
                vec![Point::new(min_x, y), Point::new(max_x, y)]
            } else {
                let x = if sides == Sides::Both {
                    (min_x + max_x) / 2.0
                } else if sides == Sides::Left {
                    max_x - wc / 2.0
                } else {
                    min_x + wc / 2.0
                };
                vec![Point::new(x, min_y), Point::new(x, max_y)]
            };
            (spine, deep)
        }
        _ => {
            issues
                .push(Issue::new("/spaces", "spine_stack needs a spine of at least 2 points, or a boundary"));
            return LevelResult { issues, ..Default::default() };
        }
    };

    let depth_of = |t: &SpaceType| -> f64 {
        let want = t.depth.target(6000.0 / mm);
        want.min(room_for_depth).min(t.depth.max).max(0.0)
    };
    let width_of = |t: &SpaceType| -> (f64, f64) {
        let (lo, hi) = (t.width.min, t.width.max);
        match (lo > 0.0, hi.is_finite()) {
            (true, true) => (lo, hi),
            (true, false) => (lo, lo * 1.5),
            (false, true) => (hi * 0.7, hi),
            (false, false) => (3000.0 / mm, 4500.0 / mm),
        }
    };
    for &ti in lined.iter().chain(&ends) {
        let d = depth_of(&types[ti]);
        if d + 1e-9 < types[ti].depth.min {
            issues.push(Issue::new(
                format!("/spaces/{}/depth", types[ti].key),
                format!(
                    "{} needs a depth of at least {}, only {} fits",
                    types[ti].key,
                    num(types[ti].depth.min),
                    num(d)
                ),
            ));
        }
    }
    let dmax = lined.iter().chain(&ends).map(|&ti| depth_of(&types[ti])).fold(0.0, f64::max);

    // Runs: each side of each spine segment, trimmed at inside corners.
    let nseg = spine.len() - 1;
    let mut runs: Vec<Run> = Vec::new();
    for k in 0..nseg {
        let len = spine[k].distance(spine[k + 1]);
        let trim_start = if k > 0 { wc / 2.0 + dmax } else { 0.0 };
        let trim_end = if k + 1 < nseg { wc / 2.0 + dmax } else { 0.0 };
        let usable = len - trim_start - trim_end;
        if usable <= 0.0 {
            continue;
        }
        for side in [1i8, -1] {
            let allowed = match sides {
                Sides::Both => true,
                Sides::Left => side == 1,
                Sides::Right => side == -1,
            };
            if allowed {
                runs.push(Run { seg: k, side, start: trim_start, len: usable, head: 0.0, tail: 0.0 });
            }
        }
    }
    if runs.is_empty() {
        issues.push(Issue::new("/spaces", "the spine is too short for any bays"));
    }

    let mut bays: Vec<Bay> = Vec::new();
    let mut placed_count = vec![0u32; types.len()];
    let snap = |w: f64, lo: f64| {
        let s = (w / ctx.grid + 1e-9).floor() * ctx.grid;
        if s + 1e-9 >= lo {
            s
        } else {
            w
        }
    };

    // End items (stairs, lifts): alternate between the start of a run and the end of another.
    let mut end_items: Vec<usize> = Vec::new();
    for &ti in &ends {
        for _ in 0..types[ti].min.max(if types[ti].max.is_none() { 1 } else { 0 }) {
            end_items.push(ti);
        }
    }
    for (k, &ti) in end_items.iter().enumerate() {
        if runs.is_empty() {
            break;
        }
        let t = &types[ti];
        let (lo, hi) = width_of(t);
        let w = snap(t.width.target((lo + hi) / 2.0), lo);
        let at_end = k % 2 == 1;
        let r = if at_end { runs.len() - 1 - (k / 2) % runs.len() } else { (k / 2) % runs.len() };
        let run = &mut runs[r];
        if run.free() + 1e-9 < w {
            continue;
        }
        let u0 = if at_end { run.start + run.len - run.tail - w } else { run.start + run.head };
        if at_end {
            run.tail += w;
        } else {
            run.head += w;
        }
        bays.push(Bay {
            type_index: ti,
            seg: run.seg,
            u0,
            side: run.side,
            w,
            d: depth_of(t),
            run: r,
            slot: 0,
        });
        placed_count[ti] += 1;
    }

    // Fixed counts first, each into the run with the most room.
    for &ti in &lined {
        let t = &types[ti];
        let open = t.max.is_none_or(|m| m > t.min);
        if open {
            continue;
        }
        let (lo, hi) = width_of(t);
        let w = snap(t.width.target((lo + hi) / 2.0), lo);
        for _ in 0..t.min {
            let Some(r) = (0..runs.len()).max_by(|&a, &b| runs[a].free().total_cmp(&runs[b].free())) else {
                break;
            };
            let run = &mut runs[r];
            if run.free() + 1e-9 < w {
                break;
            }
            let u0 = run.start + run.head;
            run.head += w;
            bays.push(Bay {
                type_index: ti,
                seg: run.seg,
                u0,
                side: run.side,
                w,
                d: depth_of(t),
                run: r,
                slot: 0,
            });
            placed_count[ti] += 1;
        }
    }

    // Then the open-ended types fill what's left, at equal widths snapped to the grid.
    for &ti in &lined {
        let t = &types[ti];
        let open = t.max.is_none_or(|m| m > t.min);
        if !open {
            continue;
        }
        let (lo, hi) = width_of(t);
        for (r, run) in runs.iter_mut().enumerate() {
            let free = run.free();
            let mut n = (free / lo + 1e-9).floor() as u32;
            if let Some(max) = t.max {
                n = n.min(max.saturating_sub(placed_count[ti]));
            }
            if n == 0 {
                continue;
            }
            let mut w = (free / n as f64).min(hi);
            if w < hi {
                // Fewer, wider bays if the max allows; never narrower than the minimum.
                let fewer = (free / hi).ceil().max(1.0) as u32;
                if fewer < n && free / fewer as f64 >= lo {
                    n = fewer;
                    w = free / n as f64;
                }
            }
            let w = snap(w.min(hi), lo);
            // What snapping leaves goes to the last bay when it can take it (so the others stay
            // alike); otherwise the bays share it equally. Nothing is left over.
            let rem = (free - n as f64 * w).max(0.0);
            let (w, last) = if rem <= 1e-9 || w + rem <= hi + 1e-9 {
                (w, w + rem)
            } else {
                let even = free / n as f64;
                (even, even)
            };
            for k in 0..n {
                let w = if k + 1 == n { last } else { w };
                let u0 = run.start + run.head;
                run.head += w;
                bays.push(Bay {
                    type_index: ti,
                    seg: run.seg,
                    u0,
                    side: run.side,
                    w,
                    d: depth_of(t),
                    run: r,
                    slot: 0,
                });
                placed_count[ti] += 1;
            }
        }
    }
    for &ti in lined.iter().chain(&ends) {
        let t = &types[ti];
        if placed_count[ti] < t.min {
            issues.push(Issue::new(
                format!("/spaces/{}/count", t.key),
                format!("only {} of {} {} fit along the corridor", placed_count[ti], t.min, t.key),
            ));
        }
    }

    // Order along the spine: segment, then position, then left before right; that order is
    // also the numbering and each run's sequence (for wet walls).
    bays.sort_by(|a, b| {
        (a.seg, a.u0, -a.side).partial_cmp(&(b.seg, b.u0, -b.side)).unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut seq = vec![0usize; runs.len()];
    for b in &mut bays {
        b.slot = seq[b.run];
        seq[b.run] += 1;
    }

    // Geometry: each bay's frame maps [0, w] × [0, d] (y away from the corridor) to the level.
    let seg_frame = |k: usize| {
        let (a, b) = (spine[k], spine[k + 1]);
        Transform::translate(a.x, a.y).then_after(&Transform::rotate((b.y - a.y).atan2(b.x - a.x)))
    };
    let mut named: Vec<Named> = Vec::new();
    let mut counters = vec![0usize; types.len()];
    let mut frames = Vec::new();
    let mut polygons: Vec<Vec<Point>> = Vec::new();
    let mut keep: Vec<bool> = Vec::new();
    for b in &bays {
        let local = if b.side > 0 {
            Transform { a: 1.0, b: 0.0, c: 0.0, d: 1.0, tx: b.u0, ty: wc / 2.0 }
        } else {
            Transform { a: -1.0, b: 0.0, c: 0.0, d: -1.0, tx: b.u0 + b.w, ty: -wc / 2.0 }
        };
        let frame = seg_frame(b.seg).then_after(&local);
        let poly: Vec<Point> = [(0.0, 0.0), (b.w, 0.0), (b.w, b.d), (0.0, b.d)]
            .iter()
            .map(|&(x, y)| frame.apply(Point::new(x, y)))
            .collect();
        let inside = match &slot.boundary {
            Some(boundary) => poly.iter().all(|p| {
                let c = poly
                    .iter()
                    .fold(Point::new(0.0, 0.0), |s, q| Point::new(s.x + q.x / 4.0, s.y + q.y / 4.0));
                // Test points pulled slightly towards the centre, so edges on the boundary count.
                let q = Point::new(p.x + (c.x - p.x) * 0.01, p.y + (c.y - p.y) * 0.01);
                grid::point_in_polygon(q, boundary)
            }),
            None => true,
        };
        let t = &types[b.type_index];
        named.push(Named { type_index: b.type_index, name: t.instance_name(counters[b.type_index]) });
        counters[b.type_index] += 1;
        frames.push(frame);
        polygons.push(poly);
        keep.push(inside);
    }
    let dropped = keep.iter().filter(|k| !**k).count();
    if dropped > 0 {
        issues.push(Issue::new(
            "/spaces",
            format!("{dropped} bays along the spine don't fit inside and were left out"),
        ));
    }

    // The corridor: the spine offset to both sides, mitred at the bends.
    let corridor_poly = offset_band(&spine, wc / 2.0);
    let corridor_name = types[corridor].instance_name(0);
    let corridor_edges = edges(&corridor_poly);

    let tol = 1e-6 * (1.0 + wc);
    let slot_segments: Vec<(Point, Point)> = slot.ports.iter().map(|p| (p.a, p.b)).collect();
    let open_space = slot.boundary.is_none();
    let mut placed = Vec::new();
    let poly_edges: Vec<Vec<(Point, Point)>> = polygons.iter().map(|p| edges(p)).collect();
    let level_ports: Vec<Vec<PortSeg>> = (0..bays.len())
        .map(|i| {
            if !keep[i] {
                return Vec::new();
            }
            let mut sources: Vec<Source> = Vec::new();
            for (k, p) in slot.ports.iter().enumerate() {
                sources.push(Source {
                    segments: &slot_segments[k..k + 1],
                    roles: p.roles.clone(),
                    other: None,
                    occupies: false,
                });
            }
            sources.push(Source {
                segments: &corridor_edges,
                roles: vec!["entrance".into()],
                other: Some(corridor_name.clone()),
                occupies: true,
            });
            let mut owned: Vec<(usize, Vec<String>)> = Vec::new();
            for j in 0..bays.len() {
                if j == i || !keep[j] {
                    continue;
                }
                let (bi, bj) = (&bays[i], &bays[j]);
                let same = bi.type_index == bj.type_index && types[bi.type_index].space.plan.is_some();
                let mut roles = vec![if same { "party" } else { "partition" }.to_owned()];
                if bi.run == bj.run && bi.slot.abs_diff(bj.slot) == 1 && bi.slot.min(bj.slot) % 2 == 0 {
                    roles.push("wet".into());
                }
                owned.push((j, roles));
            }
            let mut out = Vec::new();
            {
                let mut all = sources;
                for (j, roles) in &owned {
                    all.push(Source {
                        segments: &poly_edges[*j],
                        roles: roles.clone(),
                        other: Some(named[*j].name.clone()),
                        occupies: true,
                    });
                }
                out.extend(ports_along(&edges(&polygons[i]), &all, open_space.then_some("facade"), tol));
            }
            out
        })
        .collect();

    // Doors and windows the rules ask for at this level.
    let mut openings = Vec::new();
    let all_named: Vec<Named> = named.clone();
    for (ptr, rule) in &rules {
        match rule {
            Rule::Door { a, b, port, width, .. } => {
                let width = width.map_or(900.0 / mm, |l| l.0.to(units));
                for i in select(a, &all_named, &types).into_iter().filter(|&i| keep[i]) {
                    let seg = match (b, port) {
                        (Some(b), _) => {
                            let to_corridor = b.matches(&corridor_name, &types[corridor].key);
                            let mut best: Option<((Point, Point), String)> = None;
                            if to_corridor {
                                best = longest_shared(&edges(&polygons[i]), &corridor_edges, tol)
                                    .map(|s| (s, corridor_name.clone()));
                            }
                            for j in select(b, &all_named, &types).into_iter().filter(|&j| j != i && keep[j])
                            {
                                if let Some(s) =
                                    longest_shared(&edges(&polygons[i]), &edges(&polygons[j]), tol)
                                {
                                    if best
                                        .as_ref()
                                        .is_none_or(|(o, _)| o.0.distance(o.1) < s.0.distance(s.1))
                                    {
                                        best = Some((s, named[j].name.clone()));
                                    }
                                }
                            }
                            best
                        }
                        (_, Some(role)) => {
                            longest_port(&level_ports[i], role).map(|s| (s, format!("'{role}")))
                        }
                        _ => None,
                    };
                    match seg.and_then(|(s, other)| centred(s, width, 100.0 / mm).map(|s| (s, other))) {
                        Some(((p, q), other)) => openings.push(Opening {
                            kind: ElementKind::Door,
                            a: p,
                            b: q,
                            between: vec![named[i].name.clone(), other],
                        }),
                        None if rule.weight().is_none() => issues.push(Issue::new(
                            ptr.clone(),
                            format!("no wall for a door from {}", named[i].name),
                        )),
                        None => {}
                    }
                }
            }
            Rule::Window { a, port, width, .. } => {
                let width = width.map_or(1200.0 / mm, |l| l.0.to(units));
                let role = port.clone().unwrap_or_else(|| "facade".into());
                for i in select(a, &all_named, &types).into_iter().filter(|&i| keep[i]) {
                    match longest_port(&level_ports[i], &role).and_then(|s| centred(s, width, 300.0 / mm)) {
                        Some((p, q)) => openings.push(Opening {
                            kind: ElementKind::Window,
                            a: p,
                            b: q,
                            between: vec![named[i].name.clone(), format!("'{role}")],
                        }),
                        None if rule.weight().is_none() => issues.push(Issue::new(
                            ptr.clone(),
                            format!("{} has no '{role} wall for a window", named[i].name),
                        )),
                        None => {}
                    }
                }
            }
            Rule::Touch { .. } | Rule::Apart { .. } | Rule::Near { .. } | Rule::Faces { .. } => {
                issues.push(Issue::new(
                    ptr.clone(),
                    format!(
                    "`{}` isn't used by the spine_stack engine; put it in the plan of the spaces it concerns",
                    rule.name()
                ),
                ))
            }
            _ => {}
        }
    }

    placed.push(Placed {
        name: corridor_name.clone(),
        type_key: types[corridor].key.clone(),
        polygon: corridor_poly.clone(),
        rect: (nseg == 1).then(|| {
            let len = spine[0].distance(spine[1]);
            (len, wc, seg_frame(0).then_after(&Transform::translate(0.0, -wc / 2.0)))
        }),
        ports: Vec::new(),
        fixture: None,
        holes: Vec::new(),
    });
    let doors: Vec<(Point, Point)> =
        openings.iter().filter(|o| o.kind == ElementKind::Door).map(|o| (o.a, o.b)).collect();
    let windows: Vec<(Point, Point)> =
        openings.iter().filter(|o| o.kind == ElementKind::Window).map(|o| (o.a, o.b)).collect();
    for (i, b) in bays.iter().enumerate() {
        if !keep[i] {
            continue;
        }
        let back = frames[i].inverse();
        let mut ports = level_ports[i].clone();
        ports.extend(door_ports(&edges(&polygons[i]), &doors, tol));
        ports.extend(window_ports(&edges(&polygons[i]), &windows, tol));
        placed.push(Placed {
            name: named[i].name.clone(),
            type_key: types[b.type_index].key.clone(),
            polygon: polygons[i].clone(),
            rect: Some((b.w, b.d, frames[i])),
            ports: ports.iter().map(|p| p.transformed(&back)).collect(),
            fixture: None,
            holes: Vec::new(),
        });
    }
    LevelResult { placed, openings, issues }
}

fn edges(poly: &[Point]) -> Vec<(Point, Point)> {
    (0..poly.len()).map(|i| (poly[i], poly[(i + 1) % poly.len()])).collect()
}

fn longest_shared(a: &[(Point, Point)], b: &[(Point, Point)], tol: f64) -> Option<(Point, Point)> {
    let mut best: Option<(Point, Point)> = None;
    for &(p, q) in a {
        for &(r, s) in b {
            if let Some((t0, t1)) = collinear_overlap(p, q, r, s, tol) {
                let seg = (lerp(p, q, t0), lerp(p, q, t1));
                if best.is_none_or(|o| o.0.distance(o.1) < seg.0.distance(seg.1)) {
                    best = Some(seg);
                }
            }
        }
    }
    best
}

fn longest_port(ports: &[PortSeg], role: &str) -> Option<(Point, Point)> {
    ports.iter().filter(|p| p.has(role)).max_by(|x, y| x.len().total_cmp(&y.len())).map(|p| (p.a, p.b))
}

fn lerp(p: Point, q: Point, t: f64) -> Point {
    Point::new(p.x + (q.x - p.x) * t, p.y + (q.y - p.y) * t)
}

fn centred(seg: (Point, Point), width: f64, clear: f64) -> Option<(Point, Point)> {
    let (p, q) = seg;
    let len = p.distance(q);
    let w = width.min(len - 2.0 * clear);
    if w < width * 0.5 {
        return None;
    }
    let (ux, uy) = ((q.x - p.x) / len, (q.y - p.y) / len);
    let mid = Point::new((p.x + q.x) / 2.0, (p.y + q.y) / 2.0);
    Some((
        Point::new(mid.x - ux * w / 2.0, mid.y - uy * w / 2.0),
        Point::new(mid.x + ux * w / 2.0, mid.y + uy * w / 2.0),
    ))
}

/// A band of half-width `h` around a polyline, mitred at the bends, as a polygon.
fn offset_band(spine: &[Point], h: f64) -> Vec<Point> {
    let n = spine.len();
    let normal = |k: usize| {
        let (a, b) = (spine[k], spine[k + 1]);
        let len = a.distance(b).max(1e-12);
        ((a.y - b.y) / len, (b.x - a.x) / len)
    };
    let offset = |sign: f64| -> Vec<Point> {
        (0..n)
            .map(|i| {
                let (nx, ny) = if i == 0 {
                    normal(0)
                } else if i == n - 1 {
                    normal(n - 2)
                } else {
                    let (a, b) = (normal(i - 1), normal(i));
                    let (mx, my) = (a.0 + b.0, a.1 + b.1);
                    let dot = (mx * a.0 + my * a.1).max(1e-6);
                    (mx / dot, my / dot)
                };
                Point::new(spine[i].x + sign * h * nx, spine[i].y + sign * h * ny)
            })
            .collect()
    };
    // Counter-clockwise: along the right side, then back along the left.
    let mut poly = offset(-1.0);
    poly.extend(offset(1.0).into_iter().rev());
    poly
}
