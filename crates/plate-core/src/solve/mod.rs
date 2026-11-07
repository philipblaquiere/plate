//! The host solver: solves plans top-down, one level at a time.
//!
//! An *engine* (`grid`, `spine_stack`) solves one level: given a slot (a boundary with ports)
//! and a plan, it places that plan's spaces. The host does everything else:
//!
//! - **Recursion.** A placed space with a `plan` becomes a slot of its own: a rectangle whose
//!   edges carry the ports the parent gave it. The plan is solved inside it, and so on down.
//! - **Canonical frames and typicals.** Each child slot is turned so its entrance is at the
//!   bottom (trying all rotations, and mirrors unless `mirror = false`), and described by its
//!   size and ports. Slots that describe the same way are solved once and the solution is
//!   copied, turned back into place: that's a typical. `plate.unique` and `plate.instance` take
//!   instances out of their typical.
//! - **Needs.** Before solving a child, its plan's `needs` are checked against the slot (accept
//!   or reject). Engines also size spaces to their plan's needs up front.
//! - **Diagnostics.** Rules and sizes that can't be met come back as warnings that point at the
//!   rule or space in the file (a JSON pointer, mapped to the text by the editor).

pub mod build;
#[cfg(test)]
mod built_tests;
mod common;
pub mod connect;
pub mod context;
pub mod fit;
pub mod frame;
mod grid;
pub mod outline;
pub mod ports;
pub mod route;
mod spine;
pub mod spread;

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex, OnceLock};

use self::common::hash64;
pub use self::common::num;
use self::frame::{Rect, Transform};
use self::ports::PortSeg;
use serde::Deserialize;

use self::context::{origin, resolved, under, Within};
use crate::config::{Config, Start};
use crate::diagnostic::{Code, Diagnostic};
use crate::geometry::Point;
use crate::layout::{
    Element, ElementKind, LayerInfo, Layout, OpeningInfo, PortInfo, Report, SlotInfo, SpaceInfo, Status,
    SymbolInfo, WallInfo,
};
use crate::plan::{bounds, role_matches, Engine, Plan, PortNeed, Previews, Rule, SidePorts, SpaceKind};
use crate::service::Service;
use crate::units::{System, Units};

/// What one level is solved inside, in that level's own frame.
#[derive(Debug, Clone, Default)]
pub struct Slot {
    /// The boundary polygon, or none for open space (around a spine or an origin).
    pub boundary: Option<Vec<Point>>,
    pub ports: Vec<PortSeg>,
    /// The plate's spine (or origin), at the root.
    pub spine: Option<Vec<Point>>,
    /// Maps the plan's own coordinates (where `at` corners are written) into this slot's frame.
    pub pin: Transform,
    /// Holes in `boundary`: a fill space's slot can surround other spaces.
    pub holes: Vec<Vec<Point>>,
}

pub struct Ctx<'a> {
    pub units: Units,
    pub grid: f64,
    pub seed: u64,
    /// Moves made so far in the whole solve, by every engine: extra effort (more runs, another
    /// seed) is only spent while this is under [`WORK_LIMIT`]. Counted, not timed, so the same
    /// file always gets the same effort and the same answer.
    pub work: std::sync::Arc<std::sync::atomic::AtomicU64>,
    /// The plan's absolute pointer in the configuration ("" at the root).
    pub plan_pointer: &'a str,
    /// The top level of a plate: it builds the walls on the plate's boundary too.
    pub root: bool,
}

/// Moves the extra effort of a solve may take it up to (see [`Ctx::work`]).
pub const WORK_LIMIT: u64 = 6_000_000;

/// Seeds a level is tried with, at most, while something in it isn't met.
const ATTEMPTS: u64 = 4;

impl Ctx<'_> {
    /// Is there effort left for another try?
    pub fn can_retry(&self, share: f64) -> bool {
        (self.work.load(std::sync::atomic::Ordering::Relaxed) as f64) < WORK_LIMIT as f64 * share
    }
}

/// A space an engine placed.
#[derive(Debug, Clone)]
pub struct Placed {
    pub name: String,
    pub type_key: String,
    pub polygon: Vec<Point>,
    /// For rectangles: size, and the frame mapping `[0, w] × [0, h]` into the level.
    pub rect: Option<(f64, f64, Transform)>,
    /// Ports of the space, in its own `[0, w] × [0, h]` frame.
    pub ports: Vec<PortSeg>,
    /// Fixtures: which way it faces and the floor it keeps clear.
    pub fixture: Option<FixtureOut>,
    /// Holes in `polygon` (clockwise), for a fill space that surrounds others.
    pub holes: Vec<Vec<Point>>,
}

/// Where a placed fixture faces, and the floor it keeps clear, in the level's frame.
#[derive(Debug, Clone)]
pub struct FixtureOut {
    /// The front edge, fixture on its left (see [`SpaceInfo::front`]).
    pub front: (Point, Point),
    /// The fixture and its clearance, when it has one.
    pub clearance: Option<Vec<Point>>,
}

#[derive(Debug, Clone)]
pub struct Opening {
    pub kind: ElementKind,
    pub a: Point,
    pub b: Point,
    pub between: Vec<String>,
}

/// How much an issue matters: a requirement not met makes a solution incomplete; a missed
/// preference or floor left over only keeps it from being perfect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Required,
    Preferred,
    Leftover,
}

/// A problem with the solution, pointing at the plan value it concerns.
#[derive(Debug, Clone, PartialEq)]
pub struct Issue {
    /// JSON pointer relative to the plan.
    pub pointer: String,
    pub message: String,
    pub level: Level,
    /// What to change for the solver to meet it.
    pub hint: Option<String>,
}

impl Issue {
    pub fn new(pointer: impl Into<String>, message: impl Into<String>) -> Issue {
        Issue { pointer: pointer.into(), message: message.into(), level: Level::Required, hint: None }
    }

    pub fn preferred(pointer: impl Into<String>, message: impl Into<String>) -> Issue {
        Issue { level: Level::Preferred, ..Issue::new(pointer, message) }
    }

    pub fn with_hint(mut self, hint: impl Into<String>) -> Issue {
        self.hint = Some(hint.into());
        self
    }
}

#[derive(Debug, Clone, Default)]
pub struct LevelResult {
    pub placed: Vec<Placed>,
    pub openings: Vec<Opening>,
    pub issues: Vec<Issue>,
}

/// A solved plan, in its slot's frame, including everything nested inside it.
#[derive(Debug, Clone, Default)]
struct Sub {
    spaces: Vec<SubSpace>,
    openings: Vec<SubOpening>,
    issues: Vec<SubIssue>,
    /// Floor no space covers.
    leftover: Vec<SubLeftover>,
    /// Moves it took to solve (counted again when it comes from the cache, so a solve spends
    /// the same effort whatever is cached).
    work: u64,
    walls: Vec<SubWall>,
    /// The levels that declare services, for the passes that place and connect their parts.
    services: Vec<ServiceLevel>,
}

#[derive(Debug, Clone)]
struct SubWall {
    path: String,
    polygon: Vec<Point>,
    info: WallInfo,
    source: String,
}

/// A level that declares services: the path of the spaces it placed (`flat_2.`), and the
/// services in effect there (its own and those it inherits).
#[derive(Debug, Clone)]
pub struct ServiceLevel {
    pub prefix: String,
    pub services: Arc<BTreeMap<String, Service>>,
}

#[derive(Debug, Clone)]
struct SubIssue {
    /// Pointer from the root of this plan.
    pointer: String,
    message: String,
    /// Where: instance paths.
    places: Vec<String>,
    level: Level,
    hint: Option<String>,
}

#[derive(Debug, Clone)]
struct SubLeftover {
    path: String,
    polygon: Vec<Point>,
    holes: Vec<Vec<Point>>,
    area: f64,
    /// The plan it's in, as a pointer.
    source: String,
}

#[derive(Debug, Clone)]
struct SubSpace {
    path: String,
    polygon: Vec<Point>,
    info: SpaceInfo,
    source: String,
    typical_key: Option<String>,
    /// Fixtures: the fixture with its clearance.
    clearance: Option<Vec<Point>>,
    holes: Vec<Vec<Point>>,
    /// What its walls leave of it, when they take some: drawn instead of `polygon`.
    clear: Option<(Vec<Point>, Vec<Vec<Point>>)>,
}

#[derive(Debug, Clone)]
struct SubOpening {
    path: String,
    kind: ElementKind,
    a: Point,
    b: Point,
    between: Vec<String>,
    width: f64,
}

struct Host {
    units: Units,
    system: System,
    root_seed: u64,
    cache: HashMap<String, Sub>,
    /// Typical key → number of instances using it.
    uses: HashMap<String, usize>,
    /// Moves made so far (see [`Ctx::work`]).
    work: std::sync::Arc<std::sync::atomic::AtomicU64>,
}

/// Solved plans kept between solves, so editing one level doesn't re-solve the others. Keys
/// include everything a solution depends on (plan, slot, units, grid, seed).
fn shared_cache() -> &'static Mutex<HashMap<String, Sub>> {
    static CACHE: OnceLock<Mutex<HashMap<String, Sub>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

const SHARED_CACHE_SIZE: usize = 512;

/// The result of solving: the layout, warnings about rules that couldn't be met, and how good
/// the solution is.
pub struct Solution {
    pub layout: Layout,
    pub diagnostics: Vec<Diagnostic>,
    pub report: Report,
}

/// What a plan file (one without a `start`) is shown in. Plates ignore it.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum View {
    /// The first `preview` case, or a slot sized from the plan's `needs`.
    #[default]
    Default,
    /// The `preview` case at this index.
    Case(usize),
    /// A slot a plate placed the plan in ([`SpaceInfo::slot`]): the same solution as there.
    Slot(SlotInfo),
}

pub fn solve(config: &Config) -> Solution {
    solve_view(config, &View::Default)
}

pub fn solve_view(config: &Config, view: &View) -> Solution {
    match &config.start {
        Some(start) => solve_plate(config, start),
        None => solve_preview(config, view),
    }
}

fn default_grid(units: Units) -> f64 {
    let mm = if units.system() == System::Imperial { 228.6 } else { 250.0 };
    mm / units.to_mm()
}

/// The grid a plan is solved on: its own `grid`, else the parent's (the default at the top),
/// made finer when it places fixtures: the coarsest round step their sizes and clearances are
/// whole numbers of (100 mm, 4 in at the finest), so they're drawn and kept at their real size.
fn plan_grid(plan: &Plan, units: Units, inherited: Option<f64>) -> f64 {
    if let Some(g) = plan.grid {
        return g.0.to(units);
    }
    let g = inherited.unwrap_or_else(|| default_grid(units));
    // Sizes that must come out exactly: fixtures', and any exact width or depth.
    let mut sizes: Vec<f64> = Vec::new();
    let mut fixtures = false;
    for s in plan.spaces.values() {
        if s.is_fixture() {
            fixtures = true;
            for b in [bounds(s.width, units), bounds(s.depth, units)] {
                sizes.push(b.min);
                if b.max.is_finite() {
                    sizes.push(b.max);
                }
            }
            if let Some(c) = s.clearance {
                sizes.extend(c.sides(units));
            }
        } else {
            for r in [s.width, s.depth] {
                if let Some(crate::plan::LengthRange::Exactly(l)) = r {
                    sizes.push(l.0.to(units));
                }
            }
        }
    }
    if sizes.is_empty() {
        return g;
    }
    let mm = units.to_mm();
    let steps: &[f64] = if units.system() == System::Imperial {
        &[228.6, 203.2, 152.4, 101.6]
    } else {
        &[250.0, 200.0, 100.0]
    };
    let fits = |step: f64| {
        sizes.iter().all(|v| {
            let n = v * mm / step;
            (n - n.round()).abs() < 0.02
        })
    };
    // Keep the parent's grid when it fits the exact sizes already (a fixture's own size being
    // the exception: fixtures always get a fine enough grid).
    if fits(g * mm) && (!fixtures || g * mm <= steps[0] + 1e-6) {
        return g;
    }
    steps.iter().copied().find(|&s| fits(s)).unwrap_or(steps[steps.len() - 1]) / mm
}

fn solve_plate(config: &Config, start: &Start) -> Solution {
    let units = config.units();
    let points = config.start_points();

    let start_element = match (start, points.len()) {
        (Start::Boundary { .. }, _) => Element::new("boundary", ElementKind::Boundary, points.clone(), true),
        (Start::Spine { .. }, 1) => Element::new("origin", ElementKind::Origin, points.clone(), false),
        (Start::Spine { .. }, _) => Element::new("spine", ElementKind::Spine, points.clone(), false),
    }
    .with_source("/start/points");
    let mut elements = vec![start_element];
    let mut diagnostics = Vec::new();

    let plan = config.plan();
    if !plan.spaces.is_empty() {
        let grid = plan_grid(&plan, units, None);
        // The root slot, in a frame where the boundary's longest edge is horizontal.
        let (slot, world) = match start {
            Start::Boundary { .. } => {
                let n = points.len();
                let longest = (0..n).max_by(|&a, &b| {
                    points[a]
                        .distance(points[(a + 1) % n])
                        .total_cmp(&points[b].distance(points[(b + 1) % n]))
                });
                let k = longest.unwrap_or(0);
                let (a, b) = (points[k], points[(k + 1) % n]);
                let angle = (b.y - a.y).atan2(b.x - a.x);
                // Turn by a multiple of 90° less if the edge is already axis-aligned.
                let snapped = (angle / std::f64::consts::FRAC_PI_2).round() * std::f64::consts::FRAC_PI_2;
                let angle = if (angle - snapped).abs() < 1e-9 { 0.0 } else { angle };
                let world = Transform::translate(a.x, a.y).then_after(&Transform::rotate(angle));
                let local = world.inverse();
                let boundary: Vec<Point> = points.iter().map(|p| local.apply(*p)).collect();
                let roles = config.edge_roles();
                let ports = (0..n)
                    .map(|i| PortSeg::new(roles[i].clone(), boundary[i], boundary[(i + 1) % n]))
                    .collect();
                // `at` corners are written in the plate's coordinates.
                (Slot { boundary: Some(boundary), ports, spine: None, pin: local, holes: Vec::new() }, world)
            }
            Start::Spine { .. } => {
                let slot = Slot {
                    boundary: None,
                    ports: Vec::new(),
                    spine: Some(points.clone()),
                    pin: Transform::IDENTITY,
                    holes: Vec::new(),
                };
                (slot, Transform::IDENTITY)
            }
        };
        let mut host = Host::new(units, config.system, config.seed);
        let (plan, _) = context::prepare(&plan, None, None, "");
        let ctx = Ctx {
            units,
            grid,
            seed: hash64(&[&config.seed.to_string(), "root"]),
            work: host.work.clone(),
            plan_pointer: "",
            root: true,
        };
        let sub = host.solve_plan(&plan, &slot, &ctx);
        let levels = sub.services.clone();
        let (leftover, spaces) = host.finish(sub, &world, &mut elements, &mut diagnostics);
        let circuits = fit::run(&levels, units, config.height(), &mut elements, &mut diagnostics);
        let floor = match start {
            Start::Boundary { .. } => crate::geometry::signed_area(&points).abs(),
            Start::Spine { .. } => 0.0,
        };
        let report = report(&diagnostics, leftover, floor, spaces);
        return Solution {
            layout: Layout::new(config.name.clone(), config.system, units, elements).with_circuits(circuits),
            diagnostics,
            report,
        };
    }

    let report = report(&diagnostics, 0.0, 0.0, 0);
    Solution { layout: Layout::new(config.name.clone(), config.system, units, elements), diagnostics, report }
}

