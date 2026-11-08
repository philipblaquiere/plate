//! The `grid` engine: packs rectangles on a grid inside the slot.
//!
//! The slot is rasterised into cells; each space instance is a rectangle of whole cells. A
//! simulated annealing search moves, resizes, swaps and rotates rectangles (and switches
//! optional instances on and off) to minimise one cost: hard violations (outside the slot,
//! overlaps, sizes, required rules) weigh far more than preferences and goals (utilization,
//! compactness). The random number generator is seeded, so a file always solves the same way.
//!
//! Fixtures (`kind = 'fixture`) also have a facing: which side of the rectangle is their front.
//! Turning one changes its facing; its `clearance` is a margin around it, per side, that no
//! other space may stand in (fixtures' clearances may overlap each other) and that must stay
//! inside the slot. A fixture's size is checked as width along its front and depth from front to
//! back, and its exact size is drawn inside its cells, back against the cells' back edge.

use super::build::Foresight;
use super::common::{names_instances, num, select, Edge, Gap, Named, Rng, SpaceType};
use super::frame::{Rect, Transform};
use super::outline;
use super::ports::{
    door_ports, gap_ports, is_gap, ports_along, window_ports, PortSeg, Source, DOOR, OPEN, OPENING,
};
use super::{Ctx, FixtureOut, Issue, Level, LevelResult, Opening, Placed, Slot};
use crate::geometry::Point;
use crate::layout::ElementKind;
use crate::plan::{Bounds1, Leaf, Plan, Rule, Selector, SpaceKind, ANY_WALL};

/// Weight of a hard violation, relative to goals.
const HARD: f64 = 20.0;
const WINDOW_MM: f64 = 1200.0;
const TOUCH_MM: f64 = 1000.0;
const FIXTURE_TOUCH_MM: f64 = 300.0;
/// Most instances one level will place.
const MAX_INSTANCES: usize = 48;
/// Independent annealing runs per level.
const RESTARTS: usize = 6;
/// Most cells the slot is divided into (the grid doubles until it fits).
const MAX_CELLS: f64 = 16_000.0;
/// Most cells for a level of big spaces only (rooms, not furniture): a coarser grid is plenty for
/// them, and a much smaller search.
const MAX_CELLS_COARSE: f64 = 1_600.0;
/// The smallest side a space may have for its level to count as one of big spaces, in mm.
const COARSE_SIDE_MM: f64 = 2_000.0;

