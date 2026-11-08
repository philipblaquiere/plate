use std::fmt::Write;

use plate_core::symbols::Tone;
use plate_core::{ElementKind, Layout};

pub fn to_svg(layout: &Layout) -> String {
    let b = layout.bounds;
    // A single origin has no extent: frame 1 m around it.
    let span = b.width().max(b.height()).max(1000.0 / layout.units.to_mm());
    let margin = span * 0.1;
    let (x0, y0) = (b.min_x - margin, b.min_y - margin);
    let (w, h) = (b.width() + 2.0 * margin, b.height() + 2.0 * margin);

    // Physical size on paper, in millimetres: 1:100 metric, 1/8" = 1'-0" (1:96) imperial.
    let mm_per_unit = layout.units.to_mm() / layout.system.paper_scale();
    let stroke = span / 400.0;

    let mut svg = String::new();
    let _ = writeln!(svg, r#"<?xml version="1.0" encoding="UTF-8"?>"#);
    let _ = writeln!(
        svg,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{:.2}mm" height="{:.2}mm" viewBox="{} {} {} {}">"#,
        w * mm_per_unit,
        h * mm_per_unit,
        num(x0),
        num(-(y0 + h)),
        num(w),
        num(h)
    );
    let _ = writeln!(svg, "  <title>{}</title>", escape(&layout.name));
    // Floor no space covers is hatched.
    let hatch = span / 120.0;
    let _ = writeln!(
        svg,
        r##"  <defs><pattern id="plate-leftover" patternUnits="userSpaceOnUse" width="{h}" height="{h}" patternTransform="rotate(45)"><rect width="{h}" height="{h}" fill="#fbeae6" /><line x1="0" y1="0" x2="0" y2="{h}" stroke="#c8442c" stroke-width="{w}" /></pattern></defs>"##,
        h = num(hatch),
        w = num(hatch / 4.0)
    );
    // Wall hatches.
    let _ = writeln!(
        svg,
        r##"  <defs><pattern id="plate-wall-hatch" patternUnits="userSpaceOnUse" width="{h}" height="{h}" patternTransform="rotate(45)"><rect width="{h}" height="{h}" fill="#ffffff" /><line x1="0" y1="0" x2="0" y2="{h}" stroke="#1d2125" stroke-width="{w}" /></pattern><pattern id="plate-wall-cross" patternUnits="userSpaceOnUse" width="{h}" height="{h}" patternTransform="rotate(45)"><rect width="{h}" height="{h}" fill="#ffffff" /><path d="M0 0V{h}M0 0H{h}" stroke="#1d2125" stroke-width="{w}" /></pattern></defs>"##,
        h = num(hatch / 2.0),
        w = num(hatch / 10.0)
    );
    // Flip y so drawing coordinates (y up) map to SVG (y down).
    let _ = writeln!(
        svg,
        r##"  <g transform="scale(1,-1)" fill="none" stroke="#1d2125" stroke-width="{}">"##,
        num(stroke)
    );
    for element in layout.elements.iter().filter(|e| e.exported()) {
        let points: Vec<String> =
            element.points.iter().map(|p| format!("{},{}", num(p.x), num(p.y))).collect();
        let (id, layer) = (escape(&element.id), escape(&element.cad_layer()));
        let _ = match element.kind {
            ElementKind::Origin => {
                let p = element.points[0];
                let r = span / 60.0;
                writeln!(
                    svg,
                    r#"    <g id="{id}" data-layer="{layer}"><circle cx="{}" cy="{}" r="{}" /><path d="M{} {}H{}M{} {}V{}" /></g>"#,
                    num(p.x),
                    num(p.y),
                    num(r),
                    num(p.x - 2.0 * r),
                    num(p.y),
                    num(p.x + 2.0 * r),
                    num(p.x),
                    num(p.y - 2.0 * r),
                    num(p.y + 2.0 * r)
                )
            }
            ElementKind::Space => {
                let kind = element.space.as_ref().map_or("room", |s| s.kind.name());
                let fill = match kind {
                    "circulation" => "#e9e6dc",
                    "zone" => "#f8f6f1",
                    "void" | "fixture" => "#ffffff",
                    "vertical" => "#dcdfe3",
                    _ => "#f4f2ec",
                };
                let depth = element.space.as_ref().map_or(0, |s| s.depth);
                // Fixtures are drawn lighter than walls; one with a symbol is drawn by its symbol.
                let symbol = element.space.as_ref().is_some_and(|s| s.symbol.is_some());
                let weight = match (kind, symbol) {
                    ("fixture", true) => 0.0,
                    ("fixture", false) => 0.45,
                    _ if depth == 0 => 1.0,
                    _ => 0.6,
                };
                let fill = if kind == "fixture" && symbol { "none" } else { fill };
                let fill = element.space.as_ref().and_then(|s| s.tone).map_or(fill, tone_fill);
                // A zone has no walls: its edge is dashed.
                let dash = if kind == "zone" {
                    format!(r#" stroke-dasharray="{} {}""#, num(stroke * 3.0), num(stroke * 2.0))
                } else {
                    String::new()
                };
                let weight = if kind == "zone" { 0.4 } else { weight };
                if element.holes.is_empty() {
                    writeln!(
                        svg,
                        r#"    <polygon id="{id}" data-layer="{layer}" points="{}" fill="{fill}" stroke-width="{}"{dash} />"#,
                        points.join(" "),
                        num(stroke * weight)
                    )
                } else {
                    writeln!(
                        svg,
                        r#"    <path id="{id}" data-layer="{layer}" d="{}" fill="{fill}" fill-rule="evenodd" stroke-width="{}" />"#,
                        path_d(&element.points, &element.holes),
                        num(stroke * weight)
                    )
                }
            }
            ElementKind::Leftover => writeln!(
                svg,
                r##"    <path id="{id}" data-layer="{layer}" d="{}" fill="url(#plate-leftover)" fill-rule="evenodd" stroke="#c8442c" stroke-dasharray="{} {}" stroke-width="{}" />"##,
                path_d(&element.points, &element.holes),
                num(stroke * 2.0),
                num(stroke * 1.5),
                num(stroke * 0.6)
            ),
            ElementKind::Wall => {
                let fill = match element.wall.as_ref().map_or("solid", |w| w.pattern) {
                    "hatch" => "url(#plate-wall-hatch)",
                    "cross" => "url(#plate-wall-cross)",
                    "light" => "#b9bec4",
                    _ => "#2b2f33",
                };
                writeln!(
                    svg,
                    r#"    <polygon id="{id}" data-layer="{layer}" points="{}" fill="{fill}" stroke="none" />"#,
                    points.join(" ")
                )
            }
            ElementKind::Part => {
                let mut g =
                    format!(r#"    <g id="{id}" data-layer="{layer}" stroke-width="{}">"#, num(stroke * 0.4));
                match &element.symbol {
                    Some(symbol) => {
                        for path in &symbol.paths {
                            let pts: Vec<String> =
                                path.points.iter().map(|p| format!("{},{}", num(p.x), num(p.y))).collect();
                            let fill = path.tone.map_or("#ffffff", tone_fill);
                            let tag = if path.closed { "polygon" } else { "polyline" };
                            let fill = if path.closed { fill } else { "none" };
                            let _ = write!(g, r#"<{tag} points="{}" fill="{fill}" />"#, pts.join(" "));
                        }
                    }
                    None => {
                        let _ = write!(g, r##"<polygon points="{}" fill="#ffffff" />"##, points.join(" "));
                    }
                }
                writeln!(svg, "{g}</g>")
            }
            ElementKind::Run => {
                // Coloured by its medium's jacket; dashed where it isn't low in a wall.
                let run = element.run.as_ref();
                let colour = match run.and_then(|r| r.colour) {
                    Some("white") => "#8d8d8d",
                    Some("black") => "#222222",
                    Some("grey") => "#6f7780",
                    Some("red") => "#c8442c",
                    Some("yellow") => "#d9a400",
                    Some("orange") => "#e07b1f",
                    Some("blue") => "#2f6fd0",
                    _ => "#5b4fb3",
                };
                let control = run.is_some_and(|r| r.control);
                let dash = match run.and_then(|r| r.along) {
                    _ if control => {
                        format!(r#" stroke-dasharray="{} {}""#, num(stroke * 2.0), num(stroke * 1.5))
                    }
                    Some("ceiling") => {
                        format!(r#" stroke-dasharray="{} {}""#, num(stroke * 6.0), num(stroke * 3.0))
                    }
                    Some("header") => {
                        format!(r#" stroke-dasharray="{} {}""#, num(stroke * 3.0), num(stroke * 2.0))
                    }
                    _ => String::new(),
                };
                let width = if control { 0.5 } else { 0.8 };
                writeln!(
                    svg,
                    r##"    <polyline id="{id}" data-layer="{layer}" points="{}" stroke="{colour}" stroke-width="{}"{dash} />"##,
                    points.join(" "),
                    num(stroke * width)
                )
            }
            ElementKind::Door | ElementKind::Window => {
                let colour = if element.kind == ElementKind::Door { "#c8442c" } else { "#3b6fb6" };
                // In a wall: cut it.
                if let Some(depth) = element.opening.as_ref().and_then(|o| o.depth) {
                    let (a, b) = (element.points[0], element.points[1]);
                    let len = a.distance(b).max(1e-12);
                    let (nx, ny) = (-(b.y - a.y) / len * depth / 2.0, (b.x - a.x) / len * depth / 2.0);
                    let _ = writeln!(
                        svg,
                        r##"    <polygon points="{},{} {},{} {},{} {},{}" fill="#ffffff" stroke="none" />"##,
                        num(a.x + nx),
                        num(a.y + ny),
                        num(b.x + nx),
                        num(b.y + ny),
                        num(b.x - nx),
                        num(b.y - ny),
                        num(a.x - nx),
                        num(a.y - ny)
                    );
                }
                // An opening has no door: its header, dashed across the gap.
                let opening =
                    element.opening.as_ref().is_some_and(|o| o.leaf == plate_core::plan::Leaf::NoLeaf);
                let (colour, width, dash) = if opening {
                    let dash = format!(r#" stroke-dasharray="{} {}""#, num(stroke * 4.0), num(stroke * 3.0));
                    ("#6b6f76", stroke, dash)
                } else {
                    (colour, stroke * 3.0, String::new())
                };
                writeln!(
                    svg,
                    r#"    <polyline id="{id}" data-layer="{layer}" points="{}" stroke="{colour}" stroke-width="{}"{dash} />"#,
                    points.join(" "),
                    num(width)
                )
            }
            ElementKind::Symbol => {
                let Some(symbol) = &element.symbol else { continue };
                let mut g = format!(
                    r#"    <g id="{id}" data-layer="{layer}" data-symbol="{}" stroke-width="{}">"#,
                    escape(&symbol.name),
                    num(stroke * 0.45)
                );
                for path in &symbol.paths {
                    let pts: Vec<String> =
                        path.points.iter().map(|p| format!("{},{}", num(p.x), num(p.y))).collect();
                    let fill = path.tone.map_or("none", tone_fill);
                    let tag = if path.closed { "polygon" } else { "polyline" };
                    let _ = write!(g, r#"<{tag} points="{}" fill="{fill}" />"#, pts.join(" "));
                }
                writeln!(svg, "{g}</g>")
            }
            ElementKind::Clearance => writeln!(
                svg,
                r##"    <polygon id="{id}" data-layer="{layer}" points="{}" stroke="#8a8f96" stroke-dasharray="{} {}" stroke-width="{}" />"##,
                points.join(" "),
                num(stroke * 3.0),
                num(stroke * 2.0),
                num(stroke * 0.5)
            ),
            ElementKind::Port => writeln!(
                svg,
                r##"    <polyline id="{id}" data-layer="{layer}" points="{}" stroke="#8a6d1f" stroke-dasharray="{} {}" stroke-width="{}" />"##,
                points.join(" "),
                num(stroke * 6.0),
                num(stroke * 3.0),
                num(stroke * 2.0)
            ),
            _ => writeln!(
                svg,
                r#"    <{tag} id="{id}" data-layer="{layer}" points="{}" />"#,
                points.join(" "),
                tag = if element.closed { "polygon" } else { "polyline" }
            ),
        };
    }
    let _ = writeln!(svg, "  </g>");
    // Space labels (outside the flipped group so text reads the right way up). A fixture drawn
    // with a symbol needs none.
    // Spaces divided into other spaces (not just furnished) get a tag in their corner instead.
    let divided: std::collections::HashSet<&str> = layout
        .elements
        .iter()
        .filter(|e| e.kind == ElementKind::Space)
        .filter(|e| e.space.as_ref().is_some_and(|i| i.kind != plate_core::SpaceKind::Fixture))
        .filter_map(|e| e.id.rsplit_once('.').map(|(parent, _)| parent))
        .collect();
    let mut tags: Vec<(f64, f64, f64, f64)> = Vec::new();
    // Parts on the ceiling or floor: labels move off them.
    let parts: Vec<plate_core::Bounds> = layout
        .elements
        .iter()
        .filter(|e| e.kind == ElementKind::Part)
        .map(|e| plate_core::Bounds::from_points(&e.points))
        .collect();
    for element in layout.elements.iter().filter(|e| e.kind == ElementKind::Space) {
        let Some(info) = &element.space else { continue };
        if info.kind == plate_core::SpaceKind::Fixture && info.symbol.is_some() {
            continue;
        }
        let sb = plate_core::Bounds::from_points(&element.points);
        if divided.contains(element.id.as_str()) {
            // A space with a plan: a small tag in its top-left corner, so the labels of the
            // spaces inside stay readable.
            let size = (sb.width().min(sb.height()) / 10.0).min(span / 90.0);
            // The top-left corner of its outline: the highest vertex, leftmost among those (a
            // fill's bounding box corner may be outside it).
            let tol = 1e-6 * span;
            let corner = element.points.iter().copied().fold(element.points[0], |a, p| {
                if p.y > a.y + tol || ((p.y - a.y).abs() <= tol && p.x < a.x) {
                    p
                } else {
                    a
                }
            });
            let (x, w) = (corner.x + size * 0.4, info.label.chars().count() as f64 * size * 0.6);
            // Below any tag already in that corner (a wing's and its first suite's, say).
            let mut y = corner.y - size * 1.2;
            while tags.iter().any(|&(tx, ty, tw, th)| {
                x < tx + tw && tx < x + w && y - size * 0.2 < ty + th && ty < y + size
            }) {
                y -= size * 1.2;
            }
            tags.push((x, y - size * 0.2, w, size * 1.2));
            let _ = writeln!(
                svg,
                r##"  <text x="{}" y="{}" font-family="sans-serif" font-size="{}" font-style="italic" fill="#6b5a2e">{}</text>"##,
                num(x),
                num(-y),
                num(size),
                escape(&info.label)
            );
            continue;
        }
        let size = (sb.width().min(sb.height()) / 6.0).min(span / 50.0);
        let at = element
            .label_at
            .unwrap_or(plate_core::Point::new((sb.min_x + sb.max_x) / 2.0, (sb.min_y + sb.max_y) / 2.0));
        let at =
            clear_of_parts(at, info.label.chars().count() as f64 * 0.6 * size, size, &parts, &element.points);
        let _ = writeln!(
            svg,
            r##"  <text x="{}" y="{}" font-family="sans-serif" font-size="{}" text-anchor="middle" fill="#1d2125">{}</text>"##,
            num(at.x),
            num(-at.y),
            num(size),
            escape(&info.label)
        );
    }
    let _ = writeln!(
        svg,
        r##"  <text x="{}" y="{}" font-family="sans-serif" font-size="{}" fill="#1d2125">{}</text>"##,
        num(b.min_x),
        num(-b.min_y + margin * 0.6),
        num(span / 30.0),
        escape(&layout.name)
    );
    let _ = writeln!(svg, "</svg>");
    svg
}

/// Where a label (`w` wide, its baseline at `at`, `size` high) goes so no part covers it: where it
/// is, or just below or above a part in the way, still inside its space.
fn clear_of_parts(
    at: plate_core::Point,
    w: f64,
    size: f64,
    parts: &[plate_core::Bounds],
    space: &[plate_core::Point],
) -> plate_core::Point {
    let (down, up) = (0.25 * size, 0.85 * size);
    let label = |p: plate_core::Point| (p.x - w / 2.0, p.y - down, p.x + w / 2.0, p.y + up);
    let hits = |b: (f64, f64, f64, f64)| -> Vec<&plate_core::Bounds> {
        parts.iter().filter(|q| b.0 < q.max_x && q.min_x < b.2 && b.1 < q.max_y && q.min_y < b.3).collect()
    };
    let first = hits(label(at));
    if first.is_empty() {
        return at;
    }
    let gap = 0.2 * size;
    let mut candidates: Vec<plate_core::Point> = first
        .iter()
        .flat_map(|q| {
            [
                plate_core::Point::new(at.x, q.min_y - gap - up),
                plate_core::Point::new(at.x, q.max_y + gap + down),
            ]
        })
        .collect();
    candidates.sort_by(|a, b| (a.y - at.y).abs().total_cmp(&(b.y - at.y).abs()));
    candidates
        .into_iter()
        .find(|&p| {
            let b = label(p);
            hits(b).is_empty()
                && [(b.0, b.1), (b.2, b.1), (b.2, b.3), (b.0, b.3)]
                    .iter()
                    .all(|&(x, y)| plate_core::geometry::contains(space, plate_core::Point::new(x, y)))
        })
        .unwrap_or(at)
}

/// An SVG path for a polygon and its holes.
fn path_d(outer: &[plate_core::Point], holes: &[Vec<plate_core::Point>]) -> String {
    let mut d = String::new();
    for ring in std::iter::once(outer).chain(holes.iter().map(Vec::as_slice)) {
        for (k, p) in ring.iter().enumerate() {
            let _ = write!(d, "{}{} {}", if k == 0 { "M" } else { "L" }, num(p.x), num(p.y));
        }
        d.push('Z');
    }
    d
}

fn tone_fill(tone: Tone) -> &'static str {
    match tone {
        Tone::Water => "#cfe3f2",
        Tone::Plant => "#d9ead0",
        Tone::Sand => "#efe3c4",
        Tone::Dark => "#4a4f55",
    }
}

fn num(v: f64) -> String {
    let s = format!("{v:.4}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" {
        "0".into()
    } else {
        s.into()
    }
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use plate_core::{Config, PlateSolver, Solver};
    use serde_json::json;

    fn svg(v: serde_json::Value) -> String {
        to_svg(&PlateSolver.solve(&Config::from_value(v).unwrap()).unwrap().layout)
    }

    #[test]
    fn boundary_polygon_at_print_scale() {
        let out = svg(
            json!({ "name": "A & B", "start": { "mode": "boundary", "points": [[0, 0], [30, 0], [30, 30], [0, 30]] } }),
        );
        assert!(
            out.contains(
                r#"<polygon id="boundary" data-layer="PLATE-BOUNDARY" points="0,0 30,0 30,30 0,30""#
            ),
            "{out}"
        );
        assert!(out.contains("A &amp; B"));
        assert!(out.contains(r#"width="360.00mm""#), "{out}");
    }

    #[test]
    fn spine_polyline_and_origin_marker() {
        assert!(
            svg(json!({ "start": { "mode": "spine", "points": [[0, 0], [5, 0]] } })).contains("<polyline")
        );
        assert!(svg(json!({ "start": { "mode": "spine", "points": [[0, 0]] } })).contains("<circle"));
    }
}
