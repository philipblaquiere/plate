//! `align`: which way a plan's grid runs.

use super::*;
use crate::diagnostic::Severity;
use crate::geometry::runs_along;
use serde_json::{json, Value};

fn run(v: Value) -> Solution {
    solve(&Config::from_value(v).unwrap())
}

fn space<'a>(s: &'a Solution, id: &str) -> &'a Element {
    s.layout.elements.iter().find(|e| e.kind == ElementKind::Space && e.id == id).unwrap()
}

fn assert_met(s: &Solution) {
    let unmet: Vec<&str> =
        s.diagnostics.iter().filter(|d| d.severity != Severity::Info).map(|d| d.message.as_str()).collect();
    assert!(unmet.is_empty() && s.report.unmet == 0, "{unmet:?}");
}

/// Every edge of `poly` runs along `angle` (or square to it), but those in `except`.
fn square_to(poly: &[Point], angle: f64, except: &[f64]) -> bool {
    (0..poly.len()).all(|k| {
        let (a, b) = (poly[k], poly[(k + 1) % poly.len()]);
        runs_along(a, b, angle) || except.iter().any(|e| runs_along(a, b, *e))
    })
}

/// A 3-4-5 turn: along (4, 3), across (-3, 4).
fn turned(x: f64, y: f64) -> [f64; 2] {
    [0.8 * x - 0.6 * y, 0.6 * x + 0.8 * y]
}

const TURN: f64 = 0.643_501_108_793_284_4; // atan(3 / 4)

#[test]
fn a_plate_is_framed_square_to_most_of_its_walls_not_its_longest() {
    // 15 × 15, turned, with a corner cut off by its longest wall.
    let pts: Vec<Point> = [(0.0, 0.0), (15.0, 0.0), (15.0, 3.0), (3.0, 15.0), (0.0, 15.0)]
        .iter()
        .map(|&(x, y)| {
            let [a, b] = turned(x, y);
            Point::new(a, b)
        })
        .collect();
    let (angle, k, written) = root_frame(&Plan::default(), &pts);
    assert!(!written);
    // Along the walls (the bottom and left ones are as long: either will do).
    let (a, b) = (Point::new(0.0, 0.0), Point::new(angle.cos(), angle.sin()));
    assert!(runs_along(a, b, TURN), "{angle}");
    assert!(k == 0 || k == 4, "framed from a square wall, not the long slanted one: {k}");
    // A rectangle is framed from its longest edge, as it always was.
    let rect = [Point::new(0.0, 0.0), Point::new(8.0, 0.0), Point::new(8.0, 6.0), Point::new(0.0, 6.0)];
    assert_eq!(root_frame(&Plan::default(), &rect), (0.0, 2, false));

    let pts: Vec<[f64; 2]> = [(0.0, 0.0), (15.0, 0.0), (15.0, 3.0), (3.0, 15.0), (0.0, 15.0)]
        .iter()
        .map(|&(x, y)| turned(x, y))
        .collect();
    let s = run(json!({
        "start": { "mode": "boundary", "points": pts },
        "spaces": {
            "hall": { "kind": "circulation", "fill": true, "area": { "min": 6 }, "side": { "min": 1.2 } },
            "living": { "area": { "min": 40, "max": 60 }, "side": { "min": 4.5 } },
            "room": { "count": 3, "area": { "min": 12, "max": 18 }, "side": { "min": 3 } },
        },
        "rules": [{ "rule": "door", "a": ["living", "room"], "b": "hall" }],
    }));
    assert_met(&s);
    // Every room is square to the house's walls; only the long wall cuts some.
    let slant = TURN + std::f64::consts::FRAC_PI_4;
    for id in ["living", "room_1", "room_2", "room_3"] {
        let p = &space(&s, id).points;
        assert!(square_to(p, TURN, &[slant]), "{id}: {p:?}");
    }
}

