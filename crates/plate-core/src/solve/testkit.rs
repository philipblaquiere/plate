//! Shared helpers for solver tests, and **pinned plates**: test layouts where every space has an
//! `at` corner and an exact size, so the engine has nothing to move and returns at once. A pinned
//! plate still goes through the whole host (build → fit → connect → route), so a change to walls,
//! parts or networks is tested in milliseconds, on a layout that can't shift under the test.
//!
//! Use [`pinned_flat`] (and [`power`], [`lighting`], [`plumbing`]) for anything that happens after
//! the engine. Use an unpinned plate (like `flat` in `built_tests.rs`) only when the test is about
//! where the engine puts things. `pinned_plates_dont_anneal` keeps the promise honest.

#![allow(dead_code)]

use serde_json::{json, Value};

use super::{solve, Solution};
use crate::config::Config;
use crate::geometry::Point;
use crate::layout::{Element, ElementKind};

/// Solve a configuration written as JSON (the shape `plate eval` prints).
pub fn run(v: Value) -> Solution {
    solve(&Config::from_value(v).unwrap())
}

/// Every diagnostic as (pointer, message): what to print when an assertion fails.
pub fn messages(s: &Solution) -> Vec<(Option<String>, String)> {
    s.diagnostics.iter().map(|d| (d.pointer.clone(), d.message.clone())).collect()
}

/// Panic with the diagnostics unless every requirement is met and no floor is left over.
#[track_caller]
pub fn assert_met(s: &Solution) {
    assert!(s.report.unmet == 0 && s.report.leftover < 1e-6, "{:#?}", messages(s));
}

pub fn of_kind(s: &Solution, kind: ElementKind) -> Vec<&Element> {
    s.layout.elements.iter().filter(|e| e.kind == kind).collect()
}

/// The element with this id (`bedroom`, `flat_2.bath`), or a panic listing the ids there are.
#[track_caller]
pub fn element<'a>(s: &'a Solution, id: &str) -> &'a Element {
    s.layout.elements.iter().find(|e| e.id == id).unwrap_or_else(|| {
        panic!("no {id}; there are {:?}", s.layout.elements.iter().map(|e| &e.id).collect::<Vec<_>>())
    })
}

/// Walls with `a` on one side and `b` on the other.
pub fn walls_between<'a>(s: &'a Solution, a: &str, b: &str) -> Vec<&'a Element> {
    of_kind(s, ElementKind::Wall)
        .into_iter()
        .filter(|e| {
            let w = e.wall.as_ref().unwrap();
            w.between.iter().any(|n| n == a) && w.between.iter().any(|n| n == b)
        })
        .collect()
}

/// Parts of this name (`outlet`), from any service.
pub fn parts<'a>(s: &'a Solution, part: &str) -> Vec<&'a Element> {
    of_kind(s, ElementKind::Part).into_iter().filter(|e| e.part.as_ref().unwrap().part == part).collect()
}

/// The parts among `of` that are in this space.
pub fn in_space<'a>(of: &[&'a Element], space: &str) -> Vec<&'a Element> {
    of.iter().copied().filter(|e| e.part.as_ref().unwrap().space == space).collect()
}

/// Runs of one network, drawn the way it asked (`routed` or `arcs`), without the other drawing.
pub fn runs<'a>(s: &'a Solution, draw: &str) -> Vec<&'a Element> {
    of_kind(s, ElementKind::Run)
        .into_iter()
        .filter(|e| e.run.as_ref().is_some_and(|r| r.draw == draw))
        .collect()
}

/// Every space as (id, outline).
pub fn spaces_of(s: &Solution) -> Vec<(String, Vec<Point>)> {
    of_kind(s, ElementKind::Space).iter().map(|e| (e.id.clone(), e.points.clone())).collect()
}

/// `base` with the top-level keys of `extra` set (replaced, not merged).
pub fn with(mut base: Value, extra: Value) -> Value {
    if let (Some(extra), Some(obj)) = (extra.as_object(), base.as_object_mut()) {
        for (k, v) in extra {
            obj.insert(k.clone(), v.clone());
        }
    }
    base
}