/// A plan file on its own: solved in a preview slot exactly as a plate would solve it in a space
/// of that size and ports. The slot is drawn with its corner at the origin and its entrance
/// along the bottom (unless the preview turns it).
fn solve_preview(config: &Config, view: &View) -> Solution {
    let plan = config.plan();
    let cases = config.preview.as_ref().map(|p| p.cases()).unwrap_or_default();
    let many = matches!(config.preview, Some(Previews::Many(_)));

    struct Setup {
        w: f64,
        h: f64,
        sides: Vec<PortSeg>,
        units: Units,
        system: System,
        seed: u64,
        grid: Option<f64>,
        unique: Option<String>,
        source: Option<String>,
        label: String,
    }
    let mut diagnostics = Vec::new();
    let plan = match view {
        View::Slot(s @ SlotInfo { context: Some(c), .. }) => context_plan(&plan, c, s.units),
        _ => context::prepare(&plan, None, None, "").0,
    };
    let setup = match view {
        View::Slot(s) => {
            if s.plan != format!("{:016x}", plan.fingerprint()) {
                diagnostics.push(Diagnostic::info(
                    "The plate changes this plan where it uses it (a merge or an override), so its \
                     solution there can differ from this one.",
                ));
            }
            Setup {
                w: s.width,
                h: s.depth,
                sides: s.ports.iter().map(|p| PortSeg::new(p.roles.clone(), p.a, p.b)).collect(),
                units: s.units,
                system: s.system,
                seed: s.seed,
                grid: Some(s.grid),
                unique: s.unique.clone(),
                source: None,
                label: "This slot".into(),
            }
        }
        View::Default | View::Case(_) => {
            let index = match view {
                View::Case(i) => (*i).min(cases.len().saturating_sub(1)),
                _ => 0,
            };
            let case = cases.get(index).copied();
            let units = case
                .and_then(|c| c.units)
                .unwrap_or_else(|| case.and_then(|c| c.system).unwrap_or_default().default_units());
            let system = case.and_then(|c| c.system).unwrap_or_else(|| units.system());
            let grid = plan_grid(&plan, units, None);
            let (auto_w, auto_h) = auto_size(&plan, units, grid);
            let w = case.and_then(|c| c.width).map_or(auto_w, |v| v.0.to(units));
            let h = case.and_then(|c| c.depth).map_or(auto_h, |v| v.0.to(units));
            let roles = match case {
                Some(c) => c.ports.roles(),
                None => auto_roles(&plan),
            };
            let corners = Rect::new(0.0, 0.0, w, h).corners();
            let mut sides: Vec<PortSeg> =
                (0..4).map(|i| PortSeg::new(roles[i].clone(), corners[i], corners[(i + 1) % 4])).collect();
            // 'door on a side is a door in the middle of it, as the level above would put one.
            let door = 900.0 / units.to_mm();
            for i in 0..4 {
                if let Some(k) = sides[i].roles.iter().position(|r| r == ports::DOOR) {
                    sides[i].roles.remove(k);
                    let (a, b) = (corners[i], corners[(i + 1) % 4]);
                    let len = a.distance(b);
                    let (s0, s1) = (0.5 - door / len / 2.0, 0.5 + door / len / 2.0);
                    let at = |f: f64| Point::new(a.x + (b.x - a.x) * f, a.y + (b.y - a.y) * f);
                    if len > door {
                        sides.push(PortSeg::new(vec![ports::DOOR.to_owned()], at(s0), at(s1)));
                    }
                }
            }
            Setup {
                w,
                h,
                sides,
                units,
                system,
                seed: case.and_then(|c| c.seed).unwrap_or(1),
                grid: None,
                unique: None,
                source: Some(if many { format!("/preview/{index}") } else { "/preview".into() }),
                label: match case.and_then(|c| c.name.clone()) {
                    Some(name) => format!("The preview \"{name}\""),
                    None => "The preview slot".into(),
                },
            }
        }
    };

    let units = setup.units;
    let grid = setup.grid.unwrap_or_else(|| plan_grid(&plan, units, None));
    let mut slot =
        Element::new("slot", ElementKind::Slot, Rect::new(0.0, 0.0, setup.w, setup.h).corners(), true);
    slot.source = setup.source.clone();
    let mut elements = vec![slot];
    for (i, p) in setup.sides.iter().enumerate() {
        let mut e = Element::new(format!("port:{i}"), ElementKind::Port, vec![p.a, p.b], false);
        e.roles = Some(p.roles.clone());
        elements.push(e);
    }

    let mut host = Host::new(units, setup.system, setup.seed);
    let child = host.solve_child(
        &plan,
        setup.w,
        setup.h,
        &setup.sides,
        true,
        setup.unique.as_deref(),
        grid,
        "",
        None,
    );
    let mut levels = Vec::new();
    let (leftover, spaces) = match child.outcome {
        Err((pointer, message)) => {
            let label = setup.label;
            diagnostics.push(
                Diagnostic::warning(format!("{label}: {message}"))
                    .with_pointer(pointer)
                    .with_code(Code::Unmet)
                    .with_note("Resize the slot (drag its corners) or change the plan's `needs`."),
            );
            (0.0, 0)
        }
        Ok((sub, _)) => {
            levels = sub
                .services
                .iter()
                .map(|l| ServiceLevel { prefix: l.prefix.clone(), services: l.services.clone() })
                .collect();
            host.finish(sub, &child.canon.inverse(), &mut elements, &mut diagnostics)
        }
    };
    let circuits = fit::run(&levels, units, config.height(), &mut elements, &mut diagnostics);
    let report = report(&diagnostics, leftover, setup.w * setup.h, spaces);
    Solution {
        layout: Layout::new(config.name.clone(), setup.system, units, elements).with_circuits(circuits),
        diagnostics,
        report,
    }
}

/// A slot for a plan with no `preview`: sized from its `needs` (or, failing that, from the
/// areas of its spaces), rounded up to the grid.
fn auto_size(plan: &Plan, units: Units, grid: f64) -> (f64, f64) {
    let needs = &plan.needs;
    let wb = bounds(needs.width, units);
    let db = bounds(needs.depth, units);
    let ab = crate::plan::area_bounds(needs.area);
    let mut area = 0.0;
    for space in plan.spaces.values() {
        let a = crate::plan::area_bounds(space.area).min;
        let (sw, sd, side) =
            (bounds(space.width, units).min, bounds(space.depth, units).min, bounds(space.side, units).min);
        let a = if a > 0.0 {
            a
        } else if sw > 0.0 && sd > 0.0 {
            sw * sd
        } else {
            side * side
        };
        area += a * space.count.min().max(1) as f64;
    }
    // Room to move: walls between spaces, odd corners.
    let area = (area * 1.25).max(ab.min);
    let fallback = 4000.0 / units.to_mm();
    let (w, h) = match (!wb.is_any(), !db.is_any()) {
        (true, true) => (wb.target(fallback), db.target(fallback)),
        (true, false) => {
            let w = wb.target(fallback);
            (w, if area > 0.0 { area / w } else { 2.0 * w })
        }
        (false, true) => {
            let h = db.target(fallback);
            (if area > 0.0 { area / h } else { h / 2.0 }, h)
        }
        (false, false) if area > 0.0 => ((area / 2.0).sqrt(), (area * 2.0).sqrt()),
        (false, false) => (fallback, 2.0 * fallback),
    };
    let snap = |v: f64, b: crate::plan::Bounds1| {
        let v = if grid > 0.0 { (v / grid - 1e-9).ceil() * grid } else { v };
        v.max(b.min).min(b.max)
    };
    let w = snap(w, wb);
    let mut h = snap(h.max(db.min), db);
    if w * h < ab.min {
        h = snap(ab.min / w, db);
    }
    (w, h)
}

/// Port roles for a slot with no `preview`: a typical bay, minus the roles the plan forbids,
/// plus the ones it requires (on the top side).
fn auto_roles(plan: &Plan) -> [Vec<String>; 4] {
    let mut sides = SidePorts::default().roles();
    for (role, need) in &plan.needs.ports {
        match need {
            PortNeed::Forbidden => {
                for side in sides.iter_mut() {
                    side.retain(|r| !role_matches(r, role));
                    if side.is_empty() {
                        side.push("partition".into());
                    }
                }
            }
            PortNeed::Required if !sides.iter().flatten().any(|r| role_matches(r, role)) => {
                sides[2].push(role.clone());
            }
            _ => {}
        }
    }
    sides
}

fn letter(i: usize) -> String {
    let mut s = String::new();
    let mut n = i;
    loop {
        s.insert(0, (b'A' + (n % 26) as u8) as char);
        if n < 26 {
            break;
        }
        n = n / 26 - 1;
    }
    s
}

/// A child plan solved in a space: the frame it was solved in, the slot as the plan saw it, and
/// the solution (with its typical key), or why the space doesn't suit the plan.
struct Child {
    /// Maps the space's own `[0, w] × [0, h]` frame to the canonical one.
    canon: Transform,
    slot: SlotInfo,
    outcome: Result<(Sub, String), (String, String)>,
}

impl Host {
    fn new(units: Units, system: System, root_seed: u64) -> Host {
        Host {
            units,
            system,
            root_seed,
            cache: HashMap::new(),
            uses: HashMap::new(),
            work: Default::default(),
        }
    }

    /// Solve `child` in a `w × h` space with `ports` (in the space's own frame), the way every
    /// level does it: in the canonical frame, shared with equivalent slots (typicals), cached.
    #[allow(clippy::too_many_arguments)]
    fn solve_child(
        &mut self,
        child: &Plan,
        w: f64,
        h: f64,
        ports: &[PortSeg],
        mirror: bool,
        unique: Option<&str>,
        grid: f64,
        abs: &str,
        context: Option<serde_json::Value>,
    ) -> Child {
        let mm = self.units.to_mm();
        let (canon, cw, ch) = canonical(w, h, ports, mirror, mm);
        let ports: Vec<PortSeg> = ports.iter().map(|p| p.transformed(&canon)).collect();
        let slot = SlotInfo {
            width: cw,
            depth: ch,
            ports: ports.iter().map(|p| PortInfo { roles: p.roles.clone(), a: p.a, b: p.b }).collect(),
            units: self.units,
            system: self.system,
            seed: self.root_seed,
            grid,
            unique: unique.map(str::to_owned),
            plan: format!("{:016x}", child.fingerprint()),
            context,
        };
        if let Err(e) = check_needs(child, cw, ch, &ports, self.units) {
            return Child { canon, slot, outcome: Err(e) };
        }
        let signature = signature(cw, ch, &ports, mm);
        let mut key = format!("{:016x}|{signature}", child.fingerprint());
        if let Some(name) = unique {
            key = format!("{key}|{name}");
        }
        let child_slot = Slot {
            boundary: Some(Rect::new(0.0, 0.0, cw, ch).corners()),
            ports,
            spine: None,
            pin: Transform::IDENTITY,
            holes: Vec::new(),
        };
        // Symbols don't change a solution (nor its seed or typical), but the cached copy draws them.
        let cache_key = format!("{key}|{}", view_key(child));
        let shared_key = format!("{cache_key}|{:?}|{grid}|{}", self.units, self.root_seed);
        let remembered = self
            .cache
            .get(&cache_key)
            .cloned()
            .or_else(|| shared_cache().lock().ok().and_then(|c| c.get(&shared_key).cloned()));
        let solved = match remembered {
            Some(s) => {
                self.work.fetch_add(s.work, std::sync::atomic::Ordering::Relaxed);
                s
            }
            None => {
                let child_ctx = Ctx {
                    units: self.units,
                    grid,
                    seed: hash64(&[&self.root_seed.to_string(), &key]),
                    work: self.work.clone(),
                    plan_pointer: abs,
                    root: false,
                };
                let before = self.work.load(std::sync::atomic::Ordering::Relaxed);
                let mut s = self.solve_plan(child, &child_slot, &child_ctx);
                s.work = self.work.load(std::sync::atomic::Ordering::Relaxed) - before;
                if let Ok(mut c) = shared_cache().lock() {
                    if c.len() >= SHARED_CACHE_SIZE {
                        c.clear();
                    }
                    c.insert(shared_key, s.clone());
                }
                s
            }
        };
        self.cache.insert(cache_key, solved.clone());
        *self.uses.entry(key.clone()).or_insert(0) += 1;
        Child { canon, slot, outcome: Ok((solved, key)) }
    }

    /// Turn a solved top level into layout elements (moved by `world`) and diagnostics. Returns
    /// the floor left over and how many spaces were placed.
    fn finish(
        &self,
        sub: Sub,
        world: &Transform,
        elements: &mut Vec<Element>,
        diagnostics: &mut Vec<Diagnostic>,
    ) -> (f64, usize) {
        // Typical groups used more than once get letters, in order of first appearance.
        let mut letters: HashMap<String, String> = HashMap::new();
        for s in &sub.spaces {
            if let Some(key) = &s.typical_key {
                if self.uses.get(key).copied().unwrap_or(0) > 1 && !letters.contains_key(key) {
                    let letter = letter(letters.len());
                    letters.insert(key.clone(), letter);
                }
            }
        }
        let spaces = sub.spaces.len();
        for s in sub.spaces {
            let mut info = s.info;
            info.plan = resolved(&info.plan);
            info.typical = s.typical_key.as_ref().and_then(|k| letters.get(k).cloned());
            info.front = info.front.map(|f| move_front(f, world));
            let (outline, inner) = s.clear.clone().unwrap_or((s.polygon, s.holes));
            let polygon = move_polygon(&outline, world);
            let holes: Vec<Vec<Point>> = inner.iter().map(|h| move_polygon(h, world)).collect();
            let source = resolved(&s.source);
            // A fill space's shape isn't a rectangle a symbol could be fitted to: its tone colours it.
            let symbol = info
                .symbol
                .as_deref()
                .filter(|_| !info.fill)
                .and_then(|name| symbol_element(&s.path, &source, name, &polygon, info.front));
            if let Some(clear) = &s.clearance {
                let mut e = Element::new(
                    format!("clearance:{}", s.path),
                    ElementKind::Clearance,
                    move_polygon(clear, world),
                    true,
                );
                e.source = Some(format!("{source}/clearance"));
                elements.push(e);
            }
            elements.push(
                Element::space(s.path, polygon, info)
                    .with_holes(holes)
                    .with_label_point()
                    .with_source(source),
            );
            elements.extend(symbol);
        }
        let walls: Vec<(Point, Point, f64)> = sub
            .walls
            .iter()
            .map(|w| (world.apply(w.info.line[0]), world.apply(w.info.line[1]), w.info.thickness))
            .collect();
        for w in sub.walls {
            let mut info = w.info;
            info.line = [world.apply(info.line[0]), world.apply(info.line[1])];
            let mut e = Element::new(w.path, ElementKind::Wall, move_polygon(&w.polygon, world), true);
            if !w.source.is_empty() {
                e.source = Some(resolved(&w.source));
            }
            e.wall = Some(info);
            elements.push(e);
        }
        for o in sub.openings {
            let (a, b, depth) = in_wall(world.apply(o.a), world.apply(o.b), &walls);
            elements.push(Element::opening(
                o.path,
                o.kind,
                a,
                b,
                OpeningInfo { between: o.between, width: o.width, depth },
            ));
        }
        let mut leftover = 0.0;
        for l in sub.leftover {
            leftover += l.area;
            let holes = l.holes.iter().map(|h| move_polygon(h, world)).collect();
            let e = Element::new(
                format!("leftover:{}", l.path),
                ElementKind::Leftover,
                move_polygon(&l.polygon, world),
                true,
            )
            .with_holes(holes)
            .with_label_point()
            .with_source(resolved(&l.source));
            elements.push(e);
        }
        for issue in sub.issues {
            let message = match issue.places.len() {
                0 => issue.message,
                1 => format!("{}: {}", issue.places[0], issue.message),
                n => format!("{} and {} more like it: {}", issue.places[0], n - 1, issue.message),
            };
            let d = match issue.level {
                Level::Required => Diagnostic::warning(message).with_code(Code::Unmet),
                Level::Preferred => Diagnostic::info(message).with_code(Code::Preference),
                Level::Leftover => Diagnostic::info(message).with_code(Code::Leftover),
            }
            .with_pointer(resolved(&issue.pointer));
            diagnostics.push(match issue.hint {
                Some(h) => d.with_note(h),
                None => d,
            });
        }
        (leftover, spaces)
    }

