//! The fit pass: services place their parts in the solved plate.
//!
//! Runs once everything is solved and in the plate's frame. It never moves anything: it reads
//! the rooms (the spaces with no rooms inside them), their walls' faces, their doors, windows
//! and fixtures, and puts parts on them. Every service's parts share the faces: placements that
//! are anchored to something go first (`on`, `beside`, `one`), the ones that can flow around them
//! after (`every`, `over`). Then [`super::connect`] connects them.

use std::collections::BTreeMap;

use crate::diagnostic::{Code, Diagnostic};
use crate::geometry::{interior_point, point_segment_distance, signed_area, Point};
use crate::layout::{Element, ElementKind, PartInfo, SymbolInfo};
use crate::plan::{role_matches, tag_of, Selector, SpaceKind};
use crate::service::{Mount, OpeningKind, Part, Placement, Service, Side};
use crate::units::Units;

use super::{num, ServiceLevel};

/// A side of a room, or the part of it one wall makes.
#[derive(Debug, Clone)]
pub struct Face {
    pub a: Point,
    pub b: Point,
    /// The wall behind it (an element index), if walls are built.
    pub wall: Option<usize>,
    /// Is it a wall at all (not an edge open to the floor beside it)?
    pub walled: bool,
    pub roles: Vec<String>,
    pub cavity: f64,
    /// Into the room.
    pub inward: Point,
    /// Index of the loop it's on, and where it starts along that loop.
    pub at: (usize, f64),
}

impl Face {
    pub fn len(&self) -> f64 {
        self.a.distance(self.b)
    }

    pub fn dir(&self) -> Point {
        let l = self.len().max(1e-12);
        Point::new((self.b.x - self.a.x) / l, (self.b.y - self.a.y) / l)
    }

    pub fn point(&self, s: f64) -> Point {
        let u = self.dir();
        Point::new(self.a.x + u.x * s, self.a.y + u.y * s)
    }

    /// Where `p` is along it, and how far off its line.
    fn locate(&self, p: Point) -> (f64, f64) {
        let u = self.dir();
        let (dx, dy) = (p.x - self.a.x, p.y - self.a.y);
        (dx * u.x + dy * u.y, (dx * u.y - dy * u.x).abs())
    }
}

/// An opening on a face: where along it, which kind.
#[derive(Debug, Clone)]
pub struct OnFace {
    pub face: usize,
    pub s0: f64,
    pub s1: f64,
    pub door: bool,
    /// The other side (a space's path or a port role such as `'entrance`).
    pub to: Option<String>,
}

/// A fixture in a room: its element, and the span of the face its back is against.
#[derive(Debug, Clone)]
pub struct Fixture {
    pub element: usize,
    pub back: (Point, Point),
    pub against: Option<(usize, f64, f64)>,
}

/// A room: a space nothing but fixtures is placed in.
#[derive(Debug, Clone)]
pub struct Room {
    pub element: usize,
    pub path: String,
    pub name: String,
    pub key: String,
    pub kind: SpaceKind,
    pub tags: Vec<String>,
    pub loops: Vec<Vec<Point>>,
    pub faces: Vec<Face>,
    pub openings: Vec<OnFace>,
    pub fixtures: Vec<Fixture>,
    pub area: f64,
}

impl Room {
    fn selected_by(&self, sel: &Selector) -> bool {
        sel.matches_tagged(&self.name, &self.key, &self.tags, self.kind)
    }

    fn perimeter(&self, k: usize) -> f64 {
        let l = &self.loops[k];
        (0..l.len()).map(|i| l[i].distance(l[(i + 1) % l.len()])).sum()
    }
}

/// A placed part, before it becomes an element.
#[derive(Debug, Clone)]
pub struct Placed {
    pub service: String,
    pub part: String,
    pub room: String,
    /// Where it stands: the middle of its wall side, or of its footprint.
    pub at: Point,
    pub polygon: Vec<Point>,
    pub front: [Point; 2],
    /// The face it's on (room index, face index), for wall parts.
    pub face: Option<(usize, usize)>,
    pub source: String,
    pub how: String,
    pub mount: Mount,
    pub above: Option<f64>,
    pub group: Option<String>,
    /// The scope it's in (a path prefix), for connecting.
    pub scope: String,
}

/// The plate as the services see it.
pub struct Model {
    pub units: Units,
    pub rooms: Vec<Room>,
    /// Every space (rooms and the rest): element index, path, name, type, kind, tags.
    pub spaces: Vec<(usize, String, String, String, SpaceKind, Vec<String>)>,
    /// Built walls: element indices.
    pub walls: Vec<usize>,
}

/// A service in a scope: where it applies.
#[derive(Debug, Clone)]
pub struct Instance {
    pub service: String,
    pub def: Service,
    /// Paths of the spaces it covers start with this; `""` is everything.
    pub prefix: String,
    /// The scope space itself, if it has one.
    pub space: Option<String>,
    /// Deeper levels that declare services of their own: not covered.
    pub except: Vec<String>,
}

impl Instance {
    pub fn covers(&self, path: &str) -> bool {
        let inside = path.starts_with(&self.prefix) || self.space.as_deref() == Some(path);
        inside && !self.except.iter().any(|e| path.starts_with(e.as_str()))
    }
}

pub fn run(
    levels: &[ServiceLevel],
    units: Units,
    elements: &mut Vec<Element>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if levels.is_empty() {
        return;
    }
    let model = Model::new(elements, units);
    let instances = instances(levels, &model);
    let mut issues = Issues::default();
    let mut placed = place(&model, &instances, elements, &mut issues);
    super::connect::run(&model, &instances, &mut placed, elements, &mut issues);
    for (k, p) in placed.into_iter().enumerate() {
        elements.push(part_element(k, p, &instances, units));
    }
    issues.into_diagnostics(diagnostics);
}

