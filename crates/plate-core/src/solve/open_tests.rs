//! Open plans: what's built between two neighbours (a wall, a door, an opening, nothing), on a
//! pinned plate so every edge is where the test says.

use serde_json::{json, Value};

use super::testkit::{assert_met, element, messages, of_kind, run, walls_between, with};
use crate::layout::ElementKind;
use crate::plan::Leaf;

/// A pinned 12 × 8 flat with an open living area.
///
/// ```text
///  y=8 ┌────────┬─────────────────────────────┐
///      │ bath   │                             │
///  y=6 ├────────┘      hall (the fill)        │
///y=4.5 ├──────────────────────┬──────┐       │
///      │                      ┊      │       │
///      │   living (zone)      ┊dining│ y=3.5 ├───────┐ … nook
///      │                      ┊(zone)│ nook  │
///  y=0 └──────────────────────┴──────┴───────┘
///     x=0                    x=6    x=9     x=12
/// ```
///
/// Living and dining are zones: open to each other and to the hall. The nook is a room with an
/// opening onto the dining area, at least 0.6 m of wall left each side. The bath has a door.
fn open_flat(rules: Value) -> Value {
    json!({
        "seed": 1,
        "start": {
            "mode": "boundary",
            "points": [[0, 0], [12, 0], [12, 8], [0, 8]],
            "ports": ["facade", "blank", "facade", "blank"]
        },
        "assemblies": {
            "exterior": { "on": ["facade", "blank"], "thickness": 0.3 },
            "partition": { "on": ["partition", "link", "entrance"], "thickness": 0.1 }
        },
        "spaces": {
            "hall": { "kind": "circulation", "fill": true, "area": { "min": 8, "max": 60 }, "side": { "min": 1.2 } },
            "living": { "kind": "zone", "at": [0, 0], "width": 6, "depth": 4.5 },
            "dining": { "kind": "zone", "at": [6, 0], "width": 3, "depth": 4.5 },
            "nook": { "at": [9, 0], "width": 3, "depth": 3.5 },
            "bath": { "at": [0, 6], "width": 3.5, "depth": 2 }
        },
        "rules": rules
    })
}

fn base_rules() -> Vec<Value> {
    vec![
        json!({ "rule": "door", "a": "bath", "b": "hall" }),
        json!({ "rule": "door", "a": "nook", "b": "dining", "leaf": "none", "width": { "min": 1.5 }, "margin": 0.6 }),
    ]
}

fn rules_with(extra: Vec<Value>) -> Value {
    Value::Array(base_rules().into_iter().chain(extra).collect())
}

#[test]
fn zones_open_onto_zones_and_circulation() {
    let s = run(open_flat(rules_with(vec![])));
    assert_met(&s);
    assert!(walls_between(&s, "living", "dining").is_empty(), "living and dining are open floor");
    assert!(walls_between(&s, "living", "hall").is_empty(), "a zone is open to circulation");
    assert!(walls_between(&s, "dining", "hall").is_empty());
    assert!(!walls_between(&s, "nook", "dining").is_empty(), "the nook is a room: walled");
    assert!(!walls_between(&s, "bath", "hall").is_empty());
}

#[test]
fn an_opening_is_as_wide_as_its_margins_allow() {
    let s = run(open_flat(rules_with(vec![])));
    let o = element(&s, "opening:nook|dining");
    let info = o.opening.as_ref().unwrap();
    assert_eq!(info.leaf, Leaf::NoLeaf);
    // The shared edge is 3.5 long: 0.6 of wall at each end leaves 2.3.
    assert!((info.width - 2.3).abs() < 1e-6, "{}", info.width);
    let ys: Vec<f64> = o.points.iter().map(|p| p.y).collect();
    let (lo, hi) = (ys.iter().copied().fold(f64::INFINITY, f64::min), ys.iter().copied().fold(0.0, f64::max));
    assert!((lo - 0.6).abs() < 1e-6 && (hi - 2.9).abs() < 1e-6, "{lo}..{hi}");
    // A door still has its leaf.
    assert_eq!(element(&s, "door:bath|hall").opening.as_ref().unwrap().leaf, Leaf::Swing);
}

#[test]
fn a_door_needs_a_wall() {
    let s = run(open_flat(rules_with(vec![json!({ "rule": "door", "a": "living", "b": "dining" })])));
    assert!(
        messages(&s)
            .iter()
            .any(|(p, m)| p.as_deref() == Some("/rules/2") && m.contains("no wall between them")),
        "{:?}",
        messages(&s)
    );
    // It's still drawn sensibly: the wall is built, with the door in it.
    assert!(!walls_between(&s, "living", "dining").is_empty());
}

#[test]
fn a_wall_rule_closes_open_floor() {
    let s = run(open_flat(rules_with(vec![
        json!({ "rule": "wall", "a": "living", "b": "dining" }),
        json!({ "rule": "door", "a": "living", "b": "dining" }),
    ])));
    assert_met(&s);
    assert!(!walls_between(&s, "living", "dining").is_empty());
    assert_eq!(element(&s, "door:living|dining").opening.as_ref().unwrap().leaf, Leaf::Swing);
}

#[test]
fn open_and_walled_is_an_error() {
    let s = run(open_flat(rules_with(vec![
        json!({ "rule": "wall", "a": "living", "b": "dining" }),
        json!({ "rule": "door", "a": "living", "b": "dining", "leaf": "none", "width": "full" }),
    ])));
    assert!(
        messages(&s)
            .iter()
            .any(|(p, m)| p.as_deref() == Some("/rules/3") && m.contains("open to each other")),
        "{:?}",
        messages(&s)
    );
}

#[test]
fn open_draws_no_opening() {
    let s = run(open_flat(rules_with(vec![
        json!({ "rule": "door", "a": "dining", "b": "living", "leaf": "none", "width": "full" }),
    ])));
    assert_met(&s);
    assert!(walls_between(&s, "living", "dining").is_empty());
    assert!(of_kind(&s, ElementKind::Door).iter().all(|e| !e.id.contains("dining|living")));
}

#[test]
fn reach_goes_through_open_floor() {
    // Dining reaches the bath through the living room's open floor and the hall: not through the
    // nook, so the rule is broken. (Through doors alone it couldn't reach anything.)
    let s = run(open_flat(rules_with(vec![
        json!({ "rule": "reach", "a": "dining", "b": "bath", "via": "nook" }),
    ])));
    assert!(
        messages(&s).iter().any(|(_, m)| m.contains("dining reaches bath without going through nook")),
        "{:?}",
        messages(&s)
    );
    let s =
        run(open_flat(rules_with(vec![json!({ "rule": "reach", "a": "nook", "b": "bath", "via": "hall" })])));
    assert_met(&s);
}

#[test]
fn a_zone_walled_all_round_is_reported() {
    let mut v = open_flat(rules_with(vec![]));
    v["spaces"]["nook"]["kind"] = json!("zone");
    v["spaces"]["dining"]["kind"] = json!("room");
    v["rules"] = json!([
        { "rule": "door", "a": "bath", "b": "hall" },
        { "rule": "door", "a": "nook", "b": "dining" },
        { "rule": "wall", "a": "nook", "b": "hall" },
    ]);
    let s = run(with(v, json!({})));
    assert!(
        messages(&s).iter().any(|(_, m)| m.contains("nook is a zone with no open edge")),
        "{:?}",
        messages(&s)
    );
}