    /// Solve one plan in `slot`, and every plan nested in it. Its own spaces are at depth 0.
    ///
    /// When something inside doesn't work out (a plan that can't fit its room, say), the level
    /// is solved again with another seed, up to a few times, and the best attempt is kept: the
    /// layout of a level is what decides whether the levels inside it can work.
    fn solve_plan(&mut self, plan: &Plan, slot: &Slot, ctx: &Ctx) -> Sub {
        let score = |sub: &Sub| -> (usize, i64) {
            let unmet =
                sub.issues.iter().filter(|i| i.level == Level::Required).map(|i| i.places.len().max(1)).sum();
            let left: f64 = sub.leftover.iter().map(|l| l.area).sum();
            (unmet, (left * 1000.0).round() as i64)
        };
        // With a fill space nothing should be left over: floor cut off from it is worth retrying.
        let fill = plan.spaces.values().any(|s| s.fill);
        let uses = self.uses.clone();
        // (unmet, floor left over in thousandths), the attempt, and the typicals it used.
        type Attempt = ((usize, i64), Sub, HashMap<String, usize>);
        let mut best: Option<Attempt> = None;
        for attempt in 0..ATTEMPTS {
            let seed =
                if attempt == 0 { ctx.seed } else { hash64(&[&ctx.seed.to_string(), &attempt.to_string()]) };
            if attempt > 0 && !ctx.can_retry(0.5) {
                break;
            }
            let try_ctx = Ctx {
                units: ctx.units,
                grid: ctx.grid,
                seed,
                work: ctx.work.clone(),
                plan_pointer: ctx.plan_pointer,
                root: ctx.root,
            };
            self.uses = uses.clone();
            let sub = self.solve_plan_once(plan, slot, &try_ctx);
            let s = score(&sub);
            // The search is stochastic: another seed can find what this one missed, at this level
            // or inside it (or floor cut off from the fill). A level that can't work at all stops
            // early anyway: its attempts run out of budget fast.
            let done = s.0 == 0 && (s.1 == 0 || !fill);
            if best.as_ref().is_none_or(|(b, _, _)| s < *b) {
                best = Some((s, sub, self.uses.clone()));
            }
            if done {
                break;
            }
        }
        let (_, sub, kept) = best.expect("at least one attempt");
        self.uses = kept;
        sub
    }

    /// One attempt at a level.
    fn solve_plan_once(&mut self, plan: &Plan, slot: &Slot, ctx: &Ctx) -> Sub {
        let engine = plan.engine.unwrap_or(match &slot.spine {
            Some(s) if s.len() >= 2 => Engine::SpineStack,
            _ => Engine::Grid,
        });
        let level = match engine {
            Engine::Grid => grid::solve(plan, slot, ctx),
            Engine::SpineStack => spine::solve(plan, slot, ctx),
        };
        let built = build::build(plan, slot, &level, self.units, ctx.root);
        let mut sub = Sub::default();
        for issue in needs_conflicts(plan, self.units).iter().chain(&level.issues).chain(&built.issues) {
            let mut i = SubIssue::from(issue);
            i.pointer = origin(plan, &i.pointer);
            push_issue(&mut sub.issues, i);
        }
        if let Some(boundary) = &slot.boundary {
            // Floor left over: only where a plan divides its space into spaces. A plan that places
            // fixtures leaves the rest as the room's own floor.
            if !plan.spaces.is_empty() && plan.spaces.values().all(|s| !s.is_fixture()) {
                leftover(plan, boundary, &slot.holes, &level, ctx, &mut sub);
            }
        }
        // Services declared here (the top level, or a plan with its own): where they apply.
        let own = format!("{}/services/", ctx.plan_pointer);
        let declares =
            plan.services.values().any(|s| s.source.as_deref().is_some_and(|src| src.starts_with(&own)));
        if !plan.services.is_empty() && (ctx.plan_pointer.is_empty() || declares) {
            sub.services
                .push(ServiceLevel { prefix: String::new(), services: Arc::new(plan.services.clone()) });
        }

        // Host-level rules: which instances are unique, and which get another plan.
        let rules = plan.rules();
        let unique = |name: &str, key: &str| {
            rules.iter().any(|(_, r)| matches!(r, Rule::Unique { a } if a.matches(name, key)))
        };
        let override_plan = |name: &str, key: &str| {
            rules.iter().rev().find_map(|(ptr, r)| match r {
                Rule::Instance { a, plan } if a.matches(name, key) => {
                    Some((format!("{ptr}/plan"), plan.as_ref()))
                }
                _ => None,
            })
        };

        for o in &level.openings {
            let width = o.a.distance(o.b);
            let tag = if o.kind == ElementKind::Door { "door" } else { "window" };
            sub.openings.push(SubOpening {
                path: format!("{tag}:{}", o.between.join("|")),
                kind: o.kind,
                a: o.a,
                b: o.b,
                between: o.between.clone(),
                width,
            });
        }
        reach_issues(plan, &level, &mut sub);
        let own_openings = sub.openings.len();

        for (k, w) in built.walls.iter().enumerate() {
            let owner = level.placed[w.owner].name.clone();
            let other = match w.other {
                Some(o) => level.placed[o].name.clone(),
                None => format!("'{}", w.roles.first().map_or("outside", String::as_str)),
            };
            sub.walls.push(SubWall {
                path: format!("wall:{owner}|{other}#{k}"),
                polygon: w.polygon(),
                info: wall_info(w, vec![owner, other], self.units),
                source: w.assembly.source.as_ref().map_or_else(String::new, |s| format!("@{s}")),
            });
        }

        for (index, placed) in level.placed.iter().enumerate() {
            let Some(space) = plan.spaces.get(&placed.type_key) else { continue };
            let overridden = override_plan(&placed.name, &placed.type_key);
            let is_unique = unique(&placed.name, &placed.type_key) || overridden.is_some();
            let (child_plan, child_ptr) = match &overridden {
                Some((ptr, p)) => (Some(*p), ptr.clone()),
                None => (space.plan.as_deref(), format!("/spaces/{}/plan", placed.type_key)),
            };
            let child_ptr = origin(plan, &child_ptr);
            let child_abs = match child_ptr.strip_prefix('@') {
                Some(abs) => abs.to_owned(),
                None => format!("{}{child_ptr}", ctx.plan_pointer),
            };
            let within = Within { name: &placed.name, key: &placed.type_key, space, units: self.units };
            // A service may give this space what it claims (a fixture, rules) even without a plan.
            let child_plan: Option<Plan> = match child_plan {
                Some(p) => Some(p.clone()),
                None if !space.is_fixture() && claims_inside(plan, within) => Some(Plan::default()),
                None => None,
            };
            let child_plan = child_plan.map(|c| {
                let (prepared, issues) = context::prepare(&c, Some(plan), Some(within), &child_abs);
                for i in &issues {
                    push_issue(&mut sub.issues, SubIssue::from(i).at(vec![placed.name.clone()]));
                }
                prepared
            });
            // What the walls leave of it, and what they're built as (a role of the ports inside).
            let insets = built.insets.get(index).copied().unwrap_or_default();
            let own_ports = assembly_roles(placed, index, &built.walls, self.units);
            let clear = match (&placed.rect, built.fill.get(index).cloned().flatten()) {
                (Some((w, h, t)), _) if insets.iter().any(|v| *v > 0.0) => {
                    let r = Rect::new(insets[3], insets[0], w - insets[1], h - insets[2]);
                    Some((r.corners().into_iter().map(|p| t.apply(p)).collect(), Vec::new()))
                }
                (None, Some(shape)) if shape.0 != placed.polygon => Some(shape),
                _ => None,
            };
            let mut info = SpaceInfo {
                name: placed.name.clone(),
                type_key: placed.type_key.clone(),
                label: space.label.clone().unwrap_or_else(|| placed.name.clone()),
                kind: space.kind,
                depth: 0,
                plan: String::new(),
                typical: None,
                unique: is_unique,
                has_plan: child_plan.is_some() && !space.is_fixture(),
                ok: true,
                slot: None,
                front: placed
                    .fixture
                    .as_ref()
                    .map(|f| [f.front.0, f.front.1])
                    .or_else(|| entered_from(placed)),
                symbol: space.symbol.clone(),
                load: space.load.clone(),
                fill: space.fill,
                tone: space.symbol.as_deref().filter(|_| space.fill).and_then(crate::symbols::tone),
                tags: space.tags.clone(),
                gross: None,
            };
            let mut typical_key = None;
            let mut nested: Option<(Sub, Transform)> = None;

            if let (true, Some((ptr, _))) = (space.is_fixture(), &overridden) {
                push_issue(
                    &mut sub.issues,
                    SubIssue::required(
                        origin(plan, ptr),
                        "a fixture can't have a plan: nothing goes inside it",
                    )
                    .at(vec![placed.name.clone()]),
                );
            }
            let child_plan = child_plan.filter(|_| !space.is_fixture());
            if let (Some(child), None, true) = (child_plan.as_ref(), placed.rect, space.fill) {
                // A fill space's plan is solved in its own shape, in this level's frame: it's one
                // of a kind, so there's no typical to share.
                let grid = plan_grid(child, self.units, Some(ctx.grid));
                let (outer, holes) =
                    clear.clone().unwrap_or_else(|| (placed.polygon.clone(), placed.holes.clone()));
                let mut loops = vec![outer.clone()];
                loops.extend(holes.iter().cloned());
                let fill_slot = Slot {
                    boundary: Some(outer),
                    ports: own_ports.iter().map(|p| snap_port(p, &loops)).collect(),
                    spine: None,
                    pin: slot.pin,
                    holes,
                };
                let child_ctx = Ctx {
                    units: self.units,
                    grid,
                    seed: hash64(&[&ctx.seed.to_string(), &placed.name, "fill"]),
                    work: self.work.clone(),
                    plan_pointer: &child_abs,
                    root: false,
                };
                let child_sub = self.solve_plan(child, &fill_slot, &child_ctx);
                if child_sub.issues.iter().any(|i| i.level == Level::Required) {
                    info.ok = false;
                }
                nested = Some((child_sub, Transform::IDENTITY));
            } else if let (Some(child), Some((w, h, to_level))) = (child_plan.as_ref(), placed.rect) {
                let grid = plan_grid(child, self.units, Some(ctx.grid));
                let name = is_unique.then_some(placed.name.as_str());
                let (cw, ch) = (w - insets[1] - insets[3], h - insets[0] - insets[2]);
                let ports = clear_ports(&own_ports, w, h, insets);
                let context = slot_context(plan, within);
                let solved =
                    self.solve_child(child, cw, ch, &ports, space.mirror, name, grid, &child_abs, context);
                info.slot = Some(solved.slot);
                let to_clear = to_level.then_after(&Transform::translate(insets[3], insets[0]));
                match solved.outcome {
                    Err((ptr, msg)) => {
                        info.ok = false;
                        let hint = needs_hint(&ptr);
                        push_issue(
                            &mut sub.issues,
                            SubIssue::required(under(&child_ptr, &ptr), msg)
                                .at(vec![placed.name.clone()])
                                .hint(hint),
                        );
                    }
                    Ok((child_sub, key)) => {
                        if child_sub.issues.iter().any(|i| i.level == Level::Required) {
                            info.ok = false;
                        }
                        typical_key = Some(key);
                        nested = Some((child_sub, to_clear.then_after(&solved.canon.inverse())));
                    }
                }
            }
            if clear.is_some() {
                info.gross = Some(
                    crate::geometry::signed_area(&placed.polygon).abs()
                        - placed.holes.iter().map(|h| crate::geometry::signed_area(h).abs()).sum::<f64>(),
                );
            }

            sub.spaces.push(SubSpace {
                path: placed.name.clone(),
                polygon: placed.polygon.clone(),
                info,
                source: origin(plan, &format!("/spaces/{}", placed.type_key)),
                typical_key,
                clearance: placed.fixture.as_ref().and_then(|f| f.clearance.clone()),
                holes: placed.holes.clone(),
                clear,
            });

            // Everything inside, moved into this level's frame and renamed under this space.
            if let Some((child, t)) = nested {
                let prefix = format!("{}.", placed.name);
                for mut s in child.spaces {
                    s.path = format!("{prefix}{}", s.path);
                    s.polygon = move_polygon(&s.polygon, &t);
                    s.holes = s.holes.iter().map(|h| move_polygon(h, &t)).collect();
                    s.clear = s.clear.map(|(o, h)| {
                        (move_polygon(&o, &t), h.iter().map(|h| move_polygon(h, &t)).collect())
                    });
                    s.clearance = s.clearance.map(|c| move_polygon(&c, &t));
                    s.info.front = s.info.front.map(|f| move_front(f, &t));
                    s.info.depth += 1;
                    s.info.plan = under(&child_ptr, &s.info.plan);
                    s.source = under(&child_ptr, &s.source);
                    sub.spaces.push(s);
                }
                for mut o in child.openings {
                    o.path = format!("{prefix}{}", o.path);
                    o.a = t.apply(o.a);
                    o.b = t.apply(o.b);
                    o.between = o
                        .between
                        .iter()
                        .map(|n| if n.starts_with('\'') { n.clone() } else { format!("{prefix}{n}") })
                        .collect();
                    sub.openings.push(o);
                }
                for mut w in child.walls {
                    w.path = format!("{prefix}{}", w.path);
                    w.polygon = move_polygon(&w.polygon, &t);
                    w.info.line = [t.apply(w.info.line[0]), t.apply(w.info.line[1])];
                    w.info.between = w
                        .info
                        .between
                        .iter()
                        .map(|n| if n.starts_with('\'') { n.clone() } else { format!("{prefix}{n}") })
                        .collect();
                    w.source = under(&child_ptr, &w.source);
                    sub.walls.push(w);
                }
                for mut l in child.services {
                    l.prefix = format!("{prefix}{}", l.prefix);
                    sub.services.push(l);
                }
                for mut l in child.leftover {
                    l.path = format!("{prefix}{}", l.path);
                    l.polygon = move_polygon(&l.polygon, &t);
                    l.holes = l.holes.iter().map(|h| move_polygon(h, &t)).collect();
                    l.source = under(&child_ptr, &l.source);
                    sub.leftover.push(l);
                }
                for mut issue in child.issues {
                    issue.places = if issue.places.is_empty() {
                        vec![placed.name.clone()]
                    } else {
                        issue.places.into_iter().map(|p| format!("{prefix}{p}")).collect()
                    };
                    issue.pointer = under(&child_ptr, &issue.pointer);
                    push_issue(&mut sub.issues, issue);
                }
            }
        }
        align_doors(&mut sub, own_openings, build::thickest(plan, self.units));
        sub
    }
}

