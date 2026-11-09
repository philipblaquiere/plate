//! The build pass: once a level is placed, its walls are built.
//!
//! Walls come from ports. Every placed space's ports already say which sibling is on the other
//! side, so each shared edge becomes one wall, with the roles both sides see. At the root, the
//! boundary's edges are walls too. Each wall is built as the thinnest assembly that meets every
//! requirement on it: those of the assemblies its roles match (`on`), and those of `wall` rules.
//!
//! An assembly takes floor. The plans inside a space are solved in what its walls leave: its
//! *clear* slot, inset on each side by the widest wall there.

use crate::assembly::{Assembly, Need, WallLine};
use crate::geometry::Point;
use crate::plan::{LengthSer, Plan, Rule, Selector};
use crate::solve::frame::{Rect, Transform};
use crate::solve::grid::point_in_polygon;
use crate::solve::ports::{collinear_overlap, is_gap, OPEN};
use crate::solve::{num, Issue, LevelResult, Slot};
use crate::units::Units;

/// A wall a level built, in the level's frame.
#[derive(Debug, Clone)]
pub struct BuiltWall {
    /// Index of the placed space on one side.
    pub owner: usize,
    /// The placed space on the other side, or none for the plate's boundary.
    pub other: Option<usize>,
    /// The centreline (for a boundary wall, the boundary's edge).
    pub a: Point,
    pub b: Point,
    pub roles: Vec<String>,
    pub name: String,
    pub assembly: Assembly,
    pub thickness: f64,
    /// The way into the plate, for a wall on its boundary that grows inward.
    pub inward: Option<Point>,
    /// How far it reaches past each end, to meet the walls there.
    pub extend: [f64; 2],
    /// Why it's built so, in words.
    pub because: String,
}

impl BuiltWall {
    /// How far it takes floor from a space beside it.
    pub fn intrusion(&self) -> f64 {
        if self.inward.is_some() {
            self.thickness
        } else {
            self.thickness / 2.0
        }
    }

    /// Its true centreline: for a wall that grows inward from the boundary, half its thickness in.
    pub fn centreline(&self) -> [Point; 2] {
        match self.inward {
            Some(n) => {
                let h = self.thickness / 2.0;
                [
                    Point::new(self.a.x + n.x * h, self.a.y + n.y * h),
                    Point::new(self.b.x + n.x * h, self.b.y + n.y * h),
                ]
            }
            None => [self.a, self.b],
        }
    }

    pub fn polygon(&self) -> Vec<Point> {
        let len = self.a.distance(self.b);
        if len <= 0.0 {
            return Vec::new();
        }
        let u = Point::new((self.b.x - self.a.x) / len, (self.b.y - self.a.y) / len);
        let a = Point::new(self.a.x - u.x * self.extend[0], self.a.y - u.y * self.extend[0]);
        let b = Point::new(self.b.x + u.x * self.extend[1], self.b.y + u.y * self.extend[1]);
        let at = |p: Point, n: Point, d: f64| Point::new(p.x + n.x * d, p.y + n.y * d);
        match self.inward {
            Some(n) => vec![a, b, at(b, n, self.thickness), at(a, n, self.thickness)],
            None => {
                let n = Point::new(-u.y, u.x);
                let h = self.thickness / 2.0;
                vec![at(a, n, -h), at(b, n, -h), at(b, n, h), at(a, n, h)]
            }
        }
    }
}

/// A clear shape: its outline and its holes.
pub type Shape = (Vec<Point>, Vec<Vec<Point>>);

/// What a level built: its walls, the insets of each placed rectangle (bottom, right, top, left,
/// in its own frame), and the clear shape of a fill.
#[derive(Debug, Clone, Default)]
pub struct Built {
    pub walls: Vec<BuiltWall>,
    pub insets: Vec<[f64; 4]>,
    pub fill: Vec<Option<Shape>>,
    pub issues: Vec<Issue>,
}

struct Edge {
    owner: usize,
    other: Option<usize>,
    a: Point,
    b: Point,
    roles: Vec<String>,
}

