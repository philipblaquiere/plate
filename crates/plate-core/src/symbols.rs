//! The symbol catalog: drawings a space can be shown with (`symbol = 'bed_queen`).
//!
//! The catalog is `schema/symbols.json`, built into the engine. Each symbol is a few shapes in
//! millimetres, in its own frame: x along its front from left to right (seen standing at the
//! front, facing it), y from the front to the back. Placed in a space, it is fitted to the
//! space (stretched, or scaled to fit and centred) and flattened to polylines, so the viewer and
//! every exporter draw it the same way without knowing the catalog.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

use crate::geometry::Point;

/// The catalog's source.
pub const SOURCE: &str = include_str!("../../../schema/symbols.json");

/// How a closed shape is filled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Tone {
    Water,
    Plant,
    Sand,
    Dark,
}

/// How a symbol is fitted to its space.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Fit {
    /// Fills the space: a 2 m bed symbol in a 2.1 m bed is drawn 2.1 m long.
    #[default]
    Stretch,
    /// Keeps its proportions: as large as fits, centred.
    Contain,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SymbolDef {
    pub label: String,
    pub category: String,
    #[serde(default)]
    pub tags: Vec<String>,
    /// Width along the front and depth front to back, in millimetres.
    pub size: [f64; 2],
    #[serde(default)]
    pub fit: Fit,
    pub shapes: Vec<ShapeDef>,
}

/// One shape: exactly one of the geometry fields.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShapeDef {
    /// `[x, y, w, h]`.
    #[serde(default)]
    pub rect: Option<[f64; 4]>,
    /// Corner radius of a `rect`.
    #[serde(default)]
    pub r: Option<f64>,
    /// `[x0, y0, x1, y1, …]`, open.
    #[serde(default)]
    pub line: Option<Vec<f64>>,
    /// `[x0, y0, x1, y1, …]`, closed.
    #[serde(default)]
    pub poly: Option<Vec<f64>>,
    /// `[cx, cy, r]`.
    #[serde(default)]
    pub circle: Option<[f64; 3]>,
    /// `[cx, cy, rx, ry]`.
    #[serde(default)]
    pub ellipse: Option<[f64; 4]>,
    /// `[cx, cy, r, from, to]`, degrees counter-clockwise from +x.
    #[serde(default)]
    pub arc: Option<[f64; 5]>,
    #[serde(default)]
    pub tone: Option<Tone>,
}

#[derive(Deserialize)]
struct CatalogFile {
    symbols: BTreeMap<String, SymbolDef>,
}

/// Every symbol, by name.
pub fn catalog() -> &'static BTreeMap<String, SymbolDef> {
    static CATALOG: OnceLock<BTreeMap<String, SymbolDef>> = OnceLock::new();
    CATALOG.get_or_init(|| {
        serde_json::from_str::<CatalogFile>(SOURCE).map(|f| f.symbols).expect("schema/symbols.json is valid")
    })
}

/// The colour a symbol fills its space with when it's drawn as a ground rather than a figure (a
/// lawn or water filling a space of any shape): its first filled shape's tone.
pub fn tone(name: &str) -> Option<Tone> {
    catalog().get(name)?.shapes.iter().find_map(|s| s.tone)
}

/// A flattened piece of a placed symbol.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SymbolPath {
    pub points: Vec<Point>,
    pub closed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tone: Option<Tone>,
}

const CIRCLE_STEPS: usize = 36;
const CORNER_STEPS: usize = 6;

fn arc_points(cx: f64, cy: f64, rx: f64, ry: f64, from: f64, to: f64, steps: usize) -> Vec<(f64, f64)> {
    (0..=steps)
        .map(|i| {
            let a = (from + (to - from) * i as f64 / steps as f64).to_radians();
            (cx + rx * a.cos(), cy + ry * a.sin())
        })
        .collect()
}

fn pairs(v: &[f64]) -> Vec<(f64, f64)> {
    v.as_chunks::<2>().0.iter().map(|c| (c[0], c[1])).collect()
}

impl ShapeDef {
    /// The shape as one polyline in the symbol's frame: (points, closed). An error names what's
    /// wrong with it.
    pub fn flatten(&self) -> Result<(Vec<(f64, f64)>, bool), String> {
        let given = [
            self.rect.is_some(),
            self.line.is_some(),
            self.poly.is_some(),
            self.circle.is_some(),
            self.ellipse.is_some(),
            self.arc.is_some(),
        ];
        if given.iter().filter(|g| **g).count() != 1 {
            return Err("a shape needs exactly one of rect, line, poly, circle, ellipse, arc".into());
        }
        if self.r.is_some() && self.rect.is_none() {
            return Err("`r` is for rect".into());
        }
        if let Some([x, y, w, h]) = self.rect {
            let r = self.r.unwrap_or(0.0).clamp(0.0, w.min(h) / 2.0);
            if r <= 0.0 {
                return Ok((vec![(x, y), (x + w, y), (x + w, y + h), (x, y + h)], true));
            }
            let mut out = Vec::new();
            for (cx, cy, from) in [
                (x + w - r, y + r, -90.0),
                (x + w - r, y + h - r, 0.0),
                (x + r, y + h - r, 90.0),
                (x + r, y + r, 180.0),
            ] {
                out.extend(arc_points(cx, cy, r, r, from, from + 90.0, CORNER_STEPS));
            }
            return Ok((out, true));
        }
        if let Some(v) = self.line.as_ref().or(self.poly.as_ref()) {
            if v.len() < 4 || v.len() % 2 != 0 {
                return Err("line and poly need x, y pairs, at least two points".into());
            }
            return Ok((pairs(v), self.poly.is_some()));
        }
        if let Some([cx, cy, r]) = self.circle {
            let mut pts = arc_points(cx, cy, r, r, 0.0, 360.0, CIRCLE_STEPS);
            pts.pop();
            return Ok((pts, true));
        }
        if let Some([cx, cy, rx, ry]) = self.ellipse {
            let mut pts = arc_points(cx, cy, rx, ry, 0.0, 360.0, CIRCLE_STEPS);
            pts.pop();
            return Ok((pts, true));
        }
        if let Some([cx, cy, r, from, to]) = self.arc {
            let steps = ((to - from).abs() / 10.0).ceil().max(2.0) as usize;
            return Ok((arc_points(cx, cy, r, r, from, to, steps), false));
        }
        unreachable!()
    }
}