/// How many cells this level's grid may have: fewer when it only divides floor into rooms (no
/// fixtures, nothing pinned, no exact size, every side at least [`COARSE_SIDE_MM`]).
fn max_cells(types: &[SpaceType], ctx: &Ctx) -> f64 {
    let mm = ctx.units.to_mm();
    let big = types.iter().all(|t| {
        let smallest = t.side.min.max(t.width.min.min(t.depth.min));
        !t.space.is_fixture()
            && t.pins.is_empty()
            && t.width.min < t.width.max
            && t.depth.min < t.depth.max
            && (t.space.fill || smallest * mm >= COARSE_SIDE_MM - 1e-6)
    });
    if big {
        MAX_CELLS_COARSE
    } else {
        MAX_CELLS
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct R {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    on: bool,
    /// Fixtures: the side of the rectangle that is the front: 0 bottom, 1 right, 2 top, 3 left
    /// (counter-clockwise, like the sides of [`Rect::corners`]).
    f: u8,
}

impl R {
    fn x1(&self) -> i32 {
        self.x + self.w
    }
    fn y1(&self) -> i32 {
        self.y + self.h
    }
}

struct Grid {
    ox: f64,
    oy: f64,
    gx: f64,
    gy: f64,
    nx: i32,
    ny: i32,
    /// Prefix sums of inside cells, (nx + 1) × (ny + 1).
    prefix: Vec<i32>,
    inside_cells: i32,
    /// Each cell: inside the slot?
    mask: Vec<bool>,
}

impl Grid {
    fn new(polygon: &[Point], holes: &[Vec<Point>], grid: f64, max_cells: f64) -> Grid {
        let (mut min_x, mut min_y, mut max_x, mut max_y) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
        for p in polygon {
            min_x = min_x.min(p.x);
            min_y = min_y.min(p.y);
            max_x = max_x.max(p.x);
            max_y = max_y.max(p.y);
        }
        let (w, h) = (max_x - min_x, max_y - min_y);
        // A big slot gets a coarser grid: doubled while there would be too many cells.
        let mut step = grid;
        while (w / step) * (h / step) > max_cells {
            step *= 2.0;
        }
        let nx = ((w / step).round() as i32).max(1);
        let ny = ((h / step).round() as i32).max(1);
        let (gx, gy) = (w / nx as f64, h / ny as f64);
        let stride = (nx + 1) as usize;
        let mut prefix = vec![0i32; stride * (ny + 1) as usize];
        let inset = 0.02;
        for j in 0..ny {
            for i in 0..nx {
                let corners =
                    [(inset, inset), (1.0 - inset, inset), (1.0 - inset, 1.0 - inset), (inset, 1.0 - inset)];
                let inside = corners.iter().all(|(u, v)| {
                    let p = Point::new(min_x + (i as f64 + u) * gx, min_y + (j as f64 + v) * gy);
                    point_in_polygon(p, polygon) && !holes.iter().any(|h| point_in_polygon(p, h))
                });
                let at = (j + 1) as usize * stride + (i + 1) as usize;
                prefix[at] = inside as i32 + prefix[at - 1] + prefix[at - stride] - prefix[at - stride - 1];
            }
        }
        let inside_cells = prefix[prefix.len() - 1];
        let mut mask = vec![false; (nx * ny) as usize];
        for j in 0..ny {
            for i in 0..nx {
                let (a, b) = ((j + 1) as usize * stride + (i + 1) as usize, j as usize * stride + i as usize);
                mask[(j * nx + i) as usize] = prefix[a] - prefix[a - 1] - prefix[a - stride] + prefix[b] > 0;
            }
        }
        Grid { ox: min_x, oy: min_y, gx, gy, nx, ny, prefix, inside_cells, mask }
    }

    fn inside(&self, r: &R) -> i32 {
        let stride = (self.nx + 1) as usize;
        let (x0, y0) = (r.x.clamp(0, self.nx) as usize, r.y.clamp(0, self.ny) as usize);
        let (x1, y1) = (r.x1().clamp(0, self.nx) as usize, r.y1().clamp(0, self.ny) as usize);
        self.prefix[y1 * stride + x1] - self.prefix[y0 * stride + x1] - self.prefix[y1 * stride + x0]
            + self.prefix[y0 * stride + x0]
    }

    /// Is cell `(i, j)` inside the slot?
    #[inline]
    fn cell_inside(&self, i: i32, j: i32) -> bool {
        i >= 0 && j >= 0 && i < self.nx && j < self.ny && self.mask[(j * self.nx + i) as usize]
    }

    fn point(&self, i: i32, j: i32) -> Point {
        Point::new(self.ox + i as f64 * self.gx, self.oy + j as f64 * self.gy)
    }

    fn rect(&self, r: &R) -> Rect {
        Rect::new(
            self.ox + r.x as f64 * self.gx,
            self.oy + r.y as f64 * self.gy,
            self.ox + r.x1() as f64 * self.gx,
            self.oy + r.y1() as f64 * self.gy,
        )
    }
}

pub fn point_in_polygon(p: Point, polygon: &[Point]) -> bool {
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

/// A port of the slot, as a grid line: `at` is the line's position and `from..to` its extent,
/// in cells.
#[derive(Debug, Clone)]
struct PortLine {
    horizontal: bool,
    at: f64,
    from: f64,
    to: f64,
    seg: PortSeg,
}

/// What a fixture type adds to a space type.
#[derive(Debug, Clone)]
struct Fix {
    /// Clearance `[front, right, back, left]`, in the level's units.
    clear: [f64; 4],
    against: Option<String>,
}

/// Width, depth and the size limits as the cost reads them.
struct Shape {
    /// The size that counts for area, side and aspect: a fixture's real size, else the cells'.
    w: f64,
    h: f64,
    /// How far width and depth are outside their limits (in either orientation, for a space
    /// that has no front).
    miss: f64,
    compact: bool,
}

#[derive(Debug, Clone)]
enum Target {
    Spaces(Vec<usize>),
    Port(String),
    /// These spaces, or the fill space.
    Fill(Vec<usize>),
}

/// The fill space of a plan: it takes the cells no rectangle covers.
#[derive(Debug, Clone)]
struct FillSpec {
    type_index: usize,
    name: String,
    area: Bounds1,
    /// Narrowest it may be, in cells: thinner strips are slivers.
    k: i32,
    min_width: f64,
    /// The biggest thing its plan places (a bed, with the floor it keeps clear): the fill must
    /// have a rectangle this big somewhere, or the plan inside can't work. (width, depth, name)
    hold: Option<(f64, f64, String)>,
}

/// How many rectangles cover each cell, how many inside cells nothing covers, and how many
/// corners the uncovered floor's outline has (each jog in a hall adds two).
#[derive(Debug, Clone)]
struct Occ {
    n: Vec<u16>,
    free: i32,
    /// Kept up to date only when `track` is set (it costs as much as the rest).
    corners: i32,
    track: bool,
    /// Vertices already looked at in this move, and a buffer for them.
    stamp: Vec<u32>,
    gen: u32,
    scratch: Vec<(i32, i32)>,
    /// For finding the fill's main piece: cells seen, a stack, and a cell that was in it.
    seen: std::cell::RefCell<(Vec<u32>, u32, Vec<u32>, usize)>,
}

impl Occ {
    fn build(g: &Grid, rects: &[R]) -> Occ {
        let mut o = Occ {
            n: vec![0; (g.nx * g.ny) as usize],
            free: g.inside_cells,
            corners: 0,
            track: true,
            stamp: vec![0; ((g.nx + 1) * (g.ny + 1)) as usize],
            gen: 0,
            scratch: Vec::new(),
            seen: std::cell::RefCell::new((vec![0; (g.nx * g.ny) as usize], 0, Vec::new(), usize::MAX)),
        };
        for r in rects {
            o.cover(g, r, 1);
        }
        o.corners = o.corners_in(g, 0, 0, g.nx, g.ny);
        o
    }

    /// Move a rectangle. Only the cells it leaves or newly covers change (where it was and is
    /// both, nothing does), and corners are recounted only around cells that change between
    /// covered and uncovered, so a small move costs little.
    fn shift(&mut self, g: &Grid, from: &R, to: &R) {
        let clip = |r: &R| R {
            x: r.x.max(0),
            y: r.y.max(0),
            w: (r.x1().min(g.nx) - r.x.max(0)).max(0),
            h: (r.y1().min(g.ny) - r.y.max(0)).max(0),
            on: r.on && r.w > 0 && r.h > 0,
            f: 0,
        };
        let (a, b) = (clip(from), clip(to));
        let mut parts: Vec<(R, i32)> = Vec::with_capacity(8);
        for p in minus(&a, &b) {
            parts.push((p, -1));
        }
        for p in minus(&b, &a) {
            parts.push((p, 1));
        }
        if !self.track {
            for (p, by) in &parts {
                self.cover(g, p, *by);
            }
            return;
        }
        self.gen = self.gen.wrapping_add(1);
        if self.gen == 0 {
            self.stamp.iter_mut().for_each(|v| *v = 0);
            self.gen = 1;
        }
        let mut verts = std::mem::take(&mut self.scratch);
        verts.clear();
        let vstride = g.nx + 1;
        for (p, by) in &parts {
            for j in p.y..p.y1() {
                for i in p.x..p.x1() {
                    let k = (j * g.nx + i) as usize;
                    if !g.mask[k] {
                        continue;
                    }
                    let n = self.n[k] as i32;
                    if (n == 0) == (n + by == 0) {
                        continue;
                    }
                    for (vi, vj) in [(i, j), (i + 1, j), (i, j + 1), (i + 1, j + 1)] {
                        let v = (vj * vstride + vi) as usize;
                        if self.stamp[v] != self.gen {
                            self.stamp[v] = self.gen;
                            verts.push((vi, vj));
                        }
                    }
                }
            }
        }
        let before: i32 = verts.iter().map(|&(i, j)| self.corner(g, i, j) as i32).sum();
        for (p, by) in &parts {
            self.cover(g, p, *by);
        }
        let after: i32 = verts.iter().map(|&(i, j)| self.corner(g, i, j) as i32).sum();
        self.corners += after - before;
        self.scratch = verts;
    }

    /// Uncovered cells not in the largest connected piece. Starts from a cell of the last main
    /// piece: when that piece holds most of the floor, nothing else needs a look.
    fn cut(&self, g: &Grid) -> i32 {
        if self.free <= 0 {
            return 0;
        }
        let mut guard = self.seen.borrow_mut();
        let (seen, gen, stack, seed) = &mut *guard;
        let nx = g.nx as usize;
        let ny = g.ny as usize;
        let n = &self.n;
        let mask = &g.mask;
        let open = |k: usize| n[k] == 0 && mask[k];
        // A scanline flood: whole runs of a row at a time.
        let fill_from = |start: usize, seen: &mut Vec<u32>, gen: u32, stack: &mut Vec<u32>| -> i32 {
            let mut size = 0i32;
            stack.clear();
            stack.push(start as u32);
            while let Some(k) = stack.pop() {
                let k = k as usize;
                if seen[k] == gen {
                    continue;
                }
                let row = k / nx * nx;
                let (mut l, mut r) = (k, k);
                while l > row && open(l - 1) && seen[l - 1] != gen {
                    l -= 1;
                }
                while r + 1 < row + nx && open(r + 1) && seen[r + 1] != gen {
                    r += 1;
                }
                seen[l..=r].fill(gen);
                size += (r - l + 1) as i32;
                let j = k / nx;
                for nb in [j.wrapping_sub(1), j + 1] {
                    if nb >= ny {
                        continue;
                    }
                    let off = nb * nx;
                    let mut in_run = false;
                    for c in (l - row)..=(r - row) {
                        let m = off + c;
                        let ok = open(m) && seen[m] != gen;
                        if ok && !in_run {
                            stack.push(m as u32);
                        }
                        in_run = ok;
                    }
                }
            }
            size
        };
        *gen = gen.wrapping_add(1);
        if *gen == 0 {
            seen.iter_mut().for_each(|v| *v = 0);
            *gen = 1;
        }
        let g0 = *gen;
        if *seed < seen.len() && open(*seed) {
            let size = fill_from(*seed, seen, g0, stack);
            if size * 2 > self.free {
                return self.free - size;
            }
        }
        // Look at every piece.
        let mut best = (0, usize::MAX);
        for k in 0..seen.len() {
            if seen[k] != g0 && open(k) {
                let size = fill_from(k, seen, g0, stack);
                if size > best.0 {
                    best = (size, k);
                }
            }
        }
        *seed = best.1;
        self.free - best.0
    }

    /// Is grid vertex `(i, j)` a corner of the uncovered floor?
    #[inline]
    fn corner(&self, g: &Grid, i: i32, j: i32) -> bool {
        let (sw, se, nw, ne) = (
            self.is_free(g, i - 1, j - 1),
            self.is_free(g, i, j - 1),
            self.is_free(g, i - 1, j),
            self.is_free(g, i, j),
        );
        let k = sw as i32 + se as i32 + nw as i32 + ne as i32;
        k == 1 || k == 3 || (k == 2 && sw == ne)
    }

    fn cover(&mut self, g: &Grid, r: &R, by: i32) {
        if !r.on {
            return;
        }
        let (x0, y0, x1, y1) = (r.x.max(0), r.y.max(0), r.x1().min(g.nx), r.y1().min(g.ny));
        if x1 <= x0 || y1 <= y0 {
            return;
        }
        let mut free = 0;
        for j in y0..y1 {
            let row = (j * g.nx) as usize;
            let (a, b) = (row + x0 as usize, row + x1 as usize);
            for (c, &inside) in self.n[a..b].iter_mut().zip(&g.mask[a..b]) {
                let was = *c == 0;
                *c = (*c as i32 + by) as u16;
                if inside && was != (*c == 0) {
                    free += if was { -1 } else { 1 };
                }
            }
        }
        self.free += free;
    }

    /// Corners of the uncovered floor at the grid vertices of cells `x0..x1` × `y0..y1`.
    fn corners_in(&self, g: &Grid, x0: i32, y0: i32, x1: i32, y1: i32) -> i32 {
        if x1 < x0 || y1 < y0 {
            return 0;
        }
        let mut n = 0;
        for j in y0..=y1 {
            let (mut sw, mut nw) = (self.is_free(g, x0 - 1, j - 1), self.is_free(g, x0 - 1, j));
            for i in x0..=x1 {
                let (se, ne) = (self.is_free(g, i, j - 1), self.is_free(g, i, j));
                let k = sw as i32 + se as i32 + nw as i32 + ne as i32;
                if k == 1 || k == 3 || (k == 2 && sw == ne) {
                    n += 1;
                }
                sw = se;
                nw = ne;
            }
        }
        n
    }

    /// Inside the slot and covered by nothing: part of the fill.
    #[inline]
    fn is_free(&self, g: &Grid, i: i32, j: i32) -> bool {
        if i < 0 || j < 0 || i >= g.nx || j >= g.ny {
            return false;
        }
        let k = (j * g.nx + i) as usize;
        self.n[k] == 0 && g.mask[k]
    }
}

#[derive(Debug, Clone)]
enum C {
    /// Each `a` shares at least `need` with a `b` (touch, door).
    Touch {
        a: Vec<usize>,
        b: Target,
        need: f64,
        door: Option<Gap>,
        w: f64,
        ptr: String,
        verb: &'static str,
    },
    Window {
        a: Vec<usize>,
        role: String,
        need: f64,
        width: f64,
        w: f64,
        ptr: String,
    },
    /// No `a` shares a wall with a `b`, or comes closer than `d`.
    Apart {
        a: Vec<usize>,
        b: Vec<usize>,
        d: f64,
        w: f64,
        ptr: String,
    },
    /// No `a` shares a wall with the port `lines` (ports with `role`, or the walls they're on),
    /// or comes closer than `d`.
    ApartPort {
        a: Vec<usize>,
        role: String,
        lines: Vec<PortLine>,
        wall: bool,
        d: f64,
        w: f64,
        ptr: String,
    },
    Near {
        a: Vec<usize>,
        b: Vec<usize>,
        d: f64,
        w: f64,
        ptr: String,
    },
    /// Each `a` has a `b` in front of it (fixtures).
    Faces {
        a: Vec<usize>,
        b: Vec<usize>,
        w: f64,
        ptr: String,
    },
    /// The fill space shares at least `need` with a `b` (a space or a port): touch, door.
    FillTouch {
        b: Target,
        need: f64,
        door: Option<Gap>,
        w: f64,
        ptr: String,
        verb: &'static str,
    },
    /// The fill space has a window on a port.
    FillWindow {
        role: String,
        need: f64,
        width: f64,
        w: f64,
        ptr: String,
    },
}

impl C {
    /// Terms that depend on the fill, so on where every rectangle is: solved as a whole.
    fn global(&self) -> bool {
        matches!(self, C::Touch { b: Target::Fill(_), .. } | C::FillTouch { .. } | C::FillWindow { .. })
    }
}

/// The fill's share of the cost, kept up to date move by move: only rectangles near a change
/// are looked at again.
struct FillState {
    occ: Occ,
    near: Vec<f64>,
    counts: f64,
    rest: f64,
    /// The cost of floor cut off from the main piece, as last measured.
    cut: f64,
    /// Exact: corners, cut-off floor and slivers measured on the cells (polishing). Otherwise
    /// only what annealing needs on every move: the area and the runs of fill doors need.
    exact: bool,
}

/// What a move changed, to put it back.
struct FillUndo {
    near: Vec<(usize, f64)>,
    counts: f64,
    rest: f64,
    cut: f64,
}

impl FillState {
    fn new(p: &Problem, rects: &[R], hs: f64, exact: bool) -> FillState {
        let mut occ = Occ::build(&p.grid, rects);
        occ.track = exact;
        let near = (0..rects.len()).map(|i| p.fill_near(i, rects, &occ, hs, exact)).collect();
        let counts = p.fill_counts(&occ, hs, exact);
        let rest = p.fill_rest(rects, &occ, hs);
        let cut = p.cut_pen(&occ, hs, exact);
        FillState { occ, near, counts, rest, cut, exact }
    }

    fn total(&self) -> f64 {
        self.counts + self.rest + self.cut + self.near.iter().sum::<f64>()
    }

    /// Measure the cut-off floor again (after `update`): the change in cost.
    fn recut(&mut self, p: &Problem, hs: f64) -> f64 {
        let cut = p.cut_pen(&self.occ, hs, self.exact);
        let d = cut - self.cut;
        self.cut = cut;
        d
    }

    /// Bring the cached terms up to date after `moved` (index, before, after) were applied to
    /// `rects` and to `occ`. Returns the change in cost and how to undo it.
    fn update(&mut self, p: &Problem, rects: &[R], moved: &[(usize, R, R)], hs: f64) -> (f64, FillUndo) {
        let k = p.fill.as_ref().map_or(1, |f| f.k) + 1;
        let mut undo = FillUndo { near: Vec::new(), counts: self.counts, rest: self.rest, cut: self.cut };
        let mut delta = 0.0;
        let counts = p.fill_counts(&self.occ, hs, self.exact);
        delta += counts - self.counts;
        self.counts = counts;
        let rest = p.fill_rest(rects, &self.occ, hs);
        delta += rest - self.rest;
        self.rest = rest;
        // A rectangle's own terms see the floor up to `k` cells around it.
        let touches = |a: &R, b: &R| {
            a.on && b.on && a.x - k < b.x1() && b.x < a.x1() + k && a.y - k < b.y1() && b.y < a.y1() + k
        };
        for i in 0..rects.len() {
            let ri = &rects[i];
            let hit =
                moved.iter().any(|(j, before, after)| *j == i || touches(ri, before) || touches(ri, after));
            if !hit {
                continue;
            }
            let v = p.fill_near(i, rects, &self.occ, hs, self.exact);
            delta += v - self.near[i];
            undo.near.push((i, self.near[i]));
            self.near[i] = v;
        }
        (delta, undo)
    }

    fn restore(&mut self, undo: FillUndo) {
        for (i, v) in undo.near {
            self.near[i] = v;
        }
        self.counts = undo.counts;
        self.rest = undo.rest;
        self.cut = undo.cut;
    }
}

struct Problem<'a> {
    ctx: &'a Ctx<'a>,
    grid: Grid,
    types: Vec<SpaceType>,
    /// Per type: fixture details, for fixtures.
    fix: Vec<Option<Fix>>,
    /// Where the slot's doors swing: no fixture may stand there.
    swings: Vec<R>,
    inst: Vec<Named>,
    mandatory: Vec<bool>,
    /// Instances fixed in place (`at`): their rectangle never moves.
    pinned: Vec<bool>,
    fill: Option<FillSpec>,
    ports: Vec<PortLine>,
    /// The slot's own edges, as grid lines (what a port is "on" for `apart … & { wall = true }`).
    edges: Vec<PortLine>,
    rules: Vec<C>,
    /// Types whose instances are named individually in rules: keep them numbered by position.
    ordered: Vec<bool>,
    l0: f64,
    a0: f64,
    utilization: f64,
    compactness: f64,
    /// Instances whose plan's clear needs depend on the walls around them.
    needy: Vec<usize>,
    walls: Foreseen,
    /// Pairs a `wall` rule builds a wall between, open floor or not.
    walled: Vec<(Selector, Selector)>,
}

/// How far the walls around each needy instance will take floor from it (see [`Foresight`]).
#[derive(Default)]
struct Foreseen {
    /// `between[k * (n + 1) + j]`: from instance `k`, by the wall to `j` (`n`: the fill).
    between: Vec<f64>,
    /// Per instance: the slot's ports with a wall on them, and how far it takes floor.
    ports: Vec<Vec<(usize, f64)>>,
    /// Per instance: the deepest of all those.
    deepest: Vec<f64>,
}

pub fn solve(plan: &Plan, slot: &Slot, ctx: &Ctx) -> LevelResult {
    let mut issues = Vec::new();
    let Some((mut problem, mut rects)) = setup(plan, slot, ctx, &mut issues) else {
        return LevelResult { issues, ..Default::default() };
    };
    problem.anneal(&mut rects);

    // Number instances by position, then report against the final names.
    problem.renumber(&mut rects);
    problem.rules = problem.compile(plan, &mut Vec::new());
    problem.report(&rects, &mut issues);
    if std::env::var_os("PLATE_DEBUG").is_some() && issues.iter().any(|i| i.level == Level::Required) {
        eprintln!("--- slot {:?}", slot.boundary);
        for p in &slot.ports {
            eprintln!("    {:?} {:.2},{:.2} - {:.2},{:.2}", p.roles, p.a.x, p.a.y, p.b.x, p.b.y);
        }
        for i in &issues {
            eprintln!("    ! {}", i.message);
        }
    }
    problem.result(rects, issues, slot)
}

/// The problem a level poses, and where its rectangles start (pinned ones in place), before any
/// annealing: what `solve` anneals, and what the tests below drive move by move. `None` when
/// there's nothing to place (or no room to place it; `issues` says so).
fn setup<'a>(
    plan: &Plan,
    slot: &Slot,
    ctx: &'a Ctx,
    issues: &mut Vec<Issue>,
) -> Option<(Problem<'a>, Vec<R>)> {
    let types = SpaceType::all(plan, ctx.units);
    if types.is_empty() {
        return None;
    }

    // The area to pack into: the slot, or a square around the origin or spine.
    let (polygon, slot_ports) = match &slot.boundary {
        Some(b) => (b.clone(), slot.ports.clone()),
        None => {
            let total: f64 = types.iter().map(|t| t.target_area(ctx.units) * t.min.max(1) as f64).sum();
            let side = ((total * 1.25).sqrt() / ctx.grid).ceil() * ctx.grid;
            let c = slot.spine.as_ref().and_then(|s| s.first().copied()).unwrap_or(Point::new(0.0, 0.0));
            let r = Rect::new(c.x - side / 2.0, c.y - side / 2.0, c.x + side / 2.0, c.y + side / 2.0);
            let ports = r.edges().iter().map(|&(a, b)| PortSeg::new(vec!["facade".into()], a, b)).collect();
            (r.corners(), ports)
        }
    };
    let holes = if slot.boundary.is_some() { slot.holes.clone() } else { Vec::new() };
    let grid = Grid::new(&polygon, &holes, ctx.grid, max_cells(&types, ctx));
    if grid.inside_cells == 0 {
        issues.push(Issue::new("/spaces", "the space is too small for the grid to place anything"));
        return None;
    }
    let cell = grid.gx * grid.gy;
    let inside_area = grid.inside_cells as f64 * cell;

    // The fill space isn't placed: it's whatever the rectangles leave.
    let fill_ti = types.iter().position(|t| t.space.fill);

    // Instances: the required ones, then optional ones up to what could fit.
    let mut inst = Vec::new();
    let mut mandatory = Vec::new();
    let mut pin_at: Vec<Option<Point>> = Vec::new();
    for (ti, t) in types.iter().enumerate() {
        if Some(ti) == fill_ti {
            continue;
        }
        let room = t.target_area(ctx.units).max(t.area.min).max(cell);
        let fit = (inside_area / room).floor() as u32;
        // Open-ended counts get a few optional instances to switch on, not as many as could fit:
        // each one makes the search slower.
        let open = t.min.max(fit).min(t.min + t.min.max(2));
        let max = t.max.unwrap_or(open).max(t.min);
        for i in 0..max as usize {
            if inst.len() >= MAX_INSTANCES {
                break;
            }
            inst.push(Named { type_index: ti, name: t.instance_name(i) });
            mandatory.push((i as u32) < t.min);
            pin_at.push(t.pins.get(i).map(|p| slot.pin.apply(*p)));
        }
    }
    let required: f64 = inst
        .iter()
        .zip(&mandatory)
        .filter(|(_, m)| **m)
        .map(|(n, _)| types[n.type_index].area.min.max(types[n.type_index].side.min.powi(2)))
        .sum::<f64>()
        + fill_ti.map_or(0.0, |ti| types[ti].area.min);
    if required > inside_area * 1.001 {
        let sym = ctx.units.symbol();
        issues.push(
            Issue::new(
                "/spaces",
                format!(
                    "the spaces need at least {} {sym}² but only {} {sym}² fit here",
                    num(required),
                    num(inside_area)
                ),
            )
            .with_hint(format!(
                "Make the outline {} {sym}² bigger, or lower some area.min (or count) so the program fits.",
                num(required - inside_area)
            )),
        );
    }

    // With a fill space, the spaces must leave it no more than it can take.
    if let Some(ti) = fill_ti {
        let most: Option<f64> = types
            .iter()
            .enumerate()
            .filter(|(k, _)| *k != ti)
            .map(|(_, t)| {
                let one = if t.area.max.is_finite() {
                    Some(t.area.max)
                } else if t.width.max.is_finite() && t.depth.max.is_finite() {
                    Some(t.width.max * t.depth.max)
                } else {
                    None
                };
                Some(one? * t.max? as f64)
            })
            .sum();
        let fill_max = types[ti].area.max;
        if let (Some(most), true) = (most, fill_max.is_finite()) {
            if most + fill_max < inside_area * 0.999 {
                let sym = ctx.units.symbol();
                issues.push(
                    Issue::new(
                        format!("/spaces/{}/area", types[ti].key),
                        format!(
                            "the spaces take at most {} {sym}² and {} at most {}: {} {sym}² of the {} {sym}² here has nowhere to go",
                            num(most),
                            types[ti].key,
                            num(fill_max),
                            num(inside_area - most - fill_max),
                            num(inside_area)
                        ),
                    )
                    .with_hint(format!(
                        "Raise some area.max (or {}'s), or add a space.",
                        types[ti].key
                    )),
                );
            }
        }
    }

    let ports = port_lines(&slot_ports, &grid);
    let edge_segs: Vec<PortSeg> = std::iter::once(&polygon)
        .chain(holes.iter())
        .flat_map(|l| (0..l.len()).map(move |i| PortSeg::new(Vec::new(), l[i], l[(i + 1) % l.len()])))
        .collect();
    let edges = port_lines(&edge_segs, &grid);
    let swings = door_swings(&ports, &grid);
    let mm = ctx.units.to_mm();
    let l0 = 3000.0 / mm;
    let fix = types
        .iter()
        .map(|t| {
            t.space.is_fixture().then(|| Fix {
                clear: t.space.clearance.map_or([0.0; 4], |c| c.sides(ctx.units)),
                against: t.space.against.clone(),
            })
        })
        .collect();
    let fill = fill_ti.map(|ti| {
        let t = &types[ti];
        // A hall narrower than this isn't a hall: 0.9 m unless its side.min says otherwise.
        let min_width = if t.side.min > 0.0 {
            t.side.min
        } else if t.space.kind == SpaceKind::Circulation {
            900.0 / mm
        } else {
            0.0
        };
        // The biggest fixture its plan must place, with its clearance.
        let hold = t.space.plan.as_ref().and_then(|plan| {
            plan.spaces
                .iter()
                .filter(|(_, f)| f.is_fixture() && f.count.min() >= 1)
                .map(|(key, f)| {
                    let c = f.clearance.map_or([0.0; 4], |c| c.sides(ctx.units));
                    let w = crate::plan::bounds(f.width, ctx.units).min + c[1] + c[3];
                    let d = crate::plan::bounds(f.depth, ctx.units).min + c[0] + c[2];
                    (w, d, key.clone())
                })
                .filter(|(w, d, _)| *w > 0.0 && *d > 0.0)
                .max_by(|a, b| (a.0 * a.1).total_cmp(&(b.0 * b.1)))
        });
        FillSpec {
            type_index: ti,
            name: t.instance_name(0),
            area: t.area,
            k: ((min_width / grid.gx.min(grid.gy)) - 1e-6).ceil().max(1.0) as i32,
            min_width,
            hold,
        }
    });
    let pinned: Vec<bool> = pin_at.iter().map(Option::is_some).collect();
    let mut problem = Problem {
        ctx,
        grid,
        types,
        fix,
        swings,
        inst,
        mandatory,
        pinned,
        fill,
        ports,
        edges,
        rules: Vec::new(),
        ordered: Vec::new(),
        l0,
        a0: l0 * l0,
        utilization: plan.goals.utilization,
        compactness: plan.goals.compactness,
        needy: Vec::new(),
        walls: Foreseen::default(),
        walled: Vec::new(),
    };
    problem.ordered = problem.types.iter().map(|_| false).collect();
    problem.rules = problem.compile(plan, issues);
    problem.foresee_walls(plan, slot);

    let mut rects = problem.initial();
    problem.pin(&mut rects, &pin_at, issues);
    Some((problem, rects))
}

fn port_lines(ports: &[PortSeg], grid: &Grid) -> Vec<PortLine> {
    let mut out = Vec::new();
    for seg in ports {
        let (a, b) = (seg.a, seg.b);
        let tol = 1e-6 * (grid.gx + grid.gy);
        if (a.y - b.y).abs() < tol {
            let at = (a.y - grid.oy) / grid.gy;
            let (f, t) = ((a.x.min(b.x) - grid.ox) / grid.gx, (a.x.max(b.x) - grid.ox) / grid.gx);
            out.push(PortLine { horizontal: true, at, from: f, to: t, seg: seg.clone() });
        } else if (a.x - b.x).abs() < tol {
            let at = (a.x - grid.ox) / grid.gx;
            let (f, t) = ((a.y.min(b.y) - grid.oy) / grid.gy, (a.y.max(b.y) - grid.oy) / grid.gy);
            out.push(PortLine { horizontal: false, at, from: f, to: t, seg: seg.clone() });
        }
        // Diagonal ports can't be reached by rectangles on this grid.
    }
    out
}

/// The square each `door` port of the slot sweeps on the inside, in cells.
fn door_swings(ports: &[PortLine], grid: &Grid) -> Vec<R> {
    let mut out = Vec::new();
    for p in ports.iter().filter(|p| p.seg.has(DOOR)) {
        let (from, to) = (p.from.floor() as i32, p.to.ceil() as i32);
        let at = p.at.round() as i32;
        let width = p.seg.len();
        let deep = if p.horizontal { width / grid.gy } else { width / grid.gx };
        let deep = (deep - 1e-9).ceil().max(1.0) as i32;
        let (before, after) = if p.horizontal {
            (
                R { x: from, y: at - deep, w: to - from, h: deep, on: true, f: 0 },
                R { x: from, y: at, w: to - from, h: deep, on: true, f: 0 },
            )
        } else {
            (
                R { x: at - deep, y: from, w: deep, h: to - from, on: true, f: 0 },
                R { x: at, y: from, w: deep, h: to - from, on: true, f: 0 },
            )
        };
        // It swings into the slot: whichever side is inside.
        out.push(if grid.inside(&after) >= grid.inside(&before) { after } else { before });
    }
    out
}

/// A port role as written: without the `#k` that tells apart the stretches of an `each` rule.
fn shown(role: &str) -> &str {
    role.split('#').next().unwrap_or(role)
}

/// Overlap length of [a0, a1] and [b0, b1].
fn overlap(a0: f64, a1: f64, b0: f64, b1: f64) -> f64 {
    (a1.min(b1) - a0.max(b0)).max(0.0)
}

impl Problem<'_> {
    fn compile(&mut self, plan: &Plan, issues: &mut Vec<Issue>) -> Vec<C> {
        let mm = self.ctx.units.to_mm();
        self.walled = plan
            .rules()
            .into_iter()
            .filter_map(|(_, r)| match r {
                Rule::Wall { a, b: Some(b), .. } => Some((a.clone(), b.clone())),
                _ => None,
            })
            .collect();
        let pick = |s: &crate::plan::Selector| select(s, &self.inst, &self.types);
        let fill_key = self.fill.as_ref().map(|f| (f.name.clone(), self.types[f.type_index].key.clone()));
        let hits_fill = |s: &crate::plan::Selector| fill_key.as_ref().is_some_and(|(n, k)| s.matches(n, k));
        let mut out = Vec::new();
        let mut ordered = vec![false; self.types.len()];
        for (ptr, rule) in plan.rules() {
            let w = rule.weight().unwrap_or(HARD);
            for (_, sel) in rule.selectors() {
                if names_instances(sel, &self.types) {
                    for i in pick(sel) {
                        ordered[self.inst[i].type_index] = true;
                    }
                }
            }
            match rule {
                Rule::Touch { a, b, port, length, each, .. }
                | Rule::Door { a, b, port, length, each, .. } => {
                    let gap = match rule {
                        Rule::Door { leaf, width, margin, length, .. } => {
                            Some(Gap::of(*leaf, *width, *margin, *length, self.ctx.units))
                        }
                        _ => None,
                    };
                    if let (Some(gap), Some(b)) = (gap, b) {
                        self.check_edge(a, b, gap, &ptr, issues);
                    }
                    let (need, door, verb) = if let Some(gap) = gap {
                        let verb = match (gap.leaf, gap.full) {
                            (Leaf::Swing, _) => "have a door to",
                            (_, false) => "have an opening to",
                            (_, true) => "open onto",
                        };
                        (gap.need(), Some(gap), verb)
                    } else {
                        // Fixtures are small: touching means sharing a bit of their side.
                        let fixtures = pick(a).iter().any(|&i| self.fix[self.inst[i].type_index].is_some());
                        let default = if fixtures { FIXTURE_TOUCH_MM } else { TOUCH_MM };
                        (length.map_or(default / mm, |l| l.0.to(self.ctx.units)), None, "touch")
                    };
                    let targets = match (b, port) {
                        (Some(b), _) if hits_fill(b) => vec![Target::Fill(pick(b))],
                        (Some(b), _) => vec![Target::Spaces(pick(b))],
                        // Every stretch of the port: each gets a role of its own (`link#2`), which
                        // no rule written in a file can name.
                        (_, Some(p)) if *each => {
                            let mut tags = Vec::new();
                            for (k, line) in self.ports.iter_mut().enumerate().filter(|(_, l)| l.seg.has(p)) {
                                let tag = format!("{p}#{k}");
                                if !line.seg.roles.contains(&tag) {
                                    line.seg.roles.push(tag.clone());
                                }
                                tags.push(Target::Port(tag));
                            }
                            if tags.is_empty() {
                                tags.push(Target::Port(p.clone()));
                            }
                            tags
                        }
                        (_, Some(p)) => vec![Target::Port(p.clone())],
                        _ => continue,
                    };
                    for target in targets {
                        // A port shorter than the length asked for (a door): all of it will do.
                        let need = match &target {
                            Target::Port(role) => {
                                let longest = self
                                    .ports
                                    .iter()
                                    .filter(|l| l.seg.has(role))
                                    .map(|l| {
                                        (l.to - l.from)
                                            * if l.horizontal { self.grid.gx } else { self.grid.gy }
                                    })
                                    .fold(0.0, f64::max);
                                if longest > 0.0 {
                                    need.min(longest)
                                } else {
                                    need
                                }
                            }
                            _ => need,
                        };
                        if hits_fill(a) {
                            let b = match &target {
                                Target::Fill(ids) => Target::Spaces(ids.clone()),
                                t => t.clone(),
                            };
                            out.push(C::FillTouch { b, need, door, w, ptr: ptr.clone(), verb });
                        }
                        out.push(C::Touch { a: pick(a), b: target, need, door, w, ptr: ptr.clone(), verb });
                    }
                }
                Rule::Window { a, port, width, .. } => {
                    let width = width.map_or(WINDOW_MM / mm, |l| l.0.to(self.ctx.units));
                    let role = port.clone().unwrap_or_else(|| "facade".into());
                    if hits_fill(a) {
                        out.push(C::FillWindow {
                            role: role.clone(),
                            need: width,
                            width,
                            w,
                            ptr: ptr.clone(),
                        });
                    }
                    out.push(C::Window { a: pick(a), role, need: width, width, w, ptr });
                }
                Rule::Apart { a, b, port, distance, wall, .. } => {
                    if hits_fill(a) || b.as_ref().is_some_and(hits_fill) {
                        issues.push(Issue::new(ptr.clone(), "`apart` can't name the fill space").with_hint(
                            "The fill space touches whatever the others leave: keep the others apart instead.",
                        ));
                    }
                    let d = distance.map_or(0.0, |l| l.0.to(self.ctx.units));
                    match (b, port) {
                        (Some(b), _) => out.push(C::Apart { a: pick(a), b: pick(b), d, w, ptr }),
                        (_, Some(role)) => out.push(C::ApartPort {
                            a: pick(a),
                            role: role.clone(),
                            lines: self.apart_lines(role, *wall),
                            wall: *wall,
                            d,
                            w,
                            ptr,
                        }),
                        _ => continue,
                    }
                }
                Rule::Near { a, b, distance, .. } => {
                    if hits_fill(a) || hits_fill(b) {
                        issues.push(Issue::new(ptr.clone(), "`near` can't name the fill space").with_hint(
                            "The fill space reaches everywhere the others don't: use plate.touches or plate.door with it.",
                        ));
                    }
                    out.push(C::Near { a: pick(a), b: pick(b), d: distance.0.to(self.ctx.units), w, ptr })
                }
                Rule::Faces { a, b, .. } => out.push(C::Faces { a: pick(a), b: pick(b), w, ptr }),
                Rule::Along { .. } => {
                    issues.push(Issue::new(ptr, "`along` needs the spine_stack engine").with_hint(
                        "Add engine = 'spine_stack to this plan, or use plate.door / plate.touches.",
                    ))
                }
                // Walls are built by the host once the level is placed.
                Rule::Reach { .. } | Rule::Unique { .. } | Rule::Instance { .. } | Rule::Wall { .. } => {}
            }
        }
        self.ordered = ordered;
        out
    }

    /// Does a `wall` rule build a wall between these two (name, type key)?
    fn is_walled(&self, x: (&str, &str), y: (&str, &str)) -> bool {
        self.walled.iter().any(|(a, b)| {
            (a.matches(x.0, x.1) && b.matches(y.0, y.1)) || (a.matches(y.0, y.1) && b.matches(x.0, x.1))
        })
    }

    /// A door rule between `a` and `b` that can't be built as written: a door in an edge with no
    /// wall, or an open edge where a `wall` rule builds one.
    fn check_edge(&self, a: &Selector, b: &Selector, gap: Gap, ptr: &str, issues: &mut Vec<Issue>) {
        // Every instance (and the fill), by (name, type key, type, is it the fill).
        let mut all: Vec<(&str, &SpaceType, bool)> =
            self.inst.iter().map(|i| (i.name.as_str(), &self.types[i.type_index], false)).collect();
        if let Some(f) = &self.fill {
            all.push((f.name.as_str(), &self.types[f.type_index], true));
        }
        let of = |s: &Selector| -> Vec<(&str, &SpaceType, bool)> {
            all.iter().filter(|(n, t, _)| s.matches(n, &t.key)).copied().collect()
        };
        let (aa, bb) = (of(a), of(b));
        for &(na, ta, fa) in &aa {
            for &(nb, tb, fb) in &bb {
                if na == nb || ta.space.is_fixture() || tb.space.is_fixture() {
                    continue;
                }
                let walled = self.is_walled((na, &ta.key), (nb, &tb.key));
                if gap.leaf == Leaf::Swing && !walled && ta.open_to(fa, tb, fb) {
                    issues.push(
                        Issue::new(ptr, format!("{na} and {nb} have no wall between them for a door"))
                            .with_hint(format!(
                                "Use plate.opening for a gap with no door, or build a wall: plate.wall \"{}\" \"{}\".",
                                ta.key, tb.key
                            )),
                    );
                    return;
                }
                if gap.full && walled {
                    issues.push(
                        Issue::new(
                            ptr,
                            format!(
                                "{na} and {nb} are open to each other, and a wall rule builds a wall there"
                            ),
                        )
                        .with_hint(
                            "Remove one of the two rules, or use plate.opening for a gap in the wall.",
                        ),
                    );
                    return;
                }
            }
        }
    }

    /// Put pinned instances where `at` says, at their exact size, in cells.
    fn pin(&self, rects: &mut [R], at: &[Option<Point>], issues: &mut Vec<Issue>) {
        let g = &self.grid;
        for (i, p) in at.iter().enumerate() {
            let Some(p) = p else { continue };
            let t = &self.types[self.inst[i].type_index];
            let (w, h) = (t.width.min, t.depth.min);
            let x = ((p.x - g.ox) / g.gx).round() as i32;
            let y = ((p.y - g.oy) / g.gy).round() as i32;
            let cw = ((w / g.gx).round() as i32).max(1);
            let ch = ((h / g.gy).round() as i32).max(1);
            let off = (x as f64 * g.gx + g.ox - p.x).abs().max((y as f64 * g.gy + g.oy - p.y).abs());
            if off > 0.01 * g.gx.min(g.gy) {
                issues.push(
                    Issue::preferred(
                        format!("/spaces/{}/at", t.key),
                        format!("{} is moved {} to sit on the grid", self.inst[i].name, num(off)),
                    )
                    .with_hint(
                        "Put its corner on a multiple of the grid, or set the plan's `grid` to suit it.",
                    ),
                );
            }
            rects[i] = R { x, y, w: cw, h: ch, on: true, f: 0 };
        }
    }

    /// Required instances in rows, at their target size.
    fn initial(&self) -> Vec<R> {
        let g = &self.grid;
        let mut rects = Vec::with_capacity(self.inst.len());
        let (mut x, mut y, mut row) = (0, 0, 0);
        for (i, inst) in self.inst.iter().enumerate() {
            let t = &self.types[inst.type_index];
            let area = t.target_area(self.ctx.units);
            let wd = if t.width.min > 0.0 { t.width.target(0.0) } else { area.sqrt() };
            let ht = if t.depth.min > 0.0 { t.depth.target(0.0) } else { area / wd };
            // A fixture's cells hold its whole size: round up.
            let cells = |v: f64, g: f64| {
                if self.fix[inst.type_index].is_some() {
                    (v / g - 1e-9).ceil() as i32
                } else {
                    (v / g).round() as i32
                }
            };
            let w = cells(wd, g.gx).clamp(1, g.nx);
            let h = cells(ht, g.gy).clamp(1, g.ny);
            if x + w > g.nx {
                x = 0;
                y += row;
                row = 0;
            }
            let r = R { x, y: y.min(g.ny - h), w, h, on: self.mandatory[i], f: 0 };
            x += w;
            row = row.max(h);
            rects.push(r);
        }
        rects
    }

    fn shared(&self, a: &R, b: &R) -> f64 {
        let g = &self.grid;
        if a.x1() == b.x || b.x1() == a.x {
            overlap(a.y as f64, a.y1() as f64, b.y as f64, b.y1() as f64) * g.gy
        } else if a.y1() == b.y || b.y1() == a.y {
            overlap(a.x as f64, a.x1() as f64, b.x as f64, b.x1() as f64) * g.gx
        } else {
            0.0
        }
    }

    /// The wall `a` and `b` share, in the level's coordinates.
    fn shared_segment(&self, a: &R, b: &R) -> Option<(Point, Point)> {
        let g = &self.grid;
        let px = |i: f64| g.ox + i * g.gx;
        let py = |j: f64| g.oy + j * g.gy;
        let span = |a0: i32, a1: i32, b0: i32, b1: i32| {
            let (s, e) = (a0.max(b0), a1.min(b1));
            (e > s).then_some((s as f64, e as f64))
        };
        for (x, other) in [(a.x1(), b.x), (a.x, b.x1())] {
            if x == other {
                if let Some((s, e)) = span(a.y, a.y1(), b.y, b.y1()) {
                    return Some((Point::new(px(x as f64), py(s)), Point::new(px(x as f64), py(e))));
                }
            }
        }
        for (y, other) in [(a.y1(), b.y), (a.y, b.y1())] {
            if y == other {
                if let Some((s, e)) = span(a.x, a.x1(), b.x, b.x1()) {
                    return Some((Point::new(px(s), py(y as f64)), Point::new(px(e), py(y as f64))));
                }
            }
        }
        None
    }

    fn overlap_cells(a: &R, b: &R) -> i32 {
        let w = (a.x1().min(b.x1()) - a.x.max(b.x)).max(0);
        let h = (a.y1().min(b.y1()) - a.y.max(b.y)).max(0);
        w * h
    }

    /// The longest stretch of `r`'s walls on a port with `role`: (length, segment).
    fn on_port(&self, r: &R, role: &str) -> (f64, Option<(Point, Point)>) {
        let g = &self.grid;
        let tol = 0.6;
        let mut best = (0.0, None);
        for p in self.ports.iter().filter(|p| p.seg.has(role)) {
            let (lines, from, to, scale) = if p.horizontal {
                ([r.y as f64, r.y1() as f64], r.x as f64, r.x1() as f64, g.gx)
            } else {
                ([r.x as f64, r.x1() as f64], r.y as f64, r.y1() as f64, g.gy)
            };
            for line in lines {
                if (line - p.at).abs() <= tol {
                    let len = overlap(from, to, p.from, p.to) * scale;
                    if len > best.0 {
                        let (s, e) = (from.max(p.from), to.min(p.to));
                        let seg = if p.horizontal {
                            let y = g.oy + p.at * g.gy;
                            (Point::new(g.ox + s * g.gx, y), Point::new(g.ox + e * g.gx, y))
                        } else {
                            let x = g.ox + p.at * g.gx;
                            (Point::new(x, g.oy + s * g.gy), Point::new(x, g.oy + e * g.gy))
                        };
                        best = (len, Some(seg));
                    }
                }
            }
        }
        best
    }

    /// What `apart` keeps away from for a port `role`: the ports themselves, or with `wall`, the
    /// whole edges of the slot they lie on.
    fn apart_lines(&self, role: &str, wall: bool) -> Vec<PortLine> {
        let mut out: Vec<PortLine> = Vec::new();
        for p in self.ports.iter().filter(|p| p.seg.has(role)) {
            let line = if wall {
                self.edges
                    .iter()
                    .find(|e| {
                        e.horizontal == p.horizontal
                            && (e.at - p.at).abs() <= 0.6
                            && overlap(e.from, e.to, p.from, p.to) > 0.0
                    })
                    .map(|e| PortLine { seg: p.seg.clone(), ..e.clone() })
                    .unwrap_or_else(|| p.clone())
            } else {
                p.clone()
            };
            let same = |o: &PortLine| {
                o.horizontal == line.horizontal
                    && (o.at - line.at).abs() < 1e-9
                    && (o.from - line.from).abs() < 1e-9
                    && (o.to - line.to).abs() < 1e-9
            };
            if !out.iter().any(same) {
                out.push(line);
            }
        }
        out
    }

    /// Length of `r`'s walls on the line `l`.
    fn line_shared(&self, r: &R, l: &PortLine) -> f64 {
        let g = &self.grid;
        let (sides, from, to, scale) = if l.horizontal {
            ([r.y as f64, r.y1() as f64], r.x as f64, r.x1() as f64, g.gx)
        } else {
            ([r.x as f64, r.x1() as f64], r.y as f64, r.y1() as f64, g.gy)
        };
        sides
            .iter()
            .filter(|&&side| (side - l.at).abs() <= 0.6)
            .map(|_| overlap(from, to, l.from, l.to) * scale)
            .fold(0.0, f64::max)
    }

    /// The gap between `r` and the line `l` (0 where they meet).
    fn line_gap(&self, r: &R, l: &PortLine) -> f64 {
        let g = &self.grid;
        let (lo, hi, from, to, across, along) = if l.horizontal {
            (r.y as f64, r.y1() as f64, r.x as f64, r.x1() as f64, g.gy, g.gx)
        } else {
            (r.x as f64, r.x1() as f64, r.y as f64, r.y1() as f64, g.gx, g.gy)
        };
        let off = (lo - l.at).max(l.at - hi).max(0.0);
        let off = if off <= 0.6 { 0.0 } else { off };
        let by = (l.from - to).max(from - l.to).max(0.0);
        ((off * across).powi(2) + (by * along).powi(2)).sqrt()
    }

    /// How far `r` is from keeping `d` (and no shared wall) from every line in `lines`.
    fn too_close_to_lines(&self, r: &R, lines: &[PortLine], d: f64) -> f64 {
        lines.iter().map(|l| self.line_shared(r, l) + (d - self.line_gap(r, l)).max(0.0)).sum()
    }

    /// How far `a` and `b` are from keeping `d` apart (and no shared wall).
    fn too_close(&self, a: &R, b: &R, d: f64) -> f64 {
        let s = self.shared(a, b);
        if d > 0.0 {
            s + (d - self.gap(a, b)).max(0.0)
        } else {
            s
        }
    }

    fn gap(&self, a: &R, b: &R) -> f64 {
        let g = &self.grid;
        let dx = (b.x - a.x1()).max(a.x - b.x1()).max(0) as f64 * g.gx;
        let dy = (b.y - a.y1()).max(a.y - b.y1()).max(0) as f64 * g.gy;
        (dx * dx + dy * dy).sqrt()
    }

    /// The longest run of fill cells along the outside of `r`'s sides, `ok` saying which free
    /// cells count: (length, segment).
    fn free_run(&self, r: &R, ok: &dyn Fn(i32, i32) -> bool) -> (f64, Option<(Point, Point)>) {
        let g = &self.grid;
        let mut best: (f64, Option<(Point, Point)>) = (0.0, None);
        // (cells along the side, the outside cell for step k, the wall line's ends for a run)
        let sides: [(i32, i32, bool); 4] =
            [(r.y - 1, r.y, true), (r.x1(), r.x1(), false), (r.y1(), r.y1(), true), (r.x - 1, r.x, false)];
        for (out, line, horizontal) in sides {
            let (from, to) = if horizontal { (r.x, r.x1()) } else { (r.y, r.y1()) };
            let mut start = None;
            for k in from..=to {
                let free = k < to && if horizontal { ok(k, out) } else { ok(out, k) };
                match (free, start) {
                    (true, None) => start = Some(k),
                    (false, Some(s0)) => {
                        let len = (k - s0) as f64 * if horizontal { g.gx } else { g.gy };
                        if len > best.0 {
                            let seg = if horizontal {
                                (g.point(s0, line), g.point(k, line))
                            } else {
                                (g.point(line, s0), g.point(line, k))
                            };
                            best = (len, Some(seg));
                        }
                        start = None;
                    }
                    _ => {}
                }
            }
        }
        best
    }

    /// The longest run of fill cells along the slot's ports with `role`: (length, segment).
    fn port_run(&self, role: &str, ok: &dyn Fn(i32, i32) -> bool) -> (f64, Option<(Point, Point)>) {
        let g = &self.grid;
        let mut best: (f64, Option<(Point, Point)>) = (0.0, None);
        for p in self.ports.iter().filter(|p| p.seg.has(role)) {
            let at = p.at.round() as i32;
            // Every cell the port runs along, even partly: a port need not sit on the grid (a
            // door the level above placed), and the length that counts is the part on the port.
            let (from, to) = ((p.from + 1e-6).floor() as i32, (p.to - 1e-6).ceil() as i32);
            if to <= from {
                continue;
            }
            let mid = (from + to) / 2;
            // The row (or column) of cells on the inside of the port.
            let row = if p.horizontal {
                if g.cell_inside(mid, at) {
                    at
                } else {
                    at - 1
                }
            } else if g.cell_inside(at, mid) {
                at
            } else {
                at - 1
            };
            let mut start = None;
            for k in from..=to {
                let free = k < to && if p.horizontal { ok(k, row) } else { ok(row, k) };
                match (free, start) {
                    (true, None) => start = Some(k),
                    (false, Some(s0)) => {
                        let (a, b) = ((s0 as f64).max(p.from), (k as f64).min(p.to));
                        let len = (b - a).max(0.0) * if p.horizontal { g.gx } else { g.gy };
                        if len > best.0 {
                            let seg = if p.horizontal {
                                let y = g.oy + at as f64 * g.gy;
                                (Point::new(g.ox + a * g.gx, y), Point::new(g.ox + b * g.gx, y))
                            } else {
                                let x = g.ox + at as f64 * g.gx;
                                (Point::new(x, g.oy + a * g.gy), Point::new(x, g.oy + b * g.gy))
                            };
                            best = (len, Some(seg));
                        }
                        start = None;
                    }
                    _ => {}
                }
            }
        }
        best
    }

    /// How far the fill falls short of its width beside `r`: for each free cell along `r`'s
    /// sides, the cells missing before the fill is `k` deep (it runs into another space or the
    /// edge sooner).
    fn sliver(&self, r: &R, occ: &Occ, k: i32) -> i32 {
        if k <= 1 {
            return 0;
        }
        let g = &self.grid;
        let mut miss = 0;
        let mut probe = |x: i32, y: i32, dx: i32, dy: i32| {
            if !occ.is_free(g, x, y) {
                return;
            }
            let mut d = 1;
            while d < k && occ.is_free(g, x + dx * d, y + dy * d) {
                d += 1;
            }
            if d < k {
                miss += k - d;
            }
        };
        for x in r.x..r.x1() {
            probe(x, r.y - 1, 0, -1);
            probe(x, r.y1(), 0, 1);
        }
        for y in r.y..r.y1() {
            probe(r.x - 1, y, -1, 0);
            probe(r.x1(), y, 1, 0);
        }
        miss
    }

    /// The fill's area and corners: from the counts kept in `occ`.
    fn fill_counts(&self, occ: &Occ, hs: f64, exact: bool) -> f64 {
        let Some(fill) = &self.fill else { return 0.0 };
        let g = &self.grid;
        // Too much floor left for the fill is a lesser fault than pushing the other spaces past
        // their limits: when the program can't cover the floor, the fill takes the excess.
        let area = occ.free as f64 * g.gx * g.gy;
        let over = if area > fill.area.max { 0.5 } else { 1.0 };
        let mut c = over * HARD * hs * fill.area.miss(area) / self.a0;
        if exact {
            // A hall with fewer jogs: every corner past four costs a little.
            c += self.compactness * 0.25 * (occ.corners - 4).max(0) as f64;
        }
        c
    }

    /// Floor cut off from the fill's main piece isn't part of it: it's left over. When the fill
    /// is big that's too slow to check on every move: annealing leaves it to polishing then.
    fn cut_pen(&self, occ: &Occ, hs: f64, exact: bool) -> f64 {
        let g = &self.grid;
        if self.fill.is_none() || (!exact && occ.free > 2000) {
            return 0.0;
        }
        HARD * hs * (occ.cut(g) + self.hold_missing(occ)) as f64 * g.gx * g.gy / self.a0
    }

    /// Cells the fill lacks to hold its plan's biggest fixture (see [`FillSpec::hold`]): what
    /// the best placed window of that size is short of.
    fn hold_missing(&self, occ: &Occ) -> i32 {
        let Some((w, h, _)) = self.fill.as_ref().and_then(|f| f.hold.as_ref()) else { return 0 };
        let g = &self.grid;
        let (nx, ny) = (g.nx as usize, g.ny as usize);
        let stride = nx + 1;
        let mut pre = vec![0i32; stride * (ny + 1)];
        for j in 0..ny {
            for i in 0..nx {
                let v = occ.is_free(g, i as i32, j as i32) as i32;
                pre[(j + 1) * stride + i + 1] =
                    v + pre[(j + 1) * stride + i] + pre[j * stride + i + 1] - pre[j * stride + i];
            }
        }
        let cells = |v: f64, step: f64| ((v / step) - 1e-6).ceil().max(1.0) as usize;
        let mut missing = (cells(*w, g.gx) * cells(*h, g.gy)) as i32;
        for (cw, ch) in [(cells(*w, g.gx), cells(*h, g.gy)), (cells(*h, g.gx), cells(*w, g.gy))] {
            if cw > nx || ch > ny {
                continue;
            }
            let need = (cw * ch) as i32;
            for j in 0..=ny - ch {
                for i in 0..=nx - cw {
                    let free = pre[(j + ch) * stride + i + cw]
                        - pre[j * stride + i + cw]
                        - pre[(j + ch) * stride + i]
                        + pre[j * stride + i];
                    missing = missing.min(need - free);
                    if missing == 0 {
                        return 0;
                    }
                }
            }
        }
        missing
    }

    /// The fill terms of one rectangle, which only depend on the floor right around it: the
    /// fill's width beside it, and its rules to reach the fill.
    fn fill_near(&self, i: usize, rects: &[R], occ: &Occ, hs: f64, exact: bool) -> f64 {
        let Some(fill) = &self.fill else { return 0.0 };
        let r = &rects[i];
        if !r.on {
            return 0.0;
        }
        let g = &self.grid;
        let free = |a: i32, b: i32| occ.is_free(g, a, b);
        let mut c = 0.0;
        if exact && self.fixture(i).is_none() {
            c += HARD * hs * self.sliver(r, occ, fill.k) as f64 * g.gx * g.gy / self.a0;
            // Corners of two spaces too close, with fill squeezing between them (half the
            // pair's cost here, half in the other's).
            for (j, s) in rects.iter().enumerate() {
                if j != i && s.on && self.fixture(j).is_none() && self.pinch_free(r, s, occ, fill.k) {
                    let w = self.pinch_width(r, s);
                    c += HARD * hs * ((fill.k - w) * fill.k) as f64 / 4.0 * g.gx * g.gy / self.a0;
                }
            }
        }
        let mut run = None;
        for rule in &self.rules {
            if let C::Touch { a, b: Target::Fill(bs), need, w, .. } = rule {
                if !a.contains(&i) {
                    continue;
                }
                let near = bs
                    .iter()
                    .filter(|&&bi| bi != i && rects[bi].on)
                    .map(|&bi| self.shared(r, &rects[bi]))
                    .fold(0.0, f64::max);
                let reach = *run.get_or_insert_with(|| self.free_run(r, &free).0);
                let w = if *w >= HARD { w * hs } else { *w };
                c += w * (need - near.max(reach)).max(0.0) / self.l0;
            }
        }
        c
    }

    /// The fill terms about the fill itself: its doors and windows.
    fn fill_rest(&self, rects: &[R], occ: &Occ, hs: f64) -> f64 {
        let g = &self.grid;
        let free = |i: i32, j: i32| occ.is_free(g, i, j);
        let wh = |w: f64| if w >= HARD { w * hs } else { w };
        let mut c = 0.0;
        for rule in &self.rules {
            match rule {
                C::FillTouch { b, need, w, .. } => {
                    c += wh(*w) * (need - self.fill_reach(b, rects, &free)).max(0.0) / self.l0;
                }
                C::FillWindow { role, need, w, .. } => {
                    c += wh(*w) * (need - self.port_run(role, &free).0).max(0.0) / self.l0;
                }
                _ => {}
            }
        }
        c
    }

    /// How much the fill shares with the target of a `FillTouch`.
    fn fill_reach(&self, b: &Target, rects: &[R], ok: &dyn Fn(i32, i32) -> bool) -> f64 {
        match b {
            Target::Port(role) => self.port_run(role, ok).0,
            Target::Spaces(bs) | Target::Fill(bs) => bs
                .iter()
                .filter(|&&bi| rects[bi].on)
                .map(|&bi| self.free_run(&rects[bi], ok).0)
                .fold(0.0, f64::max),
        }
    }

    /// Cells of the fill, labelled by connected piece; the main piece is the largest.
    fn fill_pieces(&self, occ: &Occ) -> (Vec<usize>, usize, Vec<i32>) {
        let g = &self.grid;
        let (labels, sizes) = outline::components(g.nx, g.ny, |i, j| occ.is_free(g, i, j));
        let main = (0..sizes.len()).max_by_key(|&k| sizes[k]).unwrap_or(usize::MAX);
        (labels, main, sizes)
    }

    fn fixture(&self, i: usize) -> Option<&Fix> {
        self.fix[self.inst[i].type_index].as_ref()
    }

    /// Width and depth limits and the size that counts, for instance `i` in rectangle `r`.
    fn shape(&self, i: usize, r: &R) -> Shape {
        let g = &self.grid;
        let t = &self.types[self.inst[i].type_index];
        let (dw, dh) = (r.w as f64 * g.gx, r.h as f64 * g.gy);
        if self.fixture(i).is_none() {
            let (width, depth, _) = t.gross_limits();
            let o1 = width.miss(dw) + depth.miss(dh);
            let o2 = width.miss(dh) + depth.miss(dw);
            return Shape { w: dw, h: dh, miss: o1.min(o2), compact: true };
        }
        // Along the front and front to back; a limit may be up to a cell over, since the fixture
        // is drawn at its own size inside its cells.
        let (along, deep, ga, gd) =
            if r.f.is_multiple_of(2) { (dw, dh, g.gx, g.gy) } else { (dh, dw, g.gy, g.gx) };
        let up = |b: Bounds1, cell: f64| Bounds1 {
            min: b.min,
            max: if b.max.is_finite() { (b.max / cell - 1e-9).ceil() * cell } else { b.max },
        };
        let miss = up(t.width, ga).miss(along) + up(t.depth, gd).miss(deep);
        Shape { w: along.min(t.width.max), h: deep.min(t.depth.max), miss, compact: false }
    }

    /// Clearance of fixture `i` on each side of its rectangle (bottom, right, top, left), in
    /// cells.
    fn clear_cells(&self, i: usize, r: &R) -> [i32; 4] {
        let Some(fix) = self.fixture(i) else { return [0; 4] };
        let g = &self.grid;
        let mut out = [0; 4];
        for (side, cells) in out.iter_mut().enumerate() {
            let c = fix.clear[(side + 4 - r.f as usize) % 4];
            let step = if side % 2 == 0 { g.gy } else { g.gx };
            *cells = (c / step - 1e-9).ceil().max(0.0) as i32;
        }
        out
    }

    /// Fixture `i` with its clearance, or none.
    fn zone(&self, i: usize, r: &R) -> Option<R> {
        let c = self.clear_cells(i, r);
        (c != [0; 4]).then_some(R {
            x: r.x - c[3],
            y: r.y - c[0],
            w: r.w + c[1] + c[3],
            h: r.h + c[0] + c[2],
            ..*r
        })
    }

    /// Cells of fixture `i`'s clearance outside the slot.
    fn zone_outside(&self, i: usize, r: &R) -> i32 {
        let Some(z) = self.zone(i, r) else { return 0 };
        let g = &self.grid;
        (z.w * z.h - g.inside(&z)) - (r.w * r.h - g.inside(r))
    }

    /// Cells of `b`'s body standing in `a`'s clearance (fixture `ai` in `a`). A zone is open
    /// floor: it may share a clearance. So may a fixture round the corner from `a`'s end (see
    /// [`Self::round_the_corner`]).
    fn in_clearance(&self, ai: usize, a: &R, bi: usize, b: &R) -> i32 {
        if self.types[self.inst[bi].type_index].space.kind == SpaceKind::Zone {
            return 0;
        }
        if self.fixture(bi).is_some() && Self::round_the_corner(a, b) {
            return 0;
        }
        match self.zone(ai, a) {
            Some(z) => Self::overlap_cells(&z, b) - Self::overlap_cells(a, b),
            None => 0,
        }
    }

    /// Is fixture `b` round the corner from fixture `a`: in front of one end of `a`, against its
    /// front, and backed towards that end (so against the wall `a` runs into)? That's an L, as
    /// kitchens are built: a counter along one wall, the range on the next wall right after the
    /// corner. `b` takes the corner of `a`'s clear floor, which nobody stands in anyway.
    fn round_the_corner(a: &R, b: &R) -> bool {
        let back = (b.f + 2) % 4;
        match a.f {
            // Front down or up: `b` touches that edge, within `a`'s width, at its left or right end.
            0 | 2 => {
                let touches = if a.f == 0 { b.y1() == a.y } else { b.y == a.y1() };
                touches
                    && b.x >= a.x
                    && b.x1() <= a.x1()
                    && ((b.x == a.x && back == 3) || (b.x1() == a.x1() && back == 1))
            }
            // Front right or left: within `a`'s height, at its bottom or top end.
            _ => {
                let touches = if a.f == 1 { b.x == a.x1() } else { b.x1() == a.x };
                touches
                    && b.y >= a.y
                    && b.y1() <= a.y1()
                    && ((b.y == a.y && back == 0) || (b.y1() == a.y1() && back == 2))
            }
        }
    }

    /// Cells of `r` where a door swings.
    fn in_swing(&self, r: &R) -> i32 {
        self.swings.iter().map(|s| Self::overlap_cells(r, s)).sum()
    }

    /// How much of fixture `i`'s back is not against what its `against` asks for.
    fn off_wall(&self, i: usize, r: &R) -> f64 {
        let Some(role) = self.fixture(i).and_then(|f| f.against.as_deref()) else { return 0.0 };
        let g = &self.grid;
        let back = (r.f as usize + 2) % 4;
        let (len, cell) = if back.is_multiple_of(2) { (r.w, g.gx) } else { (r.h, g.gy) };
        if role == ANY_WALL {
            // The row of cells behind the back must be outside the slot, and not open floor.
            let strip = match back {
                0 => R { y: r.y - 1, h: 1, ..*r },
                1 => R { x: r.x1(), w: 1, ..*r },
                2 => R { y: r.y1(), h: 1, ..*r },
                _ => R { x: r.x - 1, w: 1, ..*r },
            };
            let (horizontal, at, from, to) = match back {
                0 => (true, r.y, r.x, r.x1()),
                1 => (false, r.x1(), r.y, r.y1()),
                2 => (true, r.y1(), r.x, r.x1()),
                _ => (false, r.x, r.y, r.y1()),
            };
            let open: f64 = self
                .ports
                .iter()
                .filter(|p| p.horizontal == horizontal && p.seg.has(OPEN) && (at as f64 - p.at).abs() <= 0.6)
                .map(|p| overlap(from as f64, to as f64, p.from, p.to))
                .sum();
            return g.inside(&strip) as f64 * cell + open * cell;
        }
        let (horizontal, at, from, to) = match back {
            0 => (true, r.y, r.x, r.x1()),
            1 => (false, r.x1(), r.y, r.y1()),
            2 => (true, r.y1(), r.x, r.x1()),
            _ => (false, r.x, r.y, r.y1()),
        };
        let best = self
            .ports
            .iter()
            .filter(|p| p.horizontal == horizontal && p.seg.has(role) && (at as f64 - p.at).abs() <= 0.6)
            .map(|p| overlap(from as f64, to as f64, p.from, p.to))
            .fold(0.0, f64::max);
        (len as f64 - best).max(0.0) * cell
    }

    /// Everything wrong with a state, as issues: requirements not met (with what to change),
    /// and preferences missed.
    fn report(&self, rects: &[R], out: &mut Vec<Issue>) {
        let g = &self.grid;
        let cell = g.gx * g.gy;
        let sym = self.ctx.units.symbol();
        let tol = 1e-6;
        // Snapping to the grid can leave a size a hair off its limit: don't report that.
        let slack = |limit: f64| 1e-6 + 0.025 * limit.abs();
        let occ = Occ::build(g, rects);
        let (labels, main, _) = self.fill_pieces(&occ);
        // Fill cells that count for rules: the main piece only (the rest is cut off).
        let in_fill = |i: i32, j: i32| {
            occ.is_free(g, i, j) && main != usize::MAX && labels[(j * g.nx + i) as usize] == main
        };
        let fill_name = self.fill.as_ref().map_or(String::new(), |f| f.name.clone());
        let required_area: f64 = (0..rects.len())
            .filter(|&i| rects[i].on)
            .map(|i| {
                let t = &self.types[self.inst[i].type_index];
                t.area.min.max(t.side.min.powi(2))
            })
            .sum::<f64>()
            + self.fill.as_ref().map_or(0.0, |f| f.area.min);
        let short = required_area > g.inside_cells as f64 * cell * 0.97;
        let crowded = if short {
            format!(
                "The spaces need {} {sym}² and there's {} {sym}² here: make some smaller or the outline bigger.",
                num(required_area),
                num(g.inside_cells as f64 * cell)
            )
        } else {
            "Loosen its sizes (or its neighbours'), or try another seed.".to_owned()
        };
        let mut say = |ptr: String, msg: String, hint: String| out.push(Issue::new(ptr, msg).with_hint(hint));
        let fixture_level = (0..rects.len()).any(|i| self.fixture(i).is_some());
        let room_word = if fixture_level { "the room" } else { "the outline" };
        if let Some((fill, (w, h, what))) = self.fill.as_ref().and_then(|f| f.hold.as_ref().map(|h| (f, h))) {
            if self.hold_missing(&occ) > 0 {
                say(
                    format!("/spaces/{}", self.types[fill.type_index].key),
                    format!("{} has no room for its {what} ({} × {} {sym} with the floor it keeps clear)", fill.name, num(*w), num(*h)),
                    format!("Leave {} a part that big: lower the other spaces' sizes, or give it a larger side.min.", fill.name),
                );
            }
        }

        for (i, r) in rects.iter().enumerate() {
            if !r.on {
                continue;
            }
            let t = &self.types[self.inst[i].type_index];
            let name = &self.inst[i].name;
            let ptr = || format!("/spaces/{}", t.key);
            let inside = g.inside(r);
            if r.w * r.h > inside {
                say(
                    ptr(),
                    format!("{name} doesn't fit inside"),
                    format!("Make {room_word} bigger, or {name} smaller."),
                );
            }
            let shape = self.shape(i, r);
            let (dw, dh) = (shape.w, shape.h);
            let area = dw * dh;
            let (_, _, limit) = t.gross_limits();
            if limit.miss(area) > slack(area) {
                if area < limit.min {
                    say(
                        format!("{}/area", ptr()),
                        format!("{name} is {} {sym}², less than {}", num(area), num(limit.min)),
                        if short {
                            crowded.clone()
                        } else {
                            format!("Nothing else gives way to it. Lower its area.min, make its neighbours smaller, or make {room_word} bigger.")
                        },
                    );
                } else {
                    say(
                        format!("{}/area", ptr()),
                        format!("{name} is {} {sym}², more than {}", num(area), num(limit.max)),
                        "The floor around it has nowhere else to go. Raise its area.max, or let another space (or a `fill = true` space) take the rest.".into(),
                    );
                }
            }
            if t.side.miss(dw) + t.side.miss(dh) > slack(dw.min(dh)) {
                say(
                    format!("{}/side", ptr()),
                    format!("{name} is {} × {}, outside its side limits", num(dw), num(dh)),
                    "Its shape can't fit what's around it. Loosen its side limits or its area, or its neighbours' sizes.".into(),
                );
            }
            if shape.miss > slack(dw.min(dh)) {
                say(
                    ptr(),
                    format!("{name} is {} × {}, outside its width/depth limits", num(dw), num(dh)),
                    if self.fixture(i).is_some() {
                        format!("There's no room for it at that size: make {room_word} bigger or loosen its width/depth.")
                    } else {
                        "Its width or depth can't fit here: loosen them, or the sizes of the spaces around it.".into()
                    },
                );
            }
            let ratio = dw.max(dh) / dw.min(dh);
            if ratio > t.aspect + tol {
                say(
                    format!("{}/aspect", ptr()),
                    format!("{name} is {} times longer than wide", num(ratio)),
                    "Raise its `aspect`, or loosen its sizes so it can be squarer.".into(),
                );
            }
            if self.zone_outside(i, r) > 0 {
                say(
                    format!("{}/clearance", ptr()),
                    format!("{name}'s clearance runs outside"),
                    format!("Make {room_word} bigger, or the clearance smaller."),
                );
            }
            if self.fixture(i).is_some() && self.in_swing(r) > 0 {
                say(
                    ptr(),
                    format!("{name} stands in a door's swing"),
                    format!("Make {room_word} bigger so it can stand clear of the door."),
                );
            }
            let off = self.off_wall(i, r);
            if off > tol && off > slack(dw) {
                let wall = match self.fixture(i).and_then(|f| f.against.as_deref()) {
                    Some(ANY_WALL) | None => "a wall".to_owned(),
                    Some(role) => format!("a '{role} edge"),
                };
                say(
                    format!("{}/against", ptr()),
                    format!("{name}'s back isn't against {wall}"),
                    format!("No free stretch of {wall} is long enough: make {room_word} bigger, or let it stand elsewhere (against = 'wall)."),
                );
            }
        }

        let mut overlaps = Vec::new();
        for i in 0..rects.len() {
            if !rects[i].on {
                continue;
            }
            for j in i + 1..rects.len() {
                if !rects[j].on {
                    continue;
                }
                if Self::overlap_cells(&rects[i], &rects[j]) > 0 {
                    overlaps.push((i, j));
                }
                for (a, b) in [(i, j), (j, i)] {
                    if self.in_clearance(a, &rects[a], b, &rects[b]) > 0 {
                        let t = &self.types[self.inst[a].type_index];
                        say(
                            format!("/spaces/{}/clearance", t.key),
                            format!("{} stands in {}'s clearance", self.inst[b].name, self.inst[a].name),
                            format!("There isn't room for both: make {room_word} bigger, or a size or clearance smaller."),
                        );
                    }
                }
            }
        }
        for (i, j) in overlaps {
            let t = &self.types[self.inst[i].type_index];
            say(
                format!("/spaces/{}", t.key),
                format!("{} overlaps {}", self.inst[i].name, self.inst[j].name),
                crowded.clone(),
            );
        }

        if let Some(fill) = &self.fill {
            let ptr = format!("/spaces/{}", self.types[fill.type_index].key);
            let area = occ.free as f64 * cell;
            if fill.area.miss(area) > slack(area) {
                if area < fill.area.min {
                    say(
                        format!("{ptr}/area"),
                        format!("{} is {} {sym}², less than {}", fill.name, num(area), num(fill.area.min)),
                        "The other spaces leave too little: make them smaller (lower some area.max), or lower its area.min.".into(),
                    );
                } else {
                    say(
                        format!("{ptr}/area"),
                        format!("{} is {} {sym}², more than {}", fill.name, num(area), num(fill.area.max)),
                        "The other spaces leave too much: let them grow (raise some area.max), add a space, or raise its area.max.".into(),
                    );
                }
            }
            let rooms: Vec<usize> =
                (0..rects.len()).filter(|&i| rects[i].on && self.fixture(i).is_none()).collect();
            // Corner to corner, where the gap between the corners is fill.
            let pinched = rooms.iter().any(|&i| {
                rooms.iter().any(|&j| i < j && self.pinch_free(&rects[i], &rects[j], &occ, fill.k))
            });
            let thin: i32 =
                rooms.iter().map(|&i| self.sliver(&rects[i], &occ, fill.k)).sum::<i32>() + pinched as i32;
            if thin > 0 {
                say(
                    format!("{ptr}/side"),
                    format!("{} is narrower than {} in places", fill.name, num(fill.min_width)),
                    "Spaces stop short of each other or of the outline. Loosen their sizes so they meet, or lower its side.min.".into(),
                );
            }
        }

        let on = |i: &&usize| rects[**i].on;
        for rule in &self.rules {
            let weight = |w: f64| w >= HARD;
            let mut tell = |w: f64, ptr: &String, msg: String, hint: &str| {
                if weight(w) {
                    out.push(Issue::new(ptr.clone(), msg).with_hint(hint));
                } else {
                    out.push(Issue::preferred(ptr.clone(), msg));
                }
            };
            match rule {
                C::Touch { a, b, need, w, ptr, verb, .. } => {
                    for &ai in a.iter().filter(on) {
                        let near = |bs: &[usize]| {
                            bs.iter()
                                .filter(|&&bi| bi != ai && rects[bi].on)
                                .map(|&bi| self.shared(&rects[ai], &rects[bi]))
                                .fold(0.0, f64::max)
                        };
                        let best = match b {
                            Target::Spaces(bs) => near(bs),
                            Target::Port(role) => self.on_port(&rects[ai], role).0,
                            Target::Fill(bs) => near(bs).max(self.free_run(&rects[ai], &in_fill).0),
                        };
                        if need - best > tol {
                            let names = |bs: &[usize]| -> Vec<String> {
                                bs.iter()
                                    .filter(|&&bi| bi != ai)
                                    .map(|&bi| self.inst[bi].name.clone())
                                    .collect()
                            };
                            let (other, hint) = match b {
                                Target::Spaces(bs) => {
                                    let n = names(bs);
                                    (
                                        if n.is_empty() { "anything".to_owned() } else { n.join(" or ") },
                                        "They don't share a long enough wall. Loosen sizes around them, try another seed, or make it a preference (plate.prefer).".to_owned(),
                                    )
                                }
                                Target::Fill(bs) => {
                                    let mut n = names(bs);
                                    n.push(fill_name.clone());
                                    (
                                        n.join(" or "),
                                        format!("{} doesn't reach it. Raise {}'s area (or lower others') so it can, or try another seed.", fill_name, fill_name),
                                    )
                                }
                                Target::Port(role) => (
                                    format!("its '{}{}", shown(role), if role.contains('#') { " on every side" } else { "" }),
                                    format!("It doesn't reach a '{} edge. Check that `ports` marks one, loosen sizes so it can reach it, or make it a preference.", shown(role)),
                                ),
                            };
                            tell(*w, ptr, format!("{} doesn't {verb} {other}", self.inst[ai].name), &hint);
                        }
                    }
                }
                C::Window { a, role, need, w, ptr, .. } => {
                    for &ai in a.iter().filter(on) {
                        if need - self.on_port(&rects[ai], role).0 > tol {
                            tell(
                                *w,
                                ptr,
                                format!("{} has no '{role} wall for a window", self.inst[ai].name),
                                &format!("It doesn't reach a '{role} edge. Make room for it there (loosen the sizes of what's in the way), check `ports`, or make it a preference."),
                            );
                        }
                    }
                }
                C::FillTouch { b, need, w, ptr, verb, .. } => {
                    if need - self.fill_reach(b, rects, &in_fill) > tol {
                        let other = match b {
                            Target::Port(role) => format!("its '{}", shown(role)),
                            Target::Spaces(bs) | Target::Fill(bs) => bs
                                .iter()
                                .map(|&bi| self.inst[bi].name.clone())
                                .collect::<Vec<_>>()
                                .join(" or "),
                        };
                        tell(
                            *w,
                            ptr,
                            format!("{fill_name} doesn't {verb} {other}"),
                            &format!("Other spaces close it off. Leave {fill_name} a way through (lower some area.max), or try another seed."),
                        );
                    }
                }
                C::FillWindow { role, need, w, ptr, .. } => {
                    if need - self.port_run(role, &in_fill).0 > tol {
                        tell(
                            *w,
                            ptr,
                            format!("{fill_name} has no '{role} wall for a window"),
                            &format!("Other spaces take the whole '{role} edge. Leave it some (lower their sizes), or make it a preference."),
                        );
                    }
                }
                C::Apart { a, b, d, w, ptr } => {
                    for &ai in a.iter().filter(on) {
                        for &bi in b.iter().filter(on) {
                            // Each pair once when both sides pick the same spaces.
                            if bi == ai || (bi < ai && a.contains(&bi) && b.contains(&ai)) {
                                continue;
                            }
                            if self.too_close(&rects[ai], &rects[bi], *d) > tol {
                                let (na, nb) = (&self.inst[ai].name, &self.inst[bi].name);
                                let gap = self.gap(&rects[ai], &rects[bi]);
                                let msg = if self.shared(&rects[ai], &rects[bi]) > tol || *d <= 0.0 {
                                    format!("{na} touches {nb}")
                                } else {
                                    format!("{na} is {} from {nb}, less than {}", num(gap), num(*d))
                                };
                                tell(*w, ptr, msg, "There's no room to keep them apart. Loosen sizes, try another seed, or make it a preference.");
                            }
                        }
                    }
                }
                C::ApartPort { a, role, lines, wall, d, w, ptr } => {
                    let what = if *wall { format!("a wall with a '{role}") } else { format!("its '{role}") };
                    for &ai in a.iter().filter(on) {
                        let r = &rects[ai];
                        if self.too_close_to_lines(r, lines, *d) > tol {
                            let name = &self.inst[ai].name;
                            let touching = lines.iter().any(|l| self.line_shared(r, l) > tol);
                            let gap = lines.iter().map(|l| self.line_gap(r, l)).fold(f64::INFINITY, f64::min);
                            let msg = if touching || *d <= 0.0 {
                                format!("{name} touches {what}")
                            } else {
                                format!("{name} is {} from {what}, less than {}", num(gap), num(*d))
                            };
                            tell(*w, ptr, msg, &format!("There's nowhere else for it: make {room_word} bigger, or make it a preference."));
                        }
                    }
                }
                C::Faces { a, b, w, ptr } => {
                    for &ai in a.iter().filter(on) {
                        let best = b
                            .iter()
                            .filter(|&&bi| bi != ai && rects[bi].on)
                            .map(|&bi| self.not_facing(&rects[ai], &rects[bi]))
                            .fold(f64::INFINITY, f64::min);
                        if best.is_finite() && best > tol {
                            let names: Vec<&str> = b
                                .iter()
                                .filter(|&&bi| bi != ai)
                                .map(|&bi| self.inst[bi].name.as_str())
                                .collect();
                            tell(
                                *w,
                                ptr,
                                format!("{} doesn't face {}", self.inst[ai].name, names.join(" or ")),
                                &format!("There's no room in front of it for that. Make {room_word} bigger, or make it a preference."),
                            );
                        }
                    }
                }
                C::Near { a, b, d, w, ptr } => {
                    for &ai in a.iter().filter(on) {
                        let best = b
                            .iter()
                            .filter(|&&bi| bi != ai && rects[bi].on)
                            .map(|&bi| self.gap(&rects[ai], &rects[bi]))
                            .fold(f64::INFINITY, f64::min);
                        if best.is_finite() && best > d + tol {
                            tell(
                                *w,
                                ptr,
                                format!("{} is {} away, more than {}", self.inst[ai].name, num(best), num(*d)),
                                "Loosen sizes so they can sit closer, raise the distance, or make it a preference.",
                            );
                        }
                    }
                }
            }
        }
    }

    /// Hard violations left in a state (anything a requirement doesn't allow).
    fn unmet(&self, rects: &[R]) -> usize {
        let mut issues = Vec::new();
        self.report(rects, &mut issues);
        let short = self.needy.iter().filter(|&&k| self.clear_miss(k, rects) > 1e-6).count();
        issues.iter().filter(|i| i.level == Level::Required).count() + short
    }

    fn clamp(&self, r: &mut R) {
        let g = &self.grid;
        r.w = r.w.clamp(1, g.nx);
        r.h = r.h.clamp(1, g.ny);
        r.x = r.x.clamp(0, g.nx - r.w);
        r.y = r.y.clamp(0, g.ny - r.h);
    }

    /// Several independent annealing runs from scattered starts; the best one wins. When the
    /// best still breaks a requirement, more runs follow (up to three times as many), so hard
    /// plans get more effort and easy ones stay fast.
    fn anneal(&self, rects: &mut [R]) {
        let n = rects.len();
        if n == 0 {
            return;
        }
        let start = rects.to_vec();
        // A fixed budget of moves, split between independent runs: small plans get fewer runs.
        let runs = (n / 2 + 2).clamp(3, RESTARTS);
        let iterations = (n * 25_000).clamp(50_000, 300_000) / runs;
        // One run: its own random numbers, so runs can go side by side and still give the same
        // answer every time.
        let run_one = |run: usize| -> ((usize, f64), Vec<R>) {
            let mut rng = Rng::new(super::common::hash64(&[&self.ctx.seed.to_string(), &run.to_string()]));
            let mut state = start.clone();
            if run > 0 {
                for (i, r) in state.iter_mut().enumerate() {
                    if self.pinned[i] {
                        continue;
                    }
                    if self.fixture(i).is_some() {
                        turn(r, rng.below(4) as u8);
                    }
                    r.x = rng.below((self.grid.nx - r.w + 1).max(1) as usize) as i32;
                    r.y = rng.below((self.grid.ny - r.h + 1).max(1) as usize) as i32;
                }
            }
            self.anneal_once(&mut state, &mut rng, iterations);
            self.polish(&mut state);
            // Fewest broken requirements first: a small violation can cost less than the
            // goals, but a layout that meets every requirement is always the better one.
            ((self.unmet(&state), self.final_cost(&state)), state)
        };
        let mut best: Option<((usize, f64), Vec<R>)> = None;
        let debug = std::env::var_os("PLATE_DEBUG").is_some();
        let t0 = debug.then(std::time::Instant::now);
        for batch in 0..3 {
            if batch > 0
                && (best.as_ref().is_some_and(|((unmet, _), _)| *unmet == 0) || !self.ctx.can_retry(1.0))
            {
                break;
            }
            self.ctx.work.fetch_add((runs * iterations) as u64, std::sync::atomic::Ordering::Relaxed);
            if let Some(t) = t0 {
                eprintln!(
                    "grid n={n} cells={} fill={} batch={batch} runs={runs} it={iterations} best={:?} t={:?}",
                    self.grid.inside_cells,
                    self.fill.is_some(),
                    best.as_ref().map(|((u, c), _)| (*c as i64, *u)),
                    t.elapsed()
                );
            }
            let ids: Vec<usize> = (batch * runs..(batch + 1) * runs).collect();
            for (cost, state) in run_all(&ids, &run_one) {
                if best.as_ref().is_none_or(|(c, _)| cost.0 < c.0 || (cost.0 == c.0 && cost.1 < c.1)) {
                    best = Some((cost, state));
                }
            }
        }
        if let Some(t) = t0 {
            eprintln!(
                "grid done n={n} cells={} fill={} best={:?} t={:?} work={}",
                self.grid.inside_cells,
                self.fill.is_some(),
                best.as_ref().map(|((u, c), _)| (*c as i64, *u)),
                t.elapsed(),
                self.ctx.work.load(std::sync::atomic::Ordering::Relaxed)
            );
        }
        if let Some((_, state)) = best {
            if debug && std::env::var_os("PLATE_DEBUG_RECTS").is_some() {
                for (i, r) in state.iter().enumerate() {
                    eprintln!("    {} {:?}", self.inst[i].name, r);
                }
            }
            rects.copy_from_slice(&state);
        }
    }

    /// The cost runs are compared by: the annealing cost, plus fill cut off from its main piece.
    fn final_cost(&self, rects: &[R]) -> f64 {
        self.exact(rects, 1.5)
    }

    fn anneal_once(&self, rects: &mut [R], rng: &mut Rng, iterations: usize) {
        let n = rects.len();
        let g = &self.grid;
        let span = g.nx.max(g.ny);
        let optional: Vec<usize> = (0..n).filter(|&i| !self.mandatory[i]).collect();
        let movable: Vec<usize> = (0..n).filter(|&i| !self.pinned[i]).collect();
        let (t0, t1) = (2.0f64, 0.003f64);
        let hs_at = |frac: f64| 0.5 + frac;
        let members = self.membership();
        let mut hs = hs_at(0.0);
        let mut fill = self.fill.as_ref().map(|_| FillState::new(self, rects, hs, false));
        let mut cost = self.total(rects, hs);
        let mut best = (cost, rects.to_vec());

        for k in 0..iterations {
            let frac = k as f64 / iterations as f64;
            let temp = t0 * (t1 / t0).powf(frac);
            if k % 1000 == 0 {
                // The running total must match a full evaluation (checks the incremental terms).
                debug_assert!(
                    (cost - self.total(rects, hs)).abs() < 1e-6 * (1.0 + cost.abs()),
                    "incremental cost drifted: {cost} vs {}",
                    self.total(rects, hs)
                );
                hs = hs_at(frac);
                cost = self.total(rects, hs);
                if fill.is_some() {
                    fill = Some(FillState::new(self, rects, hs, false));
                }
                best.0 = self.total(&best.1, hs);
            }
            let reach = 1 + ((1.0 - frac) * span as f64 / 3.0) as i32;
            let active: Vec<usize> = movable.iter().copied().filter(|&i| rects[i].on).collect();
            if active.is_empty() && optional.is_empty() {
                break;
            }
            self.ctx.moves.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let roll = rng.f64();
            let mut changed: Vec<(usize, R)> = Vec::with_capacity(2);
            if (roll < 0.08 && !optional.is_empty()) || active.is_empty() {
                let i = optional[rng.below(optional.len())];
                let mut r = rects[i];
                r.on = !r.on;
                if r.on {
                    r.x = rng.below(g.nx as usize) as i32;
                    r.y = rng.below(g.ny as usize) as i32;
                }
                self.clamp(&mut r);
                changed.push((i, r));
            } else {
                let i = active[rng.below(active.len())];
                let mut r = rects[i];
                if roll < 0.24 && self.fixture(i).is_none() {
                    // Move a wall: one side of this space, and the facing sides of the spaces
                    // across it, by the same amount: one grows as the others shrink.
                    let side = rng.below(4);
                    let d = rng.around(1 + ((1.0 - frac) * 3.0) as i32);
                    if d != 0 {
                        let line = match side {
                            0 => r.y,
                            1 => r.x1(),
                            2 => r.y1(),
                            _ => r.x,
                        };
                        move_side(&mut r, side, d);
                        for (j, s) in rects.iter().enumerate() {
                            if j == i || !s.on || self.pinned[j] || self.fixture(j).is_some() {
                                continue;
                            }
                            let facing = match side {
                                0 => s.y1() == line && s.x < r.x1() && r.x < s.x1(),
                                1 => s.x == line && s.y < r.y1() && r.y < s.y1(),
                                2 => s.y == line && s.x < r.x1() && r.x < s.x1(),
                                _ => s.x1() == line && s.y < r.y1() && r.y < s.y1(),
                            };
                            if facing {
                                let mut s = *s;
                                move_side(&mut s, (side + 2) % 4, d);
                                if s.w >= 1 && s.h >= 1 {
                                    changed.push((j, s));
                                }
                            }
                        }
                    }
                } else if roll < 0.35 {
                    r.x += rng.around(reach);
                    r.y += rng.around(reach);
                } else if roll < 0.44 && self.fixture(i).is_none() && n > 1 {
                    // Line a side up with the nearest parallel edge of another space (or the
                    // slot's), so walls meet in straight lines instead of leaving jogs.
                    let side = rng.below(4);
                    let at = match side {
                        0 => r.y,
                        1 => r.x1(),
                        2 => r.y1(),
                        _ => r.x,
                    };
                    let horizontal = side.is_multiple_of(2);
                    let mut best: Option<i32> = None;
                    let mut consider = |line: i32| {
                        let d = line - at;
                        if d != 0 && d.abs() <= 2 * reach && best.is_none_or(|b| d.abs() < (b - at).abs()) {
                            best = Some(line);
                        }
                    };
                    for (j, s) in rects.iter().enumerate() {
                        if j == i || !s.on || self.fixture(j).is_some() {
                            continue;
                        }
                        if horizontal {
                            consider(s.y);
                            consider(s.y1());
                        } else {
                            consider(s.x);
                            consider(s.x1());
                        }
                    }
                    consider(0);
                    consider(if horizontal { g.ny } else { g.nx });
                    if let Some(line) = best {
                        move_side(&mut r, side, line - at);
                    }
                } else if roll < 0.62 {
                    let d = rng.around(1 + ((1.0 - frac) * 4.0) as i32);
                    match rng.below(4) {
                        0 => {
                            r.x -= d;
                            r.w += d;
                        }
                        1 => r.w += d,
                        2 => {
                            r.y -= d;
                            r.h += d;
                        }
                        _ => r.h += d,
                    }
                } else if roll < 0.82 && n > 1 {
                    // Dock: put this space against another one (or a pinned one), on a random side.
                    let j = rng.below(n);
                    if j != i && rects[j].on {
                        let s = rects[j];
                        match rng.below(4) {
                            0 => r.x = s.x1(),
                            1 => r.x = s.x - r.w,
                            2 => r.y = s.y1(),
                            _ => r.y = s.y - r.h,
                        }
                        let slide = |lo: i32, len: i32, olen: i32, rng: &mut Rng| {
                            lo - len + 1 + rng.below((len + olen - 1).max(1) as usize) as i32
                        };
                        if r.x == s.x1() || r.x == s.x - r.w {
                            r.y = slide(s.y, r.h, s.h, rng);
                        } else {
                            r.x = slide(s.x, r.w, s.w, rng);
                        }
                    }
                } else if roll < 0.92 && active.len() > 1 {
                    let j = active[rng.below(active.len())];
                    if j != i {
                        let mut s = rects[j];
                        let (ci, cj) = ((2 * r.x + r.w, 2 * r.y + r.h), (2 * s.x + s.w, 2 * s.y + s.h));
                        r.x = (cj.0 - r.w) / 2;
                        r.y = (cj.1 - r.h) / 2;
                        s.x = (ci.0 - s.w) / 2;
                        s.y = (ci.1 - s.h) / 2;
                        self.clamp(&mut s);
                        changed.push((j, s));
                    }
                } else if self.fixture(i).is_some() {
                    // A quarter turn either way, or turn around.
                    turn(&mut r, 1 + rng.below(3) as u8);
                } else if rng.below(2) == 0 {
                    turn(&mut r, 1);
                } else {
                    // Reshape at about the same area: wider and shallower, or the other way,
                    // keeping one corner where it is.
                    let d = rng.around(1 + ((1.0 - frac) * 3.0) as i32);
                    let area = r.w * r.h;
                    let w = (r.w + d).max(1);
                    let h = ((area + w / 2) / w).max(1);
                    if rng.below(2) == 0 {
                        r.x += r.w - w;
                    }
                    if rng.below(2) == 0 {
                        r.y += r.h - h;
                    }
                    r.w = w;
                    r.h = h;
                }
                self.clamp(&mut r);
                changed.push((i, r));
            }

            // Apply the change one rectangle at a time, adding up the change in cost.
            let old: Vec<(usize, R)> = changed.iter().map(|&(i, _)| (i, rects[i])).collect();
            let mut delta = 0.0;
            for &(i, r) in &changed {
                delta += self.apply(i, r, rects, &members, hs, &mut fill);
            }
            let undo = fill.as_mut().map(|f| {
                let moved: Vec<(usize, R, R)> =
                    old.iter().zip(&changed).map(|(&(i, before), &(_, after))| (i, before, after)).collect();
                let (d, undo) = f.update(self, rects, &moved, hs);
                delta += d;
                undo
            });
            let u = rng.f64();
            let accept = |delta: f64| delta <= 0.0 || u < (-delta / temp).exp();
            if let Some(f) = fill.as_mut() {
                // The cut-off floor costs a flood fill: skip it when even losing all of it
                // couldn't make the move acceptable.
                if accept(delta - f.cut) {
                    delta += f.recut(self, hs);
                } else {
                    delta = f64::INFINITY;
                }
            }
            if accept(delta) {
                cost += delta;
                if cost < best.0 {
                    best = (cost, rects.to_vec());
                }
            } else {
                for (i, r) in old.into_iter().rev() {
                    if let Some(f) = fill.as_mut() {
                        f.occ.shift(g, &rects[i], &r);
                    }
                    rects[i] = r;
                }
                if let (Some(f), Some(u)) = (fill.as_mut(), undo) {
                    f.restore(u);
                }
            }
        }
        rects.copy_from_slice(&best.1);
    }

    /// Grow, shrink and nudge each rectangle by one cell, and move the walls it shares with its
    /// neighbours by one cell, while that helps: the last few cells a requirement is short of.
    fn polish(&self, rects: &mut [R]) {
        let members = self.membership();
        let hs = 1.5;
        let g = &self.grid;
        let mut fill = self.fill.as_ref().map(|_| FillState::new(self, rects, hs, true));
        for _ in 0..30 {
            let mut improved = false;
            for i in 0..rects.len() {
                if !rects[i].on || self.pinned[i] {
                    continue;
                }
                let moves = if self.fixture(i).is_some() { 15 } else { 28 };
                for m in 0..moves {
                    let mut r = rects[i];
                    let mut changed: Vec<(usize, R)> = Vec::new();
                    match m {
                        0 => r.w += 1,
                        1 => {
                            r.x -= 1;
                            r.w += 1
                        }
                        2 => r.h += 1,
                        3 => {
                            r.y -= 1;
                            r.h += 1
                        }
                        4 => r.w -= 1,
                        5 => {
                            r.x += 1;
                            r.w -= 1
                        }
                        6 => r.h -= 1,
                        7 => {
                            r.y += 1;
                            r.h -= 1
                        }
                        8 => r.x += 1,
                        9 => r.x -= 1,
                        10 => r.y += 1,
                        11 => r.y -= 1,
                        12..=19 if self.fixture(i).is_none() => {
                            // Move a wall: this side and the facing sides across it.
                            let side = (m - 12) % 4;
                            let d = if m < 16 { 1 } else { -1 };
                            changed = self.wall_shift(rects, i, side, d);
                            if changed.is_empty() {
                                continue;
                            }
                            r = changed.pop().map(|(_, r)| r).unwrap();
                        }
                        20..=27 => {
                            // Reshape at about the same area: a cell deeper and narrower to
                            // match, or the other way, anchored at one of its corners.
                            let k = m - 20;
                            let (deeper, from_start, other_start) = (k < 4, k % 2 == 0, (k / 2) % 2 == 0);
                            let area = r.w * r.h;
                            if deeper {
                                let h = r.h + 1;
                                let w = (area / h).max(1);
                                if !from_start {
                                    r.y -= 1;
                                }
                                if !other_start {
                                    r.x += r.w - w;
                                }
                                r.w = w;
                                r.h = h;
                            } else {
                                let w = r.w + 1;
                                let h = (area / w).max(1);
                                if !from_start {
                                    r.x -= 1;
                                }
                                if !other_start {
                                    r.y += r.h - h;
                                }
                                r.w = w;
                                r.h = h;
                            }
                        }
                        _ => turn(&mut r, (m - 11) as u8),
                    }
                    if r.w < 1 || r.h < 1 || r.x < 0 || r.y < 0 || r.x1() > g.nx || r.y1() > g.ny {
                        continue;
                    }
                    changed.push((i, r));
                    let old: Vec<(usize, R)> = changed.iter().map(|&(j, _)| (j, rects[j])).collect();
                    let mut delta = 0.0;
                    for &(j, nr) in &changed {
                        delta += self.apply(j, nr, rects, &members, hs, &mut fill);
                    }
                    let undo = match fill.as_mut() {
                        Some(f) => {
                            let moved: Vec<(usize, R, R)> =
                                old.iter().zip(&changed).map(|(&(j, b), &(_, a))| (j, b, a)).collect();
                            let (d, u) = f.update(self, rects, &moved, hs);
                            delta += d + f.recut(self, hs);
                            Some(u)
                        }
                        None => None,
                    };
                    if delta < -1e-9 {
                        improved = true;
                    } else {
                        for &(j, b) in old.iter().rev() {
                            if let Some(f) = fill.as_mut() {
                                f.occ.shift(g, &rects[j], &b);
                            }
                            rects[j] = b;
                        }
                        if let (Some(f), Some(u)) = (fill.as_mut(), undo) {
                            f.restore(u);
                        }
                    }
                }
            }
            if !improved {
                break;
            }
        }
    }

    /// Move side `side` of rectangle `i` by `d` cells, with the facing sides of the rectangles
    /// across it: the new rectangles, `i`'s last (empty if nothing would be left of one).
    fn wall_shift(&self, rects: &[R], i: usize, side: usize, d: i32) -> Vec<(usize, R)> {
        let mut r = rects[i];
        let line = match side {
            0 => r.y,
            1 => r.x1(),
            2 => r.y1(),
            _ => r.x,
        };
        move_side(&mut r, side, d);
        if r.w < 1 || r.h < 1 {
            return Vec::new();
        }
        let mut out = Vec::new();
        for (j, s) in rects.iter().enumerate() {
            if j == i || !s.on || self.fixture(j).is_some() {
                continue;
            }
            let facing = match side {
                0 => s.y1() == line && s.x < r.x1() && r.x < s.x1(),
                1 => s.x == line && s.y < r.y1() && r.y < s.y1(),
                2 => s.y == line && s.x < r.x1() && r.x < s.x1(),
                _ => s.x1() == line && s.y < r.y1() && r.y < s.y1(),
            };
            if facing {
                if self.pinned[j] {
                    return Vec::new();
                }
                let mut s = *s;
                move_side(&mut s, (side + 2) % 4, d);
                if s.w < 1 || s.h < 1 {
                    return Vec::new();
                }
                out.push((j, s));
            }
        }
        out.push((i, r));
        out
    }

    /// Rename instances of each type by position: left to right, then top to bottom.
    fn renumber(&mut self, rects: &mut Vec<R>) {
        let mut order: Vec<usize> = (0..rects.len()).filter(|&i| rects[i].on).collect();
        order.sort_by_key(|&i| {
            let r = rects[i];
            (self.inst[i].type_index, 2 * r.x + r.w, -(2 * r.y + r.h))
        });
        let mut counters = vec![0usize; self.types.len()];
        let mut inst = Vec::with_capacity(order.len());
        let mut kept = Vec::with_capacity(order.len());
        let mut mandatory = Vec::with_capacity(order.len());
        let mut pinned = Vec::with_capacity(order.len());
        for i in order {
            let ti = self.inst[i].type_index;
            inst.push(Named { type_index: ti, name: self.types[ti].instance_name(counters[ti]) });
            counters[ti] += 1;
            kept.push(rects[i]);
            mandatory.push(true);
            pinned.push(self.pinned[i]);
        }
        self.inst = inst;
        self.mandatory = mandatory;
        self.pinned = pinned;
        *rects = kept;
    }

    /// Doors, windows, ports and the placed spaces (the fill space last, if there is one).
    fn result(&self, rects: Vec<R>, issues: Vec<Issue>, slot: &Slot) -> LevelResult {
        let mm = self.ctx.units.to_mm();
        let g = &self.grid;
        let n = rects.len();
        let occ = Occ::build(g, &rects);
        let (labels, main, _) = self.fill_pieces(&occ);
        let fill_on = self.fill.is_some() && main != usize::MAX;
        let in_fill =
            |i: i32, j: i32| fill_on && occ.is_free(g, i, j) && labels[(j * g.nx + i) as usize] == main;
        let fill_name = self.fill.as_ref().map_or(String::new(), |f| f.name.clone());
        let name_of = |i: usize| if i == n { fill_name.clone() } else { self.inst[i].name.clone() };
        let len = |s: &(Point, Point)| s.0.distance(s.1);
        let mut openings = Vec::new();
        // (i, j) pairs with a door or an opening between them; `n` stands for the fill space.
        let mut doors: Vec<(usize, usize)> = Vec::new();
        // Those with no wall at all (`width = 'full`).
        let mut opens: Vec<(usize, usize)> = Vec::new();
        let has_door = |doors: &Vec<(usize, usize)>, a: usize, b: usize| {
            doors.contains(&(a, b)) || doors.contains(&(b, a))
        };
        for rule in &self.rules {
            match rule {
                C::Touch { a, b, door: Some(gap), .. } => {
                    let width = &gap.min;
                    for &ai in a {
                        let ra = &rects[ai];
                        match b {
                            Target::Spaces(bs) | Target::Fill(bs) => {
                                let mut best = bs
                                    .iter()
                                    .filter(|&&bi| bi != ai && !has_door(&doors, ai, bi))
                                    .filter_map(|&bi| self.shared_segment(ra, &rects[bi]).map(|s| (bi, s)))
                                    .max_by(|x, y| len(&x.1).total_cmp(&len(&y.1)));
                                if matches!(b, Target::Fill(_)) && !has_door(&doors, ai, n) {
                                    if let (_, Some(seg)) = self.free_run(ra, &in_fill) {
                                        if best.as_ref().is_none_or(|(_, s)| len(s) < len(&seg)) {
                                            best = Some((n, seg));
                                        }
                                    }
                                }
                                if let Some((bi, seg)) = best {
                                    let corners = g.rect(ra).corners();
                                    // A room's door goes by a corner, leaving it wall to furnish; a
                                    // door between spaces with plans of their own (wings, flats)
                                    // goes mid-wall, where both plans can best meet it.
                                    let planned = |i: usize| {
                                        let t = if i == n {
                                            self.fill.as_ref().map(|f| &self.types[f.type_index])
                                        } else {
                                            Some(&self.types[self.inst[i].type_index])
                                        };
                                        t.and_then(|t| t.space.plan.as_ref())
                                            .is_some_and(|p| p.spaces.values().any(|s| !s.is_fixture()))
                                    };
                                    let at = if gap.full {
                                        None
                                    } else if gap.leaf == Leaf::NoLeaf {
                                        centred(seg, gap.width_on(len(&seg)), gap.margin)
                                    } else if planned(ai) && planned(bi) {
                                        centred(seg, *width, gap.margin)
                                    } else {
                                        by_corner(seg, *width, gap.margin, &corners)
                                    };
                                    if gap.full {
                                        doors.push((ai, bi));
                                        opens.push((ai, bi));
                                    } else if let Some((p, q)) = at {
                                        doors.push((ai, bi));
                                        openings.push(Opening {
                                            kind: ElementKind::Door,
                                            leaf: gap.leaf,
                                            a: p,
                                            b: q,
                                            between: vec![self.inst[ai].name.clone(), name_of(bi)],
                                        });
                                    }
                                }
                            }
                            Target::Port(_) if gap.full => {}
                            Target::Port(role) => {
                                if let (_, Some(seg)) = self.on_port(ra, role) {
                                    if let Some((p, q)) = centred(seg, gap.width_on(len(&seg)), gap.margin) {
                                        openings.push(Opening {
                                            kind: ElementKind::Door,
                                            leaf: gap.leaf,
                                            a: p,
                                            b: q,
                                            between: vec![
                                                self.inst[ai].name.clone(),
                                                format!("'{}", shown(role)),
                                            ],
                                        });
                                    }
                                }
                            }
                        }
                    }
                }
                C::FillTouch { b, door: Some(gap), .. } if fill_on => match b {
                    Target::Port(_) if gap.full => {}
                    Target::Port(role) => {
                        if let (_, Some(seg)) = self.port_run(role, &in_fill) {
                            if let Some((p, q)) = centred(seg, gap.width_on(len(&seg)), gap.margin) {
                                openings.push(Opening {
                                    kind: ElementKind::Door,
                                    leaf: gap.leaf,
                                    a: p,
                                    b: q,
                                    between: vec![fill_name.clone(), format!("'{}", shown(role))],
                                });
                            }
                        }
                    }
                    Target::Spaces(bs) | Target::Fill(bs) => {
                        let best = bs
                            .iter()
                            .filter(|&&bi| !has_door(&doors, bi, n))
                            .filter_map(|&bi| self.free_run(&rects[bi], &in_fill).1.map(|s| (bi, s)))
                            .max_by(|x, y| len(&x.1).total_cmp(&len(&y.1)));
                        if let Some((bi, seg)) = best {
                            if gap.full {
                                doors.push((n, bi));
                                opens.push((n, bi));
                            } else if let Some((p, q)) = centred(seg, gap.width_on(len(&seg)), gap.margin) {
                                doors.push((n, bi));
                                openings.push(Opening {
                                    kind: ElementKind::Door,
                                    leaf: gap.leaf,
                                    a: p,
                                    b: q,
                                    between: vec![fill_name.clone(), self.inst[bi].name.clone()],
                                });
                            }
                        }
                    }
                },
                C::Window { a, role, width, .. } => {
                    for &ai in a {
                        if let (_, Some(seg)) = self.on_port(&rects[ai], role) {
                            if let Some((p, q)) = centred(seg, *width, 300.0 / mm) {
                                openings.push(Opening {
                                    kind: ElementKind::Window,
                                    leaf: Leaf::Swing,
                                    a: p,
                                    b: q,
                                    between: vec![self.inst[ai].name.clone(), format!("'{role}")],
                                });
                            }
                        }
                    }
                }
                C::FillWindow { role, width, .. } if fill_on => {
                    if let (_, Some(seg)) = self.port_run(role, &in_fill) {
                        if let Some((p, q)) = centred(seg, *width, 300.0 / mm) {
                            openings.push(Opening {
                                kind: ElementKind::Window,
                                leaf: Leaf::Swing,
                                a: p,
                                b: q,
                                between: vec![fill_name.clone(), format!("'{role}")],
                            });
                        }
                    }
                }
                _ => {}
            }
        }

        // The fill space's outline: its main piece (the rest is left over).
        let fill_shape = fill_on.then(|| {
            let region = outline::regions(g.nx, g.ny, in_fill).into_iter().max_by_key(|r| r.cells);
            region.map(|r| {
                let to = |l: &[(i32, i32)]| l.iter().map(|&(i, j)| g.point(i, j)).collect::<Vec<Point>>();
                (to(&r.outer), r.holes.iter().map(|h| to(h)).collect::<Vec<_>>())
            })
        });
        let fill_shape = fill_shape.flatten();
        let fill_walls: Vec<(Point, Point)> = fill_shape
            .iter()
            .flat_map(|(outer, holes)| std::iter::once(outer).chain(holes.iter()))
            .flat_map(|l| (0..l.len()).map(move |k| (l[k], l[(k + 1) % l.len()])))
            .collect();

        let tol = 1e-6 * (g.gx + g.gy);
        let walls: Vec<Vec<(Point, Point)>> = rects.iter().map(|r| g.rect(r).edges().to_vec()).collect();
        let fixed: Vec<bool> = (0..n).map(|i| self.fixture(i).is_some()).collect();
        let door_segs: Vec<(Point, Point)> = openings
            .iter()
            .filter(|o| o.kind == ElementKind::Door && o.leaf == Leaf::Swing)
            .map(|o| (o.a, o.b))
            .collect();
        let gap_segs: Vec<(Point, Point)> = openings
            .iter()
            .filter(|o| o.kind == ElementKind::Door && o.leaf == Leaf::NoLeaf)
            .map(|o| (o.a, o.b))
            .collect();
        let window_segs: Vec<(Point, Point)> =
            openings.iter().filter(|o| o.kind == ElementKind::Window).map(|o| (o.a, o.b)).collect();
        let slot_segments: Vec<(Point, Point)> = slot.ports.iter().map(|p| (p.a, p.b)).collect();
        // What's built between two neighbours, and what that says about the other.
        let type_of = |i: usize| {
            if i == n {
                &self.types[self.fill.as_ref().unwrap().type_index]
            } else {
                &self.types[self.inst[i].type_index]
            }
        };
        let edge = |i: usize, j: usize| -> Edge {
            if opens.contains(&(i, j)) || opens.contains(&(j, i)) {
                Edge::Open
            } else if has_door(&doors, i, j) {
                Edge::Door
            } else if self.is_walled((&name_of(i), &type_of(i).key), (&name_of(j), &type_of(j).key)) {
                Edge::Wall
            } else if type_of(i).open_to(i == n, type_of(j), j == n) {
                Edge::Open
            } else {
                Edge::Wall
            }
        };
        let roles_from = |i: usize, j: usize, t: &SpaceType| t.roles_from(type_of(j), edge(i, j));
        let mut issues = issues;
        let mut placed = Vec::new();
        for (i, r) in rects.iter().enumerate() {
            let t = &self.types[self.inst[i].type_index];
            if let Some(fix) = self.fixture(i) {
                placed.extend(self.place_fixture(i, r, fix));
                continue;
            }
            let rect = g.rect(r);
            let mut sources: Vec<Source> = slot_segments
                .iter()
                .zip(&slot.ports)
                .map(|(seg, p)| Source {
                    segments: std::slice::from_ref(seg),
                    roles: p.roles.clone(),
                    other: None,
                    occupies: false,
                })
                .collect();
            for (j, other) in walls.iter().enumerate() {
                // Fixtures stand in a space; they don't make its walls.
                if j == i || fixed[j] {
                    continue;
                }
                sources.push(Source {
                    segments: other,
                    roles: roles_from(i, j, t),
                    other: Some(self.inst[j].name.clone()),
                    occupies: true,
                });
            }
            if !fill_walls.is_empty() {
                sources.push(Source {
                    segments: &fill_walls,
                    roles: roles_from(i, n, t),
                    other: Some(fill_name.clone()),
                    occupies: true,
                });
            }
            let mut ports = ports_along(&walls[i], &sources, None, tol);
            ports.extend(door_ports(&walls[i], &door_segs, tol));
            ports.extend(gap_ports(&walls[i], &gap_segs, tol));
            ports.extend(window_ports(&walls[i], &window_segs, tol));
            // A zone walled all round is a room: say so.
            if t.space.kind == SpaceKind::Zone && !ports.iter().any(|p| p.has(OPEN) || p.has(OPENING)) {
                issues.push(
                    Issue::preferred(
                        format!("/spaces/{}", t.key),
                        format!("{} is a zone with no open edge", self.inst[i].name),
                    )
                    .with_hint(
                        "Zones have no walls of their own only where they meet open floor: let it touch another zone, circulation or the fill (plate.open), or make it a room.",
                    ),
                );
            }
            let to_level = Transform::translate(rect.x0, rect.y0);
            let back = to_level.inverse();
            placed.push(Placed {
                name: self.inst[i].name.clone(),
                type_key: t.key.clone(),
                polygon: rect.corners(),
                rect: Some((rect.w(), rect.h(), to_level)),
                ports: ports.iter().map(|p| p.transformed(&back)).collect(),
                fixture: None,
                holes: Vec::new(),
            });
        }
        if let (Some(fill), Some((outer, holes))) = (&self.fill, fill_shape) {
            // Its ports, like a room's: the slot's edges it runs along, and the walls of the
            // spaces around it (in this level's frame: its plan is solved here).
            let t = &self.types[fill.type_index];
            let mut sources: Vec<Source> = slot_segments
                .iter()
                .zip(&slot.ports)
                .map(|(seg, p)| Source {
                    segments: std::slice::from_ref(seg),
                    roles: p.roles.clone(),
                    other: None,
                    occupies: false,
                })
                .collect();
            for (j, other) in walls.iter().enumerate() {
                if !fixed[j] {
                    sources.push(Source {
                        segments: other,
                        roles: roles_from(n, j, t),
                        other: Some(self.inst[j].name.clone()),
                        occupies: true,
                    });
                }
            }
            let mut ports = ports_along(&fill_walls, &sources, None, tol);
            ports.extend(door_ports(&fill_walls, &door_segs, tol));
            ports.extend(gap_ports(&fill_walls, &gap_segs, tol));
            ports.extend(window_ports(&fill_walls, &window_segs, tol));
            placed.push(Placed {
                name: fill.name.clone(),
                type_key: t.key.clone(),
                polygon: outer,
                rect: None,
                ports,
                fixture: None,
                holes,
            });
        }
        LevelResult { placed, openings, issues }
    }

    /// A fixture at its own size inside its cells: back against the cells' back edge, centred
    /// along its front. Its clearance is measured from there.
    fn place_fixture(&self, i: usize, r: &R, fix: &Fix) -> Vec<Placed> {
        let cells = self.grid.rect(r);
        let shape = self.shape(i, r);
        let (along, deep) = (shape.w, shape.h);
        let (cx, cy) = ((cells.x0 + cells.x1) / 2.0, (cells.y0 + cells.y1) / 2.0);
        let rect = match r.f {
            0 => Rect::new(cx - along / 2.0, cells.y1 - deep, cx + along / 2.0, cells.y1),
            1 => Rect::new(cells.x0, cy - along / 2.0, cells.x0 + deep, cy + along / 2.0),
            2 => Rect::new(cx - along / 2.0, cells.y0, cx + along / 2.0, cells.y0 + deep),
            _ => Rect::new(cells.x1 - deep, cy - along / 2.0, cells.x1, cy + along / 2.0),
        };
        let f = r.f as usize;
        let t = &self.types[self.inst[i].type_index];
        // A row is cut into its pieces along the front; each keeps the clearance in front and
        // behind it, and the end pieces the clearance at the ends.
        let n = t.row.max(1) as usize;
        let horizontal = f.is_multiple_of(2);
        // The order the pieces come in along the front, left to right as seen facing it.
        let forward = f == 0 || f == 1;
        (0..n)
            .map(|k| {
                let (a, b) = if forward { (k, k + 1) } else { (n - k - 1, n - k) };
                let piece = if horizontal {
                    let w = rect.w() / n as f64;
                    Rect::new(rect.x0 + a as f64 * w, rect.y0, rect.x0 + b as f64 * w, rect.y1)
                } else {
                    let h = rect.h() / n as f64;
                    Rect::new(rect.x0, rect.y0 + a as f64 * h, rect.x1, rect.y0 + b as f64 * h)
                };
                let corners = piece.corners();
                // Clearance on each side of the rectangle (bottom, right, top, left), without the
                // ends between pieces.
                let mut c: Vec<f64> = (0..4).map(|side| fix.clear[(side + 4 - f) % 4]).collect();
                if n > 1 {
                    let (lo, hi) = if horizontal { (3, 1) } else { (0, 2) };
                    if a > 0 {
                        c[lo] = 0.0;
                    }
                    if b < n {
                        c[hi] = 0.0;
                    }
                }
                let clearance = c.iter().any(|v| *v > 0.0).then(|| {
                    Rect::new(piece.x0 - c[3], piece.y0 - c[0], piece.x1 + c[1], piece.y1 + c[2]).corners()
                });
                let name = if n > 1 { format!("{}_{}", t.key, k + 1) } else { self.inst[i].name.clone() };
                Placed {
                    name,
                    type_key: t.key.clone(),
                    polygon: corners.clone(),
                    rect: Some((piece.w(), piece.h(), Transform::translate(piece.x0, piece.y0))),
                    ports: Vec::new(),
                    fixture: Some(FixtureOut { front: (corners[f], corners[(f + 1) % 4]), clearance }),
                    holes: Vec::new(),
                }
            })
            .collect()
    }
}