/// Build the walls of a placed level. `root` builds the plate's boundary too.
pub fn build(plan: &Plan, slot: &Slot, level: &LevelResult, units: Units, root: bool) -> Built {
    let n = level.placed.len();
    let mut out = Built { insets: vec![[0.0; 4]; n], fill: vec![None; n], ..Default::default() };
    let wall_rules = wall_rules(plan);
    let tol = units.tolerance() * 10.0;
    let solid = |i: usize| level.placed[i].fixture.is_none();
    let index = |name: &str| level.placed.iter().position(|p| p.name == name).filter(|&j| solid(j));

    // Every edge a space shares, in the level's frame.
    let mut edges: Vec<Edge> = Vec::new();
    for (i, p) in level.placed.iter().enumerate() {
        if !solid(i) {
            continue;
        }
        let t = p.rect.as_ref().map_or(Transform::IDENTITY, |r| r.2);
        for port in &p.ports {
            let (a, b) = (t.apply(port.a), t.apply(port.b));
            match &port.other {
                Some(o) => {
                    if let Some(j) = index(o) {
                        edges.push(Edge { owner: i, other: Some(j), a, b, roles: port.roles.clone() });
                    }
                }
                None if root && slot.boundary.is_some() => {
                    let opening = is_gap(&port.roles);
                    if !opening {
                        edges.push(Edge { owner: i, other: None, a, b, roles: port.roles.clone() });
                    }
                }
                None => {}
            }
        }
    }

    // One wall per shared edge, with the roles both sides see.
    let mut walls: Vec<BuiltWall> = Vec::new();
    for e in &edges {
        let mut roles = e.roles.clone();
        if let Some(j) = e.other {
            let reverse: Vec<&Edge> = edges
                .iter()
                .filter(|f| f.owner == j && f.other == Some(e.owner))
                .filter(|f| collinear_overlap(e.a, e.b, f.a, f.b, tol).is_some())
                .collect();
            if j < e.owner && !reverse.is_empty() {
                continue; // built from the other side
            }
            for f in reverse {
                for r in &f.roles {
                    if !roles.contains(r) {
                        roles.push(r.clone());
                    }
                }
            }
        }
        if roles.iter().any(|r| r == OPEN) || e.a.distance(e.b) <= tol {
            continue;
        }
        let inward = match e.other {
            Some(_) => None,
            None => {
                let len = e.a.distance(e.b);
                let nrm = Point::new(-(e.b.y - e.a.y) / len, (e.b.x - e.a.x) / len);
                let mid = Point::new(
                    (e.a.x + e.b.x) / 2.0 + nrm.x * tol * 10.0,
                    (e.a.y + e.b.y) / 2.0 + nrm.y * tol * 10.0,
                );
                Some(if point_in_polygon(mid, &level.placed[e.owner].polygon) {
                    nrm
                } else {
                    Point::new(-nrm.x, -nrm.y)
                })
            }
        };
        walls.push(BuiltWall {
            owner: e.owner,
            other: e.other,
            a: e.a,
            b: e.b,
            roles,
            name: String::new(),
            assembly: Assembly::default(),
            thickness: 0.0,
            inward,
            extend: [0.0; 2],
            because: String::new(),
        });
    }

    // What each wall must be.
    let name_of = |i: usize| (level.placed[i].name.as_str(), level.placed[i].type_key.as_str());
    let applies = |w: &BuiltWall, a: &Selector, b: &Option<Selector>, port: &Option<String>| -> bool {
        let them = w.other.map(name_of);
        rule_applies(a, b, port, &w.roles, name_of(w.owner), them)
            || w.other.is_some_and(|o| rule_applies(a, b, port, &w.roles, name_of(o), Some(name_of(w.owner))))
    };
    if plan.assemblies.is_empty() {
        const HINT: &str = "Add `assemblies` to the plate (or this plan), or import a set of them.";
        for (ptr, rule) in &wall_rules {
            let Rule::Wall { a, b, port, assembly, .. } = rule else { continue };
            if let Some(name) = assembly {
                let message = format!("no assembly named \"{name}\": this plan has none");
                out.issues.push(Issue::new(format!("{ptr}/assembly"), message).with_hint(HINT));
            } else if rule_need(rule, units) != Need::default()
                && walls.iter().any(|w| applies(w, a, b, port))
            {
                let message =
                    format!("no assembly is {}: this plan has none", rule_need(rule, units).describe());
                out.issues.push(Issue::new(ptr.clone(), message).with_hint(HINT));
            }
        }
        return out;
    }
    let mut keep = vec![true; walls.len()];
    let mut needs: Vec<Asked> = walls
        .iter()
        .map(|w| asked(plan, &wall_rules, &w.roles, |a, b, p| applies(w, a, b, p), units))
        .collect();

    // Rules asking for one stretch: the wall of the space with the longest run clear of doors.
    for (ptr, rule) in &wall_rules {
        let Rule::Wall { a, b, port, assembly, length: Some(len), .. } = rule else { continue };
        let len = len.0.to(units);
        let preferred = rule.weight().is_some();
        for i in (0..n).filter(|&i| solid(i)) {
            let (name, key) = name_of(i);
            if !a.matches(name, key) {
                continue;
            }
            let mine = |w: &BuiltWall| {
                let only = Selector::One(name.to_owned());
                applies(w, &only, b, port) && (w.owner == i || w.other == Some(i))
            };
            let best = walls
                .iter()
                .enumerate()
                .filter(|(_, w)| mine(w))
                .map(|(k, w)| (k, clear_run(w, level, tol)))
                .max_by(|x, y| x.1.total_cmp(&y.1));
            match best {
                Some((k, run)) if run + tol >= len => {
                    needs[k].3 = needs[k].3.and(rule_need(rule, units));
                    match assembly {
                        Some(n) => needs[k].2 = Some((n.clone(), ptr.clone())),
                        None => {
                            needs[k].0 = needs[k].0.and(rule_need(rule, units));
                            needs[k].1.push(format!("the rule at {ptr}"));
                        }
                    }
                }
                best => {
                    let had = best.map_or(0.0, |b| b.1);
                    let message = format!(
                        "{name} has no {} of wall clear of doors to build as it asks (the longest is {})",
                        num(len),
                        num(had)
                    );
                    let issue = if preferred {
                        Issue::preferred(ptr.clone(), message)
                    } else {
                        Issue::new(ptr.clone(), message)
                    };
                    out.issues.push(issue.with_hint(format!(
                        "Lower its `length`, or let {name} grow (its size, or fewer doors on one wall)."
                    )));
                }
            }
        }
    }

    // The thinnest assembly that meets it all.
    for (k, w) in walls.iter_mut().enumerate() {
        let (need, why, explicit, ruled) = &needs[k];
        if let Some((name, ptr)) = explicit {
            match plan.assemblies.get(name) {
                Some(a) => {
                    if !a.meets(ruled, units) {
                        let (on, off) = (name_of(w.owner).0, w.other.map_or("the outside", |o| name_of(o).0));
                        out.issues.push(
                            Issue::new(
                                format!("{ptr}/assembly"),
                                format!(
                                    "\"{name}\" isn't {}, as rules ask of the wall between {on} and {off}",
                                    ruled.describe()
                                ),
                            )
                            .with_hint("Name an assembly that is, or ask for less."),
                        );
                    }
                    w.name = name.clone();
                    w.assembly = a.clone();
                    w.because = format!("the rule at {ptr}");
                }
                None => {
                    let names: Vec<&str> = plan.assemblies.keys().map(String::as_str).collect();
                    out.issues.push(Issue::new(
                        format!("{ptr}/assembly"),
                        format!("no assembly named \"{name}\" (assemblies here: {})", names.join(", ")),
                    ));
                    keep[k] = false;
                }
            }
            continue;
        }
        if why.is_empty() {
            keep[k] = false; // nothing says what to build: an edge, as before
            continue;
        }
        let best = thinnest(plan, &w.roles, need, units);
        match best {
            Some((name, a)) => {
                w.name = name.clone();
                w.assembly = a.clone();
                w.because = why.join(" + ");
            }
            None => {
                // Nothing is good enough: the most demanding of those asked for, and say so.
                let (name, a) = most_demanding(plan, &w.roles, units);
                let (on, off) = (name_of(w.owner).0, w.other.map_or("the outside", |o| name_of(o).0));
                let pointer = a.source.as_ref().map_or_else(|| "/assemblies".to_owned(), |s| format!("@{s}"));
                out.issues.push(
                    Issue::new(
                        pointer,
                        format!(
                            "no assembly is {} (the wall between {on} and {off}: {})",
                            need.describe(),
                            why.join(" + ")
                        ),
                    )
                    .with_hint(format!(
                        "Add one that is, e.g. `{} = {{ on = [{}], … }}`, or ask for less.",
                        w.roles.join("_"),
                        w.roles.iter().map(|r| format!("'{r}")).collect::<Vec<_>>().join(", ")
                    )),
                );
                w.name = name.clone();
                w.assembly = a.clone();
                w.because = why.join(" + ");
            }
        }
    }
    let mut k = 0;
    walls.retain(|_| {
        k += 1;
        keep[k - 1]
    });
    for w in walls.iter_mut() {
        w.thickness = w.assembly.thickness(units);
        if w.inward.is_some() && w.assembly.line == Some(WallLine::Centre) {
            w.inward = None;
        }
    }

    // Reach past the ends to meet the walls there.
    let ends: Vec<[f64; 2]> = (0..walls.len())
        .map(|k| {
            let w = &walls[k];
            let mut ext = [0.0; 2];
            for (e, p) in [w.a, w.b].into_iter().enumerate() {
                let mut reach: f64 = 0.0;
                let mut on_boundary = false;
                for (m, v) in walls.iter().enumerate() {
                    if m == k || crate::geometry::point_segment_distance(p, v.a, v.b) > tol {
                        continue;
                    }
                    if collinear_overlap(w.a, w.b, v.a, v.b, tol).is_some() || parallel(w, v) {
                        continue;
                    }
                    if w.inward.is_none() {
                        if v.inward.is_some() {
                            on_boundary = true;
                        } else {
                            reach = reach.max(v.thickness / 2.0);
                        }
                    } else if let (Some(n), Some(boundary)) = (w.inward, &slot.boundary) {
                        // A boundary wall reaches round a corner that turns into the plate.
                        if v.inward.is_some() {
                            let len = w.a.distance(w.b);
                            let dir = if e == 0 { -1.0 } else { 1.0 };
                            let u = Point::new((w.b.x - w.a.x) / len * dir, (w.b.y - w.a.y) / len * dir);
                            let test = Point::new(
                                p.x + u.x * tol * 20.0 + n.x * w.thickness / 2.0,
                                p.y + u.y * tol * 20.0 + n.y * w.thickness / 2.0,
                            );
                            if point_in_polygon(test, boundary) {
                                reach = reach.max(v.thickness);
                            }
                        }
                    }
                }
                ext[e] = if on_boundary { 0.0 } else { reach };
            }
            ext
        })
        .collect();
    for (w, e) in walls.iter_mut().zip(ends) {
        w.extend = e;
    }

    // What the walls leave each space.
    for (i, p) in level.placed.iter().enumerate() {
        if !solid(i) {
            continue;
        }
        let mine: Vec<&BuiltWall> = walls.iter().filter(|w| w.owner == i || w.other == Some(i)).collect();
        let depth_on = |a: Point, b: Point| -> f64 {
            mine.iter()
                .filter(|w| {
                    collinear_overlap(a, b, w.a, w.b, tol)
                        .is_some_and(|(t0, t1)| (t1 - t0) * a.distance(b) > tol)
                })
                .map(|w| w.intrusion())
                .fold(0.0, f64::max)
        };
        match &p.rect {
            Some((w, h, t)) if p.is_rect() => {
                let c = Rect::new(0.0, 0.0, *w, *h).corners();
                for side in 0..4 {
                    out.insets[i][side] = depth_on(t.apply(c[side]), t.apply(c[(side + 1) % 4]));
                }
            }
            _ => {
                let inset = |l: &[Point]| -> Vec<Point> {
                    let d: Vec<f64> = (0..l.len()).map(|k| depth_on(l[k], l[(k + 1) % l.len()])).collect();
                    inset_polygon(l, &d)
                };
                out.fill[i] = Some((inset(&p.polygon), p.holes.iter().map(|h| inset(h)).collect()));
            }
        }
    }
    out.walls = walls;
    out
}