/// Does a service in scope here put something inside this space (`claims.inside`)?
fn claims_inside(plan: &Plan, within: Within) -> bool {
    plan.services.values().any(|s| {
        let here = s.in_scope
            || s.scope.as_ref().is_some_and(|sc| {
                sc.matches_tagged(within.name, within.key, &within.space.tags, within.space.kind)
            });
        here && s.claims.inside.keys().any(|k| {
            crate::plan::Selector::One(k.clone()).matches_tagged(
                within.name,
                within.key,
                &within.space.tags,
                within.space.kind,
            )
        })
    })
}

/// A space's ports with the name of the assembly each is built as added to its roles, so a plan
/// inside can ask for the wall itself (`against = 'wet`: the wall built as `wet`).
fn assembly_roles(placed: &Placed, index: usize, walls: &[build::BuiltWall], units: Units) -> Vec<PortSeg> {
    let mine: Vec<&build::BuiltWall> =
        walls.iter().filter(|w| w.owner == index || w.other == Some(index)).collect();
    if mine.is_empty() {
        return placed.ports.clone();
    }
    let to_level = placed.rect.as_ref().map_or(Transform::IDENTITY, |r| r.2);
    let tol = units.tolerance() * 10.0;
    placed
        .ports
        .iter()
        .map(|p| {
            let (a, b) = (to_level.apply(p.a), to_level.apply(p.b));
            let mut p = p.clone();
            let best = mine
                .iter()
                .filter_map(|w| ports::collinear_overlap(a, b, w.a, w.b, tol).map(|(t0, t1)| (t1 - t0, w)))
                .max_by(|x, y| x.0.total_cmp(&y.0));
            if let Some((_, w)) = best {
                if !w.name.is_empty() && !p.roles.contains(&w.name) {
                    p.roles.push(w.name.clone());
                }
            }
            p
        })
        .collect()
}

/// A space's ports, moved onto what its walls leave of it (`insets`: bottom, right, top, left).
fn clear_ports(ports: &[PortSeg], w: f64, h: f64, insets: [f64; 4]) -> Vec<PortSeg> {
    if insets.iter().all(|v| *v == 0.0) {
        return ports.to_vec();
    }
    let [b, r, t, l] = insets;
    let (cw, ch) = (w - l - r, h - b - t);
    let tol = 1e-9 * (1.0 + w + h);
    let mut out = Vec::new();
    for p in ports {
        let on = |f: &dyn Fn(Point) -> f64, v: f64| (f(p.a) - v).abs() < tol && (f(p.b) - v).abs() < tol;
        let clamp = |v: f64, hi: f64| v.max(0.0).min(hi);
        let (a, bb) = if on(&|q| q.y, 0.0) {
            (Point::new(clamp(p.a.x - l, cw), 0.0), Point::new(clamp(p.b.x - l, cw), 0.0))
        } else if on(&|q| q.y, h) {
            (Point::new(clamp(p.a.x - l, cw), ch), Point::new(clamp(p.b.x - l, cw), ch))
        } else if on(&|q| q.x, 0.0) {
            (Point::new(0.0, clamp(p.a.y - b, ch)), Point::new(0.0, clamp(p.b.y - b, ch)))
        } else if on(&|q| q.x, w) {
            (Point::new(cw, clamp(p.a.y - b, ch)), Point::new(cw, clamp(p.b.y - b, ch)))
        } else {
            continue;
        };
        if a.distance(bb) > tol {
            out.push(PortSeg { roles: p.roles.clone(), a, b: bb, other: p.other.clone() });
        }
    }
    out
}

/// A port moved onto the nearest edge of a fill's clear outline.
fn snap_port(p: &PortSeg, loops: &[Vec<Point>]) -> PortSeg {
    let snap = |q: Point| -> Point {
        let mut best = (f64::INFINITY, q);
        for l in loops {
            for k in 0..l.len() {
                let (a, b) = (l[k], l[(k + 1) % l.len()]);
                let (dx, dy) = (b.x - a.x, b.y - a.y);
                let len2 = (dx * dx + dy * dy).max(1e-18);
                let t = (((q.x - a.x) * dx + (q.y - a.y) * dy) / len2).clamp(0.0, 1.0);
                let c = Point::new(a.x + dx * t, a.y + dy * t);
                let d = c.distance(q);
                if d < best.0 {
                    best = (d, c);
                }
            }
        }
        best.1
    };
    PortSeg { roles: p.roles.clone(), a: snap(p.a), b: snap(p.b), other: p.other.clone() }
}

/// What a wall is, for the drawing and the inspector.
fn wall_info(w: &build::BuiltWall, between: Vec<String>, units: Units) -> WallInfo {
    let a = &w.assembly;
    WallInfo {
        assembly: w.name.clone(),
        label: a.label.clone().unwrap_or_else(|| w.name.clone()),
        thickness: w.thickness,
        cavity: a.cavity(units),
        fire: a.fire,
        acoustic: a.acoustic,
        bearing: a.bearing,
        layers: a
            .layers
            .iter()
            .map(|l| LayerInfo {
                material: l.material.clone(),
                thickness: l.thickness.0.to(units),
                cavity: l.cavity,
            })
            .collect(),
        pattern: a.pattern.unwrap_or_default().name(),
        cross: a.crossing().name(),
        roles: w.roles.clone(),
        between,
        because: w.because.clone(),
        line: w.centreline(),
        layer: a.layer.clone(),
    }
}

/// What a space's plan inherits from `parent`, for its slot: so the plan opened on its own (in
/// that slot) solves the same.
fn slot_context(parent: &Plan, within: Within) -> Option<serde_json::Value> {
    if parent.assemblies.is_empty() && parent.services.is_empty() {
        return None;
    }
    let assemblies: serde_json::Map<String, serde_json::Value> =
        parent.assemblies.iter().map(|(k, a)| (k.clone(), a.full())).collect();
    let services: serde_json::Map<String, serde_json::Value> =
        parent.services.iter().map(|(k, s)| (k.clone(), s.full())).collect();
    let scoped: Vec<&String> = parent.services.iter().filter(|(_, s)| s.in_scope).map(|(k, _)| k).collect();
    Some(serde_json::json!({
        "assemblies": assemblies,
        "services": services,
        "scoped": scoped,
        "within": { "name": within.name, "key": within.key, "kind": within.space.kind, "tags": within.space.tags },
    }))
}

/// A plan prepared as it was in a plate's space, from that space's [`slot_context`].
fn context_plan(plan: &Plan, c: &serde_json::Value, units: Units) -> Plan {
    let mut parent = Plan::default();
    if let Ok(a) = serde_json::from_value(c["assemblies"].clone()) {
        parent.assemblies = a;
    }
    if let Ok(s) = serde_json::from_value::<BTreeMap<String, Service>>(c["services"].clone()) {
        parent.services = s;
    }
    let scoped: Vec<String> = serde_json::from_value(c["scoped"].clone()).unwrap_or_default();
    for (k, s) in parent.services.iter_mut() {
        s.in_scope = scoped.contains(k);
    }
    let w = &c["within"];
    let space: Option<crate::plan::Space> =
        serde_json::from_value(serde_json::json!({ "kind": w["kind"], "tags": w["tags"] })).ok();
    let (name, key) = (w["name"].as_str().unwrap_or_default(), w["key"].as_str().unwrap_or_default());
    match &space {
        Some(space) => context::prepare(plan, Some(&parent), Some(Within { name, key, space, units }), "").0,
        None => context::prepare(plan, Some(&parent), None, "").0,
    }
}

/// A space whose own width or depth can't meet what its plan needs: no slot can work.
fn needs_conflicts(plan: &Plan, units: crate::units::Units) -> Vec<Issue> {
    let mut out = Vec::new();
    // Needs are clear sizes and a space's own are gross: its walls can take this much across it.
    let walls = 2.0 * build::thickest(plan, units);
    for (key, space) in &plan.spaces {
        let Some(child) = space.plan.as_ref() else { continue };
        for (field, own, needs) in
            [("width", space.width, child.needs.width), ("depth", space.depth, child.needs.depth)]
        {
            let (own, needs) = (bounds(own, units), bounds(needs, units));
            let sym = units.symbol();
            if needs.min > own.max + 1e-9 {
                out.push(
                    Issue::new(
                        format!("/spaces/{key}/plan/needs/{field}"),
                        format!(
                            "{key}'s plan needs a {field} of at least {} {sym}, but {key} is at most {} {sym}",
                            num(needs.min),
                            num(own.max)
                        ),
                    )
                    .with_hint(format!("Raise {key}'s {field}.max, or lower the {field} its plan needs.")),
                );
            } else if needs.max < own.min - walls - 1e-9 {
                out.push(
                    Issue::new(
                        format!("/spaces/{key}/plan/needs/{field}"),
                        format!(
                            "{key}'s plan takes a {field} of at most {} {sym}, but {key} is at least {} {sym}",
                            num(needs.max),
                            num(own.min)
                        ),
                    )
                    .with_hint(format!("Lower {key}'s {field}.min, or let its plan take more.")),
                );
            }
        }
    }
    out
}

/// Doors this level put between two of its spaces, once the plans inside them are solved: where a
/// plan inside put its own door on that wall (to its 'entrance, say), they are the same door; else
/// the door goes where it opens into circulation on both sides (a gallery, not someone's
/// bedroom), if the wall has such a place.
fn align_doors(sub: &mut Sub, own: usize, slack: f64) {
    let mut dropped = Vec::new();
    for k in 0..own {
        let o = sub.openings[k].clone();
        if o.kind != ElementKind::Door
            || o.between.len() != 2
            || o.between.iter().any(|n| n.starts_with('\''))
        {
            continue;
        }
        let w = o.a.distance(o.b);
        let inside = |n: &str| format!("{n}.");
        let has_plan = |n: &str| sub.spaces.iter().any(|s| s.path.starts_with(&inside(n)));
        if w <= 0.0 || !(has_plan(&o.between[0]) || has_plan(&o.between[1])) {
            continue;
        }
        let u = ((o.b.x - o.a.x) / w, (o.b.y - o.a.y) / w);
        // Walls take floor: a plan inside meets this wall at its face, up to `slack` away.
        let tol = 1e-6 * (1.0 + o.a.x.abs() + o.a.y.abs() + w) + slack;
        let t = |p: Point| (p.x - o.a.x) * u.0 + (p.y - o.a.y) * u.1;
        let off = |p: Point| ((p.x - o.a.x) * u.1 - (p.y - o.a.y) * u.0).abs();
        // Stretches of a polygon's edges on the door's line, as (from, to) along it.
        let on_line = |polys: &[&Vec<Point>]| -> Vec<(f64, f64)> {
            let mut out = Vec::new();
            for poly in polys {
                for i in 0..poly.len() {
                    let (p, q) = (poly[i], poly[(i + 1) % poly.len()]);
                    if off(p) < tol && off(q) < tol {
                        out.push((t(p).min(t(q)), t(p).max(t(q))));
                    }
                }
            }
            out
        };
        let space_line = |s: &SubSpace| {
            let mut polys = vec![&s.polygon];
            polys.extend(s.holes.iter());
            on_line(&polys)
        };
        let wall_of = |n: &str| sub.spaces.iter().find(|s| s.path == n).map(space_line).unwrap_or_default();
        // The shared wall: the stretch of both spaces' edges that holds the door.
        let mid = w / 2.0;
        let around = |v: Vec<(f64, f64)>| v.into_iter().find(|&(a, b)| a - tol <= mid && mid <= b + tol);
        let (Some(wa), Some(wb)) = (around(wall_of(&o.between[0])), around(wall_of(&o.between[1]))) else {
            continue;
        };
        let (lo, hi) = (wa.0.max(wb.0), wa.1.min(wb.1));
        if hi - lo < w - tol {
            continue;
        }
        let place = |from: f64| {
            let from = from.min(hi - w).max(lo);
            (from, from + w)
        };
        let mut moved: Option<(f64, f64)> = None;

        // The same door, put there by a plan inside.
        for (j, c) in sub.openings.iter().enumerate().skip(own) {
            let ours = o.between.iter().any(|n| c.path.starts_with(&inside(n)));
            if !ours || c.kind != ElementKind::Door || !c.between.iter().any(|n| n.starts_with('\'')) {
                continue;
            }
            if off(c.a) < tol && off(c.b) < tol {
                let (ca, cb) = (t(c.a).min(t(c.b)), t(c.a).max(t(c.b)));
                if ca >= lo - tol && cb <= hi + tol {
                    if moved.is_none() {
                        moved = Some(place((ca + cb) / 2.0 - w / 2.0));
                    }
                    dropped.push(j);
                }
            }
        }

        // Else: where circulation meets circulation. Each side's spaces along the wall, by kind.
        if moved.is_none() {
            let side = |n: &str| -> Option<Vec<(f64, f64, bool)>> {
                let prefix = inside(n);
                let kids: Vec<(f64, f64, bool)> = sub
                    .spaces
                    .iter()
                    .filter(|s| s.path.strip_prefix(&prefix).is_some_and(|r| !r.contains('.')))
                    .filter(|s| s.info.kind != SpaceKind::Fixture)
                    .flat_map(|s| {
                        let open = matches!(s.info.kind, SpaceKind::Circulation | SpaceKind::Zone);
                        space_line(s).into_iter().map(move |(a, b)| (a, b, open))
                    })
                    .collect();
                (!kids.is_empty()).then_some(kids)
            };
            let sides = [side(&o.between[0]), side(&o.between[1])];
            let mut cuts = vec![lo, hi];
            for (a, b, _) in sides.iter().flatten().flatten() {
                cuts.extend([*a, *b].into_iter().filter(|&c| c > lo && c < hi));
            }
            cuts.sort_by(f64::total_cmp);
            cuts.dedup_by(|a, b| (*a - *b).abs() < tol);
            // A stretch with one space on each side scores a point per side it opens into
            // circulation; a side without a plan is the same everywhere.
            let score = |a: f64, b: f64| -> Option<i32> {
                let mut total = 0;
                for s in &sides {
                    let Some(kids) = s else { continue };
                    let here = kids.iter().find(|&&(x, y, _)| x <= a + tol && b <= y + tol)?;
                    total += here.2 as i32;
                }
                Some(total)
            };
            let now = score(0.0, w).unwrap_or(-1);
            let mut best: Option<(i32, f64, (f64, f64))> = None;
            for pair in cuts.windows(2) {
                let (a, b) = (pair[0], pair[1]);
                if b - a < w - tol {
                    continue;
                }
                let Some(sc) = score(a, b) else { continue };
                let at = place(0.0f64.min(b - w).max(a));
                let dist = at.0.abs();
                if sc > now && best.as_ref().is_none_or(|(bs, bd, _)| sc > *bs || (sc == *bs && dist < *bd)) {
                    best = Some((sc, dist, at));
                }
            }
            moved = best.map(|(_, _, at)| at);
        }
        if let Some((a, b)) = moved {
            let at = |s: f64| Point::new(o.a.x + u.0 * s, o.a.y + u.1 * s);
            sub.openings[k].a = at(a);
            sub.openings[k].b = at(b);
        }
    }
    dropped.sort_unstable();
    dropped.dedup();
    for j in dropped.into_iter().rev() {
        sub.openings.remove(j);
    }
}