/// Which instances each rule concerns, for incremental evaluation.
struct Membership {
    in_a: Vec<Vec<bool>>,
    in_b: Vec<Vec<bool>>,
}

impl Problem<'_> {
    fn membership(&self) -> Membership {
        let n = self.inst.len();
        let mark = |idx: &[usize]| {
            let mut v = vec![false; n];
            for &i in idx {
                v[i] = true;
            }
            v
        };
        let mut in_a = Vec::new();
        let mut in_b = Vec::new();
        for rule in &self.rules {
            let (a, b): (&[usize], &[usize]) = match rule {
                _ if rule.global() => (&[], &[]),
                C::Touch { a, b: Target::Spaces(b), .. } => (a, b),
                C::Touch { a, .. } | C::Window { a, .. } | C::ApartPort { a, .. } => (a, &[]),
                C::Apart { a, b, .. } | C::Near { a, b, .. } | C::Faces { a, b, .. } => (a, b),
                C::FillTouch { .. } | C::FillWindow { .. } => (&[], &[]),
            };
            in_a.push(mark(a));
            in_b.push(mark(b));
        }
        Membership { in_a, in_b }
    }

    /// Work out what the walls around each space with clear needs will take from it. Plans with
    /// no such space (most) pay nothing for any of this, here or while annealing.
    fn foresee_walls(&mut self, plan: &Plan, slot: &Slot) {
        let n = self.inst.len();
        self.needy = (0..n).filter(|&i| self.types[self.inst[i].type_index].clear.is_some()).collect();
        if self.needy.is_empty() {
            return;
        }
        let foresight = Foresight::new(plan, self.ctx.units);
        // The door rule between `k` and `j`, if there is one: is it open floor?
        let gap_between = |k: usize, j: usize| {
            self.rules.iter().find_map(|rule| match rule {
                C::Touch { a, b: Target::Spaces(b) | Target::Fill(b), door: Some(g), .. }
                    if j < n
                        && ((a.contains(&k) && b.contains(&j)) || (a.contains(&j) && b.contains(&k))) =>
                {
                    Some(g.full)
                }
                C::Touch { a, b: Target::Fill(_), door: Some(g), .. } if j == n && a.contains(&k) => {
                    Some(g.full)
                }
                C::FillTouch { b: Target::Spaces(b) | Target::Fill(b), door: Some(g), .. }
                    if j == n && b.contains(&k) =>
                {
                    Some(g.full)
                }
                _ => None,
            })
        };
        let fill_type = self.fill.as_ref().map(|f| f.type_index);
        let mut between = vec![0.0; n * (n + 1)];
        let mut ports = vec![Vec::new(); n];
        for &k in &self.needy {
            let (tk, name) = (&self.types[self.inst[k].type_index], self.inst[k].name.as_str());
            for j in 0..=n {
                let (tj, other) = match (j == n, fill_type) {
                    (false, _) => (&self.types[self.inst[j].type_index], self.inst[j].name.as_str()),
                    (true, Some(ti)) => (&self.types[ti], self.fill.as_ref().unwrap().name.as_str()),
                    (true, None) => continue,
                };
                if j == k || tj.space.is_fixture() {
                    continue;
                }
                let edge = match gap_between(k, j) {
                    Some(true) => Edge::Open,
                    Some(false) => Edge::Door,
                    None if self.is_walled((name, &tk.key), (other, &tj.key)) => Edge::Wall,
                    None if tk.open_to(false, tj, j == n) => Edge::Open,
                    None => Edge::Wall,
                };
                let mut roles = tk.roles_from(tj, edge);
                for r in tj.roles_from(tk, edge) {
                    if !roles.contains(&r) {
                        roles.push(r);
                    }
                }
                between[k * (n + 1) + j] =
                    foresight.intrusion(&roles, (name, &tk.key), Some((other, &tj.key)));
            }
            if self.ctx.root && slot.boundary.is_some() {
                for (p, line) in self.ports.iter().enumerate() {
                    let roles = &line.seg.roles;
                    if is_gap(roles) {
                        continue;
                    }
                    let depth = foresight.intrusion(roles, (name, &tk.key), None);
                    if depth > 0.0 {
                        ports[k].push((p, depth));
                    }
                }
            }
        }
        let deepest = (0..n)
            .map(|k| {
                let row = &between[k * (n + 1)..(k + 1) * (n + 1)];
                let on_ports = ports[k].iter().map(|p: &(usize, f64)| p.1);
                row.iter().copied().chain(on_ports).fold(0.0, f64::max)
            })
            .collect();
        self.walls = Foreseen { between, ports, deepest };
    }

    /// What the walls will take from needy instance `k` on each side (bottom, right, top, left),
    /// as `build` will work it out: the deepest wall along that side.
    fn clear_insets(&self, k: usize, rects: &[R]) -> [f64; 4] {
        let n = self.inst.len();
        let row = &self.walls.between[k * (n + 1)..(k + 1) * (n + 1)];
        let r = &rects[k];
        let mut insets = [0.0f64; 4];
        let mut covered = [0i32; 4];
        for (j, s) in rects.iter().enumerate() {
            if j == k || !s.on || self.fixture(j).is_some() {
                continue;
            }
            let along_x = (r.x1().min(s.x1()) - r.x.max(s.x)).max(0);
            let along_y = (r.y1().min(s.y1()) - r.y.max(s.y)).max(0);
            let touch = if s.y1() == r.y && along_x > 0 {
                Some((0, along_x))
            } else if s.x == r.x1() && along_y > 0 {
                Some((1, along_y))
            } else if s.y == r.y1() && along_x > 0 {
                Some((2, along_x))
            } else if s.x1() == r.x && along_y > 0 {
                Some((3, along_y))
            } else {
                None
            };
            if let Some((side, len)) = touch {
                insets[side] = insets[side].max(row[j]);
                covered[side] += len;
            }
        }
        if self.fill.is_some() {
            let strip = |x, y, w, h| R { x, y, w, h, on: true, f: 0 };
            let beyond = [
                strip(r.x, r.y - 1, r.w, 1),
                strip(r.x1(), r.y, 1, r.h),
                strip(r.x, r.y1(), r.w, 1),
                strip(r.x - 1, r.y, 1, r.h),
            ];
            for (side, b) in beyond.iter().enumerate() {
                if b.x >= 0 && b.y >= 0 && self.grid.inside(b) > covered[side] {
                    insets[side] = insets[side].max(row[n]);
                }
            }
        }
        for &(p, depth) in &self.walls.ports[k] {
            let l = &self.ports[p];
            let (sides, from, to) = if l.horizontal {
                ([(0, r.y), (2, r.y1())], r.x as f64, r.x1() as f64)
            } else {
                ([(3, r.x), (1, r.x1())], r.y as f64, r.y1() as f64)
            };
            if overlap(from, to, l.from, l.to) > 0.0 {
                for (side, at) in sides {
                    if (at as f64 - l.at).abs() <= 0.6 {
                        insets[side] = insets[side].max(depth);
                    }
                }
            }
        }
        insets
    }

    /// How far what the walls leave needy instance `k` is from what its plan needs, in the cost's
    /// terms: in either orientation, like a space's own limits.
    fn clear_miss(&self, k: usize, rects: &[R]) -> f64 {
        let r = &rects[k];
        let Some(c) = self.types[self.inst[k].type_index].clear.as_ref().filter(|_| r.on) else {
            return 0.0;
        };
        let (dw, dh) = (r.w as f64 * self.grid.gx, r.h as f64 * self.grid.gy);
        // Whatever the walls turn out to be, it's fine: no need to go and look (most moves).
        let d = 2.0 * self.walls.deepest[k];
        let (lw, lh) = ((dw - d).max(0.0), (dh - d).max(0.0));
        let holds = |b: Bounds1, lo: f64, hi: f64| b.miss(lo) == 0.0 && b.miss(hi) == 0.0;
        let fits = (holds(c.width, lw, dw) && holds(c.depth, lh, dh))
            || (holds(c.width, lh, dh) && holds(c.depth, lw, dw));
        if fits && holds(c.area, lw * lh, dw * dh) {
            return 0.0;
        }
        let insets = self.clear_insets(k, rects);
        let w = (dw - insets[1] - insets[3]).max(0.0);
        let h = (dh - insets[0] - insets[2]).max(0.0);
        let sides = (c.width.miss(w) + c.depth.miss(h)).min(c.width.miss(h) + c.depth.miss(w));
        sides / self.l0 + c.area.miss(w * h) / self.a0
    }

    /// Terms of one instance alone: fitting inside, its size, and its share of utilization.
    fn unary(&self, i: usize, r: &R, hard: f64) -> f64 {
        if !r.on {
            return 0.0;
        }
        let g = &self.grid;
        let cell = g.gx * g.gy;
        let t = &self.types[self.inst[i].type_index];
        let (l0, a0) = (self.l0, self.a0);
        let inside = g.inside(r);
        let mut c = hard * ((r.w * r.h - inside) as f64 * cell) / a0;
        c -= self.utilization * inside as f64 * cell / a0;
        let shape = self.shape(i, r);
        let (dw, dh) = (shape.w, shape.h);
        let area = dw * dh;
        c += hard * t.gross_limits().2.miss(area) / a0;
        c += hard * (t.side.miss(dw) + t.side.miss(dh)) / l0;
        c += hard * shape.miss / l0;
        let ratio = dw.max(dh) / dw.min(dh);
        if ratio > t.aspect {
            c += hard * (ratio - t.aspect) * 0.5;
        }
        if shape.compact {
            let p = 2.0 * (dw + dh);
            c += self.compactness * (p * p / (16.0 * area) - 1.0);
        }
        if self.fixture(i).is_some() {
            c += hard * self.zone_outside(i, r) as f64 * cell / a0;
            c += hard * self.off_wall(i, r) / l0;
            c += hard * self.in_swing(r) as f64 * cell / a0;
        } else if let Some(f) = &self.fill {
            c += hard * self.edge_sliver(r, f.k) as f64 * cell / a0;
        }
        c
    }

    /// Fill between `r` and the slot's edge that is thinner than `k` cells, if nothing else
    /// stands there: for each depth below `k`, the cells where the slot ends at that depth.
    fn edge_sliver(&self, r: &R, k: i32) -> i32 {
        if k <= 1 {
            return 0;
        }
        let g = &self.grid;
        let mut miss = 0;
        // The strip `d` cells out from each side.
        let strip = |side: usize, d: i32| match side {
            0 => R { y: r.y - d, h: 1, ..*r },
            1 => R { x: r.x1() + d - 1, w: 1, ..*r },
            2 => R { y: r.y1() + d - 1, h: 1, ..*r },
            _ => R { x: r.x - d, w: 1, ..*r },
        };
        for side in 0..4 {
            let mut prev = g.inside(&strip(side, 1));
            for d in 1..k {
                let next = g.inside(&strip(side, d + 1));
                miss += (prev - next).max(0) * (k - d);
                prev = next;
                if prev == 0 {
                    break;
                }
            }
        }
        miss
    }

    /// Fill between two rectangles facing each other that is thinner than `k` cells: the cells
    /// of the gap times how much they fall short.
    fn gap_sliver(&self, a: &R, b: &R, k: i32) -> i32 {
        if k <= 1 {
            return 0;
        }
        let along_x = (a.x1().min(b.x1()) - a.x.max(b.x)).max(0);
        let along_y = (a.y1().min(b.y1()) - a.y.max(b.y)).max(0);
        let gy = (b.y - a.y1()).max(a.y - b.y1());
        let gx = (b.x - a.x1()).max(a.x - b.x1());
        let mut miss = 0;
        if along_x > 0 && gy > 0 && gy < k {
            miss += along_x * gy * (k - gy);
        }
        if along_y > 0 && gx > 0 && gx < k {
            miss += along_y * gx * (k - gx);
        }
        miss
    }

    /// Two rectangles corner to corner with less than `k` cells between them: the fill would
    /// squeeze through there, if that gap is fill.
    fn pinch(&self, a: &R, b: &R, k: i32) -> bool {
        let along_x = a.x1().min(b.x1()) - a.x.max(b.x);
        let along_y = a.y1().min(b.y1()) - a.y.max(b.y);
        let w = self.pinch_width(a, b);
        along_x <= 0 && along_y <= 0 && w > 0 && w < k
    }

    /// How far apart two rectangles' nearest corners are, in cells (the wider of the two gaps).
    fn pinch_width(&self, a: &R, b: &R) -> i32 {
        (b.x - a.x1()).max(a.x - b.x1()).max(0).max((b.y - a.y1()).max(a.y - b.y1()).max(0))
    }

    /// A [`Self::pinch`] where the gap between the corners is fill.
    fn pinch_free(&self, a: &R, b: &R, occ: &Occ, k: i32) -> bool {
        let g = &self.grid;
        self.pinch(a, b, k) && {
            let (x0, x1) = (a.x1().min(b.x1()), a.x.max(b.x));
            let (y0, y1) = (a.y1().min(b.y1()), a.y.max(b.y));
            (x0..x1.max(x0 + 1)).any(|x| (y0..y1.max(y0 + 1)).any(|y| occ.is_free(g, x, y)))
        }
    }

    /// Terms of a pair: overlap (which also doesn't count as covered) and numbering order.
    fn pair(&self, i: usize, ri: &R, j: usize, rj: &R, hard: f64) -> f64 {
        if !ri.on || !rj.on {
            return 0.0;
        }
        let g = &self.grid;
        let mut c = 0.0;
        let ov = Self::overlap_cells(ri, rj);
        if ov > 0 {
            c += (hard + self.utilization) * ov as f64 * g.gx * g.gy / self.a0;
        }
        let blocked = self.in_clearance(i, ri, j, rj) + self.in_clearance(j, rj, i, ri);
        if blocked > 0 {
            c += hard * blocked as f64 * g.gx * g.gy / self.a0;
        }
        if let Some(f) = &self.fill {
            if self.fixture(i).is_none() && self.fixture(j).is_none() {
                c += hard * self.gap_sliver(ri, rj, f.k) as f64 * g.gx * g.gy / self.a0;
            }
        }
        let ti = self.inst[i].type_index;
        if ti == self.inst[j].type_index && self.ordered[ti] {
            let (first, second) = if i < j { (ri, rj) } else { (rj, ri) };
            let (c1, c2) = (2 * first.x + first.w, 2 * second.x + second.w);
            if c1 > c2 {
                c += 0.3 * (c1 - c2) as f64 * g.gx / 2.0 / self.l0;
            }
        }
        c
    }

    /// One rule's term for instance `a` (touch, door, window, near).
    fn rule_term(&self, rule: &C, a: usize, rects: &[R], hs: f64) -> f64 {
        let ra = &rects[a];
        if !ra.on {
            return 0.0;
        }
        let wh = |w: f64| if w >= HARD { w * hs } else { w };
        match rule {
            C::Touch { b, need, w, .. } => {
                let best = match b {
                    Target::Spaces(bs) => bs
                        .iter()
                        .filter(|&&bi| bi != a && rects[bi].on)
                        .map(|&bi| self.shared(ra, &rects[bi]))
                        .fold(0.0, f64::max),
                    Target::Port(role) => self.on_port(ra, role).0,
                    Target::Fill(_) => return 0.0,
                };
                wh(*w) * (need - best).max(0.0) / self.l0
            }
            C::Window { role, need, w, .. } => wh(*w) * (need - self.on_port(ra, role).0).max(0.0) / self.l0,
            C::ApartPort { lines, d, w, .. } => wh(*w) * self.too_close_to_lines(ra, lines, *d) / self.l0,
            C::Near { b, d, w, .. } => {
                let best = b
                    .iter()
                    .filter(|&&bi| bi != a && rects[bi].on)
                    .map(|&bi| self.gap(ra, &rects[bi]))
                    .fold(f64::INFINITY, f64::min);
                if best.is_finite() && best > *d {
                    wh(*w) * (best - d) / self.l0
                } else {
                    0.0
                }
            }
            C::Faces { b, w, .. } => {
                let best = b
                    .iter()
                    .filter(|&&bi| bi != a && rects[bi].on)
                    .map(|&bi| self.not_facing(ra, &rects[bi]))
                    .fold(f64::INFINITY, f64::min);
                if best.is_finite() {
                    wh(*w) * best / self.l0
                } else {
                    0.0
                }
            }
            C::Apart { .. } | C::FillTouch { .. } | C::FillWindow { .. } => 0.0,
        }
    }

    /// How far `b` is from being in front of fixture `a` (0 when it is): how much of it is
    /// behind `a`'s front line, plus how far it is from lying across from at least half of the
    /// narrower front.
    fn not_facing(&self, a: &R, b: &R) -> f64 {
        let g = &self.grid;
        let (behind, ov, need, cell) = match a.f {
            0 => (
                (b.y1() - a.y).max(0),
                overlap(a.x as f64, a.x1() as f64, b.x as f64, b.x1() as f64),
                a.w.min(b.w),
                g.gx,
            ),
            1 => (
                (a.x1() - b.x).max(0),
                overlap(a.y as f64, a.y1() as f64, b.y as f64, b.y1() as f64),
                a.h.min(b.h),
                g.gy,
            ),
            2 => (
                (a.y1() - b.y).max(0),
                overlap(a.x as f64, a.x1() as f64, b.x as f64, b.x1() as f64),
                a.w.min(b.w),
                g.gx,
            ),
            _ => (
                (b.x1() - a.x).max(0),
                overlap(a.y as f64, a.y1() as f64, b.y as f64, b.y1() as f64),
                a.h.min(b.h),
                g.gy,
            ),
        };
        (behind as f64 + (need as f64 * 0.5 - ov).max(0.0)) * cell
    }

    /// An `apart` pair's term, if the pair counts (each unordered pair once).
    fn apart_term(
        &self,
        (a, b, d, w): (&[usize], &[usize], f64, f64),
        ai: usize,
        bi: usize,
        rects: &[R],
        hs: f64,
    ) -> f64 {
        if bi == ai || !rects[ai].on || !rects[bi].on || (bi < ai && a.contains(&bi) && b.contains(&ai)) {
            return 0.0;
        }
        let w = if w >= HARD { w * hs } else { w };
        w * self.too_close(&rects[ai], &rects[bi], d) / self.l0
    }

    /// The whole cost, from the same terms as [`Self::contribution`].
    fn total(&self, rects: &[R], hs: f64) -> f64 {
        let hard = HARD * hs;
        let g = &self.grid;
        let mut c = self.utilization * g.inside_cells as f64 * g.gx * g.gy / self.a0;
        for (i, r) in rects.iter().enumerate() {
            c += self.unary(i, r, hard);
            for (j, s) in rects.iter().enumerate().skip(i + 1) {
                c += self.pair(i, r, j, s, hard);
            }
        }
        for &k in &self.needy {
            c += hard * self.clear_miss(k, rects);
        }
        for rule in &self.rules {
            match rule {
                C::Apart { a, b, d, w, .. } => {
                    for &ai in a {
                        for &bi in b {
                            c += self.apart_term((a, b, *d, *w), ai, bi, rects, hs);
                        }
                    }
                }
                _ if rule.global() => {}
                C::Touch { a, .. }
                | C::Window { a, .. }
                | C::Near { a, .. }
                | C::Faces { a, .. }
                | C::ApartPort { a, .. } => {
                    for &ai in a {
                        c += self.rule_term(rule, ai, rects, hs);
                    }
                }
                C::FillTouch { .. } | C::FillWindow { .. } => {}
            }
        }
        if self.fill.is_some() {
            c += FillState::new(self, rects, hs, false).total();
        }
        c
    }

    /// The cost with the fill measured exactly (occupancy, contiguous runs, corners), for
    /// polishing and choosing between runs.
    fn exact(&self, rects: &[R], hs: f64) -> f64 {
        let mut c = self.total(rects, hs);
        if self.fill.is_some() {
            c -= FillState::new(self, rects, hs, false).total();
            c += FillState::new(self, rects, hs, true).total();
        }
        c
    }

    /// Move instance `i` to `to`. Returns the change in cost, the fill's cut aside.
    fn apply(
        &self,
        i: usize,
        to: R,
        rects: &mut [R],
        m: &Membership,
        hs: f64,
        fill: &mut Option<FillState>,
    ) -> f64 {
        let from = rects[i];
        // A needy space's walls depend on what it touches: this move can only change those near
        // where `i` was and where it's going.
        let mut near = 0u64; // a bit per instance: there are at most MAX_INSTANCES
        if !self.needy.is_empty() && self.fixture(i).is_none() {
            let close =
                |a: &R, b: &R| a.on && a.x <= b.x1() && b.x <= a.x1() && a.y <= b.y1() && b.y <= a.y1();
            for &k in &self.needy {
                if k == i || close(&from, &rects[k]) || close(&to, &rects[k]) {
                    near |= 1 << k;
                }
            }
        }
        let hard = HARD * hs;
        let needs = |rects: &[R]| {
            let (mut left, mut c) = (near, 0.0);
            while left != 0 {
                c += hard * self.clear_miss(left.trailing_zeros() as usize, rects);
                left &= left - 1;
            }
            c
        };
        let before = self.contribution(i, rects, m, hs) + needs(rects);
        if let Some(f) = fill.as_mut() {
            f.occ.shift(&self.grid, &from, &to);
        }
        rects[i] = to;
        self.contribution(i, rects, m, hs) + needs(rects) - before
    }

    /// Every term that depends on instance `i`: moving `i` changes the cost by the change in this.
    fn contribution(&self, i: usize, rects: &[R], m: &Membership, hs: f64) -> f64 {
        let hard = HARD * hs;
        let ri = &rects[i];
        let mut c = self.unary(i, ri, hard);
        for (j, rj) in rects.iter().enumerate() {
            if j != i {
                c += self.pair(i, ri, j, rj, hard);
            }
        }
        for (k, rule) in self.rules.iter().enumerate() {
            match rule {
                C::Apart { a, b, d, w, .. } => {
                    if m.in_a[k][i] {
                        for &bi in b {
                            c += self.apart_term((a, b, *d, *w), i, bi, rects, hs);
                        }
                    }
                    if m.in_b[k][i] {
                        for &ai in a {
                            if ai != i || !m.in_a[k][i] {
                                c += self.apart_term((a, b, *d, *w), ai, i, rects, hs);
                            }
                        }
                    }
                }
                _ if rule.global() => {}
                C::FillTouch { .. } | C::FillWindow { .. } => {}
                C::Touch { a, .. }
                | C::Near { a, .. }
                | C::Faces { a, .. }
                | C::Window { a, .. }
                | C::ApartPort { a, .. } => {
                    if m.in_a[k][i] {
                        c += self.rule_term(rule, i, rects, hs);
                    }
                    if m.in_b[k][i] {
                        for &ai in a {
                            if ai != i {
                                c += self.rule_term(rule, ai, rects, hs);
                            }
                        }
                    }
                }
            }
        }
        c
    }
}