fn parallel(w: &BuiltWall, v: &BuiltWall) -> bool {
    let (ux, uy) = (w.b.x - w.a.x, w.b.y - w.a.y);
    let (vx, vy) = (v.b.x - v.a.x, v.b.y - v.a.y);
    let cross = ux * vy - uy * vx;
    cross.abs() <= 1e-9 * (ux.hypot(uy) * vx.hypot(vy)).max(1e-12)
}

/// The longest stretch of a wall no door is in.
fn clear_run(w: &BuiltWall, level: &LevelResult, tol: f64) -> f64 {
    let len = w.a.distance(w.b);
    let mut cuts: Vec<(f64, f64)> = level
        .openings
        .iter()
        .filter(|o| o.kind == crate::layout::ElementKind::Door)
        .filter_map(|o| collinear_overlap(w.a, w.b, o.a, o.b, tol))
        .collect();
    cuts.sort_by(|a, b| a.0.total_cmp(&b.0));
    let (mut at, mut best) = (0.0, 0.0f64);
    for (a, b) in cuts {
        best = best.max(a - at);
        at = f64::max(at, b);
    }
    (best.max(1.0 - at)) * len
}

/// A polygon moved in by `d[k]` along each edge `k` (from point `k` to `k + 1`), toward its left:
/// the inside of a counter-clockwise outline, the floor around a clockwise hole. Where two edges
/// in line are moved by different amounts, the outline steps between them.
pub fn inset_polygon(l: &[Point], d: &[f64]) -> Vec<Point> {
    let n = l.len();
    if n < 3 || d.iter().all(|v| *v == 0.0) {
        return l.to_vec();
    }
    let normal = |k: usize| {
        let (a, b) = (l[k], l[(k + 1) % n]);
        let len = a.distance(b).max(1e-12);
        Point::new(-(b.y - a.y) / len, (b.x - a.x) / len)
    };
    let mut out = Vec::with_capacity(n);
    for k in 0..n {
        let prev = (k + n - 1) % n;
        let (np, nk) = (normal(prev), normal(k));
        let v = l[k];
        let pp = Point::new(v.x + np.x * d[prev], v.y + np.y * d[prev]);
        let pk = Point::new(v.x + nk.x * d[k], v.y + nk.y * d[k]);
        // Directions of the two edges.
        let up = Point::new(np.y, -np.x);
        let uk = Point::new(nk.y, -nk.x);
        let cross = up.x * uk.y - up.y * uk.x;
        if cross.abs() < 1e-9 {
            out.push(pp);
            if pp.distance(pk) > 1e-12 {
                out.push(pk);
            }
        } else {
            // pp + s·up = pk + t·uk
            let s = ((pk.x - pp.x) * uk.y - (pk.y - pp.y) * uk.x) / cross;
            out.push(Point::new(pp.x + up.x * s, pp.y + up.y * s));
        }
    }
    out
}