/// A pinned 12 × 8 flat. Facades along the bottom and top, neighbours left and right.
///
/// ```text
///  y=8 ┌────────┬────────────────────────┐  facade
///      │ bath   │                        │
///  y=6 ├────────┘                        │
///      │              hall (the fill)    │
///y=4.5 ├─────────────┐                   │
///      │             │        y=3.5 ┌────┤
///      │   living    │              │bed-│
///      │             │              │room│
///  y=0 └─────────────┴──────────────┴────┘  facade
///     x=0  x=3.5    x=6            x=8  x=12
/// ```
///
/// living 6 × 4.5 at (0, 0), bedroom 4 × 3.5 at (8, 0), bath 3.5 × 2 at (0, 6), the hall takes the
/// rest (at least 1.5 wide everywhere).
/// Doors from each room to the hall, windows in the living room and the bedroom. Assemblies as
/// in `built_tests.rs`. Override any top-level key with `extra` (`services`, `assemblies`, `seed`).
pub fn pinned_flat(extra: Value) -> Value {
    with(
        json!({
            "seed": 1,
            "start": {
                "mode": "boundary",
                "points": [[0, 0], [12, 0], [12, 8], [0, 8]],
                "ports": ["facade", "blank", "facade", "blank"]
            },
            "assemblies": {
                "exterior": { "on": "facade", "thickness": 0.3, "cavity": 0.14 },
                "party": { "on": "blank", "thickness": 0.25, "fire": 1 },
                "partition": { "on": ["partition", "link", "entrance"], "thickness": 0.1, "cavity": 0.09 },
                "wet": { "on": "wet", "thickness": 0.15, "cavity": 0.14 }
            },
            "spaces": {
                "hall": { "kind": "circulation", "fill": true, "area": { "min": 8, "max": 60 }, "side": { "min": 1.2 } },
                "living": { "at": [0, 0], "width": 6, "depth": 4.5 },
                "bedroom": { "at": [8, 0], "width": 4, "depth": 3.5, "tags": ["sleep"] },
                "bath": { "at": [0, 6], "width": 3.5, "depth": 2, "walls": "wet", "tags": ["wet"] }
            },
            "rules": [
                { "rule": "door", "a": ["living", "#sleep", "#wet"], "b": "hall" },
                { "rule": "window", "a": ["living", "#sleep"] }
            ]
        }),
        extra,
    )
}

/// [`pinned_flat`] with these services (`{ "power": power(), … }`).
pub fn pinned_flat_with(services: Value) -> Value {
    pinned_flat(json!({ "services": services }))
}

/// A small power service: a panel in the hall, outlets along the rooms' walls, one circuit kind,
/// routed through walls and the hall's ceiling.
pub fn power() -> Value {
    json!({
        "parts": {
            "panel": { "above": 1.5, "width": 0.4 },
            "outlet": { "above": 0.3 }
        },
        "place": [
            { "place": "one", "part": "panel", "spaces": "hall" },
            { "place": "every", "part": "outlet", "spaces": ["living", "#sleep"], "spacing": 3.6 }
        ],
        "connect": [
            { "label": "General", "from": "panel", "to": "outlet", "max": 6, "draw": "routed", "via": { "ceilings": "hall" } }
        ]
    })
}

/// Switches by the latch of each room's door, a light in the middle of each room.
pub fn lighting() -> Value {
    json!({
        "parts": { "switch": { "above": 1.1 }, "light": { "mount": "ceiling" } },
        "place": [
            { "place": "beside", "part": "switch", "opening": "door", "spaces": ["living", "#sleep", "#wet"] },
            { "place": "over", "part": "light", "spaces": ["living", "#sleep", "#wet"] }
        ]
    })
}

/// A stack in the bath's wet wall, a drain in the bath, routed in the walls.
pub fn plumbing() -> Value {
    json!({
        "parts": {
            "stack": { "mount": "cavity", "width": 0.1, "depth": 0.1 },
            "drain": { "mount": "floor" }
        },
        "place": [
            { "place": "one", "part": "stack", "spaces": "#wet", "wall": { "cavity": 0.12 } },
            { "place": "over", "part": "drain", "spaces": "#wet" }
        ],
        "connect": [{ "from": "stack", "to": "drain", "draw": "routed", "limit": 10 }]
    })
}

#[test]
fn pinned_plates_dont_anneal() {
    let s =
        run(pinned_flat_with(json!({ "power": power(), "lighting": lighting(), "plumbing": plumbing() })));
    assert_met(&s);
    assert_eq!(s.moves, 0, "a pinned plate should leave the engine nothing to move");
    // Every room is where it was pinned (drawn to its walls' faces, so within a wall's width).
    let b = |id: &str| crate::geometry::Bounds::from_points(&element(&s, id).points);
    for (id, x0, y0, x1, y1) in
        [("living", 0.0, 0.0, 6.0, 4.5), ("bedroom", 8.0, 0.0, 12.0, 3.5), ("bath", 0.0, 6.0, 3.5, 8.0)]
    {
        let r = b(id);
        for (got, want) in [(r.min_x, x0), (r.min_y, y0), (r.max_x, x1), (r.max_y, y1)] {
            assert!((got - want).abs() <= 0.31, "{id}: {r:?}");
        }
    }
}
