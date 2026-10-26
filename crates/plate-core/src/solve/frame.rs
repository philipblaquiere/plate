//! Affine frames and axis-aligned rectangles.

use crate::geometry::Point;

/// `p' = (a·x + b·y + tx, c·x + d·y + ty)`. Only rotations, mirrors and translations are used.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transform {
    pub a: f64,
    pub b: f64,
    pub c: f64,
    pub d: f64,
    pub tx: f64,
    pub ty: f64,
}

impl Default for Transform {
    fn default() -> Self {
        Transform::IDENTITY
    }
}

impl Transform {
    pub const IDENTITY: Transform = Transform { a: 1.0, b: 0.0, c: 0.0, d: 1.0, tx: 0.0, ty: 0.0 };

    pub fn translate(x: f64, y: f64) -> Transform {
        Transform { tx: x, ty: y, ..Transform::IDENTITY }
    }

    pub fn rotate(angle: f64) -> Transform {
        let (s, c) = angle.sin_cos();
        Transform { a: c, b: -s, c: s, d: c, tx: 0.0, ty: 0.0 }
    }

    pub fn apply(&self, p: Point) -> Point {
        Point::new(self.a * p.x + self.b * p.y + self.tx, self.c * p.x + self.d * p.y + self.ty)
    }

    /// `self ∘ inner`: apply `inner` first.
    pub fn then_after(&self, inner: &Transform) -> Transform {
        Transform {
            a: self.a * inner.a + self.b * inner.c,
            b: self.a * inner.b + self.b * inner.d,
            c: self.c * inner.a + self.d * inner.c,
            d: self.c * inner.b + self.d * inner.d,
            tx: self.a * inner.tx + self.b * inner.ty + self.tx,
            ty: self.c * inner.tx + self.d * inner.ty + self.ty,
        }
    }

    pub fn inverse(&self) -> Transform {
        let det = self.a * self.d - self.b * self.c;
        let (a, b, c, d) = (self.d / det, -self.b / det, -self.c / det, self.a / det);
        Transform { a, b, c, d, tx: -(a * self.tx + b * self.ty), ty: -(c * self.tx + d * self.ty) }
    }

    /// Mirrored frames reverse polygon orientation.
    pub fn is_mirror(&self) -> bool {
        self.a * self.d - self.b * self.c < 0.0
    }

    /// The 8 symmetries of a `w × h` rectangle, each mapping it onto `[0, w'] × [0, h']`, with
    /// the resulting size. Without `mirror`, only the 4 rotations.
    pub fn rect_symmetries(w: f64, h: f64, mirror: bool) -> Vec<(Transform, f64, f64)> {
        let rot = |k: u32| -> (Transform, f64, f64) {
            match k {
                0 => (Transform::IDENTITY, w, h),
                // 90° counter-clockwise: (x, y) -> (h - y, x)
                1 => (Transform { a: 0.0, b: -1.0, c: 1.0, d: 0.0, tx: h, ty: 0.0 }, h, w),
                2 => (Transform { a: -1.0, b: 0.0, c: 0.0, d: -1.0, tx: w, ty: h }, w, h),
                _ => (Transform { a: 0.0, b: 1.0, c: -1.0, d: 0.0, tx: 0.0, ty: w }, h, w),
            }
        };
        let mut out: Vec<_> = (0..4).map(rot).collect();
        if mirror {
            for k in 0..4 {
                let (t, w2, h2) = rot(k);
                // Mirror x after rotating: x -> w2 - x.
                let m = Transform { a: -1.0, b: 0.0, c: 0.0, d: 1.0, tx: w2, ty: 0.0 };
                out.push((m.then_after(&t), w2, h2));
            }
        }
        out
    }
}

/// An axis-aligned rectangle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
}

impl Rect {
    pub fn new(x0: f64, y0: f64, x1: f64, y1: f64) -> Rect {
        Rect { x0: x0.min(x1), y0: y0.min(y1), x1: x0.max(x1), y1: y0.max(y1) }
    }

    pub fn w(&self) -> f64 {
        self.x1 - self.x0
    }

    pub fn h(&self) -> f64 {
        self.y1 - self.y0
    }

    /// Corners counter-clockwise from the bottom left.
    pub fn corners(&self) -> Vec<Point> {
        vec![
            Point::new(self.x0, self.y0),
            Point::new(self.x1, self.y0),
            Point::new(self.x1, self.y1),
            Point::new(self.x0, self.y1),
        ]
    }

    /// Edges counter-clockwise: bottom, right, top, left.
    pub fn edges(&self) -> [(Point, Point); 4] {
        let c = self.corners();
        [(c[0], c[1]), (c[1], c[2]), (c[2], c[3]), (c[3], c[0])]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: Point, b: Point) -> bool {
        (a.x - b.x).abs() < 1e-9 && (a.y - b.y).abs() < 1e-9
    }

    #[test]
    fn symmetries_map_the_rectangle_onto_itself() {
        for (t, w2, h2) in Transform::rect_symmetries(4.0, 2.0, true) {
            let corners: Vec<Point> =
                Rect::new(0.0, 0.0, 4.0, 2.0).corners().iter().map(|p| t.apply(*p)).collect();
            for p in corners {
                assert!(p.x > -1e-9 && p.x < w2 + 1e-9 && p.y > -1e-9 && p.y < h2 + 1e-9, "{p:?}");
            }
        }
    }

    #[test]
    fn inverse_and_compose() {
        let t = Transform::translate(3.0, 1.0).then_after(&Transform::rotate(0.7));
        let p = Point::new(2.0, -5.0);
        assert!(close(t.inverse().apply(t.apply(p)), p));
    }
}
