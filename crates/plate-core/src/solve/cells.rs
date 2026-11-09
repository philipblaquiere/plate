//! What each cell of a level's grid is, for spaces cut by the outline (`shape = 'fit`).
//!
//! The grid engine counts a cell as inside the slot only when it's wholly inside. Where the
//! outline is slanted, or doesn't fall on the grid, that leaves floor no rectangle may take. These
//! cells say how much of each cell is floor, so a rectangle may cross such an edge and be cut by
//! it:
//!
//! - **inside**: wholly floor (the engine's own `mask`).
//! - **partial**: partly floor, partly outside.
//! - **shadow**: wholly outside, in the corner triangle a slanted edge cuts off its bounding box.
//!   A rectangle may reach into it (that part is cut away). Elsewhere outside stays outside, so a
//!   rectangle never reaches across a notch.
//! - **outside**: anywhere else.
//!
//! Counts are prefix sums, so any rectangle's usable cells, floor cells and floor area are O(1).
//! Lengths along grid lines are prefix sums too: a wall two cut spaces share is only where both
//! have floor.

use crate::geometry::{clip_to_box, contains, segment_inside, signed_area, Point};

pub const OUTSIDE: u8 = 0;
pub const INSIDE: u8 = 1;
pub const PARTIAL: u8 = 2;
pub const SHADOW: u8 = 3;

/// Below this share of a cell, a cell is no floor at all.
const SPECK: f64 = 1e-9;

#[derive(Debug, Clone)]
pub struct Cells {
    nx: i32,
    ny: i32,
    /// Each cell: its class.
    pub kind: Vec<u8>,
    usable: Vec<i32>,
    solid: Vec<i32>,
    floor: Vec<f64>,
    /// Per vertical grid line `i`, cumulative floor length along it, in cells: `(ny + 1)` each.
    vline: Vec<f64>,
    /// Per horizontal grid line `j`, cumulative floor length along it: `(nx + 1)` each.
    hline: Vec<f64>,
    /// All the floor, in cells.
    pub floor_total: f64,
}

/// The grid a level is solved on: origin, cell size and count, and which cells are wholly inside.
pub struct GridSpec<'a> {
    pub ox: f64,
    pub oy: f64,
    pub gx: f64,
    pub gy: f64,
    pub nx: i32,
    pub ny: i32,
    pub mask: &'a [bool],
}

