use dxf::entities::{Circle, Entity, EntityType, LwPolyline, ModelPoint, Text};
use dxf::enums::{AcadVersion, DrawingUnits, Units as DxfUnits};
use dxf::tables::Layer;
use dxf::LwPolylineVertex;
use dxf::{Color, Drawing, Point as DxfPoint};
use plate_core::{Element, ElementKind, Layout, Point, SpaceKind, System, Units};

use crate::ExportError;

const ANNOTATION_LAYER: &str = "PLATE-ANNO";

pub fn to_dxf(layout: &Layout) -> Result<Vec<u8>, ExportError> {
    let mut drawing = Drawing::new();
    drawing.header.version = AcadVersion::R2000;
    drawing.header.default_drawing_units = dxf_units(layout.units);
    drawing.header.drawing_units = match layout.system {
        System::Imperial => DrawingUnits::English,
        System::Metric => DrawingUnits::Metric,
    };

    let mut layers: Vec<String> = layout.elements.iter().map(|e| e.cad_layer()).collect();
    layers.sort_unstable();
    layers.dedup();
    for name in layers {
        drawing.add_layer(Layer { name, color: Color::from_index(7), ..Default::default() });
    }
    drawing.add_layer(Layer {
        name: ANNOTATION_LAYER.to_owned(),
        color: Color::from_index(8),
        ..Default::default()
    });

    // Annotation size relative to the drawing (a single origin has no extent: use 1 m).
    let b = layout.bounds;
    let span = b.width().max(b.height()).max(1000.0 / layout.units.to_mm());
    let height = span / 40.0;

    for element in &layout.elements {
        let layer = element.cad_layer();
        // A symbol, or a part drawn with one: one polyline per path, on its own layer.
        if let (ElementKind::Symbol | ElementKind::Part, Some(symbol)) = (element.kind, &element.symbol) {
            for path in &symbol.paths {
                let mut poly = LwPolyline::default();
                poly.set_is_closed(path.closed);
                poly.vertices = path
                    .points
                    .iter()
                    .map(|p| LwPolylineVertex { x: p.x, y: p.y, ..Default::default() })
                    .collect();
                let mut entity = Entity::new(EntityType::LwPolyline(poly));
                entity.common.layer = layer.clone();
                drawing.add_entity(entity);
            }
            continue;
        }
        // A wall: one closed polyline per stretch between its openings.
        if element.kind == ElementKind::Wall {
            for piece in wall_pieces(element, &layout.elements) {
                let mut poly = LwPolyline::default();
                poly.set_is_closed(true);
                poly.vertices =
                    piece.iter().map(|p| LwPolylineVertex { x: p.x, y: p.y, ..Default::default() }).collect();
                let mut entity = Entity::new(EntityType::LwPolyline(poly));
                entity.common.layer = layer.clone();
                drawing.add_entity(entity);
            }
            continue;
        }
        let specific = match element.kind {
            // A free origin: a point plus a small circle so it's visible in every CAD viewer.
            ElementKind::Origin => {
                let p = element.points[0];
                let mut point =
                    Entity::new(EntityType::ModelPoint(ModelPoint::new(DxfPoint::new(p.x, p.y, 0.0))));
                point.common.layer = layer.clone();
                drawing.add_entity(point);
                EntityType::Circle(Circle::new(DxfPoint::new(p.x, p.y, 0.0), height / 2.0))
            }
            ElementKind::Boundary
            | ElementKind::Spine
            | ElementKind::Space
            | ElementKind::Door
            | ElementKind::Window
            | ElementKind::Slot
            | ElementKind::Port
            | ElementKind::Clearance
            | ElementKind::Leftover
            | ElementKind::Symbol
            | ElementKind::Wall
            | ElementKind::Part
            | ElementKind::Run => {
                let mut poly = LwPolyline::default();
                poly.set_is_closed(element.closed);
                poly.vertices = element
                    .points
                    .iter()
                    .map(|p| LwPolylineVertex { x: p.x, y: p.y, ..Default::default() })
                    .collect();
                EntityType::LwPolyline(poly)
            }
        };
        let mut entity = Entity::new(specific);
        entity.common.layer = layer.clone();
        drawing.add_entity(entity);
        // Holes: one closed polyline each, on the same layer.
        for hole in &element.holes {
            let mut poly = LwPolyline::default();
            poly.set_is_closed(true);
            poly.vertices =
                hole.iter().map(|p| LwPolylineVertex { x: p.x, y: p.y, ..Default::default() }).collect();
            let mut entity = Entity::new(EntityType::LwPolyline(poly));
            entity.common.layer = layer.clone();
            drawing.add_entity(entity);
        }

        // Port roles, at the middle of their edge.
        if let (ElementKind::Port, Some(roles)) = (element.kind, &element.roles) {
            let (a, b) = (element.points[0], element.points[1]);
            let centre = DxfPoint::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0, 0.0);
            let mut text = Text {
                value: roles.iter().map(|r| format!("'{r}")).collect::<Vec<_>>().join(", "),
                location: centre.clone(),
                text_height: height / 2.0,
                rotation: (b.y - a.y).atan2(b.x - a.x).to_degrees(),
                horizontal_text_justification: dxf::enums::HorizontalTextJustification::Center,
                vertical_text_justification: dxf::enums::VerticalTextJustification::Bottom,
                ..Default::default()
            };
            text.second_alignment_point = centre;
            let mut entity = Entity::new(EntityType::Text(text));
            entity.common.layer = ANNOTATION_LAYER.to_owned();
            drawing.add_entity(entity);
        }

        // Space labels: name and area, centred, sized to the space.
        // A fixture drawn with a symbol needs no label.
        let symbol_only =
            element.space.as_ref().is_some_and(|s| s.kind == SpaceKind::Fixture && s.symbol.is_some());
        if let (ElementKind::Space, Some(info), false) = (element.kind, &element.space, symbol_only) {
            let sb = plate_core::Bounds::from_points(&element.points);
            let size = (sb.width().min(sb.height()) / 6.0).min(height);
            let at = element
                .label_at
                .unwrap_or(plate_core::Point::new((sb.min_x + sb.max_x) / 2.0, (sb.min_y + sb.max_y) / 2.0));
            let centre = DxfPoint::new(at.x, at.y, 0.0);
            let area = element.area.unwrap_or(0.0);
            // Fixtures are labelled by name only: their area isn't floor space.
            let value = if info.kind == SpaceKind::Fixture {
                info.label.clone()
            } else {
                format!("{} {:.1} {}2", info.label, area, layout.units.symbol())
            };
            let mut text = Text {
                value,
                location: centre.clone(),
                text_height: size,
                horizontal_text_justification: dxf::enums::HorizontalTextJustification::Center,
                vertical_text_justification: dxf::enums::VerticalTextJustification::Middle,
                ..Default::default()
            };
            text.second_alignment_point = centre;
            let mut entity = Entity::new(EntityType::Text(text));
            entity.common.layer = ANNOTATION_LAYER.to_owned();
            drawing.add_entity(entity);
        }
    }

    // Title text below the plate.
    let mut title = Text {
        value: layout.name.clone(),
        location: DxfPoint::new(b.min_x, b.min_y - height * 2.5, 0.0),
        text_height: height,
        ..Default::default()
    };
    title.second_alignment_point = title.location.clone();
    let mut entity = Entity::new(EntityType::Text(title));
    entity.common.layer = ANNOTATION_LAYER.to_owned();
    drawing.add_entity(entity);

    let mut out = Vec::new();
    drawing.save(&mut out).map_err(|e| ExportError(e.to_string()))?;
    Ok(out)
}