/// Symbol `name` drawn in a frame: its front from `a` to `b` (the space lies on the left going
/// from `a` to `b`), `depth` deep. `None` for an unknown name or an empty frame.
pub fn draw(name: &str, a: Point, b: Point, depth: f64) -> Option<Vec<SymbolPath>> {
    let def = catalog().get(name)?;
    let width = a.distance(b);
    let [sw, sd] = def.size;
    if width <= 0.0 || depth <= 0.0 || sw <= 0.0 || sd <= 0.0 {
        return None;
    }
    let (ux, uy) = ((b.x - a.x) / width, (b.y - a.y) / width);
    // Into the space: the left of a → b.
    let (nx, ny) = (-uy, ux);
    let (kx, ky, ox, oy) = match def.fit {
        Fit::Stretch => (width / sw, depth / sd, 0.0, 0.0),
        Fit::Contain => {
            let k = (width / sw).min(depth / sd);
            (k, k, (width - sw * k) / 2.0, (depth - sd * k) / 2.0)
        }
    };
    let place = |(x, y): (f64, f64)| {
        let (along, into) = (ox + x * kx, oy + y * ky);
        Point::new(a.x + ux * along + nx * into, a.y + uy * along + ny * into)
    };
    let mut out = Vec::with_capacity(def.shapes.len());
    for shape in &def.shapes {
        let Ok((pts, closed)) = shape.flatten() else { continue };
        out.push(SymbolPath {
            points: pts.into_iter().map(place).collect(),
            closed,
            tone: if closed { shape.tone } else { None },
        });
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_symbol_is_well_formed() {
        let all = catalog();
        assert!(all.len() >= 60, "{} symbols", all.len());
        for (name, def) in all {
            assert!(
                name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'),
                "{name}: names are snake_case"
            );
            assert!(def.size[0] > 0.0 && def.size[1] > 0.0, "{name}: size");
            assert!(!def.shapes.is_empty(), "{name}: no shapes");
            for (i, shape) in def.shapes.iter().enumerate() {
                if let Err(e) = shape.flatten() {
                    panic!("{name}, shape {i}: {e}");
                }
                if shape.tone.is_some() {
                    assert!(
                        shape.line.is_none() && shape.arc.is_none(),
                        "{name}, shape {i}: tone on an open shape"
                    );
                }
            }
        }
    }

    #[test]
    fn symbols_follow_their_frame() {
        // A queen bed facing down (+x along the front from (2, 3) to (0, 3)): it lies below y = 3.
        let paths = draw("bed_queen", Point::new(2.0, 3.0), Point::new(0.0, 3.0), 2.5).unwrap();
        let pts: Vec<Point> = paths.iter().flat_map(|p| p.points.clone()).collect();
        let (min_y, max_y) = pts.iter().fold((f64::MAX, f64::MIN), |(lo, hi), p| (lo.min(p.y), hi.max(p.y)));
        let (min_x, max_x) = pts.iter().fold((f64::MAX, f64::MIN), |(lo, hi), p| (lo.min(p.x), hi.max(p.x)));
        // Stretched to fill the frame.
        assert!((max_y - 3.0).abs() < 1e-9 && (min_y - 0.5).abs() < 1e-9, "{min_y}..{max_y}");
        assert!(min_x.abs() < 1e-9 && (max_x - 2.0).abs() < 1e-9, "{min_x}..{max_x}");
        // The headboard is at the back: the end away from the front.
        let headboard = &paths[1].points;
        assert!(headboard.iter().all(|p| p.y < 0.6), "{headboard:?}");
    }

    #[test]
    fn contain_keeps_proportions_and_centres() {
        // A car (1.85 × 4.7) in a 3 × 4.7 stall keeps its width and sits in the middle.
        let paths = draw("car", Point::new(0.0, 0.0), Point::new(3.0, 0.0), 4.7).unwrap();
        let xs: Vec<f64> = paths.iter().flat_map(|p| p.points.iter().map(|q| q.x)).collect();
        let (lo, hi) = xs.iter().fold((f64::MAX, f64::MIN), |(a, b), x| (a.min(*x), b.max(*x)));
        assert!((hi - lo - 1.85).abs() < 1e-6 && ((lo + hi) / 2.0 - 1.5).abs() < 1e-6, "{lo}..{hi}");
        assert!(draw("no_such_thing", Point::new(0.0, 0.0), Point::new(1.0, 0.0), 1.0).is_none());
    }
}