/// Problems, merged across the rooms they happen in.
#[derive(Default)]
pub struct Issues {
    list: Vec<IssueRow>,
}

/// An issue: its pointer, message, the rooms it happens in, a hint, and whether it's unmet.
type IssueRow = (String, String, Vec<String>, Option<String>, bool);

impl Issues {
    pub fn add(&mut self, pointer: &str, message: String, place: &str, hint: Option<String>) {
        self.push(pointer, message, place, hint, true);
    }

    pub fn info(&mut self, pointer: &str, message: String) {
        self.push(pointer, message, "", None, false);
    }

    fn push(&mut self, pointer: &str, message: String, place: &str, hint: Option<String>, unmet: bool) {
        if let Some(e) = self.list.iter_mut().find(|e| e.0 == pointer && e.1 == message) {
            if !place.is_empty() && !e.2.iter().any(|p| p == place) {
                e.2.push(place.to_owned());
            }
            return;
        }
        let places = if place.is_empty() { Vec::new() } else { vec![place.to_owned()] };
        self.list.push((pointer.to_owned(), message, places, hint, unmet));
    }

    fn into_diagnostics(self, diagnostics: &mut Vec<Diagnostic>) {
        for (pointer, message, places, hint, unmet) in self.list {
            let message = match places.len() {
                0 => message,
                1 => format!("{}: {message}", places[0]),
                n => format!("{} and {} more like it: {message}", places[0], n - 1),
            };
            let d = if unmet {
                Diagnostic::warning(message).with_code(Code::Unmet)
            } else {
                Diagnostic::info(message)
            }
            .with_pointer(pointer);
            diagnostics.push(match hint {
                Some(h) => d.with_note(h),
                None => d,
            });
        }
    }
}

type SpaceRow = (usize, String, String, String, SpaceKind, Vec<String>);

impl Model {
    pub fn new(elements: &[Element], units: Units) -> Model {
        let tol = units.tolerance() * 100.0;
        let mut spaces: Vec<SpaceRow> = Vec::new();
        for (i, e) in elements.iter().enumerate() {
            if let (ElementKind::Space, Some(info)) = (e.kind, &e.space) {
                spaces.push((
                    i,
                    e.id.clone(),
                    info.name.clone(),
                    info.type_key.clone(),
                    info.kind,
                    info.tags.clone(),
                ));
            }
        }
        let walls: Vec<usize> = elements
            .iter()
            .enumerate()
            .filter(|(_, e)| e.kind == ElementKind::Wall)
            .map(|(i, _)| i)
            .collect();
        let any_walls = !walls.is_empty();
        let mut rooms = Vec::new();
        for (i, path, name, key, kind, tags) in &spaces {
            if *kind == SpaceKind::Fixture {
                continue;
            }
            let prefix = format!("{path}.");
            if spaces.iter().any(|s| s.1.starts_with(&prefix) && s.4 != SpaceKind::Fixture) {
                continue;
            }
            let e = &elements[*i];
            let mut outer = e.points.clone();
            if signed_area(&outer) < 0.0 {
                outer.reverse();
            }
            let mut loops = vec![outer];
            for h in &e.holes {
                let mut h = h.clone();
                if signed_area(&h) > 0.0 {
                    h.reverse();
                }
                loops.push(h);
            }
            let faces = faces_of(&loops, elements, &walls, any_walls, tol);
            let mut room = Room {
                element: *i,
                path: path.clone(),
                name: name.clone(),
                key: key.clone(),
                kind: *kind,
                tags: tags.clone(),
                loops,
                faces,
                openings: Vec::new(),
                fixtures: Vec::new(),
                area: e.area.unwrap_or(0.0),
            };
            room.openings = openings_of(&room, elements, tol);
            room.fixtures = fixtures_of(&room, elements, &spaces, tol);
            rooms.push(room);
        }
        Model { units, rooms, spaces, walls }
    }
}

