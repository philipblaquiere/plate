//! A plate drawn small, for the project switcher: its outline and top-level rooms, nothing else.
//! Seeds come from the file, so a thumbnail is the layout the editor will show.

use std::collections::hash_map::DefaultHasher;
use std::collections::{BTreeMap, HashMap};
use std::fmt::Write;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use plate_core::layout::{Element, ElementKind, Layout};
use plate_core::SpaceKind;

/// How many thumbnails are kept (projects are few; this only stops a slow leak).
const KEPT: usize = 128;
const WIDTH: f64 = 96.0;
const HEIGHT: f64 = 64.0;
const MARGIN: f64 = 3.0;

#[derive(Default)]
pub struct Cache(HashMap<u64, Arc<str>>);

impl Cache {
    pub fn get(&self, key: u64) -> Option<Arc<str>> {
        self.0.get(&key).cloned()
    }

    pub fn put(&mut self, key: u64, svg: Arc<str>) {
        if self.0.len() >= KEPT {
            self.0.clear();
        }
        self.0.insert(key, svg);
    }
}

/// What a thumbnail depends on: the plate and the files it could import.
pub fn key(main: &str, files: &BTreeMap<String, String>) -> u64 {
    let mut hasher = DefaultHasher::new();
    env!("CARGO_PKG_VERSION").hash(&mut hasher);
    main.hash(&mut hasher);
    files.hash(&mut hasher);
    hasher.finish()
}

/// Solve a plate and draw it. `None` when it doesn't evaluate or has no floor to draw.
pub fn draw(main: &str, files: BTreeMap<String, String>) -> Option<String> {
    let solved = crate::engine::run_files(main, files, None).ok()?;
    svg(&solved.layout)
}

fn svg(layout: &Layout) -> Option<String> {
    let b = &layout.bounds;
    let (w, h) = (b.max_x - b.min_x, b.max_y - b.min_y);
    if !(w > 0.0 && h > 0.0) {
        return None;
    }
    let scale = ((WIDTH - 2.0 * MARGIN) / w).min((HEIGHT - 2.0 * MARGIN) / h);
    // Centred, with y pointing up as it does in the drawing.
    let (ox, oy) = ((WIDTH - w * scale) / 2.0, (HEIGHT - h * scale) / 2.0);
    let polygon = |e: &Element| {
        let mut d = String::new();
        for (i, p) in e.points.iter().enumerate() {
            let (x, y) = (ox + (p.x - b.min_x) * scale, oy + (b.max_y - p.y) * scale);
            let _ = write!(d, "{}{x:.1} {y:.1}", if i == 0 { "M" } else { "L" });
        }
        if e.closed {
            d.push('Z');
        }
        d
    };
    let mut out = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {WIDTH} {HEIGHT}\" fill=\"none\" stroke-linejoin=\"round\">"
    );
    for e in layout.elements.iter().filter(|e| e.kind == ElementKind::Space) {
        let Some(info) = &e.space else { continue };
        // Rooms and what they hold, down to the fixtures that make a plan readable when small.
        let fill = match info.kind {
            SpaceKind::Fixture => "#3a4047",
            SpaceKind::Circulation => "#22262b",
            SpaceKind::Zone | SpaceKind::Void => continue,
            _ => "#2b3036",
        };
        let stroke = if info.kind == SpaceKind::Fixture { "#3a4047" } else { "#737b84" };
        let _ = write!(
            out,
            "<path d=\"{}\" fill=\"{fill}\" stroke=\"{stroke}\" stroke-width=\".5\"/>",
            polygon(e)
        );
    }
    for e in &layout.elements {
        if matches!(e.kind, ElementKind::Boundary | ElementKind::Spine) {
            let _ = write!(out, "<path d=\"{}\" stroke=\"#c9ced4\" stroke-width=\".9\"/>", polygon(e));
        }
    }
    out.push_str("</svg>");
    Some(out)
}