/// The side of a placed rectangle with the most `entrance` on it, as a front (space on its
/// left), in the level's frame.
fn entered_from(placed: &Placed) -> Option<[Point; 2]> {
    let (w, h, to_level) = placed.rect.as_ref()?;
    let corners = Rect::new(0.0, 0.0, *w, *h).corners();
    let tol = 1e-6 * (1.0 + w + h);
    let (best, len) = (0..4)
        .map(|k| {
            (k, ports::shared_with_role(corners[k], corners[(k + 1) % 4], &placed.ports, "entrance", tol))
        })
        .max_by(|a, b| a.1.total_cmp(&b.1))?;
    (len > tol).then(|| move_front([corners[best], corners[(best + 1) % 4]], to_level))
}

/// The element that draws symbol `name` in a space: along its front (or its first edge), as
/// deep as the space reaches from there.
fn symbol_element(
    path: &str,
    source: &str,
    name: &str,
    polygon: &[Point],
    front: Option<[Point; 2]>,
) -> Option<Element> {
    let [a, b] = front.or_else(|| (polygon.len() >= 3).then(|| [polygon[0], polygon[1]]))?;
    let len = a.distance(b);
    if len <= 0.0 {
        return None;
    }
    let (nx, ny) = (-(b.y - a.y) / len, (b.x - a.x) / len);
    let depth = polygon.iter().map(|p| (p.x - a.x) * nx + (p.y - a.y) * ny).fold(0.0, f64::max);
    let paths = crate::symbols::draw(name, a, b, depth)?;
    let frame = vec![
        a,
        b,
        Point::new(b.x + nx * depth, b.y + ny * depth),
        Point::new(a.x + nx * depth, a.y + ny * depth),
    ];
    let mut e = Element::new(format!("symbol:{path}"), ElementKind::Symbol, frame, true);
    e.source = Some(format!("{source}/symbol"));
    e.symbol = Some(SymbolInfo { name: name.to_owned(), paths });
    Some(e)
}

/// What a plan and every plan inside it draw that doesn't change a solution, as a key: symbols,
/// how assemblies are shown, what services place and connect. A cached solution must still draw
/// the current ones.
fn view_key(plan: &Plan) -> String {
    fn walk(plan: &Plan, out: &mut String) {
        if !plan.assemblies.is_empty() || !plan.services.is_empty() {
            out.push_str(&format!("{:?}{:?};", plan.assemblies, plan.services));
        }
        for (key, space) in &plan.spaces {
            if let Some(symbol) = &space.symbol {
                out.push_str(&format!("{key}={symbol};"));
            }
            if !space.load.is_empty() {
                out.push_str(&format!("{key}~{:?};", space.load));
            }
            if let Some(child) = &space.plan {
                out.push_str(&format!("{key}{{"));
                walk(child, out);
                out.push('}');
            }
        }
        for (ptr, rule) in plan.rules() {
            if let Rule::Instance { plan, .. } = rule {
                out.push_str(&format!("{ptr}{{"));
                walk(plan, out);
                out.push('}');
            }
        }
    }
    let mut out = String::new();
    walk(plan, &mut out);
    if out.is_empty() {
        out
    } else {
        format!("{:016x}", hash64(&[&out]))
    }
}

/// An opening put in the wall it's on: on that wall's centreline, as deep as the wall. Plans
/// inside a space put their doors and windows on its clear edge, a wall's half-thickness away.
fn in_wall(a: Point, b: Point, walls: &[(Point, Point, f64)]) -> (Point, Point, Option<f64>) {
    let len = a.distance(b);
    if len <= 0.0 {
        return (a, b, None);
    }
    let u = ((b.x - a.x) / len, (b.y - a.y) / len);
    let mut best: Option<(f64, Point, Point, f64)> = None;
    for &(p, q, thick) in walls {
        let wl = p.distance(q);
        if wl <= 0.0 {
            continue;
        }
        let v = ((q.x - p.x) / wl, (q.y - p.y) / wl);
        if (u.0 * v.1 - u.1 * v.0).abs() > 1e-6 {
            continue;
        }
        let off = |r: Point| ((r.x - p.x) * v.1 - (r.y - p.y) * v.0).abs();
        let d = off(a).max(off(b));
        if d > thick * 0.75 + 1e-6 {
            continue;
        }
        let t = |r: Point| (r.x - p.x) * v.0 + (r.y - p.y) * v.1;
        let (ta, tb) = (t(a), t(b));
        if ta.max(tb) < 1e-6 || ta.min(tb) > wl - 1e-6 {
            continue;
        }
        if best.as_ref().is_none_or(|x| d < x.0) {
            let at = |s: f64| Point::new(p.x + v.0 * s, p.y + v.1 * s);
            best = Some((d, at(ta), at(tb), thick));
        }
    }
    match best {
        Some((_, a, b, t)) => (a, b, Some(t)),
        None => (a, b, None),
    }
}

/// A polygon moved by `t`, kept counter-clockwise when `t` mirrors.
fn move_polygon(polygon: &[Point], t: &Transform) -> Vec<Point> {
    let mut out: Vec<Point> = polygon.iter().map(|p| t.apply(*p)).collect();
    if t.is_mirror() {
        out.reverse();
    }
    out
}

/// A fixture's front moved by `t`, still with the fixture on its left.
fn move_front([a, b]: [Point; 2], t: &Transform) -> [Point; 2] {
    let (a, b) = (t.apply(a), t.apply(b));
    if t.is_mirror() {
        [b, a]
    } else {
        [a, b]
    }
}

impl SubIssue {
    fn required(pointer: impl Into<String>, message: impl Into<String>) -> SubIssue {
        SubIssue {
            pointer: pointer.into(),
            message: message.into(),
            places: Vec::new(),
            level: Level::Required,
            hint: None,
        }
    }

    fn at(mut self, places: Vec<String>) -> SubIssue {
        self.places = places;
        self
    }

    fn hint(mut self, hint: Option<String>) -> SubIssue {
        self.hint = hint;
        self
    }
}

impl From<&Issue> for SubIssue {
    fn from(i: &Issue) -> SubIssue {
        SubIssue {
            pointer: i.pointer.clone(),
            message: i.message.clone(),
            places: Vec::new(),
            level: i.level,
            hint: i.hint.clone(),
        }
    }
}

/// Add an issue, merging repeats (the same problem in every copy of a typical).
fn push_issue(issues: &mut Vec<SubIssue>, issue: SubIssue) {
    if let Some(existing) =
        issues.iter_mut().find(|i| i.pointer == issue.pointer && i.message == issue.message)
    {
        for place in issue.places {
            if !existing.places.contains(&place) {
                existing.places.push(place);
            }
        }
    } else {
        issues.push(issue);
    }
}

/// What to change when a space doesn't give its plan what it `needs`.
fn needs_hint(pointer: &str) -> Option<String> {
    let hint = if pointer.starts_with("/needs/ports") {
        "The space it's in doesn't have that edge: change the plan's `needs.ports`, or the rules that place the space (a door, a window, `start.ports`)."
    } else {
        "Size the space to fit (its width, depth or area), or loosen the plan's `needs`."
    };
    Some(hint.to_owned())
}

/// The summary of a solution: perfect, solved with something missed, or incomplete.
fn report(diagnostics: &[Diagnostic], leftover: f64, floor: f64, spaces: usize) -> Report {
    let count = |code: Code| diagnostics.iter().filter(|d| d.code == Some(code)).count();
    let (unmet, preferences) = (count(Code::Unmet), count(Code::Preference));
    let errors = diagnostics.iter().any(|d| d.severity == crate::diagnostic::Severity::Error);
    let status = if spaces == 0 && unmet == 0 {
        Status::Empty
    } else if unmet > 0 || errors {
        Status::Incomplete
    } else if preferences > 0 || leftover > 1e-9 {
        Status::Solved
    } else {
        Status::Perfect
    };
    Report { status, unmet, preferences, leftover, floor, spaces }
}

/// The floor of a level no space covers: drawn, and reported with what would take it.
fn leftover(
    plan: &Plan,
    boundary: &[Point],
    holes: &[Vec<Point>],
    level: &LevelResult,
    ctx: &Ctx,
    sub: &mut Sub,
) {
    let (mut min_x, mut min_y, mut max_x, mut max_y) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for p in boundary {
        min_x = min_x.min(p.x);
        min_y = min_y.min(p.y);
        max_x = max_x.max(p.x);
        max_y = max_y.max(p.y);
    }
    let (w, h) = (max_x - min_x, max_y - min_y);
    let mut step = ctx.grid.max(1e-9);
    while (w / step) * (h / step) > 16_000.0 {
        step *= 2.0;
    }
    let (nx, ny) = (((w / step).round() as i32).max(1), ((h / step).round() as i32).max(1));
    let (gx, gy) = (w / nx as f64, h / ny as f64);
    let centre = |i: i32, j: i32| Point::new(min_x + (i as f64 + 0.5) * gx, min_y + (j as f64 + 0.5) * gy);
    // A cell centre on a space's edge counts as covered: edges fall on cell centres when the
    // spaces' sizes aren't whole cells.
    let eps = 1e-6 * (gx + gy);
    let on_edge = |p: Point, poly: &[Point]| {
        (0..poly.len())
            .any(|k| crate::geometry::point_segment_distance(p, poly[k], poly[(k + 1) % poly.len()]) < eps)
    };
    let covered_by = |p: Point| {
        level.placed.iter().position(|s| {
            on_edge(p, &s.polygon)
                || (grid::point_in_polygon(p, &s.polygon)
                    && !s.holes.iter().any(|h| grid::point_in_polygon(p, h)))
        })
    };
    let mut owner = vec![None; (nx * ny) as usize];
    let mut open = vec![false; (nx * ny) as usize];
    for j in 0..ny {
        for i in 0..nx {
            let c = centre(i, j);
            let k = (j * nx + i) as usize;
            if grid::point_in_polygon(c, boundary) && !holes.iter().any(|h| grid::point_in_polygon(c, h)) {
                owner[k] = covered_by(c);
                open[k] = owner[k].is_none();
            }
        }
    }
    let is_open = |i: i32, j: i32| i >= 0 && j >= 0 && i < nx && j < ny && open[(j * nx + i) as usize];
    let regions = outline::regions(nx, ny, is_open);
    if regions.is_empty() {
        return;
    }
    // Spaces next to the floor left over: they're the ones that could take it.
    let mut next_to: Vec<usize> = Vec::new();
    for j in 0..ny {
        for i in 0..nx {
            if !is_open(i, j) {
                continue;
            }
            for (u, v) in [(i + 1, j), (i - 1, j), (i, j + 1), (i, j - 1)] {
                if u >= 0 && v >= 0 && u < nx && v < ny {
                    if let Some(o) = owner[(v * nx + u) as usize] {
                        if !next_to.contains(&o) {
                            next_to.push(o);
                        }
                    }
                }
            }
        }
    }
    let to = |l: &[(i32, i32)]| -> Vec<Point> {
        l.iter().map(|&(i, j)| Point::new(min_x + i as f64 * gx, min_y + j as f64 * gy)).collect()
    };
    let mut area = 0.0;
    for (k, r) in regions.iter().enumerate() {
        let a = r.cells as f64 * gx * gy;
        area += a;
        sub.leftover.push(SubLeftover {
            path: format!("{k}"),
            polygon: to(&r.outer),
            holes: r.holes.iter().map(|h| to(h)).collect(),
            area: a,
            source: "/spaces".into(),
        });
    }
    let sym = ctx.units.symbol();
    let pieces = if regions.len() > 1 { format!(", in {} places", regions.len()) } else { String::new() };
    let slot_area = crate::geometry::signed_area(boundary).abs();
    push_issue(
        &mut sub.issues,
        SubIssue {
            pointer: "/spaces".into(),
            message: format!("{} {sym}² of floor isn't part of any space{pieces}", num(area)),
            places: Vec::new(),
            level: Level::Leftover,
            hint: Some(leftover_hint(plan, level, &next_to, ctx, area, slot_area)),
        },
    );
}

/// Why the spaces around some floor left over didn't take it, and what would.
fn leftover_hint(
    plan: &Plan,
    level: &LevelResult,
    next_to: &[usize],
    ctx: &Ctx,
    area: f64,
    slot: f64,
) -> String {
    let units = ctx.units;
    let sym = units.symbol();
    let types = common::SpaceType::all(plan, units);
    let type_of = |key: &str| types.iter().find(|t| t.key == key);
    let mut parts = Vec::new();

    // Can the program cover the floor at all?
    let mut most = 0.0;
    let mut bounded = true;
    for t in types.iter().filter(|t| !t.space.is_fixture()) {
        let count = t.max.map(|m| m as f64);
        let one = if t.area.max.is_finite() {
            Some(t.area.max)
        } else if t.width.max.is_finite() && t.depth.max.is_finite() {
            Some(t.width.max * t.depth.max)
        } else {
            None
        };
        match (count, one) {
            (Some(c), Some(a)) => most += c * a,
            _ => bounded = false,
        }
    }
    if bounded && most < slot - area * 0.5 {
        parts.push(format!(
            "At their largest the spaces cover {} {sym}² of the {} {sym}² here: raise some area.max (or count), or add a space.",
            num(most),
            num(slot)
        ));
    }

    let mut blocked = Vec::new();
    let mut could = Vec::new();
    for &k in next_to {
        let p = &level.placed[k];
        let Some(t) = type_of(&p.type_key) else { continue };
        if p.fixture.is_some() || t.space.fill {
            continue;
        }
        let a = crate::geometry::signed_area(&p.polygon).abs();
        let (w, h) = p.rect.as_ref().map_or((0.0, 0.0), |r| (r.0, r.1));
        let at_max = |limit: f64, v: f64| limit.is_finite() && v >= limit * 0.98;
        let reason = if !t.pins.is_empty() {
            Some("is fixed in place".to_owned())
        } else if at_max(t.area.max, a) {
            Some(format!("is at its area.max ({} {sym}²)", num(t.area.max)))
        } else if at_max(t.side.max, w.max(h)) {
            Some(format!("is at its side.max ({})", num(t.side.max)))
        } else if at_max(t.width.max, w.min(h)) || at_max(t.depth.max, w.max(h)) {
            Some("is at its largest width or depth".to_owned())
        } else if t.aspect.is_finite() && w.max(h) / w.min(h).max(1e-9) >= t.aspect * 0.98 {
            Some(format!("is as long as its aspect allows ({})", num(t.aspect)))
        } else {
            None
        };
        match reason {
            Some(r) => blocked.push(format!("{} {r}", p.name)),
            None => could.push(p.name.clone()),
        }
    }
    if !blocked.is_empty() {
        parts.push(format!("Next to it, {}: raise that limit to let it grow.", join_and(&blocked)));
    }
    if !could.is_empty() {
        parts.push(format!(
            "{} could grow into it, but not as a rectangle: try another seed, or loosen sizes nearby.",
            join_and(&could)
        ));
    }
    match types.iter().find(|t| t.space.fill) {
        Some(f) => parts.push(format!("It's cut off from {}: leave {} a way through to it.", f.key, f.key)),
        None => parts.push(
            "Or let one space take whatever is left: `fill = true` on a hall or living room (any shape)."
                .to_owned(),
        ),
    }
    parts.join(" ")
}

fn join_and(items: &[String]) -> String {
    match items.len() {
        0 => String::new(),
        1 => items[0].clone(),
        n => format!("{} and {}", items[..n - 1].join(", "), items[n - 1]),
    }
}