/// A room's edges, split where the wall behind them changes.
fn faces_of(
    loops: &[Vec<Point>],
    elements: &[Element],
    walls: &[usize],
    any_walls: bool,
    tol: f64,
) -> Vec<Face> {
    let mut faces = Vec::new();
    for (k, l) in loops.iter().enumerate() {
        let mut along = 0.0;
        for j in 0..l.len() {
            let (a, b) = (l[j], l[(j + 1) % l.len()]);
            let len = a.distance(b);
            if len <= tol {
                continue;
            }
            let u = Point::new((b.x - a.x) / len, (b.y - a.y) / len);
            let inward = Point::new(-u.y, u.x);
            let mut cover: Vec<(f64, f64, usize)> = Vec::new();
            for &w in walls {
                let info = elements[w].wall.as_ref().expect("a wall element has its info");
                let [c0, c1] = info.line;
                let wl = c0.distance(c1);
                if wl <= tol {
                    continue;
                }
                let v = Point::new((c1.x - c0.x) / wl, (c1.y - c0.y) / wl);
                if (u.x * v.y - u.y * v.x).abs() > 1e-6 {
                    continue;
                }
                let off = |p: Point| (p.x - a.x) * inward.x + (p.y - a.y) * inward.y;
                // Behind the face (outside the room), within the wall's thickness.
                let d = -off(c0);
                if d < -tol || d > info.thickness + tol || (off(c1) + d).abs() > tol {
                    continue;
                }
                let t = |p: Point| (p.x - a.x) * u.x + (p.y - a.y) * u.y;
                let (t0, t1) = (t(c0).min(t(c1)).max(0.0), t(c0).max(t(c1)).min(len));
                if t1 - t0 > tol {
                    cover.push((t0, t1, w));
                }
            }
            let mut cuts = vec![0.0, len];
            for c in &cover {
                cuts.push(c.0);
                cuts.push(c.1);
            }
            cuts.sort_by(f64::total_cmp);
            cuts.dedup_by(|x, y| (*x - *y).abs() < tol);
            for pair in cuts.windows(2) {
                let (s0, s1) = (pair[0], pair[1]);
                if s1 - s0 <= tol {
                    continue;
                }
                let mid = (s0 + s1) / 2.0;
                // The nearest wall behind it, where two overlap.
                let wall = cover
                    .iter()
                    .filter(|c| c.0 <= mid && mid <= c.1)
                    .min_by(|x, y| {
                        let d = |w: usize| {
                            let [c0, _] = elements[w].wall.as_ref().unwrap().line;
                            -((c0.x - a.x) * inward.x + (c0.y - a.y) * inward.y)
                        };
                        d(x.2).total_cmp(&d(y.2))
                    })
                    .map(|c| c.2);
                let (roles, cavity) = wall.map_or((Vec::new(), 0.0), |w| {
                    let info = elements[w].wall.as_ref().unwrap();
                    (info.roles.clone(), info.cavity)
                });
                let at = |s: f64| Point::new(a.x + u.x * s, a.y + u.y * s);
                faces.push(Face {
                    a: at(s0),
                    b: at(s1),
                    wall,
                    walled: wall.is_some() || !any_walls,
                    roles,
                    cavity: if any_walls { cavity } else { f64::INFINITY },
                    inward,
                    at: (k, along + s0),
                });
            }
            along += len;
        }
    }
    faces
}

fn openings_of(room: &Room, elements: &[Element], tol: f64) -> Vec<OnFace> {
    let mut out = Vec::new();
    for e in elements.iter().filter(|e| matches!(e.kind, ElementKind::Door | ElementKind::Window)) {
        let Some(info) = &e.opening else { continue };
        if !info.between.iter().any(|n| n == &room.path) {
            continue;
        }
        let other = info.between.iter().find(|n| *n != &room.path).cloned();
        let depth = info.depth.unwrap_or(0.0);
        for (f, face) in room.faces.iter().enumerate() {
            let (s0, d0) = face.locate(e.points[0]);
            let (s1, d1) = face.locate(e.points[1]);
            if d0.max(d1) > depth + tol {
                continue;
            }
            let (s0, s1) = (s0.min(s1).max(0.0), s0.max(s1).min(face.len()));
            if s1 - s0 > tol {
                out.push(OnFace { face: f, s0, s1, door: e.kind == ElementKind::Door, to: other.clone() });
            }
        }
    }
    out
}

fn fixtures_of(room: &Room, elements: &[Element], spaces: &[SpaceRow], tol: f64) -> Vec<Fixture> {
    let prefix = format!("{}.", room.path);
    let mut out = Vec::new();
    for (i, path, ..) in spaces.iter().filter(|s| s.4 == SpaceKind::Fixture) {
        let Some(rest) = path.strip_prefix(&prefix) else { continue };
        if rest.contains('.') {
            continue;
        }
        let e = &elements[*i];
        let Some([fa, fb]) = e.space.as_ref().and_then(|s| s.front) else { continue };
        let len = fa.distance(fb);
        if len <= tol {
            continue;
        }
        let n = Point::new(-(fb.y - fa.y) / len, (fb.x - fa.x) / len);
        let depth = e.points.iter().map(|p| (p.x - fa.x) * n.x + (p.y - fa.y) * n.y).fold(0.0, f64::max);
        let back = (
            Point::new(fb.x + n.x * depth, fb.y + n.y * depth),
            Point::new(fa.x + n.x * depth, fa.y + n.y * depth),
        );
        let mut against = None;
        for (f, face) in room.faces.iter().enumerate() {
            let (s0, d0) = face.locate(back.0);
            let (s1, d1) = face.locate(back.1);
            if d0.max(d1) > tol * 10.0 {
                continue;
            }
            let (s0, s1) = (s0.min(s1).max(0.0), s0.max(s1).min(face.len()));
            if s1 - s0 > tol {
                against = Some((f, s0, s1));
                break;
            }
        }
        out.push(Fixture { element: *i, back, against });
    }
    out
}

/// Where each service applies: one instance per scope space (or one for the whole level).
fn instances(levels: &[ServiceLevel], model: &Model) -> Vec<Instance> {
    let mut out = Vec::new();
    for (li, level) in levels.iter().enumerate() {
        let except: Vec<String> = levels
            .iter()
            .enumerate()
            .filter(|(lj, l)| {
                *lj != li && l.prefix.len() > level.prefix.len() && l.prefix.starts_with(&level.prefix)
            })
            .map(|(_, l)| l.prefix.clone())
            .collect();
        for (name, def) in level.services.iter() {
            let base = Instance {
                service: name.clone(),
                def: def.clone(),
                prefix: level.prefix.clone(),
                space: None,
                except: except.clone(),
            };
            match &def.scope {
                None => out.push(base),
                Some(scope) => {
                    for (_, path, n, key, kind, tags) in &model.spaces {
                        if base.covers(path) && scope.matches_tagged(n, key, tags, *kind) {
                            out.push(Instance {
                                prefix: format!("{path}."),
                                space: Some(path.clone()),
                                ..base.clone()
                            });
                        }
                    }
                }
            }
        }
    }
    out
}