/// The `wall` rules of a plan, with their pointers.
fn wall_rules(plan: &Plan) -> Vec<(String, &Rule)> {
    plan.rules().into_iter().filter(|(_, r)| matches!(r, Rule::Wall { .. })).collect()
}

/// What a `wall` rule asks of a wall.
fn rule_need(rule: &Rule, units: Units) -> Need {
    match rule {
        Rule::Wall { thickness, cavity, fire, acoustic, bearing, .. } => Need {
            thickness: thickness.map_or(0.0, |v: LengthSer| v.0.to(units)),
            cavity: cavity.map_or(0.0, |v: LengthSer| v.0.to(units)),
            fire: fire.unwrap_or(0.0),
            acoustic: acoustic.unwrap_or(0.0),
            bearing: bearing.unwrap_or(false),
        },
        _ => Need::default(),
    }
}

/// Does a `wall` rule's `a`, `b` and `port` take in this wall, seen from `me` (name, type)?
fn rule_applies(
    a: &Selector,
    b: &Option<Selector>,
    port: &Option<String>,
    roles: &[String],
    me: (&str, &str),
    them: Option<(&str, &str)>,
) -> bool {
    if !a.matches(me.0, me.1) {
        return false;
    }
    match (b, port, them) {
        (Some(b), _, Some(t)) => b.matches(t.0, t.1),
        (_, Some(p), _) => {
            p == crate::plan::ANY_WALL || roles.iter().any(|r| crate::plan::role_matches(r, p))
        }
        _ => false,
    }
}