/// `reach` rules: going from `a` to `b` through doors must pass through `via`.
fn reach_issues(plan: &Plan, level: &LevelResult, sub: &mut Sub) {
    let names: Vec<(String, String)> =
        level.placed.iter().map(|p| (p.name.clone(), p.type_key.clone())).collect();
    let index = |n: &str| names.iter().position(|(name, _)| name == n);
    let doors: Vec<(usize, usize)> = level
        .openings
        .iter()
        .filter(|o| o.kind == ElementKind::Door && o.between.len() == 2)
        .filter_map(|o| Some((index(&o.between[0])?, index(&o.between[1])?)))
        .collect();
    for (ptr, rule) in plan.rules() {
        let Rule::Reach { a, b, via, weight } = rule else { continue };
        if weight.is_some() {
            continue;
        }
        let pick = |s: &crate::plan::Selector| -> Vec<usize> {
            names.iter().enumerate().filter(|(_, (n, k))| s.matches(n, k)).map(|(i, _)| i).collect()
        };
        let (aa, bb, vv) = (pick(a), pick(b), pick(via));
        for &from in &aa {
            // Everything reachable from `from` without passing through `via`.
            let mut seen = vec![false; names.len()];
            let mut stack = vec![from];
            seen[from] = true;
            while let Some(i) = stack.pop() {
                for &(x, y) in &doors {
                    let next = if x == i {
                        y
                    } else if y == i {
                        x
                    } else {
                        continue;
                    };
                    if !seen[next] && !vv.contains(&next) {
                        seen[next] = true;
                        stack.push(next);
                    }
                }
            }
            for &to in &bb {
                if to != from && seen[to] {
                    let via: Vec<&str> = vv.iter().map(|&i| names[i].0.as_str()).collect();
                    push_issue(
                        &mut sub.issues,
                        SubIssue::required(
                            origin(plan, &ptr),
                            format!(
                                "{} reaches {} without going through {}",
                                names[from].0,
                                names[to].0,
                                via.join(" or ")
                            ),
                        )
                        .hint(Some(format!(
                            "A door or opening skips {}: make the door rules that bypass it preferences, or try another seed.",
                            via.join(" or ")
                        ))),
                    );
                }
            }
        }
    }
}

/// Pick the frame for a child slot: entrance along the bottom if it has one, then the
/// orientation with the smallest description, so equivalent slots end up identical.
fn canonical(w: f64, h: f64, ports: &[PortSeg], mirror: bool, mm: f64) -> (Transform, f64, f64) {
    let candidates = Transform::rect_symmetries(w, h, mirror);
    let scored: Vec<(bool, String, (Transform, f64, f64))> = candidates
        .into_iter()
        .map(|(t, cw, ch)| {
            let moved: Vec<PortSeg> = ports.iter().map(|p| p.transformed(&t)).collect();
            let entrance_bottom = moved.iter().any(|p| {
                p.has("entrance") && p.a.y.abs() < 1e-6 * (1.0 + ch) && p.b.y.abs() < 1e-6 * (1.0 + ch)
            });
            (entrance_bottom, signature(cw, ch, &moved, mm), (t, cw, ch))
        })
        .collect();
    let any_bottom = scored.iter().any(|s| s.0);
    scored.into_iter().filter(|s| s.0 == any_bottom).min_by(|a, b| a.1.cmp(&b.1)).map(|s| s.2).unwrap_or((
        Transform::IDENTITY,
        w,
        h,
    ))
}

/// Size and ports, rounded, as text: equal signatures are interchangeable slots.
fn signature(w: f64, h: f64, ports: &[PortSeg], mm: f64) -> String {
    let r = |v: f64| (v * mm).round() as i64;
    let mut parts: Vec<String> = ports
        .iter()
        .map(|p| {
            let (a, b) = if (r(p.a.x), r(p.a.y)) <= (r(p.b.x), r(p.b.y)) { (p.a, p.b) } else { (p.b, p.a) };
            let mut roles = p.roles.clone();
            roles.sort();
            format!("{}@{},{}-{},{}", roles.join("+"), r(a.x), r(a.y), r(b.x), r(b.y))
        })
        .collect();
    parts.sort();
    format!("{}x{}|{}", r(w), r(h), parts.join(";"))
}

/// Does the slot give the plan what it `needs`? Returns (pointer within the plan, message).
fn check_needs(plan: &Plan, w: f64, h: f64, ports: &[PortSeg], units: Units) -> Result<(), (String, String)> {
    let needs = &plan.needs;
    let tol = 1e-6 * (1.0 + w + h);
    let width = bounds(needs.width, units);
    if !width.contains(w, tol) {
        return Err((
            "/needs/width".into(),
            format!("it is {} wide; its plan needs {}", num(w), range(width)),
        ));
    }
    let depth = bounds(needs.depth, units);
    if !depth.contains(h, tol) {
        return Err((
            "/needs/depth".into(),
            format!("it is {} deep; its plan needs {}", num(h), range(depth)),
        ));
    }
    let area = crate::plan::area_bounds(needs.area);
    if !area.contains(w * h, tol) {
        return Err((
            "/needs/area".into(),
            format!("its area is {}; its plan needs {}", num(w * h), range(area)),
        ));
    }
    for (role, need) in &needs.ports {
        let has = ports.iter().any(|p| p.roles.iter().any(|r| role_matches(r, role)));
        match need {
            PortNeed::Required if !has => {
                return Err((
                    format!("/needs/ports/{role}"),
                    format!("it has no '{role} port, which its plan requires"),
                ))
            }
            PortNeed::Forbidden if has => {
                return Err((
                    format!("/needs/ports/{role}"),
                    format!("it has a '{role} port, which its plan forbids"),
                ))
            }
            _ => {}
        }
    }
    Ok(())
}