/// Run `f` for each id, side by side where threads are available; results in id order.
fn run_all<T: Send>(ids: &[usize], f: &(dyn Fn(usize) -> T + Sync)) -> Vec<T> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::thread::scope(|scope| {
            let handles: Vec<_> = ids.iter().map(|&id| scope.spawn(move || f(id))).collect();
            handles.into_iter().map(|h| h.join().expect("a solver run panicked")).collect()
        })
    }
    #[cfg(target_arch = "wasm32")]
    {
        ids.iter().map(|&id| f(id)).collect()
    }
}

/// The cells of `a` not in `b`, as up to four rectangles.
fn minus(a: &R, b: &R) -> Vec<R> {
    if !a.on {
        return Vec::new();
    }
    let ix0 = a.x.max(b.x);
    let iy0 = a.y.max(b.y);
    let ix1 = a.x1().min(b.x1());
    let iy1 = a.y1().min(b.y1());
    if !b.on || ix0 >= ix1 || iy0 >= iy1 {
        return vec![*a];
    }
    let mut out = Vec::with_capacity(4);
    let piece = |x: i32, y: i32, x1: i32, y1: i32| R { x, y, w: x1 - x, h: y1 - y, on: true, f: 0 };
    if a.y < iy0 {
        out.push(piece(a.x, a.y, a.x1(), iy0));
    }
    if iy1 < a.y1() {
        out.push(piece(a.x, iy1, a.x1(), a.y1()));
    }
    if a.x < ix0 {
        out.push(piece(a.x, iy0, ix0, iy1));
    }
    if ix1 < a.x1() {
        out.push(piece(ix1, iy0, a.x1(), iy1));
    }
    out
}