/// What a wall must be, why, the assembly a rule named (with the rule's pointer), and what rules
/// alone ask (naming an assembly stands in for those its roles match, not for them).
type Asked = (Need, Vec<String>, Option<(String, String)>, Need);

/// What the assemblies its roles match and the `wall` rules that apply ask of a wall.
fn asked(
    plan: &Plan,
    wall_rules: &[(String, &Rule)],
    roles: &[String],
    applies: impl Fn(&Selector, &Option<Selector>, &Option<String>) -> bool,
    units: Units,
) -> Asked {
    let mut need = Need::default();
    let mut why = Vec::new();
    let mut explicit = None;
    let mut ruled = Need::default();
    for (name, a) in &plan.assemblies {
        if a.matches(roles) {
            need = need.and(a.requirement(units));
            why.push(name.clone());
        }
    }
    for (ptr, rule) in wall_rules {
        let Rule::Wall { a, b, port, assembly, length: None, .. } = rule else { continue };
        if !applies(a, b, port) {
            continue;
        }
        ruled = ruled.and(rule_need(rule, units));
        match assembly {
            Some(name) => explicit = Some((name.clone(), ptr.clone())),
            None => {
                need = need.and(rule_need(rule, units));
                why.push(format!("the rule at {ptr}"));
            }
        }
    }
    (need, why, explicit, ruled)
}