fn dxf_units(units: Units) -> DxfUnits {
    match units {
        Units::Mm => DxfUnits::Millimeters,
        Units::Cm => DxfUnits::Centimeters,
        Units::M => DxfUnits::Meters,
        Units::In => DxfUnits::Inches,
        Units::Ft => DxfUnits::Feet,
    }
}

/// A wall's rectangle, cut where the doors and windows set in it are. The rectangle's first two
/// points run along the wall, the next two back along its other face.
fn wall_pieces(wall: &Element, elements: &[Element]) -> Vec<Vec<Point>> {
    let [p, q, r, _] = wall.points[..] else { return vec![wall.points.clone()] };
    let len = p.distance(q);
    let width = q.distance(r);
    if len <= 0.0 || width <= 0.0 {
        return vec![wall.points.clone()];
    }
    let u = Point::new((q.x - p.x) / len, (q.y - p.y) / len);
    let n = Point::new((r.x - q.x) / width, (r.y - q.y) / width);
    let along = |o: Point| (o.x - p.x) * u.x + (o.y - p.y) * u.y;
    let across = |o: Point| (o.x - p.x) * n.x + (o.y - p.y) * n.y;
    let tol = 1e-6 * (1.0 + len);

    let mut cuts: Vec<(f64, f64)> = elements
        .iter()
        .filter(|e| matches!(e.kind, ElementKind::Door | ElementKind::Window))
        .filter(|e| e.opening.as_ref().is_some_and(|o| o.depth.is_some()) && e.points.len() == 2)
        .filter(|e| e.points.iter().all(|o| (-tol..=width + tol).contains(&across(*o))))
        .map(|e| {
            let (a, b) = (along(e.points[0]), along(e.points[1]));
            (a.min(b).max(0.0), a.max(b).min(len))
        })
        .filter(|(a, b)| b - a > tol)
        .collect();
    cuts.sort_by(|a, b| a.0.total_cmp(&b.0));

    let rectangle = |s: f64, e: f64| {
        let at = |t: f64, d: f64| Point::new(p.x + u.x * t + n.x * d, p.y + u.y * t + n.y * d);
        vec![at(s, 0.0), at(e, 0.0), at(e, width), at(s, width)]
    };
    let mut pieces = Vec::new();
    let mut from = 0.0;
    for (a, b) in cuts {
        if a - from > tol {
            pieces.push(rectangle(from, a));
        }
        from = f64::max(from, b);
    }
    if len - from > tol {
        pieces.push(rectangle(from, len));
    }
    pieces
}