/// Free space on the faces: what's taken, by height.
#[derive(Default)]
struct Taken {
    /// Per (room, face): the stretches taken.
    spans: BTreeMap<(usize, usize), Vec<Span>>,
}

/// A stretch of a face taken: from, to, at what height.
type Span = (f64, f64, f64);

/// Parts this far apart in height can share a stretch of wall (a switch over a baseboard).
const BAND_MM: f64 = 300.0;

impl Taken {
    fn free(&self, face: (usize, usize), s0: f64, s1: f64, above: f64, band: f64) -> bool {
        self.spans.get(&face).is_none_or(|v| {
            v.iter().all(|&(a, b, h)| (h - above).abs() >= band || s1 <= a + 1e-9 || s0 >= b - 1e-9)
        })
    }

    fn take(&mut self, face: (usize, usize), s0: f64, s1: f64, above: f64) {
        self.spans.entry(face).or_default().push((s0, s1, above));
    }
}

struct Ctx<'a> {
    model: &'a Model,
    units: Units,
    /// A metre, in `units`.
    m: f64,
    taken: Taken,
    out: Vec<Placed>,
    issues: &'a mut Issues,
    /// The fixture a part is being put behind (`on`): it doesn't block it.
    behind: Option<(usize, usize)>,
}

fn place(model: &Model, instances: &[Instance], elements: &[Element], issues: &mut Issues) -> Vec<Placed> {
    let units = model.units;
    let mut ctx = Ctx {
        model,
        units,
        m: 1000.0 / units.to_mm(),
        taken: Taken::default(),
        out: Vec::new(),
        issues,
        behind: None,
    };
    let mut jobs: Vec<(u8, usize, usize)> = Vec::new();
    for (ii, inst) in instances.iter().enumerate() {
        for (pi, p) in inst.def.place.iter().enumerate() {
            jobs.push((p.order(), ii, pi));
        }
    }
    jobs.sort();
    for (_, ii, pi) in jobs {
        let inst = &instances[ii];
        let placement = &inst.def.place[pi];
        let pointer = format!("{}/place/{pi}", inst.def.source.clone().unwrap_or_default());
        let name = placement.part();
        let Some(part) = inst.def.parts.get(name) else { continue };
        let job = Job { inst, name, part, pointer: &pointer };
        let before = ctx.out.len();
        let rooms: Vec<usize> =
            (0..model.rooms.len()).filter(|&r| inst.covers(&model.rooms[r].path)).collect();
        match placement {
            Placement::Every { spaces, spacing, min_run, .. } => {
                let spacing = spacing.0.to(units).max(ctx.m * 0.3);
                let min_run = min_run.map_or(0.0, |v| v.0.to(units));
                for &r in &rooms {
                    if model.rooms[r].selected_by(spaces) {
                        every_wall(&mut ctx, &job, r, spacing, min_run);
                    }
                    for fi in 0..model.rooms[r].fixtures.len() {
                        if fixture_selected(&elements[model.rooms[r].fixtures[fi].element], spaces) {
                            every_fixture(&mut ctx, &job, r, fi, spacing);
                        }
                    }
                }
            }
            Placement::Beside { opening, spaces, side, offset, .. } => {
                let offset = offset.map_or(0.15 * ctx.m, |v| v.0.to(units));
                for &r in &rooms {
                    if spaces.as_ref().is_none_or(|s| model.rooms[r].selected_by(s)) {
                        beside(&mut ctx, &job, r, *opening, *side, offset);
                    }
                }
            }
            Placement::Over { spaces, per, spacing, .. } => {
                for &r in &rooms {
                    if model.rooms[r].selected_by(spaces) {
                        over(&mut ctx, &job, r, *per, spacing.map(|v| v.0.to(units)), elements);
                    }
                }
            }
            Placement::On { fixtures, .. } => {
                for &r in &rooms {
                    for fi in 0..model.rooms[r].fixtures.len() {
                        if fixture_selected(&elements[model.rooms[r].fixtures[fi].element], fixtures) {
                            on_fixture(&mut ctx, &job, r, fi);
                        }
                    }
                }
            }
            Placement::One { spaces, near, apart, wall, .. } => {
                for &r in &rooms {
                    if model.rooms[r].selected_by(spaces) {
                        let need = wall.map(|w| w.need(units));
                        one(&mut ctx, &job, r, near.as_ref(), apart.as_ref(), need, elements);
                    }
                }
            }
        }
        if ctx.out.len() == before && !rooms.is_empty() {
            // Say so when it names nothing here: a tag no space has is easy to miss.
            let (sel, fixtures_only) = match placement {
                Placement::Every { spaces, .. } => (Some(spaces), false),
                Placement::Over { spaces, .. } | Placement::One { spaces, .. } => (Some(spaces), false),
                Placement::On { fixtures, .. } => (Some(fixtures), true),
                Placement::Beside { .. } => (None, false),
            };
            if let Some(sel) = sel {
                let matched = rooms.iter().any(|&r| {
                    let room = &model.rooms[r];
                    (!fixtures_only && room.selected_by(sel))
                        || room.fixtures.iter().any(|f| fixture_selected(&elements[f.element], sel))
                });
                if !matched {
                    ctx.issues.info(
                        &pointer,
                        format!(
                            "{}: nothing here is {}, so no {}",
                            inst.service,
                            sel.items().join(", "),
                            part_label(name, part)
                        ),
                    );
                }
            }
        }
    }
    ctx.out
}

/// One placement at work.
struct Job<'a> {
    inst: &'a Instance,
    name: &'a str,
    part: &'a Part,
    pointer: &'a str,
}

impl Job<'_> {
    fn label(&self) -> String {
        part_label(self.name, self.part)
    }
}