impl Cells {
    pub fn new(polygon: &[Point], holes: &[Vec<Point>], g: &GridSpec) -> Cells {
        let (nx, ny) = (g.nx, g.ny);
        let n = (nx * ny) as usize;
        let at = |i: i32, j: i32| (j * nx + i) as usize;
        let mut kind = vec![OUTSIDE; n];
        let mut frac = vec![0.0; n];
        let cell_area = g.gx * g.gy;
        for j in 0..ny {
            for i in 0..nx {
                let k = at(i, j);
                if g.mask[k] {
                    kind[k] = INSIDE;
                    frac[k] = 1.0;
                    continue;
                }
                let (x0, y0) = (g.ox + i as f64 * g.gx, g.oy + j as f64 * g.gy);
                let (x1, y1) = (x0 + g.gx, y0 + g.gy);
                let mut a = signed_area(&clip_to_box(polygon, x0, y0, x1, y1)).abs();
                for h in holes {
                    a -= signed_area(&clip_to_box(h, x0, y0, x1, y1)).abs();
                }
                let f = (a / cell_area).clamp(0.0, 1.0);
                if f > SPECK {
                    kind[k] = PARTIAL;
                    frac[k] = f;
                }
            }
        }
        // Shadows: the triangle between each slanted edge and its bounding box's corner outside.
        let ccw = signed_area(polygon) > 0.0;
        let m = polygon.len();
        for e in 0..m {
            let (a, b) = (polygon[e], polygon[(e + 1) % m]);
            let span = (b.x - a.x).abs().max((b.y - a.y).abs());
            if (b.x - a.x).abs() <= 1e-9 * span.max(1.0) || (b.y - a.y).abs() <= 1e-9 * span.max(1.0) {
                continue;
            }
            // Outside is on the right of a counter-clockwise outline.
            let side = |p: Point| {
                let c = (b.x - a.x) * (p.y - a.y) - (b.y - a.y) * (p.x - a.x);
                if ccw {
                    -c
                } else {
                    c
                }
            };
            let corner = [Point::new(a.x, b.y), Point::new(b.x, a.y)]
                .into_iter()
                .find(|&c| side(c) > 0.0)
                .expect("one corner of a slanted edge's box is on each side");
            let tri = [a, b, corner];
            let lo = |v: f64, o: f64, s: f64| (((v - o) / s).floor() as i32).max(0);
            let (i0, i1) = (lo(a.x.min(b.x), g.ox, g.gx), lo(a.x.max(b.x), g.ox, g.gx).min(nx - 1));
            let (j0, j1) = (lo(a.y.min(b.y), g.oy, g.gy), lo(a.y.max(b.y), g.oy, g.gy).min(ny - 1));
            for j in j0..=j1 {
                for i in i0..=i1 {
                    let k = at(i, j);
                    if kind[k] != OUTSIDE {
                        continue;
                    }
                    let c = Point::new(g.ox + (i as f64 + 0.5) * g.gx, g.oy + (j as f64 + 0.5) * g.gy);
                    if contains(&tri, c) {
                        kind[k] = SHADOW;
                    }
                }
            }
        }

        let stride = (nx + 1) as usize;
        let mut usable = vec![0i32; stride * (ny + 1) as usize];
        let mut solid = usable.clone();
        let mut floor = vec![0.0f64; usable.len()];
        for j in 0..ny {
            for i in 0..nx {
                let k = at(i, j);
                let p = (j + 1) as usize * stride + (i + 1) as usize;
                let (u, s) = ((kind[k] != OUTSIDE) as i32, (kind[k] == INSIDE || kind[k] == PARTIAL) as i32);
                usable[p] = u + usable[p - 1] + usable[p - stride] - usable[p - stride - 1];
                solid[p] = s + solid[p - 1] + solid[p - stride] - solid[p - stride - 1];
                floor[p] = frac[k] + floor[p - 1] + floor[p - stride] - floor[p - stride - 1];
            }
        }
        let floor_total = floor[floor.len() - 1];

        // Floor along grid lines: none where a cell on either side has none.
        let has = |i: i32, j: i32| i >= 0 && j >= 0 && i < nx && j < ny && frac[at(i, j)] > SPECK;
        let full = |i: i32, j: i32| i >= 0 && j >= 0 && i < nx && j < ny && kind[at(i, j)] == INSIDE;
        let point = |i: i32, j: i32| Point::new(g.ox + i as f64 * g.gx, g.oy + j as f64 * g.gy);
        let along = |p: Point, q: Point, sides: [(i32, i32); 2]| -> f64 {
            let [(ai, aj), (bi, bj)] = sides;
            if !has(ai, aj) || !has(bi, bj) {
                0.0
            } else if full(ai, aj) && full(bi, bj) {
                1.0
            } else {
                segment_inside(p, q, polygon, holes)
            }
        };
        let mut vline = vec![0.0; ((nx + 1) * (ny + 1)) as usize];
        for i in 0..=nx {
            for j in 0..ny {
                let l = along(point(i, j), point(i, j + 1), [(i - 1, j), (i, j)]);
                let base = (i * (ny + 1)) as usize;
                vline[base + j as usize + 1] = vline[base + j as usize] + l;
            }
        }
        let mut hline = vec![0.0; ((ny + 1) * (nx + 1)) as usize];
        for j in 0..=ny {
            for i in 0..nx {
                let l = along(point(i, j), point(i + 1, j), [(i, j - 1), (i, j)]);
                let base = (j * (nx + 1)) as usize;
                hline[base + i as usize + 1] = hline[base + i as usize] + l;
            }
        }
        Cells { nx, ny, kind, usable, solid, floor, vline, hline, floor_total }
    }

    /// Any partial cells, so anything for a cut space to take?
    pub fn any_partial(&self) -> bool {
        self.kind.contains(&PARTIAL)
    }