/// Move one side of a rectangle (0 bottom, 1 right, 2 top, 3 left) by `d` cells along the
/// axis (positive is up or right).
fn move_side(r: &mut R, side: usize, d: i32) {
    match side {
        0 => {
            r.y += d;
            r.h -= d;
        }
        1 => r.w += d,
        2 => r.h += d,
        _ => {
            r.x += d;
            r.w -= d;
        }
    }
}

/// Turn a rectangle by `k` quarter turns counter-clockwise about its centre. Its facing turns
/// with it.
fn turn(r: &mut R, k: u8) {
    if k % 2 == 1 {
        let (cx, cy) = (2 * r.x + r.w, 2 * r.y + r.h);
        std::mem::swap(&mut r.w, &mut r.h);
        r.x = (cx - r.w) / 2;
        r.y = (cy - r.h) / 2;
    }
    r.f = (r.f + k) % 4;
}

/// A door of `width` on `seg`, towards whichever end is at a corner of the room (`room`'s
/// corners), leaving `clear` there: a door by a corner leaves the walls free for furniture.
fn by_corner(seg: (Point, Point), width: f64, clear: f64, room: &[Point]) -> Option<(Point, Point)> {
    let (p, q) = seg;
    let len = p.distance(q);
    let w = width.min(len - 2.0 * clear);
    if w < width * 0.5 {
        return None;
    }
    let to_corner = |e: Point| room.iter().map(|c| c.distance(e)).fold(f64::INFINITY, f64::min);
    let (ux, uy) = ((q.x - p.x) / len, (q.y - p.y) / len);
    let at = |t: f64| Point::new(p.x + ux * t, p.y + uy * t);
    Some(if to_corner(q) < to_corner(p) - 1e-9 {
        (at(len - clear - w), at(len - clear))
    } else {
        (at(clear), at(clear + w))
    })
}