fn part_label(name: &str, part: &Part) -> String {
    part.label.clone().unwrap_or_else(|| name.replace('_', " "))
}

fn fixture_selected(e: &Element, sel: &Selector) -> bool {
    let Some(info) = &e.space else { return false };
    sel.matches_tagged(&info.name, &info.type_key, &info.tags, info.kind)
}

impl Ctx<'_> {
    /// Put a wall part on face `f` of room `r`, centred `s` along it, if it fits there.
    #[allow(clippy::too_many_arguments)]
    fn on_wall(&mut self, job: &Job, how: &str, r: usize, f: usize, s: f64, width: Option<f64>) -> bool {
        let room = &self.model.rooms[r];
        let face = &room.faces[f];
        let (w, d) = job.part.size(self.units);
        let w = width.unwrap_or(w);
        let inside = job.part.mount == Mount::Cavity;
        // In the wall, a part only meets others in the wall.
        let above = if inside { -1000.0 * self.m } else { job.part.above(self.units) };
        let (s0, s1) = (s - w / 2.0, s + w / 2.0);
        if s0 < -1e-9 || s1 > face.len() + 1e-9 || !face.walled || (inside && face.cavity + 1e-9 < d) {
            return false;
        }
        let band = BAND_MM / self.units.to_mm();
        if !self.taken.free((r, f), s0, s1, above, band) {
            return false;
        }
        // Low parts can't go behind a fixture backed against the wall (but behind the one
        // they're for, they can).
        let behind = |k: usize| self.behind == Some((r, k));
        if !inside
            && above < 0.9 * self.m
            && room.fixtures.iter().enumerate().any(|(k, fx)| {
                !behind(k) && fx.against.is_some_and(|(ff, a, b)| ff == f && s1 > a + 1e-9 && s0 < b - 1e-9)
            })
        {
            return false;
        }
        // Nor in a doorway.
        if room.openings.iter().any(|o| o.face == f && o.door && s1 > o.s0 + 1e-9 && s0 < o.s1 - 1e-9) {
            return false;
        }
        self.taken.take((r, f), s0, s1, above);
        let (p0, p1) = (face.point(s0), face.point(s1));
        // Out into the room from the wall's face, or into the wall.
        let n = if inside { Point::new(-face.inward.x, -face.inward.y) } else { face.inward };
        let q = |p: Point| Point::new(p.x + n.x * d, p.y + n.y * d);
        let front = if inside { [p1, p0] } else { [q(p1), q(p0)] };
        self.out.push(Placed {
            service: job.inst.service.clone(),
            part: job.name.to_owned(),
            room: room.path.clone(),
            at: face.point(s),
            polygon: vec![p0, p1, q(p1), q(p0)],
            front,
            face: Some((r, f)),
            source: job.pointer.to_owned(),
            how: how.to_owned(),
            mount: job.part.mount,
            above: (!inside).then_some(above),
            group: None,
            scope: job.inst.prefix.clone(),
        });
        true
    }

    /// Put a part on the floor or ceiling, centred at `c`.
    fn in_room(&mut self, job: &Job, how: &str, r: usize, c: Point) {
        let (w, d) = job.part.size(self.units);
        let (hw, hd) = (w / 2.0, d / 2.0);
        let polygon = vec![
            Point::new(c.x - hw, c.y - hd),
            Point::new(c.x + hw, c.y - hd),
            Point::new(c.x + hw, c.y + hd),
            Point::new(c.x - hw, c.y + hd),
        ];
        self.out.push(Placed {
            service: job.inst.service.clone(),
            part: job.name.to_owned(),
            room: self.model.rooms[r].path.clone(),
            at: c,
            front: [polygon[0], polygon[1]],
            polygon,
            face: None,
            source: job.pointer.to_owned(),
            how: how.to_owned(),
            mount: job.part.mount,
            above: None,
            group: None,
            scope: job.inst.prefix.clone(),
        });
    }
}

/// The room's walls as runs: its perimeter, cut at doorways and where there's no wall. Each run
/// is a list of (face, from, to) pieces, in order.
fn runs(room: &Room) -> Vec<Vec<(usize, f64, f64)>> {
    const BREAK: usize = usize::MAX;
    let mut out = Vec::new();
    for k in 0..room.loops.len() {
        let total = room.perimeter(k);
        let mut pieces: Vec<(usize, f64, f64)> = Vec::new();
        for (f, face) in room.faces.iter().enumerate().filter(|(_, f)| f.at.0 == k) {
            if !face.walled {
                pieces.push((BREAK, 0.0, 0.0));
                continue;
            }
            let mut cuts: Vec<(f64, f64)> =
                room.openings.iter().filter(|o| o.face == f && o.door).map(|o| (o.s0, o.s1)).collect();
            cuts.sort_by(|a, b| a.0.total_cmp(&b.0));
            let mut at = 0.0;
            for (a, b) in cuts {
                if a > at {
                    pieces.push((f, at, a));
                }
                pieces.push((BREAK, 0.0, 0.0));
                at = f64::max(at, b);
            }
            if at < face.len() {
                pieces.push((f, at, face.len()));
            }
        }
        let mut loop_runs: Vec<Vec<(usize, f64, f64)>> = Vec::new();
        let mut current = Vec::new();
        for p in pieces {
            if p.0 == BREAK {
                if !current.is_empty() {
                    loop_runs.push(std::mem::take(&mut current));
                }
            } else {
                current.push(p);
            }
        }
        if !current.is_empty() {
            loop_runs.push(current);
        }
        // A run that reaches round from the end of the loop to its start is one run.
        if loop_runs.len() > 1 {
            let starts = loop_runs[0].first().is_some_and(|p| room.faces[p.0].at.1 + p.1 < 1e-9);
            let ends = loop_runs
                .last()
                .and_then(|r| r.last())
                .is_some_and(|p| (room.faces[p.0].at.1 + p.2 - total).abs() < 1e-9);
            if starts && ends {
                let first = loop_runs.remove(0);
                loop_runs.last_mut().unwrap().extend(first);
            }
        }
        out.extend(loop_runs);
    }
    out
}