fn range(b: crate::plan::Bounds1) -> String {
    match (b.min > 0.0, b.max.is_finite()) {
        (true, true) if (b.max - b.min).abs() < 1e-9 => num(b.min),
        (true, true) => format!("{} to {}", num(b.min), num(b.max)),
        (true, false) => format!("at least {}", num(b.min)),
        (false, true) => format!("at most {}", num(b.max)),
        _ => "anything".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::Bounds;
    use serde_json::{json, Value};

    fn run(v: Value) -> Solution {
        solve(&Config::from_value(v).unwrap())
    }

    fn spaces(s: &Solution) -> Vec<&Element> {
        s.layout.elements.iter().filter(|e| e.kind == ElementKind::Space).collect()
    }

    fn room(extra: Value) -> Value {
        let mut plan = json!({
            "needs": { "width": { "min": 3.4 }, "ports": { "entrance": "required" } },
            "spaces": {
                "entry": { "kind": "circulation", "area": { "min": 3, "max": 6 }, "side": { "min": 1.2 } },
                "bath": { "area": { "min": 4, "max": 6 }, "side": { "min": 1.6 } },
                "sleep": { "area": { "min": 14 } }
            },
            "rules": [
                { "rule": "door", "a": "entry", "port": "entrance" },
                { "rule": "door", "a": ["bath", "sleep"], "b": "entry" },
                { "rule": "window", "a": "sleep" }
            ]
        });
        if let (Some(p), Some(e)) = (plan.as_object_mut(), extra.as_object()) {
            for (k, v) in e {
                p.insert(k.clone(), v.clone());
            }
        }
        plan
    }

    fn hotel(rules: Value, room_plan: Value) -> Value {
        json!({
            "start": { "mode": "spine", "points": [[0, 0], [40, 0]] },
            "spaces": {
                "corridor": { "kind": "circulation", "width": 1.8 },
                "room": { "count": { "min": 1 }, "width": { "min": 3.6, "max": 4.2 }, "depth": 7.5, "plan": room_plan },
            },
            "rules": rules
        })
    }

    fn apartment() -> Value {
        json!({
            "start": { "mode": "boundary", "points": [[0, 0], [14, 0], [14, 9], [0, 9]], "ports": ["facade", "blank", "facade", "blank"] },
            "spaces": {
                "living": { "area": { "min": 24 } },
                "kitchen": { "area": { "min": 9, "max": 14 } },
                "hall": { "kind": "circulation", "area": { "min": 6, "max": 12 }, "side": { "min": 1.2 } },
                "bedroom": { "count": 2, "area": { "min": 11, "max": 16 }, "side": { "min": 2.8 } },
                "bath": { "area": { "min": 4.5, "max": 7 }, "side": { "min": 1.8 } }
            },
            "rules": [
                { "rule": "touch", "a": "kitchen", "b": "living" },
                { "rule": "door", "a": ["living", "bedroom", "bath"], "b": "hall" },
                [{ "rule": "touch", "a": "bedroom", "b": "hall" }, { "rule": "apart", "a": "bedroom", "b": "bedroom" }],
                { "rule": "window", "a": ["bedroom", "living"] }
            ]
        })
    }

    #[test]
    fn grid_solves_an_apartment_inside_its_boundary() {
        // The search is stochastic: some seed among the first few must meet every rule.
        let s = (1..=3)
            .map(|seed| {
                let mut v = apartment();
                v["seed"] = json!(seed);
                run(v)
            })
            .find(|s| s.report.unmet == 0)
            .expect("no seed met every rule");
        let sp = spaces(&s);
        assert_eq!(sp.len(), 6);
        let rects: Vec<Rect> = sp
            .iter()
            .map(|e| {
                let b = crate::geometry::Bounds::from_points(&e.points);
                Rect::new(b.min_x, b.min_y, b.max_x, b.max_y)
            })
            .collect();
        for (i, a) in rects.iter().enumerate() {
            assert!(a.x0 >= -1e-9 && a.y0 >= -1e-9 && a.x1 <= 14.0 + 1e-9 && a.y1 <= 9.0 + 1e-9, "{a:?}");
            for b in &rects[i + 1..] {
                let ox = (a.x1.min(b.x1) - a.x0.max(b.x0)).max(0.0);
                let oy = (a.y1.min(b.y1) - a.y0.max(b.y0)).max(0.0);
                assert!(ox * oy < 1e-9, "{a:?} overlaps {b:?}");
            }
        }
        let doors = s.layout.elements.iter().filter(|e| e.kind == ElementKind::Door).count();
        assert_eq!(doors, 4);
        let names: Vec<&str> = sp.iter().map(|e| e.space.as_ref().unwrap().name.as_str()).collect();
        assert!(names.contains(&"bedroom_1") && names.contains(&"bedroom_2"), "{names:?}");
    }

    #[test]
    fn solving_is_deterministic() {
        let a = serde_json::to_string(&run(apartment()).layout).unwrap();
        let b = serde_json::to_string(&run(apartment()).layout).unwrap();
        assert_eq!(a, b);
        let mut other = apartment();
        other["seed"] = json!(7);
        assert_ne!(a, serde_json::to_string(&run(other).layout).unwrap());
    }

    #[test]
    fn repeated_rooms_share_a_typical() {
        let s = run(hotel(json!([{ "rule": "along", "a": "room", "b": "corridor" }]), room(json!({}))));
        assert!(s.report.unmet == 0, "{:?}", s.diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>());
        let rooms: Vec<&SpaceInfo> =
            spaces(&s).iter().filter_map(|e| e.space.as_ref()).filter(|i| i.type_key == "room").collect();
        assert!(rooms.len() >= 18, "{}", rooms.len());
        let mut groups: HashMap<String, usize> = HashMap::new();
        for r in &rooms {
            *groups.entry(r.typical.clone().unwrap_or_default()).or_default() += 1;
        }
        let biggest = groups.iter().filter(|(k, _)| !k.is_empty()).map(|(_, n)| *n).max().unwrap_or(0);
        assert!(biggest >= 8, "{groups:?}");
        // Every room has its interior, named under it.
        assert!(spaces(&s).iter().any(|e| e.id == "room_3.bath"));
        // Doors into rooms are on the corridor side (the entrance port).
        let entry_doors = s
            .layout
            .elements
            .iter()
            .filter(|e| e.kind == ElementKind::Door && e.id.ends_with("'entrance"))
            .count();
        assert_eq!(entry_doors, rooms.len());
    }

    #[test]
    fn rooms_are_numbered_along_the_spine() {
        let s = run(hotel(json!([{ "rule": "along", "a": "room", "b": "corridor" }]), room(json!({}))));
        let x = |name: &str| {
            let e = spaces(&s).into_iter().find(|e| e.id == name).unwrap();
            e.points.iter().map(|p| p.x).fold(f64::MAX, f64::min)
        };
        assert!(x("room_1") < x("room_3") && x("room_3") < x("room_5"));
    }

    #[test]
    fn unique_and_instance_take_rooms_out_of_their_typical() {
        let mut other_room = room(json!({}));
        other_room["spaces"]["bath"]["area"] = json!({ "min": 5, "max": 7 });
        let rules = json!([
            { "rule": "along", "a": "room", "b": "corridor" },
            { "rule": "unique", "a": "room_5" },
            { "rule": "instance", "a": "room_6", "plan": other_room },
        ]);
        let s = run(hotel(rules, room(json!({}))));
        let info = |name: &str| spaces(&s).into_iter().find(|e| e.id == name).unwrap().space.clone().unwrap();
        assert!(info("room_5").unique && info("room_5").typical.is_none());
        assert!(info("room_6").unique);
        let bath = spaces(&s).into_iter().find(|e| e.id == "room_6.bath").unwrap();
        assert!(bath.area.unwrap() >= 5.0 - 1e-9);
    }

    #[test]
    fn needs_reject_slots_that_dont_fit() {
        let mut v = hotel(json!([{ "rule": "along", "a": "room", "b": "corridor" }]), room(json!({})));
        v["spaces"]["room"]["width"] = json!({ "min": 3, "max": 3.2 });
        v["spaces"]["room"]["plan"]["needs"]["width"] = json!({ "min": 3.4 });
        // The engine sizes bays to the plan's needs, so make them conflict outright.
        let s = run(v);
        assert!(
            s.diagnostics.iter().any(|d| d.pointer.as_deref() == Some("/spaces/room/plan/needs/width")
                || d.pointer.as_deref() == Some("/spaces/room/count")),
            "{:?}",
            s.diagnostics.iter().map(|d| (&d.pointer, &d.message)).collect::<Vec<_>>()
        );
    }

    #[test]
    fn reach_reports_shortcuts() {
        let mut v = apartment();
        v["rules"] = json!([
            { "rule": "door", "a": "bedroom_1", "b": "bedroom_2" },
            { "rule": "door", "a": "bedroom", "b": "hall" },
            { "rule": "reach", "a": "bedroom", "b": "bedroom", "via": "hall" },
        ]);
        let s = run(v);
        assert!(
            s.diagnostics.iter().any(|d| d.pointer.as_deref() == Some("/rules/2")
                && d.message.contains("without going through hall")),
            "{:?}",
            s.diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn free_mode_packs_around_the_origin() {
        let s = run(json!({
            "start": { "mode": "spine", "points": [[100, 50]] },
            "spaces": { "a": { "area": { "min": 20 } }, "b": { "area": { "min": 12 } } },
            "rules": [{ "rule": "touch", "a": "a", "b": "b" }]
        }));
        assert!(s.report.unmet == 0, "{:?}", s.diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>());
        for e in spaces(&s) {
            assert!(e.points.iter().all(|p| p.distance(Point::new(100.0, 50.0)) < 20.0));
        }
    }

    fn plan_file(extra: Value) -> Value {
        let mut plan = room(json!({}));
        plan["needs"] = json!({ "width": { "min": 3.4, "max": 4.5 }, "depth": { "min": 7 }, "ports": { "entrance": "required", "facade": "required" } });
        if let (Some(p), Some(e)) = (plan.as_object_mut(), extra.as_object()) {
            for (k, v) in e {
                p.insert(k.clone(), v.clone());
            }
        }
        plan
    }

    fn run_view(v: Value, view: &View) -> Solution {
        solve_view(&Config::from_value(v).unwrap(), view)
    }

    fn messages(s: &Solution) -> Vec<(Option<String>, String)> {
        s.diagnostics.iter().map(|d| (d.pointer.clone(), d.message.clone())).collect()
    }

    #[test]
    fn a_plan_file_is_previewed_in_a_slot_sized_from_its_needs() {
        let s = run(plan_file(json!({})));
        assert!(s.report.unmet == 0, "{:?}", messages(&s));
        let slot = &s.layout.elements[0];
        assert_eq!(slot.kind, ElementKind::Slot);
        assert_eq!(slot.source.as_deref(), Some("/preview"));
        // Width 3.4 to 4.5 aims for the middle, depth at least 7 a bit over; both on the grid.
        let b = crate::geometry::Bounds::from_points(&slot.points);
        assert_eq!((b.max_x - b.min_x, b.max_y - b.min_y), (4.0, 7.75));
        let ports: Vec<_> = s.layout.elements.iter().filter(|e| e.kind == ElementKind::Port).collect();
        assert_eq!(ports.len(), 4);
        assert_eq!(ports[0].roles.as_deref(), Some(&["entrance".to_owned()][..]));
        let names: Vec<&str> = spaces(&s).iter().map(|e| e.id.as_str()).collect();
        assert!(names.contains(&"bath") && names.contains(&"sleep"), "{names:?}");
        assert!(spaces(&s).iter().all(|e| e.space.as_ref().unwrap().depth == 0));
        // The entrance door is on the bottom edge.
        let door = s
            .layout
            .elements
            .iter()
            .find(|e| e.kind == ElementKind::Door && e.id.ends_with("'entrance"))
            .unwrap();
        assert!(door.points.iter().all(|p| p.y.abs() < 1e-9), "{:?}", door.points);
    }

    #[test]
    fn preview_cases_set_size_ports_and_units() {
        let v = plan_file(json!({ "preview": [
            { "name": "standard", "width": 3.8, "depth": 8.2 },
            { "name": "corner", "width": "13 ft", "depth": "27 ft", "system": "imperial", "ports": { "left": "facade", "right": ["party", "wet"] } },
        ] }));
        let first = run(v.clone());
        let b = crate::geometry::Bounds::from_points(&first.layout.elements[0].points);
        assert!((b.max_x - 3.8).abs() < 1e-9 && (b.max_y - 8.2).abs() < 1e-9);
        let corner = run_view(v, &View::Case(1));
        assert_eq!(corner.layout.units, Units::Ft);
        assert_eq!(corner.layout.elements[0].source.as_deref(), Some("/preview/1"));
        let left = corner.layout.elements.iter().find(|e| e.id == "port:3").unwrap();
        assert_eq!(left.roles.as_deref(), Some(&["facade".to_owned()][..]));
        // Imperial: the plan's bare numbers are feet now, so 3.4 to 4.5 wide doesn't fit 13 ft.
        assert!(
            corner.diagnostics.iter().any(|d| d.pointer.as_deref() == Some("/needs/width")),
            "{:?}",
            messages(&corner)
        );
    }

    #[test]
    fn a_slot_that_misses_the_needs_says_why() {
        let s = run(plan_file(json!({ "preview": { "width": 3.0, "depth": 8 } })));
        let d = &s.diagnostics[0];
        assert_eq!(d.pointer.as_deref(), Some("/needs/width"));
        assert!(d.message.starts_with("The preview slot: it is 3 wide"), "{}", d.message);
        assert_eq!(spaces(&s).len(), 0);
    }

    #[test]
    fn a_slot_from_a_plate_gives_the_same_solution() {
        let plate =
            run(hotel(json!([{ "rule": "along", "a": "room", "b": "corridor" }]), plan_file(json!({}))));
        let room = spaces(&plate).into_iter().find(|e| e.id == "room_3").unwrap();
        let info = room.space.as_ref().unwrap();
        let slot = info.slot.clone().expect("rooms with a plan carry their slot");
        assert!(slot.ports.iter().any(|p| p.roles == ["entrance"] && p.a.y == 0.0 && p.b.y == 0.0));

        // The plan file alone, in that slot: same spaces, same sizes, nothing to report.
        let alone = run_view(plan_file(json!({ "preview": { "width": 9, "depth": 9 } })), &View::Slot(slot));
        assert_eq!(alone.report.unmet, 0, "{:?}", messages(&alone));
        let inside = |s: &Solution, prefix: &str| {
            let mut v: Vec<(String, i64)> = spaces(s)
                .iter()
                .filter_map(|e| Some((e.id.strip_prefix(prefix)?.to_owned(), (e.area? * 1e6).round() as i64)))
                .collect();
            v.sort();
            v
        };
        let there = inside(&plate, "room_3.");
        assert!(!there.is_empty());
        assert_eq!(inside(&alone, ""), there);
        assert_eq!(alone.layout.elements[0].source, None, "a plate's slot isn't the file's to resize");
    }

    #[test]
    fn a_changed_plan_is_flagged_in_its_slot() {
        let plate =
            run(hotel(json!([{ "rule": "along", "a": "room", "b": "corridor" }]), plan_file(json!({}))));
        let slot = spaces(&plate)
            .into_iter()
            .find(|e| e.id == "room_3")
            .unwrap()
            .space
            .clone()
            .unwrap()
            .slot
            .unwrap();
        let mut other = plan_file(json!({}));
        other["spaces"]["sleep"]["area"] = json!({ "min": 15 });
        let alone = run_view(other, &View::Slot(slot));
        assert!(alone.diagnostics.iter().any(|d| d.severity == crate::Severity::Info));
    }

    #[test]
    fn auto_preview_respects_forbidden_and_required_ports() {
        let s = run(plan_file(
            json!({ "needs": { "ports": { "party": "forbidden", "outdoor": "required", "entrance": "required" } } }),
        ));
        assert!(
            s.diagnostics.iter().all(|d| d.pointer.as_deref() != Some("/needs/ports/party")),
            "{:?}",
            messages(&s)
        );
        let roles: Vec<Vec<String>> = s.layout.elements.iter().filter_map(|e| e.roles.clone()).collect();
        assert!(roles.iter().flatten().all(|r| r != "party"), "{roles:?}");
        assert!(roles[2].contains(&"outdoor".to_owned()), "{roles:?}");
    }

    #[test]
    fn nested_spaces_are_one_level_deeper() {
        let s = run(hotel(json!([{ "rule": "along", "a": "room", "b": "corridor" }]), room(json!({}))));
        let depth =
            |id: &str| spaces(&s).into_iter().find(|e| e.id == id).unwrap().space.as_ref().unwrap().depth;
        assert_eq!(depth("room_3"), 0);
        assert_eq!(depth("room_3.bath"), 1);
    }

    #[test]
    fn letters() {
        assert_eq!(letter(0), "A");
        assert_eq!(letter(25), "Z");
        assert_eq!(letter(26), "AA");
    }

    fn bedroom() -> Value {
        json!({
            "preview": { "width": 3.6, "depth": 4.2 },
            "spaces": {
                "bed": {
                    "kind": "fixture", "width": 1.53, "depth": 2.03, "against": "wall",
                    "clearance": { "front": 0.9, "left": 0.6, "right": 0.6 }
                },
                "wardrobe": {
                    "kind": "fixture", "width": { "min": 1.2, "max": 2.4 }, "depth": 0.6,
                    "against": "wall", "clearance": { "front": 0.9 }
                }
            }
        })
    }

    fn element<'a>(s: &'a Solution, id: &str) -> &'a Element {
        s.layout.elements.iter().find(|e| e.id == id).unwrap_or_else(|| panic!("no {id}"))
    }

    fn inside(inner: &Element, outer: &Element) -> bool {
        let (a, b) = (Bounds::from_points(&inner.points), Bounds::from_points(&outer.points));
        let e = 1e-9;
        a.min_x >= b.min_x - e && a.min_y >= b.min_y - e && a.max_x <= b.max_x + e && a.max_y <= b.max_y + e
    }

    fn overlaps(a: &Element, b: &Element) -> bool {
        let (a, b) = (Bounds::from_points(&a.points), Bounds::from_points(&b.points));
        a.min_x.max(b.min_x) < a.max_x.min(b.max_x) - 1e-9
            && a.min_y.max(b.min_y) < a.max_y.min(b.max_y) - 1e-9
    }

    #[test]
    fn fixtures_are_their_exact_size_with_clearance_inside_the_room() {
        for seed in [1, 2, 3] {
            let mut v = bedroom();
            v["preview"]["seed"] = json!(seed);
            let s = run(v);
            assert!(s.report.unmet == 0, "seed {seed}: {:?}", messages(&s));
            let slot = element(&s, "slot");
            let bed = element(&s, "bed");
            let info = bed.space.as_ref().unwrap();
            assert_eq!(info.kind, SpaceKind::Fixture);
            assert!(!info.has_plan);
            // Drawn at its own size, not rounded to the grid.
            assert!((bed.area.unwrap() - 1.53 * 2.03).abs() < 1e-6, "{:?}", bed.area);
            let [a, b] = info.front.unwrap();
            assert!((a.distance(b) - 1.53).abs() < 1e-6, "the front is the bed's width");
            let clear = element(&s, "clearance:bed");
            assert_eq!(clear.kind, ElementKind::Clearance);
            assert_eq!(clear.source.as_deref(), Some("/spaces/bed/clearance"));
            assert!(inside(clear, slot), "seed {seed}: clearance outside the room");
            let wardrobe = element(&s, "wardrobe");
            assert!(!overlaps(wardrobe, clear), "seed {seed}: the wardrobe stands in the bed's clearance");
            assert!(!overlaps(bed, element(&s, "clearance:wardrobe")));
            // Backs against a wall: the edge opposite the front is on the slot's boundary.
            for f in [bed, wardrobe] {
                let [a, b] = f.space.as_ref().unwrap().front.unwrap();
                let i = f.points.iter().position(|p| p.distance(a) < 1e-9).unwrap();
                let back = (f.points[(i + 2) % 4], f.points[(i + 3) % 4]);
                let on_wall = |p: Point| {
                    p.x.abs() < 1e-6
                        || p.y.abs() < 1e-6
                        || (p.x - 3.6).abs() < 1e-6
                        || (p.y - 4.2).abs() < 1e-6
                };
                let same_side = (back.0.x - back.1.x).abs() < 1e-9 || (back.0.y - back.1.y).abs() < 1e-9;
                assert!(
                    on_wall(back.0) && on_wall(back.1) && same_side,
                    "seed {seed}: {} back {back:?}",
                    f.id
                );
                assert!(a.distance(b) > 0.0);
            }
        }
    }

    #[test]
    fn a_fixture_grows_until_its_clearance_meets_the_walls() {
        // An island with 1 m all round in a 4.4 × 3.6 room: at most 2.4 × 1.6.
        let s = run(json!({
            "preview": { "width": 4.4, "depth": 3.6 },
            "spaces": {
                "island": { "kind": "fixture", "width": { "min": 1.2, "max": 3 }, "depth": { "min": 0.9, "max": 1.6 }, "clearance": 1 }
            }
        }));
        assert!(s.report.unmet == 0, "{:?}", messages(&s));
        let island = Bounds::from_points(&element(&s, "island").points);
        let (w, h) = (island.width().max(island.height()), island.width().min(island.height()));
        assert!((w - 2.4).abs() < 1e-6 && (h - 1.6).abs() < 1e-6, "{w} × {h}");
        assert!(inside(element(&s, "clearance:island"), element(&s, "slot")));
    }

    #[test]
    fn fixtures_cant_have_plans_and_clearance_is_for_fixtures() {
        let err = Config::from_value(json!({ "spaces": { "bed": { "kind": "fixture", "plan": {} } } }))
            .unwrap_err();
        assert!(err.to_string().contains("can't have a plan"), "{err}");
        let err = Config::from_value(json!({ "spaces": { "den": { "clearance": 1 } } })).unwrap_err();
        assert!(err.to_string().contains("for fixtures"), "{err}");
    }

    #[test]
    fn fixtures_are_solved_inside_a_placed_room_on_a_finer_grid() {
        let mut plan = room(json!({}));
        plan["spaces"]["sleep"]["plan"] = bedroom();
        plan["spaces"]["sleep"]["plan"].as_object_mut().unwrap().remove("preview");
        let s = run(hotel(json!([{ "rule": "along", "a": "room", "b": "corridor" }]), plan));
        let beds: Vec<&Element> = spaces(&s).into_iter().filter(|e| e.id.ends_with(".bed")).collect();
        assert!(!beds.is_empty());
        for bed in &beds {
            assert!((bed.area.unwrap() - 1.53 * 2.03).abs() < 1e-6);
            let sleep = element(&s, bed.id.trim_end_matches(".bed"));
            assert!(inside(bed, sleep), "{} isn't in its room", bed.id);
            assert!(inside(element(&s, &format!("clearance:{}", bed.id)), sleep));
            let slot = sleep.space.as_ref().unwrap().slot.as_ref().unwrap();
            assert!((slot.grid - 0.1).abs() < 1e-9, "fixtures get a 100 mm grid, not {}", slot.grid);
            // The room knows where its door is, and nothing stands in its swing.
            assert!(slot.ports.iter().any(|p| p.roles.iter().any(|r| r == "door")), "{:?}", slot.ports);
            let room = sleep.id.as_str();
            let door = s
                .layout
                .elements
                .iter()
                .find(|e| {
                    e.kind == ElementKind::Door
                        && e.opening.as_ref().unwrap().between.iter().any(|n| n == room)
                })
                .unwrap();
            let (a, b) = (door.points[0], door.points[1]);
            let len = a.distance(b);
            let c = Bounds::from_points(&sleep.points);
            let (cx, cy) = ((c.min_x + c.max_x) / 2.0, (c.min_y + c.max_y) / 2.0);
            let (mut nx, mut ny) = (-(b.y - a.y) / len, (b.x - a.x) / len);
            if (cx - a.x) * nx + (cy - a.y) * ny < 0.0 {
                (nx, ny) = (-nx, -ny);
            }
            let swing = Element::new(
                "swing",
                ElementKind::Clearance,
                vec![
                    a,
                    b,
                    Point::new(b.x + nx * len, b.y + ny * len),
                    Point::new(a.x + nx * len, a.y + ny * len),
                ],
                true,
            );
            for f in spaces(&s).into_iter().filter(|e| e.id.starts_with(&format!("{room}."))) {
                assert!(!overlaps(f, &swing), "{} stands in the door's swing", f.id);
            }
        }
    }

    /// The bedroom in a 3.6 × 4.2 slot with a window in the middle of its top wall.
    fn bedroom_with_a_window(rules: Value, seed: u64) -> Solution {
        let mut v = bedroom();
        v["rules"] = rules;
        let plan = format!("{:016x}", Config::from_value(v.clone()).unwrap().plan().fingerprint());
        let (w, h) = (3.6, 4.2);
        let port = |roles: &[&str], a: (f64, f64), b: (f64, f64)| PortInfo {
            roles: roles.iter().map(|r| r.to_string()).collect(),
            a: Point::new(a.0, a.1),
            b: Point::new(b.0, b.1),
        };
        let slot = SlotInfo {
            width: w,
            depth: h,
            ports: vec![
                port(&["entrance"], (0.0, 0.0), (w, 0.0)),
                port(&["party"], (w, 0.0), (w, h)),
                port(&["facade"], (w, h), (0.0, h)),
                port(&["party"], (0.0, h), (0.0, 0.0)),
                port(&["window"], (2.4, h), (1.2, h)),
            ],
            units: Units::M,
            system: System::Metric,
            seed,
            grid: 0.1,
            unique: None,
            plan,
            context: None,
        };
        run_view(v, &View::Slot(slot))
    }

    fn warnings(s: &Solution) -> Vec<(Option<String>, String)> {
        s.diagnostics
            .iter()
            .filter(|d| d.severity == crate::diagnostic::Severity::Warning)
            .map(|d| (d.pointer.clone(), d.message.clone()))
            .collect()
    }

    #[test]
    fn apart_keeps_a_fixture_off_a_window() {
        for seed in [1, 2, 3] {
            let s = bedroom_with_a_window(json!([{ "rule": "apart", "a": "bed", "port": "window" }]), seed);
            assert!(warnings(&s).is_empty(), "seed {seed}: {:?}", warnings(&s));
            let bed = Bounds::from_points(&element(&s, "bed").points);
            let on_top = (bed.max_y - 4.2).abs() < 1e-6;
            let under_window = bed.min_x < 2.4 - 1e-6 && bed.max_x > 1.2 + 1e-6;
            assert!(!(on_top && under_window), "seed {seed}: the bed touches the window: {bed:?}");
        }
    }

    #[test]
    fn apart_with_wall_keeps_a_fixture_off_the_whole_wall() {
        for seed in [1, 2, 3] {
            let rules = json!([{ "rule": "apart", "a": "bed", "port": "window", "wall": true }]);
            let s = bedroom_with_a_window(rules, seed);
            assert!(warnings(&s).is_empty(), "seed {seed}: {:?}", warnings(&s));
            let bed = Bounds::from_points(&element(&s, "bed").points);
            assert!(bed.max_y < 4.2 - 1e-6, "seed {seed}: the bed is against the window wall: {bed:?}");
        }
    }

    #[test]
    fn apart_keeps_a_distance_between_spaces() {
        for seed in [1, 2, 3] {
            let rules = json!([{ "rule": "apart", "a": "bed", "b": "wardrobe", "distance": 1.2 }]);
            let s = bedroom_with_a_window(rules, seed);
            assert!(warnings(&s).is_empty(), "seed {seed}: {:?}", warnings(&s));
            let (a, b) = (
                Bounds::from_points(&element(&s, "bed").points),
                Bounds::from_points(&element(&s, "wardrobe").points),
            );
            let dx = (b.min_x - a.max_x).max(a.min_x - b.max_x).max(0.0);
            let dy = (b.min_y - a.max_y).max(a.min_y - b.max_y).max(0.0);
            let gap = (dx * dx + dy * dy).sqrt();
            assert!(gap >= 1.2 - 1e-6, "seed {seed}: bed and wardrobe are {gap} apart");
        }
    }

    #[test]
    fn an_apart_that_cant_hold_says_why() {
        let rules = json!([
            { "rule": "apart", "a": "bed", "b": "wardrobe", "distance": 20 },
            { "rule": "apart", "a": "wardrobe", "port": "party", "wall": true },
            { "rule": "apart", "a": "wardrobe", "port": "facade" },
            { "rule": "apart", "a": "wardrobe", "port": "entrance" },
        ]);
        let w = warnings(&bedroom_with_a_window(rules, 1));
        assert!(
            w.iter().any(|(p, m)| p.as_deref() == Some("/rules/0") && m.contains("less than 20")),
            "{w:?}"
        );
        assert!(
            w.iter().any(|(p, m)| p.as_deref().is_some_and(|p| p.starts_with("/rules/"))
                && m.contains("wardrobe touches")),
            "{w:?}"
        );
    }

    #[test]
    fn windows_placed_by_a_parent_reach_the_plan_inside() {
        let mut room = bedroom();
        room.as_object_mut().unwrap().remove("preview");
        room["rules"] = json!([{ "rule": "apart", "a": "bed", "port": "window", "wall": true }]);
        let s = (1..=4)
            .map(|seed| {
                let mut v = apartment();
                v["seed"] = json!(seed);
                v["spaces"]["bedroom"]["plan"] = room.clone();
                run(v)
            })
            .find(|s| s.report.unmet == 0)
            .expect("no seed met every rule");
        for k in 1..=2 {
            let name = format!("bedroom_{k}");
            let slot = element(&s, &name).space.as_ref().unwrap().slot.clone().unwrap();
            assert!(slot.ports.iter().any(|p| p.roles == ["window"]), "{name}: {:?}", slot.ports);
            let window = s
                .layout
                .elements
                .iter()
                .find(|e| e.kind == ElementKind::Window && e.opening.as_ref().unwrap().between[0] == name)
                .unwrap();
            let (p, q) = (window.points[0], window.points[1]);
            let bed = Bounds::from_points(&element(&s, &format!("{name}.bed")).points);
            if (p.y - q.y).abs() < 1e-9 {
                assert!(bed.min_y > p.y + 1e-6 || bed.max_y < p.y - 1e-6, "{name}: bed on the window wall");
            } else {
                assert!(bed.min_x > p.x + 1e-6 || bed.max_x < p.x - 1e-6, "{name}: bed on the window wall");
            }
        }
    }

    #[test]
    fn spine_stack_reports_fixtures() {
        let s = run(json!({
            "start": { "mode": "spine", "points": [[0, 0], [20, 0]] },
            "spaces": {
                "corridor": { "kind": "circulation", "width": 1.8 },
                "room": { "count": 2, "width": 4, "depth": 6 },
                "bench": { "kind": "fixture", "width": 1.5, "depth": 0.5 }
            }
        }));
        assert!(messages(&s)
            .iter()
            .any(|(p, m)| p.as_deref() == Some("/spaces/bench/kind") && m.contains("fixture")));
    }

    #[test]
    fn symbols_are_drawn_without_changing_the_solution() {
        let plain = run(bedroom());
        let mut v = bedroom();
        v["spaces"]["bed"]["symbol"] = json!("bed_queen");
        let drawn = run(v);
        assert_eq!(element(&plain, "bed").points, element(&drawn, "bed").points, "a symbol moved the bed");
        assert!(plain.layout.elements.iter().all(|e| e.kind != ElementKind::Symbol));
        let symbol = element(&drawn, "symbol:bed");
        assert_eq!(symbol.kind, ElementKind::Symbol);
        assert_eq!(symbol.source.as_deref(), Some("/spaces/bed/symbol"));
        let info = symbol.symbol.as_ref().unwrap();
        assert_eq!(info.name, "bed_queen");
        // Stretched over the bed: every point is on or in it.
        let b = Bounds::from_points(&element(&drawn, "bed").points);
        for p in info.paths.iter().flat_map(|p| &p.points) {
            assert!(
                p.x > b.min_x - 1e-6 && p.x < b.max_x + 1e-6 && p.y > b.min_y - 1e-6 && p.y < b.max_y + 1e-6
            );
        }
        assert_eq!(element(&drawn, "bed").space.as_ref().unwrap().symbol.as_deref(), Some("bed_queen"));
    }

    #[test]
    fn a_cached_solution_draws_the_current_symbols() {
        let plate = |symbol: &str| {
            let mut plan = room(json!({}));
            plan["spaces"]["bath"]["symbol"] = json!(symbol);
            hotel(json!([{ "rule": "along", "a": "room", "b": "corridor" }]), plan)
        };
        let names = |s: &Solution| -> Vec<String> {
            s.layout.elements.iter().filter_map(|e| e.symbol.as_ref().map(|x| x.name.clone())).collect()
        };
        let a = run(plate("bathtub"));
        let b = run(plate("shower"));
        assert!(!names(&a).is_empty() && names(&a).iter().all(|n| n == "bathtub"), "{:?}", names(&a));
        assert!(!names(&b).is_empty() && names(&b).iter().all(|n| n == "shower"), "{:?}", names(&b));
        // Same layout either way.
        let rooms = |s: &Solution| spaces(s).iter().map(|e| e.points.clone()).collect::<Vec<_>>();
        assert_eq!(rooms(&a), rooms(&b));
        // A room's symbol is drawn from the side it's entered from.
        let bath = spaces(&a).into_iter().find(|e| e.id.ends_with(".bath")).unwrap();
        assert!(bath.space.as_ref().unwrap().front.is_some());
    }

    // ---------------------------------------------------------------- filling the floor

    fn boundary(spaces: Value, rules: Value) -> Value {
        json!({
            "start": { "mode": "boundary", "points": [[0, 0], [12, 0], [12, 8], [0, 8]], "ports": ["entrance", "blank", "facade", "blank"] },
            "spaces": spaces,
            "rules": rules,
        })
    }

    fn report_of(s: &Solution) -> String {
        format!("{:?} {:?}", s.report, messages(s))
    }

    #[test]
    fn a_fill_takes_the_floor_the_others_leave() {
        // Some seed among the first few is perfect; none leaves floor over.
        let mut perfect = false;
        for seed in 1..=4 {
            let mut v = boundary(
                json!({
                    "hall": { "kind": "circulation", "fill": true, "area": { "min": 8, "max": 30 }, "side": { "min": 1.2 } },
                    "living": { "area": { "min": 24, "max": 34 }, "side": { "min": 4 } },
                    "bedroom": { "count": 2, "area": { "min": 12, "max": 16 }, "side": { "min": 3 } },
                }),
                json!([
                    { "rule": "door", "a": "hall", "port": "entrance" },
                    { "rule": "door", "a": ["living", "bedroom"], "b": "hall" },
                    { "rule": "window", "a": ["living", "bedroom"] },
                ]),
            );
            v["seed"] = json!(seed);
            let s = run(v);
            assert!(s.report.unmet == 0 && s.report.leftover < 1e-9, "{}", report_of(&s));
            let total: f64 = spaces(&s).iter().map(|e| e.area.unwrap()).sum();
            assert!((total - 96.0).abs() < 1e-6, "the spaces cover the floor: {total}");
            let hall = spaces(&s).into_iter().find(|e| e.id == "hall").unwrap();
            assert!(hall.space.as_ref().unwrap().fill);
            perfect |= s.report.status == Status::Perfect;
        }
        assert!(perfect);
    }

    #[test]
    fn floor_no_space_covers_is_reported_and_drawn() {
        let s = run(boundary(
            json!({
                "a": { "area": { "min": 20, "max": 30 }, "side": { "min": 4 } },
                "b": { "area": { "min": 20, "max": 30 }, "side": { "min": 4 } },
            }),
            json!([]),
        ));
        assert_eq!(s.report.status, Status::Solved, "{}", report_of(&s));
        assert!(s.report.leftover > 30.0, "{}", report_of(&s));
        assert!(s.layout.elements.iter().any(|e| e.kind == ElementKind::Leftover));
        // The program can't cover the floor: said up front, with the numbers.
        let d = s.diagnostics.iter().find(|d| d.code == Some(Code::Leftover)).expect("a leftover finding");
        assert!(!d.notes.is_empty(), "with what to change");
    }

    #[test]
    fn a_fill_that_cant_take_the_rest_says_how_much_is_left() {
        let s = run(boundary(
            json!({
                "hall": { "kind": "circulation", "fill": true, "area": { "min": 6, "max": 10 }, "side": { "min": 1.2 } },
                "a": { "area": { "min": 20, "max": 30 }, "side": { "min": 4 } },
            }),
            json!([]),
        ));
        assert!(s.diagnostics.iter().any(|d| d.message.contains("nowhere to go")), "{}", report_of(&s));
    }

    #[test]
    fn a_pinned_space_stays_where_it_is() {
        let s = run(boundary(
            json!({
                "core": { "kind": "vertical", "width": 4, "depth": 3, "at": [6, 3] },
                "floor": { "fill": true },
            }),
            json!([]),
        ));
        let core = spaces(&s).into_iter().find(|e| e.id == "core").unwrap();
        let b = crate::geometry::Bounds::from_points(&core.points);
        assert_eq!((b.min_x, b.min_y, b.max_x, b.max_y), (6.0, 3.0, 10.0, 6.0));
        // The fill runs all round it: a hole.
        let floor = s.layout.elements.iter().find(|e| e.id == "floor").unwrap();
        assert_eq!(floor.holes.len(), 1);
        assert!(s.report.leftover < 1e-9, "{}", report_of(&s));
    }

    #[test]
    fn a_row_is_one_run_split_into_named_pieces() {
        let s = run_view(
            json!({
                "spaces": {
                    "desk": { "kind": "fixture", "count": 3, "row": true, "width": 1.6, "depth": 0.8, "against": "wall" },
                },
                "preview": { "width": 6, "depth": 3 },
            }),
            &View::Default,
        );
        assert!(s.report.unmet == 0, "{}", report_of(&s));
        let mut desks: Vec<_> = spaces(&s).into_iter().filter(|e| e.id.starts_with("desk_")).collect();
        assert_eq!(desks.len(), 3);
        desks.sort_by(|a, b| a.id.cmp(&b.id));
        // Side by side, in line: they share their long sides and line up front and back.
        let b: Vec<_> = desks.iter().map(|d| crate::geometry::Bounds::from_points(&d.points)).collect();
        let along_x = (b[0].min_y - b[1].min_y).abs() < 1e-6;
        for k in 0..2 {
            let touching = if along_x {
                (b[k].max_x - b[k + 1].min_x).abs() < 1e-6 || (b[k + 1].max_x - b[k].min_x).abs() < 1e-6
            } else {
                (b[k].max_y - b[k + 1].min_y).abs() < 1e-6 || (b[k + 1].max_y - b[k].min_y).abs() < 1e-6
            };
            assert!(touching, "{b:?}");
        }
    }

    #[test]
    fn each_reaches_every_stretch_of_a_port() {
        // A block between two wings: its hall reaches the doors on both sides.
        let plan = json!({
            "spaces": {
                "hall": { "kind": "circulation", "fill": true, "area": { "min": 12, "max": 50 }, "side": { "min": 1.8 } },
                "a": { "area": { "min": 30, "max": 44 }, "side": { "min": 4 } },
                "b": { "area": { "min": 30, "max": 44 }, "side": { "min": 4 } },
            },
            "rules": [
                { "rule": "touch", "a": "hall", "port": "door", "length": 1.8, "each": true },
                { "rule": "door", "a": ["a", "b"], "b": "hall" },
            ],
            "preview": { "width": 10, "depth": 12, "ports": { "left": ["link", "door"], "right": ["link", "door"], "top": "facade" } },
        });
        let s = run_view(plan, &View::Default);
        assert!(s.report.unmet == 0, "{}", report_of(&s));
        let hall = s.layout.elements.iter().find(|e| e.id == "hall").unwrap();
        let b = crate::geometry::Bounds::from_points(&hall.points);
        assert!(b.min_x.abs() < 1e-6 && (b.max_x - 10.0).abs() < 1e-6, "{b:?}");
    }

    #[test]
    fn a_plan_that_needs_more_than_its_space_allows_says_so() {
        let mut v = boundary(
            json!({
                "flat": {
                    "width": { "min": 3, "max": 4 },
                    "depth": 8,
                    "plan": { "needs": { "width": { "min": 5 } }, "spaces": { "room": {} } },
                },
            }),
            json!([]),
        );
        v["spaces"]["floor"] = json!({ "fill": true });
        let s = run(v);
        assert!(
            s.diagnostics.iter().any(|d| d.pointer.as_deref() == Some("/spaces/flat/plan/needs/width")
                && d.message.contains("at least 5")),
            "{}",
            report_of(&s)
        );
    }
}
