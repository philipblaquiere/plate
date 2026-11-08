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
use crate::plan::{role_matches, tag_of, Count, Selector, SpaceKind};
use crate::service::{Mount, OpeningKind, Part, Per, Placement, Service, Side};
use crate::units::Units;

use super::spread::{self, Arrange, Ask};
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
    /// The network that group is in.
    pub network: Option<String>,
    /// The scope it's in (a path prefix), for connecting.
    pub scope: String,
    /// The fixture it's on (an element index), for `on` placements.
    pub fixture: Option<usize>,
    /// The service whose circuit powers it, when that's another service.
    pub fed_by: Option<String>,
    /// What it draws: its own load, else its fixture's (set when connecting).
    pub load: Option<crate::service::Load>,
    /// Its part's label.
    pub label: String,
    /// The spaces it's for (paths), when that isn't only `room`: a switch in a box for one
    /// space of a region, a thermostat for a whole region.
    pub serves: Vec<String>,
}

impl Placed {
    /// The spaces it's for.
    pub fn serving(&self) -> Vec<&str> {
        if self.serves.is_empty() {
            vec![self.room.as_str()]
        } else {
            self.serves.iter().map(String::as_str).collect()
        }
    }
}

/// The plate as the services see it.
pub struct Model {
    pub units: Units,
    pub rooms: Vec<Room>,
    /// Every space (rooms and the rest): element index, path, name, type, kind, tags.
    pub spaces: Vec<(usize, String, String, String, SpaceKind, Vec<String>)>,
    /// Built walls: element indices.
    pub walls: Vec<usize>,
    /// Floor to ceiling.
    pub height: f64,
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

/// Places and connects every service's parts; returns the circuits, for a panel schedule.
pub fn run(
    levels: &[ServiceLevel],
    units: Units,
    height: f64,
    elements: &mut Vec<Element>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Vec<crate::layout::Circuit> {
    if levels.is_empty() {
        return Vec::new();
    }
    let mut model = Model::new(elements, units);
    model.height = height;
    let instances = instances(levels, &model);
    let mut issues = Issues::default();
    let mut placed = place(&model, &instances, elements, &mut issues);
    let circuits = super::connect::run(&model, &instances, &mut placed, elements, &mut issues);
    for (k, p) in placed.into_iter().enumerate() {
        elements.push(part_element(k, p, &instances, units));
    }
    issues.into_diagnostics(diagnostics);
    circuits
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
        Model { units, rooms, spaces, walls, height: 2400.0 / units.to_mm() }
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
                // Not scoped, or inherited by a plan that is itself in scope (a flat's plan
                // under `scope = "#dwelling"`, or that plan opened on its own in the flat's
                // slot): the whole level is one instance.
                None => out.push(base),
                Some(_) if def.in_scope => out.push(base),
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

/// The height of a part in the wall's cavity (a stack): it's in the way at every height.
const IN_WALL: f64 = f64::NEG_INFINITY;

impl Taken {
    /// Is this stretch free at this height? A part in the wall (`IN_WALL`) takes the wall's
    /// depth, so it meets every part on that stretch, at any height, and they meet it.
    fn free(&self, face: (usize, usize), s0: f64, s1: f64, above: f64, band: f64) -> bool {
        self.spans.get(&face).is_none_or(|v| {
            v.iter().all(|&(a, b, h)| {
                let apart = h != IN_WALL && above != IN_WALL && (h - above).abs() >= band;
                apart || s1 <= a + 1e-9 || s0 >= b - 1e-9
            })
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
            Placement::Over { spaces, count, per, spacing, layout, margin, .. } => {
                let (exactly, min, max) = match count {
                    Some(Count::Exactly(n)) => (Some(*n), None, None),
                    Some(Count::Range { min, max }) => (None, *min, *max),
                    None => (None, None, None),
                };
                let ask = Ask {
                    exactly,
                    min,
                    max,
                    spacing: spacing.map(|v| v.0.to(units).max(ctx.m * 0.3)),
                    per: per.map(|a| a * ctx.m * ctx.m),
                    arrange: layout.map_or(Arrange::Grid, Into::into),
                    margin: margin.map_or(0.3 * ctx.m, |v| v.0.to(units).max(0.0)),
                };
                for &r in &rooms {
                    if model.rooms[r].selected_by(spaces) {
                        over(&mut ctx, &job, r, None, &ask, elements);
                    }
                    // Over fixtures: a pendant over an island, a light over a table.
                    if job.part.mount != Mount::Floor {
                        for fi in 0..model.rooms[r].fixtures.len() {
                            if fixture_selected(&elements[model.rooms[r].fixtures[fi].element], spaces) {
                                over(&mut ctx, &job, r, Some(fi), &ask, elements);
                            }
                        }
                    }
                }
            }
            Placement::On { fixtures, .. } => {
                for &r in &rooms {
                    for fi in 0..model.rooms[r].fixtures.len() {
                        let fe = model.rooms[r].fixtures[fi].element;
                        if fixture_selected(&elements[fe], fixtures) {
                            let from = ctx.out.len();
                            on_fixture(&mut ctx, &job, r, fi);
                            for p in &mut ctx.out[from..] {
                                p.fixture = Some(fe);
                            }
                        }
                    }
                }
            }
            Placement::One { spaces, near, apart, wall, per, each, .. } => {
                let need = wall.map(|w| w.need(units));
                let chosen: Vec<usize> =
                    rooms.iter().copied().filter(|&r| model.rooms[r].selected_by(spaces)).collect();
                let groups = match per {
                    Per::Space => chosen.iter().map(|&r| vec![r]).collect(),
                    Per::Region => by_region(model, &chosen, elements),
                };
                for group in groups {
                    let ask = OneAsk { near: near.as_ref(), apart: apart.as_ref(), need, each: *each };
                    one(&mut ctx, &job, &group, &ask, elements);
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
        // In the wall, a part takes the cavity: no box can go in front of it, on either side.
        let above = if inside { IN_WALL } else { job.part.above(self.units) };
        let (s0, s1) = (s - w / 2.0, s + w / 2.0);
        if s0 < -1e-9 || s1 > face.len() + 1e-9 || !face.walled || (inside && face.cavity + 1e-9 < d) {
            return false;
        }
        let band = BAND_MM / self.units.to_mm();
        if !self.taken.free((r, f), s0, s1, above, band) {
            return false;
        }
        // In the wall, the faces on its other side too.
        let twins = if inside { self.twins(r, f, s0, s1) } else { Vec::new() };
        if twins.iter().any(|&(g, a, b)| !self.taken.free(g, a, b, above, band)) {
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
        for (g, a, b) in twins {
            self.taken.take(g, a, b, above);
        }
        let room = &self.model.rooms[r];
        let face = &room.faces[f];
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
            network: None,
            scope: job.inst.prefix.clone(),
            fixture: None,
            fed_by: None,
            load: job.part.load,
            label: job.label(),
            serves: Vec::new(),
        });
        true
    }

    /// The stretch `s0..s1` of room `r`'s face `f`, seen from the other faces of the same wall
    /// (the room on its other side): (room, face), from, to.
    fn twins(&self, r: usize, f: usize, s0: f64, s1: f64) -> Vec<((usize, usize), f64, f64)> {
        let face = &self.model.rooms[r].faces[f];
        let Some(wall) = face.wall else { return Vec::new() };
        let (p0, p1) = (face.point(s0), face.point(s1));
        let tol = self.units.tolerance() * 100.0;
        // Further than this from the face, it's another wall (no wall is half a metre thick).
        let reach = 0.5 * self.m;
        let mut out = Vec::new();
        for (r2, room) in self.model.rooms.iter().enumerate() {
            for (f2, g) in room.faces.iter().enumerate() {
                if (r2, f2) == (r, f) || g.wall != Some(wall) {
                    continue;
                }
                let ((a, da), (b, db)) = (g.locate(p0), g.locate(p1));
                let (a, b) = (a.min(b).max(0.0), a.max(b).min(g.len()));
                if da.max(db) <= reach && b - a > tol {
                    out.push(((r2, f2), a, b));
                }
            }
        }
        out
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
            network: None,
            scope: job.inst.prefix.clone(),
            fixture: None,
            fed_by: None,
            load: job.part.load,
            label: job.label(),
            serves: Vec::new(),
        });
    }
}

/// The room's walls as runs: its perimeter, cut at doorways, behind fixtures standing against
/// it (a counter, a wardrobe: they take that wall, as electrical codes say) and where there's no
/// wall. Each run is a list of (face, from, to) pieces, in order.
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
            cuts.extend(
                room.fixtures.iter().filter_map(|x| x.against).filter(|a| a.0 == f).map(|a| (a.1, a.2)),
            );
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
                    || round_the_corner(ctx, job, r, o.face, after, offset, w)
                    || round_the_corner(ctx, job, r, o.face, !after, offset, w)
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

/// A part beside a door with no room on its own wall (a closet's door fills its wall): on the
/// wall round the corner, next to the corner. `after` picks the corner at the face's end.
fn round_the_corner(ctx: &mut Ctx, job: &Job, r: usize, f: usize, after: bool, offset: f64, w: f64) -> bool {
    let room = &ctx.model.rooms[r];
    let face = &room.faces[f];
    let tol = ctx.units.tolerance() * 100.0;
    let corner = if after { face.b } else { face.a };
    let next = room.faces.iter().enumerate().find(|(g, n)| {
        *g != f && n.at.0 == face.at.0 && (if after { n.a } else { n.b }).distance(corner) < tol
    });
    let Some((g, n)) = next else { return false };
    let s = if after { offset + w / 2.0 } else { n.len() - offset - w / 2.0 };
    ctx.on_wall(job, "beside a door, round the corner", r, g, s, None)
}

/// Spread a part over a room's floor or ceiling, or over one of its fixtures (`fixture`).
fn over(ctx: &mut Ctx, job: &Job, r: usize, fixture: Option<usize>, ask: &Ask, elements: &[Element]) {
    let room = &ctx.model.rooms[r];
    let path = room.path.clone();
    let (outer, holes, what) = match fixture {
        Some(fi) => {
            let e = &elements[room.fixtures[fi].element];
            let name = e.space.as_ref().map_or_else(|| "fixture".to_owned(), |s| s.label.clone());
            (e.points.clone(), Vec::new(), format!("the {name}"))
        }
        None => (room.loops[0].clone(), room.loops[1..].to_vec(), "the room".to_owned()),
    };
    // Rooms are cut where they turn; a fixture is one piece.
    let min_side = if fixture.is_some() { 0.0 } else { 0.6 * ctx.m };
    let zones = crate::geometry::zones(&outer, &holes, min_side);
    let mut points =
        if zones.is_empty() { vec![interior_point(&outer, &holes)] } else { spread::spread(&zones, ask) };
    // Parts can't overlap: ask for fewer until they don't.
    let (w, d) = job.part.size(ctx.units);
    let size = w.max(d);
    let asked = points.len();
    if asked > 1 && spread::closest(&points) < size - 1e-9 {
        let mut n = asked - 1;
        loop {
            let fewer = Ask { exactly: Some(n as u32), min: None, max: None, ..*ask };
            points = spread::spread(&zones, &fewer);
            if n <= 1 || spread::closest(&points) >= size - 1e-9 {
                break;
            }
            n -= 1;
        }
        ctx.issues.add(
            job.pointer,
            format!("only {} {} fit over {what}, not {asked}", points.len(), plural(&job.label())),
            &path,
            Some(format!(
                "Each is {} across. Ask for fewer, a wider spacing, or a smaller margin.",
                num(size)
            )),
        );
    }
    let parts = zones.len() > 1;
    let mut how = match (ask.arrange, ask.exactly, ask.spacing, ask.per) {
        (Arrange::Centre, ..) if parts => format!("in the middle of each part of {what}"),
        (Arrange::Centre, ..) => format!("in the middle of {what}"),
        (_, Some(_), ..) | (_, None, None, None) if points.len() > 1 => {
            format!("one of {}, spread over {what}", points.len())
        }
        (_, None, Some(sp), _) => format!("at most {} apart over {what}", num(sp)),
        (_, None, None, Some(a)) => format!("one per {} m² over {what}", num(a / (ctx.m * ctx.m))),
        _ if parts => format!("in the middle of the biggest part of {what}"),
        _ => format!("in the middle of {what}"),
    };
    if ask.arrange == Arrange::Line && points.len() > 1 {
        how.push_str(", in a line");
    }
    let floor = job.part.mount == Mount::Floor;
    let fixtures: Vec<Vec<Point>> =
        room.fixtures.iter().map(|f| elements[f.element].points.clone()).collect();
    let blocked = |p: Point| floor && fixtures.iter().any(|f| crate::geometry::contains(f, p));
    let mut put = 0;
    for &c in &points {
        if !blocked(c) {
            ctx.in_room(job, &how, r, c);
            put += 1;
        }
    }
    if put == 0 && !points.is_empty() {
        let c = interior_point(&outer, &holes);
        if blocked(c) {
            ctx.issues.add(job.pointer, format!("no free floor for a {}", job.label()), &path, None);
        } else {
            ctx.in_room(job, &how, r, c);
        }
    }
}

/// A part's name, more than one: "lights", "switches".
fn plural(label: &str) -> String {
    if ["s", "x", "z", "ch", "sh"].iter().any(|e| label.ends_with(e)) {
        format!("{label}es")
    } else {
        format!("{label}s")
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

/// The selected rooms of one region together (biggest first), and each other room alone.
fn by_region(model: &Model, rooms: &[usize], elements: &[Element]) -> Vec<Vec<usize>> {
    let mut groups: Vec<(Option<String>, Vec<usize>)> = Vec::new();
    for &r in rooms {
        let region = elements[model.rooms[r].element].space.as_ref().and_then(|s| s.region.clone());
        match groups.iter_mut().find(|(k, _)| region.is_some() && *k == region) {
            Some((_, g)) => g.push(r),
            None => groups.push((region, vec![r])),
        }
    }
    groups
        .into_iter()
        .map(|(_, mut g)| {
            g.sort_by(|&a, &b| model.rooms[b].area.total_cmp(&model.rooms[a].area).then(a.cmp(&b)));
            g
        })
        .collect()
}

/// What a `one` placement asks for.
struct OneAsk<'a> {
    near: Option<&'a Selector>,
    apart: Option<&'a Selector>,
    need: Option<crate::assembly::Need>,
    /// A part for each room of the group, side by side.
    each: bool,
}

/// The best single place for a part, on the walls of a group of rooms (one room, or the rooms
/// of a region). With `each`, one part per room of the group, side by side there.
fn one(ctx: &mut Ctx, job: &Job, group: &[usize], ask: &OneAsk, elements: &[Element]) {
    let (near, apart, need) = (ask.near, ask.apart, ask.need);
    let paths: Vec<String> = group.iter().map(|&r| ctx.model.rooms[r].path.clone()).collect();
    let path = paths[0].clone();
    let many = group.len() > 1;
    let mut how = if many { "the best place in the region" } else { "the best place in the room" }.to_owned();
    if let Some(n) = near {
        how.push_str(&format!(", near {}", n.items().join(", ")));
    }
    if let Some(a) = apart {
        how.push_str(&format!(", away from {}", a.items().join(", ")));
    }
    // What each part placed serves: its own room with `each`, else the group.
    let serves = |k: usize| -> Vec<String> {
        match (many, ask.each) {
            (false, _) => Vec::new(),
            (true, true) => vec![paths[k].clone()],
            (true, false) => paths.clone(),
        }
    };
    let count = if ask.each { group.len() } else { 1 };
    if !matches!(job.part.mount, Mount::Wall | Mount::Cavity) {
        let room = &ctx.model.rooms[group[0]];
        let c = interior_point(&room.loops[0], &room.loops[1..]);
        for k in 0..count {
            ctx.in_room(job, &how, group[0], c);
            ctx.out.last_mut().unwrap().serves = serves(k);
        }
        return;
    }
    let (w, depth) = job.part.size(ctx.units);
    // Side by side, the parts take one stretch of wall.
    let wide = w * count as f64;
    let step = 0.1 * ctx.m;
    let keep_off = 0.6 * ctx.m;
    let inside = job.part.mount == Mount::Cavity;
    let above = if inside { IN_WALL } else { job.part.above(ctx.units) };
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
    // (score, room, face, along)
    let mut found: Vec<(f64, usize, usize, f64)> = Vec::new();
    for &r in group {
        let room = &ctx.model.rooms[r];
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
            let mut s = wide / 2.0 + step;
            while s + wide / 2.0 + step <= len + 1e-9 {
                let p = face.point(s);
                let in_opening = room
                    .openings
                    .iter()
                    .any(|o| o.face == f && s + wide / 2.0 > o.s0 - step && s - wide / 2.0 < o.s1 + step);
                let too_close = apart.is_some_and(|a| {
                    a.items().iter().any(|i| {
                        special(i) && distance_to(room, i, p, elements).is_some_and(|d| d < keep_off)
                    })
                });
                if !in_opening
                    && !too_close
                    && ctx.taken.free((r, f), s - wide / 2.0, s + wide / 2.0, above, band)
                {
                    let score: f64 = near.map_or(0.0, |n| {
                        n.items().iter().map(|i| distance_to(room, i, p, elements).unwrap_or(1e6)).sum()
                    });
                    found.push((score, r, f, s));
                }
                s += step;
            }
        }
    }
    // The best place that takes it (the wall may still turn it away: a fixture backed against
    // it, another part there).
    found.sort_by(|a, b| {
        a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)).then(a.3.total_cmp(&b.3))
    });
    let at = found.into_iter().find(|&(_, r, f, s)| ctx.on_wall(job, &how, r, f, s, Some(wide)));
    if let Some((_, r, f, s)) = at {
        let whole = ctx.out.pop().unwrap();
        if count == 1 {
            ctx.out.push(Placed { serves: serves(0), ..whole });
        } else {
            // One part per room, side by side across the stretch.
            let face = &ctx.model.rooms[r].faces[f];
            let n = if inside { Point::new(-face.inward.x, -face.inward.y) } else { face.inward };
            let d = depth;
            for k in 0..count {
                let c = s - wide / 2.0 + w * (k as f64 + 0.5);
                let (p0, p1) = (face.point(c - w / 2.0), face.point(c + w / 2.0));
                let q = |p: Point| Point::new(p.x + n.x * d, p.y + n.y * d);
                ctx.out.push(Placed {
                    at: face.point(c),
                    polygon: vec![p0, p1, q(p1), q(p0)],
                    front: if inside { [p1, p0] } else { [q(p1), q(p0)] },
                    serves: serves(k),
                    ..whole.clone()
                });
            }
        }
        return;
    }
    let mut need = need.unwrap_or_default();
    if inside {
        need.cavity = need.cavity.max(depth);
    }
    let what = if need == crate::assembly::Need::default() {
        String::new()
    } else {
        format!(" that is {}", need.describe())
    };
    let message = match (many, count) {
        (false, _) => format!("no wall{what} free for a {}", job.label()),
        (true, 1) => format!("no wall{what} in the region free for a {}", job.label()),
        (true, n) => format!("no wall{what} in the region free for {n} {}s side by side", job.label()),
    };
    ctx.issues.add(
        job.pointer,
        message,
        &path,
        Some("Loosen what it's kept away from, or claim a wall for it in the service's claims (`plate.wall … & { length = … }`).".into()),
    );
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
        network: p.network,
        fed_by: p.fed_by,
        load: p.load,
        at: p.at,
        serves: p.serves,
    });
    e
}

