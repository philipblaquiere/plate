//! Boolean operations on polygons: intersection, union and difference, for shapes with holes.
//!
//! A thin layer over `i_overlay`, so the rest of the solver only sees [`Point`]s. `i_overlay`
//! rounds to integers internally, so the same input gives the same output on every machine.
//!
//! Every input is a list of loops (any orientation: each is made counter-clockwise for an outline
//! or clockwise for a hole, by containment), and every output a list of [`Piece`]s, each an
//! outline (counter-clockwise) and its holes (clockwise), biggest piece first.

use i_overlay::core::fill_rule::FillRule;
use i_overlay::core::overlay_rule::OverlayRule;
use i_overlay::float::single::SingleFloatOverlay;
use i_overlay::i_float::float::compatible::FloatPointCompatible;

use super::{signed_area, Point};

impl FloatPointCompatible for Point {
    type Scalar = f64;

    fn from_xy(x: f64, y: f64) -> Self {
        Point::new(x, y)
    }

    fn x(&self) -> f64 {
        self.x
    }

    fn y(&self) -> f64 {
        self.y
    }
}

/// One connected piece of a result.
#[derive(Debug, Clone, PartialEq)]
pub struct Piece {
    /// Counter-clockwise.
    pub outer: Vec<Point>,
    /// Clockwise.
    pub holes: Vec<Vec<Point>>,
}

impl Piece {
    pub fn area(&self) -> f64 {
        signed_area(&self.outer).abs() - self.holes.iter().map(|h| signed_area(h).abs()).sum::<f64>()
    }
}

/// A shape: its outline (and, after it, its holes), as loops.
pub fn shape(outer: &[Point], holes: &[Vec<Point>]) -> Vec<Vec<Point>> {
    let mut loops = vec![oriented(outer, true)];
    loops.extend(holes.iter().map(|h| oriented(h, false)));
    loops
}

fn oriented(l: &[Point], ccw: bool) -> Vec<Point> {
    let mut l = l.to_vec();
    if (signed_area(&l) > 0.0) != ccw {
        l.reverse();
    }
    l
}

fn run(a: &[Vec<Point>], b: &[Vec<Point>], rule: OverlayRule) -> Vec<Piece> {
    let a: Vec<Vec<Point>> = a.iter().filter(|l| l.len() >= 3).cloned().collect();
    let b: Vec<Vec<Point>> = b.iter().filter(|l| l.len() >= 3).cloned().collect();
    if a.is_empty() && b.is_empty() {
        return Vec::new();
    }
    let shapes = a.overlay(&b, rule, FillRule::NonZero);
    let mut out: Vec<Piece> = shapes
        .into_iter()
        .filter_map(|loops| {
            let mut it = loops.into_iter();
            let outer = oriented(&it.next()?, true);
            let holes = it.map(|h| oriented(&h, false)).collect();
            Some(Piece { outer, holes })
        })
        .filter(|p| p.area() > 0.0)
        .collect();
    out.sort_by(|x, y| y.area().total_cmp(&x.area()));
    out
}

/// Where both `a` and `b` are.
pub fn intersect(a: &[Vec<Point>], b: &[Vec<Point>]) -> Vec<Piece> {
    run(a, b, OverlayRule::Intersect)
}

/// Where any of the shapes is. Each shape is a list of loops (see [`shape`]).
pub fn union(shapes: &[Vec<Vec<Point>>]) -> Vec<Piece> {
    let all: Vec<Vec<Point>> = shapes.iter().flatten().cloned().collect();
    run(&all, &[], OverlayRule::Subject)
}

/// Where `a` is and `b` isn't.
pub fn difference(a: &[Vec<Point>], b: &[Vec<Point>]) -> Vec<Piece> {
    run(a, b, OverlayRule::Difference)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(v: &[(f64, f64)]) -> Vec<Point> {
        v.iter().map(|&(x, y)| Point::new(x, y)).collect()
    }

    fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<Point> {
        p(&[(x0, y0), (x1, y0), (x1, y1), (x0, y1)])
    }

    #[test]
    fn a_rectangle_cut_by_a_diagonal_is_one_piece() {
        let slot = p(&[(0.0, 0.0), (10.0, 0.0), (10.0, 5.0), (5.0, 10.0), (0.0, 10.0)]);
        let out = intersect(&[rect(2.0, 2.0, 9.0, 9.0)], &[slot]);
        assert_eq!(out.len(), 1);
        // 49 less the corner beyond x + y = 15: a triangle with legs of 3.
        assert!((out[0].area() - (49.0 - 4.5)).abs() < 1e-6, "{}", out[0].area());
        assert!(signed_area(&out[0].outer) > 0.0);
    }

    #[test]
    fn a_rectangle_across_an_inward_corner_is_two_pieces() {
        let slot = p(&[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (5.0, 4.0), (0.0, 10.0)]);
        let out = intersect(&[rect(1.0, 6.0, 9.0, 9.0)], &[slot]);
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn union_of_touching_rectangles_is_one_outline() {
        let out = union(&[
            vec![rect(0.0, 0.0, 4.0, 3.0)],
            vec![rect(4.0, 0.0, 6.0, 3.0)],
            vec![rect(0.0, 3.0, 4.0, 5.0)],
        ]);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].outer.len(), 6, "{:?}", out[0].outer);
        assert!((out[0].area() - 26.0).abs() < 1e-6);
    }

    #[test]
    fn holes_are_kept_and_subtracted() {
        let s = shape(&rect(0.0, 0.0, 10.0, 10.0), &[rect(4.0, 4.0, 6.0, 6.0)]);
        let out = intersect(&[rect(0.0, 0.0, 10.0, 10.0)], &s);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].holes.len(), 1);
        assert!((out[0].area() - 96.0).abs() < 1e-6);
        assert!(signed_area(&out[0].holes[0]) < 0.0);
        let d = difference(&[rect(0.0, 0.0, 10.0, 10.0)], &[rect(5.0, 0.0, 10.0, 10.0)]);
        assert!((d[0].area() - 50.0).abs() < 1e-6);
    }
}