#[test]
fn pinned_spaces_keep_the_plates_axes() {
    // Mostly turned walls, but a pinned space is written in the plate's coordinates: the grid
    // stays square to the x axis, so it stays square too.
    let s = run(json!({
        "start": { "mode": "boundary", "points": [[0, 0], [12, 0], [12, 2], [8, 8], [0, 8]] },
        "spaces": {
            "core": { "at": [0, 0], "width": 4, "depth": 4 },
            "office": { "kind": "zone", "fill": true },
        },
    }));
    let core = &space(&s, "core").points;
    let b = crate::geometry::Bounds::from_points(core);
    assert!((b.min_x, b.min_y, b.max_x, b.max_y) == (0.0, 0.0, 4.0, 4.0), "{core:?}");
}

#[test]
fn a_written_angle_turns_the_grid_and_pins_along_it() {
    // A 10 × 8 outline turned by a 3-4-5, the grid written at that angle, and a space pinned at
    // the origin along the turned grid.
    let outline: Vec<[f64; 2]> =
        [(0.0, 0.0), (10.0, 0.0), (10.0, 8.0), (0.0, 8.0)].iter().map(|&(x, y)| turned(x, y)).collect();
    let s = run(json!({
        "align": TURN.to_degrees(),
        "start": { "mode": "boundary", "points": outline },
        "spaces": {
            "store": { "at": [0, 0], "width": 4, "depth": 3 },
            "room": { "kind": "zone", "fill": true },
        },
    }));
    assert_met(&s);
    let store = &space(&s, "store").points;
    let want = [turned(0.0, 0.0), turned(4.0, 0.0), turned(4.0, 3.0), turned(0.0, 3.0)];
    for w in want {
        assert!(store.iter().any(|p| p.distance(Point::new(w[0], w[1])) < 1e-6), "{w:?} not in {store:?}");
    }
}

#[test]
fn a_plan_written_at_an_angle_is_solved_on_a_turned_grid() {
    // A 10 × 10 room whose plan runs its grid at 45°: its rooms are square to that, cut by the
    // room's own walls, and floor is all taken.
    let s = run(json!({
        "start": { "mode": "boundary", "points": [[0, 0], [10, 0], [10, 10], [0, 10]] },
        "spaces": {
            "pavilion": {
                "at": [0, 0], "width": 10, "depth": 10,
                "plan": {
                    "align": 45,
                    "spaces": {
                        "a": { "area": { "min": 25, "max": 60 }, "side": { "min": 3 } },
                        "b": { "kind": "zone", "fill": true },
                    },
                },
            },
        },
    }));
    assert_met(&s);
    let a = &space(&s, "pavilion.a").points;
    let quarter = std::f64::consts::FRAC_PI_4;
    assert!(square_to(a, quarter, &[0.0]), "{a:?}");
    assert!((0..a.len())
        .any(|k| runs_along(a[k], a[(k + 1) % a.len()], quarter)
            && !runs_along(a[k], a[(k + 1) % a.len()], 0.0)));
    assert_eq!(s.report.leftover, 0.0);
}

#[test]
fn a_level_square_to_its_axes_is_never_turned() {
    let slot = Slot {
        boundary: Some(vec![
            Point::new(0.0, 0.0),
            Point::new(6.0, 0.0),
            Point::new(6.0, 4.0),
            Point::new(0.0, 4.0),
        ]),
        ports: Vec::new(),
        spine: None,
        pin: Transform::IDENTITY,
        holes: Vec::new(),
    };
    assert_eq!(turn_of(&Plan::default(), &slot).0, 0.0);
    let turned_slot = Slot {
        boundary: Some(
            slot.boundary.as_ref().unwrap().iter().map(|p| Transform::rotate(0.3).apply(*p)).collect(),
        ),
        ..slot
    };
    assert!((turn_of(&Plan::default(), &turned_slot).0 - 0.3).abs() < 1e-9);
}