/// An opening of `width` centred on `seg`, leaving `clear` at each end; narrower if needed.
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

/// The engine's cost terms, driven move by move without annealing. The running total the
/// annealer keeps (`apply` + `FillState::update`/`recut`/`restore`) must always equal a full
/// recount (`total`). In `anneal_once` a `debug_assert` checks that every 1,000 moves, on
/// whatever moves the seed happens to make; here every move is checked, so a drift is caught at
/// the move that caused it, with the instance and both rectangles named.
#[cfg(test)]
mod tests {
    use serde_json::{json, Value};

    use super::*;
    use crate::config::Config;
    use crate::units::Units;

    /// A plate's root level, as `solve_plate` poses it (the boundary must start with its
    /// longest edge along +x from the origin, so no turning is needed).
    fn level(v: Value) -> (Plan, Slot, Units, f64) {
        let config = Config::from_value(v).unwrap();
        let units = config.units();
        let points = config.start_points();
        let roles = config.edge_roles();
        let n = points.len();
        let ports = (0..n).map(|i| PortSeg::new(roles[i].clone(), points[i], points[(i + 1) % n])).collect();
        let slot =
            Slot { boundary: Some(points), ports, spine: None, pin: Transform::IDENTITY, holes: Vec::new() };
        let (plan, _) = super::super::context::prepare(&config.plan(), None, None, "");
        let grid = super::super::plan_grid(&plan, units, None);
        (plan, slot, units, grid)
    }