#[cfg(test)]
mod tests {
    use super::*;
    use plate_core::layout::OpeningInfo;
    use plate_core::{Config, PlateSolver, Solver};
    use serde_json::json;

    fn opening(kind: ElementKind, a: Point, b: Point, depth: Option<f64>) -> Element {
        let info = OpeningInfo { between: Vec::new(), width: a.distance(b), depth };
        Element::opening("o".to_owned(), kind, a, b, info)
    }

    #[test]
    fn a_wall_is_cut_where_its_openings_are() {
        let wall = Element::new(
            "w".to_owned(),
            ElementKind::Wall,
            vec![Point::new(0.0, 0.0), Point::new(10.0, 0.0), Point::new(10.0, 0.2), Point::new(0.0, 0.2)],
            true,
        );
        let at = |x: f64, y: f64| Point::new(x, y);
        let others = [
            opening(ElementKind::Door, at(2.0, 0.1), at(3.0, 0.1), Some(0.2)),
            opening(ElementKind::Window, at(6.0, 0.1), at(5.0, 0.1), Some(0.2)),
            // Not in this wall: on another line, and set in no wall at all.
            opening(ElementKind::Door, at(7.0, 3.0), at(8.0, 3.0), Some(0.2)),
            opening(ElementKind::Door, at(8.5, 0.1), at(9.0, 0.1), None),
        ];
        let pieces = wall_pieces(&wall, &others);
        let spans: Vec<(f64, f64)> = pieces.iter().map(|r| (r[0].x, r[1].x)).collect();
        assert_eq!(spans, vec![(0.0, 2.0), (3.0, 5.0), (6.0, 10.0)]);
        assert!(pieces.iter().all(|r| r.len() == 4 && (r[2].y - 0.2).abs() < 1e-9));
        // No openings: the wall as it is.
        assert_eq!(wall_pieces(&wall, &[]).len(), 1);
    }

    fn dxf(v: serde_json::Value) -> Drawing {
        let layout = PlateSolver.solve(&Config::from_value(v).unwrap()).unwrap().layout;
        let bytes = to_dxf(&layout).unwrap();
        Drawing::load(&mut bytes.as_slice()).unwrap()
    }

    fn polylines(d: &Drawing) -> Vec<(String, LwPolyline)> {
        d.entities()
            .filter_map(|e| match &e.specific {
                EntityType::LwPolyline(p) => Some((e.common.layer.clone(), p.clone())),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn boundary_is_a_closed_polyline_in_metres() {
        let d =
            dxf(json!({ "start": { "mode": "boundary", "points": [[0, 0], [30, 0], [30, 20], [0, 20]] } }));
        assert_eq!(d.header.default_drawing_units, DxfUnits::Meters);
        let polys = polylines(&d);
        assert_eq!(polys.len(), 1);
        assert_eq!(polys[0].0, "PLATE-BOUNDARY");
        assert!(polys[0].1.is_closed());
        assert_eq!(polys[0].1.vertices.len(), 4);
    }

    #[test]
    fn spine_is_open_and_origin_is_a_point() {
        let d = dxf(json!({ "start": { "mode": "spine", "points": [[0, 0], [10, 0], [10, 5]] } }));
        let polys = polylines(&d);
        assert_eq!(polys[0].0, "PLATE-SPINE");
        assert!(!polys[0].1.is_closed());

        let d = dxf(json!({ "start": { "mode": "spine", "points": [[3, 4]] } }));
        assert!(d
            .entities()
            .any(|e| matches!(e.specific, EntityType::ModelPoint(_)) && e.common.layer == "PLATE-ORIGIN"));
    }

    #[test]
    fn spaces_are_closed_polylines_with_labels() {
        let d = dxf(json!({
            "start": { "mode": "boundary", "points": [[0, 0], [12, 0], [12, 8], [0, 8]] },
            "spaces": { "living": { "area": { "min": 40 } }, "kitchen": { "area": { "min": 12 } } },
            "rules": [{ "rule": "door", "a": "kitchen", "b": "living" }]
        }));
        let spaces: Vec<_> = polylines(&d).into_iter().filter(|(l, _)| l == "PLATE-SPACE-ROOM").collect();
        assert_eq!(spaces.len(), 2);
        assert!(spaces.iter().all(|(_, p)| p.is_closed()));
        assert!(polylines(&d).iter().any(|(l, _)| l == "PLATE-DOOR"));
        assert!(d
            .entities()
            .any(|e| matches!(&e.specific, EntityType::Text(t) if t.value.starts_with("kitchen"))));
    }

    #[test]
    fn imperial_layout_is_english_in_feet() {
        let d = dxf(
            json!({ "system": "imperial", "start": { "mode": "spine", "points": [[0, 0], ["98'-6", 0]] } }),
        );
        assert_eq!(d.header.default_drawing_units, DxfUnits::Feet);
        assert_eq!(d.header.drawing_units, DrawingUnits::English);
    }
}