/// Placements on a pinned flat (`testkit::pinned_flat`): the rooms can't move, so positions are
/// exact. Each room's clear rectangle is its pinned rectangle less its walls (exterior 0.3 grows
/// inward, party 0.25 inward, partition 0.1 and wet 0.15 centred on the edge).
#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::geometry::{Bounds, Point};
    use crate::solve::testkit::*;

    /// Living, bedroom and bath, clear: (min x, min y, max x, max y).
    const CLEAR: [(&str, f64, f64, f64, f64); 3] = [
        ("living", 0.25, 0.3, 5.95, 4.45),
        ("bedroom", 8.05, 0.3, 11.75, 3.45),
        ("bath", 0.25, 6.075, 3.425, 7.7),
    ];

    fn close(a: Point, b: Point) -> bool {
        a.distance(b) < 1e-6
    }

    fn on_outline(p: Point, b: &Bounds) -> bool {
        let inside =
            p.x >= b.min_x - 1e-6 && p.x <= b.max_x + 1e-6 && p.y >= b.min_y - 1e-6 && p.y <= b.max_y + 1e-6;
        let edge = [p.x - b.min_x, b.max_x - p.x, p.y - b.min_y, b.max_y - p.y]
            .iter()
            .fold(f64::MAX, |m, d| m.min(d.abs()));
        inside && edge < 1e-6
    }

    #[test]
    fn rooms_are_drawn_to_their_clear_rectangles() {
        let s = run(pinned_flat(json!({})));
        assert_met(&s);
        for (id, x0, y0, x1, y1) in CLEAR {
            let b = Bounds::from_points(&element(&s, id).points);
            assert!(close(Point::new(b.min_x, b.min_y), Point::new(x0, y0)), "{id}: {b:?}");
            assert!(close(Point::new(b.max_x, b.max_y), Point::new(x1, y1)), "{id}: {b:?}");
        }
    }

    #[test]
    fn over_puts_one_light_in_the_middle_of_each_room() {
        let s = run(pinned_flat_with(json!({ "lighting": lighting() })));
        assert_met(&s);
        let lights = parts(&s, "light");
        for (id, x0, y0, x1, y1) in CLEAR {
            let here = in_space(&lights, id);
            assert_eq!(here.len(), 1, "{id}");
            let at = here[0].part.as_ref().unwrap().at;
            assert!(close(at, Point::new((x0 + x1) / 2.0, (y0 + y1) / 2.0)), "{id}: {at:?}");
        }
    }

    #[test]
    fn every_puts_outlets_on_the_rooms_outline_no_further_apart_than_asked() {
        let s = run(pinned_flat_with(json!({ "power": power() })));
        assert_met(&s);
        let outlets = parts(&s, "outlet");
        // 3.6 apart at most around 19.7 m of living room and 13.7 m of bedroom (doors cut it).
        for (id, want) in [("living", 6), ("bedroom", 4), ("bath", 0)] {
            assert_eq!(in_space(&outlets, id).len(), want, "{id}");
        }
        for o in &outlets {
            let p = o.part.as_ref().unwrap();
            let (_, x0, y0, x1, y1) = CLEAR.iter().find(|c| c.0 == p.space).unwrap();
            let b = Bounds { min_x: *x0, min_y: *y0, max_x: *x1, max_y: *y1 };
            assert!(on_outline(p.at, &b), "{} at {:?} is off {}'s walls", o.id, p.at, p.space);
            assert_eq!(p.above, Some(0.3));
        }
    }

    #[test]
    fn a_part_in_the_wall_keeps_clear_of_boxes_on_it() {
        // The bath's switch (by its door) and the plumbing stack (in the wet wall's cavity) were
        // put 2.5 cm apart: a box can't go where the stack takes the stud bay.
        let s = run(pinned_flat_with(json!({ "lighting": lighting(), "plumbing": plumbing() })));
        assert_met(&s);
        let stack = &parts(&s, "stack")[0];
        let (sa, sb) = (stack.points[0], stack.points[1]);
        let len = sa.distance(sb);
        let u = Point::new((sb.x - sa.x) / len, (sb.y - sa.y) / len);
        let along = |p: Point| (p.x - sa.x) * u.x + (p.y - sa.y) * u.y;
        let off = |p: Point| ((p.x - sa.x) * u.y - (p.y - sa.y) * u.x).abs();
        for e in of_kind(&s, crate::layout::ElementKind::Part) {
            let p = e.part.as_ref().unwrap();
            if p.mount != "wall" {
                continue;
            }
            // On the same wall (either face), and overlapping the stack along it?
            let (a, b) = (e.points[0], e.points[1]);
            if off(a).max(off(b)) > 0.3 {
                continue;
            }
            let (lo, hi) = (along(a).min(along(b)), along(a).max(along(b)));
            assert!(
                hi <= 1e-6 || lo >= len - 1e-6,
                "{} {} overlaps the stack: {lo}..{hi} vs 0..{len}",
                p.service,
                p.part
            );
        }
    }

    #[test]
    fn a_switch_goes_round_the_corner_when_the_door_fills_its_wall() {
        // A closet whose door takes its whole front wall: the switch goes on the wall beside.
        let mut v = pinned_flat_with(json!({ "lighting": lighting() }));
        v["spaces"]["closet"] = json!({ "at": [5, 6.9], "width": 1.1, "depth": 1.1, "tags": ["wet"] });
        let s = run(v);
        assert_met(&s);
        let here = in_space(&parts(&s, "switch"), "closet");
        assert_eq!(here.len(), 1);
        let p = here[0].part.as_ref().unwrap();
        assert!(p.placed.contains("round the corner"), "{}", p.placed);
    }

    #[test]
    fn beside_puts_one_switch_by_each_rooms_door() {
        let s = run(pinned_flat_with(json!({ "lighting": lighting() })));
        assert_met(&s);
        let switches = parts(&s, "switch");
        let doors = of_kind(&s, crate::layout::ElementKind::Door);
        for (id, x0, y0, x1, y1) in CLEAR {
            let here = in_space(&switches, id);
            assert_eq!(here.len(), 1, "{id}");
            let at = here[0].part.as_ref().unwrap().at;
            assert!(on_outline(at, &Bounds { min_x: x0, min_y: y0, max_x: x1, max_y: y1 }), "{id}: {at:?}");
            let door = doors.iter().find(|d| d.id.contains(id)).unwrap();
            let near = door.points.iter().map(|p| p.distance(at)).fold(f64::MAX, f64::min);
            assert!(near < 0.5, "{id}'s switch is {near} from its door");
        }
    }
}