fn every_wall(ctx: &mut Ctx, job: &Job, r: usize, spacing: f64, min_run: f64) {
    let how = format!("every {} along the walls", num(spacing));
    let room = &ctx.model.rooms[r];
    let (w, _) = job.part.size(ctx.units);
    let corner = 0.1 * ctx.m;
    let path = room.path.clone();
    let faces: Vec<f64> = room.faces.iter().map(Face::len).collect();
    for run in runs(room) {
        let len: f64 = run.iter().map(|p| p.2 - p.1).sum();
        if len + 1e-9 < min_run.max(w) {
            continue;
        }
        let n = ((len / spacing) - 1e-9).ceil().max(1.0) as usize;
        let step = len / n as f64;
        let locate = |x: f64| -> (usize, f64) {
            let mut at = 0.0;
            for &(f, a, b) in &run {
                if x <= at + (b - a) + 1e-9 {
                    return (f, a + (x - at).max(0.0));
                }
                at += b - a;
            }
            let last = run.last().unwrap();
            (last.0, last.2)
        };
        for k in 0..n {
            let target = (k as f64 + 0.5) * step;
            // Out from the target, as far as keeps every point of the run within reach.
            let mut done = false;
            let mut d = 0.0;
            while d <= step / 2.0 + 1e-9 && !done {
                for x in [target + d, target - d] {
                    if !(0.0..=len).contains(&x) {
                        continue;
                    }
                    let (f, s) = locate(x);
                    if s - w / 2.0 < corner || s + w / 2.0 > faces[f] - corner {
                        continue;
                    }
                    if ctx.on_wall(job, &how, r, f, s, None) {
                        done = true;
                        break;
                    }
                }
                d += 0.05 * ctx.m;
            }
            if !done {
                let label = job.label();
                ctx.issues.add(
                    job.pointer,
                    format!("no free wall for a {label} along a {} run (every {})", num(len), num(spacing)),
                    &path,
                    Some(format!(
                        "Something takes that wall (a door, a fixture, another part): space the {label}s further apart, or move what's in the way."
                    )),
                );
            }
        }
    }
}

fn every_fixture(ctx: &mut Ctx, job: &Job, r: usize, fi: usize, spacing: f64) {
    let how = format!("every {} along the back of a fixture", num(spacing));
    let room = &ctx.model.rooms[r];
    let path = room.path.clone();
    let (w, _) = job.part.size(ctx.units);
    let Some((f, a, b)) = room.fixtures[fi].against else {
        ctx.issues.add(
            job.pointer,
            format!("a fixture it's for has no wall behind it, so no {}", job.label()),
            &path,
            None,
        );
        return;
    };
    let len = b - a;
    let n = ((len / spacing) - 1e-9).ceil().max(1.0) as usize;
    let step = len / n as f64;
    for k in 0..n {
        let target = a + (k as f64 + 0.5) * step;
        let mut done = false;
        let mut d = 0.0;
        while d <= step / 2.0 + 1e-9 && !done {
            for s in [target + d, target - d] {
                if s - w / 2.0 >= a - 1e-9 && s + w / 2.0 <= b + 1e-9 && ctx.on_wall(job, &how, r, f, s, None)
                {
                    done = true;
                    break;
                }
            }
            d += 0.05 * ctx.m;
        }
        if !done {
            ctx.issues.add(
                job.pointer,
                format!("no room for a {} behind a fixture", job.label()),
                &path,
                None,
            );
        }
    }
}

fn beside(ctx: &mut Ctx, job: &Job, r: usize, kind: OpeningKind, side: Option<Side>, offset: f64) {
    let room = &ctx.model.rooms[r];
    let path = room.path.clone();
    let door = kind == OpeningKind::Door;
    let side = side.unwrap_or(if door { Side::Latch } else { Side::Under });
    let (w, _) = job.part.size(ctx.units);
    let openings: Vec<(OnFace, f64)> = room
        .openings
        .iter()
        .filter(|o| o.door == door)
        .map(|o| (o.clone(), room.faces[o.face].len()))
        .collect();
    let what = if door { "door" } else { "window" };
    let how = match side {
        Side::Under => format!("under each {what}"),
        Side::Latch => format!("by the latch side of each {what}"),
        Side::Hinge => format!("by the hinge side of each {what}"),
    };
    for (o, len) in openings {
        let placed = match side {
            Side::Under => {
                let width = job.part.width.is_none().then_some(o.s1 - o.s0);
                ctx.on_wall(job, &how, r, o.face, (o.s0 + o.s1) / 2.0, width)
            }
            _ => {
                // A door's hinges are at the end nearer the corner it was put by.
                let hinge_first = o.s0 <= len - o.s1;
                let after = hinge_first == (side == Side::Latch);
                let (s_after, s_before) = (o.s1 + offset + w / 2.0, o.s0 - offset - w / 2.0);
                let (first, second) = if after { (s_after, s_before) } else { (s_before, s_after) };
                ctx.on_wall(job, &how, r, o.face, first, None)
                    || ctx.on_wall(job, &how, r, o.face, second, None)
            }
        };
        if !placed {
            ctx.issues.add(
                job.pointer,
                format!("no room for a {} beside a {what}", job.label()),
                &path,
                Some("The wall beside it is too short or taken: move the opening off the corner, or give the part less offset.".into()),
            );
        }
    }
}