    fn clamp(&self, x0: i32, y0: i32, x1: i32, y1: i32) -> (usize, usize, usize, usize) {
        (
            x0.clamp(0, self.nx) as usize,
            y0.clamp(0, self.ny) as usize,
            x1.clamp(0, self.nx) as usize,
            y1.clamp(0, self.ny) as usize,
        )
    }

    fn sum_i(&self, p: &[i32], x0: i32, y0: i32, x1: i32, y1: i32) -> i32 {
        let (x0, y0, x1, y1) = self.clamp(x0, y0, x1, y1);
        if x1 <= x0 || y1 <= y0 {
            return 0;
        }
        let s = (self.nx + 1) as usize;
        p[y1 * s + x1] - p[y0 * s + x1] - p[y1 * s + x0] + p[y0 * s + x0]
    }

    /// Cells in `x0..x1` × `y0..y1` a cut space may cover (inside, partial or shadow).
    pub fn usable(&self, x0: i32, y0: i32, x1: i32, y1: i32) -> i32 {
        self.sum_i(&self.usable, x0, y0, x1, y1)
    }

    /// Cells with any floor (inside or partial).
    pub fn solid(&self, x0: i32, y0: i32, x1: i32, y1: i32) -> i32 {
        self.sum_i(&self.solid, x0, y0, x1, y1)
    }

    /// Floor in `x0..x1` × `y0..y1`, in cells.
    pub fn floor(&self, x0: i32, y0: i32, x1: i32, y1: i32) -> f64 {
        let (x0, y0, x1, y1) = self.clamp(x0, y0, x1, y1);
        if x1 <= x0 || y1 <= y0 {
            return 0.0;
        }
        let s = (self.nx + 1) as usize;
        let p = &self.floor;
        p[y1 * s + x1] - p[y0 * s + x1] - p[y1 * s + x0] + p[y0 * s + x0]
    }

    /// Floor along the vertical grid line `x = i` from `y = j0` to `j1`, in cells.
    pub fn vline(&self, i: i32, j0: i32, j1: i32) -> f64 {
        if i < 0 || i > self.nx {
            return 0.0;
        }
        let (j0, j1) = (j0.clamp(0, self.ny) as usize, j1.clamp(0, self.ny) as usize);
        let base = (i * (self.ny + 1)) as usize;
        (self.vline[base + j1] - self.vline[base + j0]).max(0.0)
    }

    /// Floor along the horizontal grid line `y = j` from `x = i0` to `i1`, in cells.
    pub fn hline(&self, j: i32, i0: i32, i1: i32) -> f64 {
        if j < 0 || j > self.ny {
            return 0.0;
        }
        let (i0, i1) = (i0.clamp(0, self.nx) as usize, i1.clamp(0, self.nx) as usize);
        let base = (j * (self.nx + 1)) as usize;
        (self.hline[base + i1] - self.hline[base + i0]).max(0.0)
    }