    fn ctx(units: Units, grid: f64) -> Ctx<'static> {
        Ctx {
            units,
            grid,
            seed: 1,
            work: Default::default(),
            moves: Default::default(),
            plan_pointer: "",
            root: true,
        }
    }

    /// Make `steps` random moves of the kinds annealing makes (shift, resize, turn, switch an
    /// optional instance), rejecting about a third (undone as annealing undoes them), and check
    /// the running cost against a full recount after every one.
    fn running_cost_never_drifts(v: Value, seeds: u64, steps: usize) {
        let (plan, slot, units, grid) = level(v);
        let ctx = ctx(units, grid);
        let mut issues = Vec::new();
        let (p, start) = setup(&plan, &slot, &ctx, &mut issues).expect("something to place");
        let members = p.membership();
        let n = start.len();
        let movable: Vec<usize> = (0..n).filter(|&i| !p.pinned[i]).collect();
        assert!(!movable.is_empty(), "nothing to move");
        for seed in 1..=seeds {
            let mut rng = Rng::new(seed);
            let mut rects = start.clone();
            let hs = 0.5 + rng.f64();
            let mut fill = p.fill.as_ref().map(|_| FillState::new(&p, &rects, hs, false));
            let mut cost = p.total(&rects, hs);
            for step in 0..steps {
                let i = movable[rng.below(movable.len())];
                let old = rects[i];
                let mut r = old;
                match rng.below(5) {
                    0 => {
                        r.x += rng.around(3);
                        r.y += rng.around(3);
                    }
                    1 => r.w += rng.around(2),
                    2 => r.h += rng.around(2),
                    3 if p.fixture(i).is_some() => turn(&mut r, 1 + rng.below(3) as u8),
                    3 => turn(&mut r, 1),
                    _ if !p.mandatory[i] => r.on = !r.on,
                    _ => move_side(&mut r, rng.below(4), rng.around(2)),
                }
                p.clamp(&mut r);
                let mut delta = p.apply(i, r, &mut rects, &members, hs, &mut fill);
                let undo = fill.as_mut().map(|f| {
                    let (d, undo) = f.update(&p, &rects, &[(i, old, r)], hs);
                    delta += d;
                    undo
                });
                if let Some(f) = fill.as_mut() {
                    delta += f.recut(&p, hs);
                }
                if rng.below(3) == 0 {
                    if let Some(f) = fill.as_mut() {
                        f.occ.shift(&p.grid, &rects[i], &old);
                    }
                    rects[i] = old;
                    if let (Some(f), Some(u)) = (fill.as_mut(), undo) {
                        f.restore(u);
                    }
                } else {
                    cost += delta;
                }
                let full = p.total(&rects, hs);
                assert!(
                    (cost - full).abs() < 1e-6 * (1.0 + full.abs()),
                    "seed {seed}, move {step}: {} {old:?} → {r:?}: running {cost}, recounted {full}",
                    p.inst[i].name
                );
            }
        }
    }

    fn plate(w: f64, h: f64, rest: Value) -> Value {
        let mut v = json!({
            "start": { "mode": "boundary", "points": [[0, 0], [w, 0], [w, h], [0, h]], "ports": ["facade", "party", "facade", "party"] },
        });
        for (k, x) in rest.as_object().unwrap() {
            v[k] = x.clone();
        }
        v
    }

    #[test]
    fn round_the_corner_is_an_l() {
        let r = |x, y, w, h, f| R { x, y, w, h, on: true, f };
        // A counter along the bottom wall, facing up, cells 0..10 × 0..3.
        let counter = r(0, 0, 10, 3, 2);
        // The range on the left wall right after the corner, facing right: its back to the left.
        assert!(Problem::round_the_corner(&counter, &r(0, 3, 3, 4, 1)));
        // On the right wall, facing left.
        assert!(Problem::round_the_corner(&counter, &r(7, 3, 3, 4, 3)));
        // Facing up (backed on the counter): in front of it, not round the corner.
        assert!(!Problem::round_the_corner(&counter, &r(0, 3, 3, 4, 2)));
        // Away from the end, or not touching the counter's front.
        assert!(!Problem::round_the_corner(&counter, &r(1, 3, 3, 4, 1)));
        assert!(!Problem::round_the_corner(&counter, &r(0, 4, 3, 4, 1)));
        // The same L turned: a counter up the left wall facing right, the range along the top.
        let counter = r(0, 0, 3, 10, 1);
        assert!(Problem::round_the_corner(&counter, &r(3, 7, 4, 3, 0)));
        assert!(!Problem::round_the_corner(&counter, &r(3, 7, 4, 3, 2)));
    }

    #[test]
    fn rooms_and_rules() {
        running_cost_never_drifts(
            plate(
                14.0,
                9.0,
                json!({
                    "spaces": {
                        "living": { "area": { "min": 25, "max": 40 }, "side": { "min": 4 } },
                        "kitchen": { "area": { "min": 10, "max": 16 }, "aspect": 2 },
                        "bedroom": { "count": 2, "area": { "min": 11, "max": 15 }, "side": { "min": 3 } },
                        "bath": { "area": { "min": 4, "max": 7 } },
                    },
                    "rules": [
                        { "rule": "door", "a": ["bedroom", "bath", "kitchen"], "b": "living" },
                        { "rule": "window", "a": ["living", "bedroom"] },
                        { "rule": "touch", "a": "kitchen", "b": "living" },
                        { "rule": "near", "a": "bath", "b": "bedroom", "distance": 2 },
                        { "rule": "apart", "a": "bath", "b": "kitchen", "distance": 3 },
                    ],
                }),
            ),
            6,
            400,
        );
    }

    #[test]
    fn a_fill_and_optional_rooms() {
        running_cost_never_drifts(
            plate(
                16.0,
                10.0,
                json!({
                    "spaces": {
                        "hall": { "kind": "circulation", "fill": true, "area": { "min": 10, "max": 60 }, "side": { "min": 1.2 } },
                        "office": { "count": { "min": 2, "max": 6 }, "area": { "min": 9, "max": 14 }, "side": { "min": 2.8 } },
                        "meeting": { "area": { "min": 18, "max": 26 } },
                    },
                    "rules": [
                        { "rule": "door", "a": ["office", "meeting"], "b": "hall" },
                        { "rule": "window", "a": "office" },
                    ],
                }),
            ),
            6,
            300,
        );
    }

    #[test]
    fn fixtures_with_clearances() {
        running_cost_never_drifts(
            plate(
                3.0,
                2.5,
                json!({
                    "spaces": {
                        "toilet": { "kind": "fixture", "width": 0.4, "depth": 0.7, "clearance": { "front": 0.6 }, "against": "wall" },
                        "basin": { "kind": "fixture", "width": 0.6, "depth": 0.45, "clearance": { "front": 0.7 }, "against": "wall" },
                        "shower": { "kind": "fixture", "width": 0.9, "depth": 0.9, "clearance": { "front": 0.6 }, "against": "wall" },
                    },
                    "rules": [{ "rule": "apart", "a": "toilet", "b": "basin", "distance": 0.2 }],
                }),
            ),
            6,
            400,
        );
    }

    #[test]
    fn rooms_with_plans_inside_and_walls() {
        let bedroom = json!({
            "needs": { "width": { "min": 3 }, "depth": { "min": 3 } },
            "spaces": { "bed": { "kind": "fixture", "width": 1.5, "depth": 2, "clearance": { "front": 0.6 } } },
        });
        running_cost_never_drifts(
            plate(
                12.0,
                8.0,
                json!({
                    "assemblies": {
                        "exterior": { "on": "facade", "thickness": 0.3 },
                        "party": { "on": "party", "thickness": 0.25 },
                        "partition": { "on": ["partition", "link", "entrance"], "thickness": 0.12 },
                    },
                    "spaces": {
                        "hall": { "kind": "circulation", "fill": true, "area": { "min": 8, "max": 50 }, "side": { "min": 1.2 } },
                        "bedroom": { "count": 2, "area": { "min": 11, "max": 16 }, "plan": bedroom },
                        "living": { "area": { "min": 20, "max": 34 } },
                    },
                    "rules": [{ "rule": "door", "a": ["bedroom", "living"], "b": "hall" }],
                }),
            ),
            4,
            300,
        );
    }

    #[test]
    fn pinned_spaces_start_where_they_are_pinned_and_leave_nothing_to_move() {
        let (plan, slot, units, grid) = level(plate(
            10.0,
            6.0,
            json!({
                "spaces": {
                    "a": { "at": [0, 0], "width": 4, "depth": 3 },
                    "b": { "at": [6, 3], "width": 4, "depth": 3 },
                },
            }),
        ));
        let ctx = ctx(units, grid);
        let mut issues = Vec::new();
        let (p, rects) = setup(&plan, &slot, &ctx, &mut issues).unwrap();
        assert!(issues.is_empty(), "{:?}", issues.iter().map(|i| &i.message).collect::<Vec<_>>());
        assert!(p.pinned.iter().all(|&x| x));
        let got: Vec<Rect> = rects.iter().map(|r| p.grid.rect(r)).collect();
        let near = |a: f64, b: f64| (a - b).abs() < 1e-9;
        assert!(
            near(got[0].x0, 0.0) && near(got[0].y0, 0.0) && near(got[0].x1, 4.0) && near(got[0].y1, 3.0),
            "{got:?}"
        );
        assert!(
            near(got[1].x0, 6.0) && near(got[1].y0, 3.0) && near(got[1].x1, 10.0) && near(got[1].y1, 6.0),
            "{got:?}"
        );
        // And the engine makes no move at all.
        let result = solve(&plan, &slot, &ctx);
        assert_eq!(result.placed.len(), 2);
        assert_eq!(ctx.moves.load(std::sync::atomic::Ordering::Relaxed), 0);
    }

    #[test]
    fn overlapping_and_outside_cost_more_than_side_by_side_inside() {
        let (plan, slot, units, grid) = level(plate(
            10.0,
            6.0,
            json!({
                "spaces": { "a": { "width": 4, "depth": 3 }, "b": { "width": 4, "depth": 3 } },
            }),
        ));
        let ctx = ctx(units, grid);
        let (p, start) = setup(&plan, &slot, &ctx, &mut Vec::new()).unwrap();
        let cells = |m: f64| (m / p.grid.gx).round() as i32;
        let at = |x: f64, y: f64| R {
            x: cells(x),
            y: cells(y),
            w: cells(4.0),
            h: cells(3.0),
            on: true,
            f: start[0].f,
        };
        let side_by_side = p.total(&[at(0.0, 0.0), at(4.0, 0.0)], 1.0);
        let overlapping = p.total(&[at(0.0, 0.0), at(2.0, 0.0)], 1.0);
        let outside = p.total(&[at(0.0, 0.0), at(8.0, 0.0)], 1.0);
        assert!(overlapping > side_by_side + HARD * 0.5, "{overlapping} vs {side_by_side}");
        assert!(outside > side_by_side + HARD * 0.5, "{outside} vs {side_by_side}");
    }
}