fn over(ctx: &mut Ctx, job: &Job, r: usize, per: Option<f64>, spacing: Option<f64>, elements: &[Element]) {
    let room = &ctx.model.rooms[r];
    let path = room.path.clone();
    let outer = room.loops[0].clone();
    let holes: Vec<Vec<Point>> = room.loops[1..].to_vec();
    let b = crate::geometry::Bounds::from_points(&outer);
    let (bw, bh) = (b.width(), b.height());
    let m2 = ctx.m * ctx.m;
    let (nx, ny) = match (per, spacing) {
        (_, Some(sp)) if sp > 0.0 => {
            (((bw / sp) - 1e-9).ceil().max(1.0) as usize, ((bh / sp) - 1e-9).ceil().max(1.0) as usize)
        }
        (Some(a), _) if a > 0.0 => {
            let count = ((room.area / (a * m2)) - 1e-9).ceil().max(1.0);
            let nx = (count * bw / bh.max(1e-9)).sqrt().round().max(1.0);
            let ny = (count / nx).ceil().max(1.0);
            (nx as usize, ny as usize)
        }
        _ => (1, 1),
    };
    let how = match (per, spacing) {
        (_, Some(sp)) => format!("{} apart over the room", num(sp)),
        (Some(a), _) => format!("one per {} m² of floor", num(a)),
        _ => "in the middle of the room".to_owned(),
    };
    let floor = job.part.mount == Mount::Floor;
    let fixtures: Vec<Vec<Point>> =
        room.fixtures.iter().map(|f| elements[f.element].points.clone()).collect();
    let inside = |p: Point| {
        crate::geometry::contains(&outer, p) && !holes.iter().any(|h| crate::geometry::contains(h, p))
    };
    let blocked = |p: Point| floor && fixtures.iter().any(|f| crate::geometry::contains(f, p));
    let mut put = 0;
    for j in 0..ny {
        for i in 0..nx {
            let c = Point::new(
                b.min_x + bw * (i as f64 + 0.5) / nx as f64,
                b.min_y + bh * (j as f64 + 0.5) / ny as f64,
            );
            if inside(c) && !blocked(c) {
                ctx.in_room(job, &how, r, c);
                put += 1;
            }
        }
    }
    if put == 0 {
        let c = interior_point(&outer, &holes);
        if blocked(c) {
            ctx.issues.add(job.pointer, format!("no free floor for a {}", job.label()), &path, None);
        } else {
            ctx.in_room(job, &how, r, c);
        }
    }
}

fn on_fixture(ctx: &mut Ctx, job: &Job, r: usize, fi: usize) {
    let room = &ctx.model.rooms[r];
    let path = room.path.clone();
    let fx = room.fixtures[fi].clone();
    let how = "at the back of a fixture".to_owned();
    match (job.part.mount, fx.against) {
        (Mount::Wall | Mount::Cavity, Some((f, a, b))) => {
            let mid = (a + b) / 2.0;
            let mut d = 0.0;
            ctx.behind = Some((r, fi));
            while d <= (b - a) / 2.0 + 1e-9 {
                for s in [mid + d, mid - d] {
                    if ctx.on_wall(job, &how, r, f, s, None) {
                        ctx.behind = None;
                        return;
                    }
                }
                d += 0.05 * ctx.m;
            }
            ctx.behind = None;
            ctx.issues.add(
                job.pointer,
                format!("no room for a {} behind its fixture", job.label()),
                &path,
                None,
            );
        }
        _ => {
            let c = Point::new((fx.back.0.x + fx.back.1.x) / 2.0, (fx.back.0.y + fx.back.1.y) / 2.0);
            ctx.in_room(job, &how, r, c);
        }
    }
}

/// How far `p` is from what a selector item names, in a room: a port role (the faces with it),
/// `door`, `window`, `entrance` (its doors to circulation, or to the entrance), or a tag.
fn distance_to(room: &Room, item: &str, p: Point, elements: &[Element]) -> Option<f64> {
    let faces = |pred: &dyn Fn(&Face) -> bool| -> Option<f64> {
        room.faces
            .iter()
            .filter(|f| pred(f))
            .map(|f| point_segment_distance(p, f.a, f.b))
            .min_by(f64::total_cmp)
    };
    let openings = |door: bool, entrance: bool| -> Option<f64> {
        room.openings
            .iter()
            .filter(|o| o.door == door)
            .filter(|o| {
                !entrance
                    || o.to.as_deref().is_some_and(|t| {
                        t.starts_with("'entrance")
                            || elements.iter().any(|e| {
                                e.id == t
                                    && e.space.as_ref().is_some_and(|s| {
                                        matches!(s.kind, SpaceKind::Circulation | SpaceKind::Zone)
                                    })
                            })
                    })
            })
            .map(|o| {
                let f = &room.faces[o.face];
                point_segment_distance(p, f.point(o.s0), f.point(o.s1))
            })
            .min_by(f64::total_cmp)
    };
    if let Some(tag) = tag_of(item) {
        return room
            .fixtures
            .iter()
            .filter(|f| {
                elements[f.element].space.as_ref().is_some_and(|s| crate::plan::has_tag(&s.tags, s.kind, tag))
            })
            .map(|f| {
                let pts = &elements[f.element].points;
                (0..pts.len())
                    .map(|k| point_segment_distance(p, pts[k], pts[(k + 1) % pts.len()]))
                    .fold(f64::INFINITY, f64::min)
            })
            .min_by(f64::total_cmp);
    }
    match item {
        "door" => openings(true, false),
        "window" => openings(false, false),
        "entrance" => {
            openings(true, true).or_else(|| faces(&|f| f.roles.iter().any(|r| role_matches(r, "entrance"))))
        }
        role => faces(&|f| f.roles.iter().any(|r| role_matches(r, role))),
    }
}