/// The thinnest assembly that meets `need`, for a wall with these roles.
fn thinnest<'a>(
    plan: &'a Plan,
    roles: &[String],
    need: &Need,
    units: Units,
) -> Option<(&'a String, &'a Assembly)> {
    let candidates = plan.assemblies.iter().filter(|(_, a)| a.on.is_none() || a.matches(roles));
    candidates.filter(|(_, a)| a.meets(need, units)).min_by(|(na, a), (nb, b)| {
        a.thickness(units)
            .total_cmp(&b.thickness(units))
            .then_with(|| b.matches(roles).cmp(&a.matches(roles)))
            .then_with(|| na.cmp(nb))
    })
}

/// When nothing is good enough: the most demanding of those asked for.
fn most_demanding<'a>(plan: &'a Plan, roles: &[String], units: Units) -> (&'a String, &'a Assembly) {
    plan.assemblies
        .iter()
        .filter(|(_, a)| a.matches(roles))
        .max_by(|x, y| x.1.thickness(units).total_cmp(&y.1.thickness(units)))
        .or_else(|| plan.assemblies.iter().next())
        .expect("assemblies is not empty")
}

/// The thickest wall a plan's assemblies can build.
pub fn thickest(plan: &Plan, units: Units) -> f64 {
    plan.assemblies.values().map(|a| a.thickness(units)).fold(0.0, f64::max)
}

/// What the engines know of a level's walls before they are built: how far the wall on an edge
/// will take floor from the space beside it.
pub struct Foresight<'a> {
    plan: &'a Plan,
    wall_rules: Vec<(String, &'a Rule)>,
    units: Units,
}

impl<'a> Foresight<'a> {
    pub fn new(plan: &'a Plan, units: Units) -> Foresight<'a> {
        Foresight { plan, wall_rules: wall_rules(plan), units }
    }

    /// The intrusion of the wall between `me` and `them` (name, type), or between `me` and the
    /// plate's boundary when there's no `them`. No assembly asked for, no wall: 0. A rule asking
    /// for one stretch (`length`) depends on where the doors end up, so it isn't foreseen.
    pub fn intrusion(&self, roles: &[String], me: (&str, &str), them: Option<(&str, &str)>) -> f64 {
        if self.plan.assemblies.is_empty() || roles.iter().any(|r| r == OPEN) {
            return 0.0;
        }
        let applies = |a: &Selector, b: &Option<Selector>, p: &Option<String>| {
            rule_applies(a, b, p, roles, me, them)
                || them.is_some_and(|t| rule_applies(a, b, p, roles, t, Some(me)))
        };
        let (need, why, explicit, _) = asked(self.plan, &self.wall_rules, roles, applies, self.units);
        let assembly = match explicit {
            Some((name, _)) => self.plan.assemblies.get(&name),
            None if why.is_empty() => None,
            None => Some(
                thinnest(self.plan, roles, &need, self.units)
                    .unwrap_or_else(|| most_demanding(self.plan, roles, self.units))
                    .1,
            ),
        };
        assembly.map_or(0.0, |a| {
            let t = a.thickness(self.units);
            if them.is_none() && a.line != Some(WallLine::Centre) {
                t
            } else {
                t / 2.0
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insetting_a_rectangle_and_a_step() {
        let sq = vec![Point::new(0.0, 0.0), Point::new(4.0, 0.0), Point::new(4.0, 3.0), Point::new(0.0, 3.0)];
        let r = inset_polygon(&sq, &[0.1, 0.2, 0.1, 0.0]);
        assert_eq!(r.len(), 4);
        assert!((r[0].x - 0.0).abs() < 1e-9 && (r[0].y - 0.1).abs() < 1e-9);
        assert!((r[2].x - 3.8).abs() < 1e-9 && (r[2].y - 2.9).abs() < 1e-9);
        // Two edges in line, moved by different amounts: a step.
        let line = vec![
            Point::new(0.0, 0.0),
            Point::new(2.0, 0.0),
            Point::new(4.0, 0.0),
            Point::new(4.0, 3.0),
            Point::new(0.0, 3.0),
        ];
        let r = inset_polygon(&line, &[0.1, 0.2, 0.0, 0.0, 0.0]);
        assert_eq!(r.len(), 6);
        assert!((r[1].y - 0.1).abs() < 1e-9 && (r[2].y - 0.2).abs() < 1e-9);
    }
}