    /// The pieces of floor in `x0..x1` × `y0..y1`: more than one when the outline cuts it in two.
    pub fn pieces(&self, x0: i32, y0: i32, x1: i32, y1: i32) -> usize {
        let (x0, y0, x1, y1) = (x0.max(0), y0.max(0), x1.min(self.nx), y1.min(self.ny));
        let (w, h) = (x1 - x0, y1 - y0);
        if w <= 0 || h <= 0 {
            return 0;
        }
        let floor = |i: i32, j: i32| {
            let k = self.kind[(j * self.nx + i) as usize];
            k == INSIDE || k == PARTIAL
        };
        let (_, sizes) = super::outline::components(w, h, |i, j| floor(x0 + i, y0 + j));
        sizes.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid_for(polygon: &[Point], step: f64) -> (Vec<bool>, i32, i32) {
        let xs = polygon.iter().map(|p| p.x).fold(f64::MIN, f64::max);
        let ys = polygon.iter().map(|p| p.y).fold(f64::MIN, f64::max);
        let (nx, ny) = ((xs / step).round() as i32, (ys / step).round() as i32);
        let mut mask = vec![false; (nx * ny) as usize];
        for j in 0..ny {
            for i in 0..nx {
                let corners = [(0.02, 0.02), (0.98, 0.02), (0.98, 0.98), (0.02, 0.98)];
                mask[(j * nx + i) as usize] = corners.iter().all(|(u, v)| {
                    contains(polygon, Point::new((i as f64 + u) * step, (j as f64 + v) * step))
                });
            }
        }
        (mask, nx, ny)
    }

    #[test]
    fn a_slanted_corner_has_partial_cells_and_a_shadow() {
        // A 10 × 10 square with its top right corner cut off from (6, 10) to (10, 6).
        let poly = vec![
            Point::new(0.0, 0.0),
            Point::new(10.0, 0.0),
            Point::new(10.0, 6.0),
            Point::new(6.0, 10.0),
            Point::new(0.0, 10.0),
        ];
        let (mask, nx, ny) = grid_for(&poly, 1.0);
        let c = Cells::new(&poly, &[], &GridSpec { ox: 0.0, oy: 0.0, gx: 1.0, gy: 1.0, nx, ny, mask: &mask });
        assert!((c.floor_total - (100.0 - 8.0)).abs() < 1e-9, "{}", c.floor_total);
        // The diagonal crosses cells (6, 9), (7, 8), (8, 7), (9, 6): each half floor.
        assert_eq!(c.kind[(9 * nx + 6) as usize], PARTIAL);
        assert!((c.floor(6, 9, 7, 10) - 0.5).abs() < 1e-9);
        // Beyond it, in the cut-off corner: shadow.
        assert_eq!(c.kind[(9 * nx + 9) as usize], SHADOW);
        // The whole top-right 4 × 4: 6 half cells and 10 shadow... all usable, floor 4 + 3.
        assert_eq!(c.usable(6, 6, 10, 10), 16);
        assert!((c.floor(6, 6, 10, 10) - 8.0).abs() < 1e-9, "{}", c.floor(6, 6, 10, 10));
        assert_eq!(c.solid(6, 6, 10, 10), 10);
        // The line x = 8 has floor from 0 to 8.
        assert!((c.vline(8, 0, 10) - 8.0).abs() < 1e-9, "{}", c.vline(8, 0, 10));
        assert_eq!(c.pieces(0, 0, 10, 10), 1);
    }

    #[test]
    fn an_off_grid_edge_gives_partial_cells_and_no_shadow() {
        let poly =
            vec![Point::new(0.0, 0.0), Point::new(4.5, 0.0), Point::new(4.5, 3.0), Point::new(0.0, 3.0)];
        let (mask, _, ny) = grid_for(&poly, 1.0);
        // A grid wider than the outline, as one level of a bigger plate would have.
        let nx = 6;
        let mut m = vec![false; (nx * ny) as usize];
        for j in 0..ny {
            for i in 0..4 {
                m[(j * nx + i) as usize] = mask[(j * 5 + i) as usize];
            }
        }
        let c = Cells::new(&poly, &[], &GridSpec { ox: 0.0, oy: 0.0, gx: 1.0, gy: 1.0, nx, ny, mask: &m });
        assert_eq!(c.kind[4], PARTIAL);
        assert_eq!(c.kind[5], OUTSIDE);
        assert_eq!(c.usable(0, 0, 6, 3), 15);
        assert!((c.floor(0, 0, 5, 3) - 13.5).abs() < 1e-9);
    }

    #[test]
    fn an_inward_corner_cuts_a_box_in_two() {
        let poly = vec![
            Point::new(0.0, 0.0),
            Point::new(10.0, 0.0),
            Point::new(10.0, 10.0),
            Point::new(5.0, 4.0),
            Point::new(0.0, 10.0),
        ];
        let (mask, nx, ny) = grid_for(&poly, 1.0);
        let c = Cells::new(&poly, &[], &GridSpec { ox: 0.0, oy: 0.0, gx: 1.0, gy: 1.0, nx, ny, mask: &mask });
        assert_eq!(c.pieces(1, 7, 9, 10), 2);
        assert_eq!(c.pieces(1, 2, 9, 10), 1);
    }
}