fn one(
    ctx: &mut Ctx,
    job: &Job,
    r: usize,
    near: Option<&Selector>,
    apart: Option<&Selector>,
    need: Option<crate::assembly::Need>,
    elements: &[Element],
) {
    let room = &ctx.model.rooms[r];
    let path = room.path.clone();
    let mut how = "the best place in the room".to_owned();
    if let Some(n) = near {
        how.push_str(&format!(", near {}", n.items().join(", ")));
    }
    if let Some(a) = apart {
        how.push_str(&format!(", away from {}", a.items().join(", ")));
    }
    if !matches!(job.part.mount, Mount::Wall | Mount::Cavity) {
        let c = interior_point(&room.loops[0], &room.loops[1..]);
        ctx.in_room(job, &how, r, c);
        return;
    }
    let (w, depth) = job.part.size(ctx.units);
    let step = 0.1 * ctx.m;
    let keep_off = 0.6 * ctx.m;
    let inside = job.part.mount == Mount::Cavity;
    let above = if inside { -1000.0 * ctx.m } else { job.part.above(ctx.units) };
    let band = BAND_MM / ctx.units.to_mm();
    // A part in the cavity needs one as deep as it is, whatever else is asked of the wall.
    let deep = |f: &Face| !inside || f.cavity + 1e-9 >= depth;
    let wall_ok = |f: &Face| -> bool {
        let Some(need) = need else { return true };
        match f.wall {
            Some(wi) => {
                let info = elements[wi].wall.as_ref().unwrap();
                info.thickness + 1e-9 >= need.thickness
                    && info.cavity + 1e-9 >= need.cavity
                    && info.fire.unwrap_or(0.0) + 1e-9 >= need.fire
                    && info.acoustic.unwrap_or(0.0) + 1e-9 >= need.acoustic
                    && (info.bearing || !need.bearing)
            }
            None => f.walled && need.thickness <= 0.0 && need.cavity <= 0.0,
        }
    };
    let special = |i: &str| tag_of(i).is_some() || matches!(i, "door" | "window" | "entrance");
    let mut found: Vec<(f64, usize, f64)> = Vec::new();
    for (f, face) in room.faces.iter().enumerate() {
        if !face.walled || !wall_ok(face) || !deep(face) {
            continue;
        }
        let off_limits = apart.is_some_and(|a| {
            a.items().iter().any(|i| !special(i) && face.roles.iter().any(|r| role_matches(r, i)))
        });
        if off_limits {
            continue;
        }
        let len = face.len();
        let mut s = w / 2.0 + step;
        while s + w / 2.0 + step <= len + 1e-9 {
            let p = face.point(s);
            let in_opening = room
                .openings
                .iter()
                .any(|o| o.face == f && s + w / 2.0 > o.s0 - step && s - w / 2.0 < o.s1 + step);
            let too_close = apart.is_some_and(|a| {
                a.items()
                    .iter()
                    .any(|i| special(i) && distance_to(room, i, p, elements).is_some_and(|d| d < keep_off))
            });
            if !in_opening && !too_close && ctx.taken.free((r, f), s - w / 2.0, s + w / 2.0, above, band) {
                let score: f64 = near.map_or(0.0, |n| {
                    n.items().iter().map(|i| distance_to(room, i, p, elements).unwrap_or(1e6)).sum()
                });
                found.push((score, f, s));
            }
            s += step;
        }
    }
    // The best place that takes it (the wall may still turn it away: a fixture backed against
    // it, another part there).
    found.sort_by(|a, b| a.0.total_cmp(&b.0));
    let placed = found.into_iter().any(|(_, f, s)| ctx.on_wall(job, &how, r, f, s, None));
    if !placed {
        let mut need = need.unwrap_or_default();
        if inside {
            need.cavity = need.cavity.max(depth);
        }
        let what = if need == crate::assembly::Need::default() {
            String::new()
        } else {
            format!(" that is {}", need.describe())
        };
        ctx.issues.add(
            job.pointer,
            format!("no wall{what} free for a {}", job.label()),
            &path,
            Some("Loosen what it's kept away from, or claim a wall for it in the service's claims (`plate.wall … & { length = … }`).".into()),
        );
    }
}

fn part_element(k: usize, p: Placed, instances: &[Instance], units: Units) -> Element {
    let part = instances.iter().find(|i| i.service == p.service).and_then(|i| i.def.parts.get(&p.part));
    let label = part.map_or_else(|| p.part.clone(), |d| part_label(&p.part, d));
    let mut e = Element::new(
        format!("part:{}.{}:{}#{k}", p.service, p.part, p.room),
        ElementKind::Part,
        p.polygon.clone(),
        true,
    );
    e.source = Some(p.source.clone());
    if let Some(symbol) = part.and_then(|d| d.symbol.as_deref()) {
        let [a, b] = p.front;
        let depth = part.map_or(0.0, |d| d.size(units).1);
        if let Some(paths) = crate::symbols::draw(symbol, a, b, depth) {
            e.symbol = Some(SymbolInfo { name: symbol.to_owned(), paths });
        }
    }
    e.part = Some(PartInfo {
        service: p.service,
        part: p.part,
        label,
        mount: p.mount.name(),
        above: p.above,
        space: p.room,
        placed: p.how,
        group: p.group,
        at: p.at,
    });
    e
}
