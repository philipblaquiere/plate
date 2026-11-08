//! Open plans: what's built between two neighbours (a wall, a door, an opening, nothing), on a
//! pinned plate so every edge is where the test says.

use serde_json::{json, Value};

use super::testkit::{assert_met, element, messages, of_kind, parts, run, walls_between, with};
use super::Solution;
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

fn lighting_by_region() -> Value {
    json!({
        "parts": {
            "switch": { "above": 1.1, "width": 0.08 },
            "light": { "mount": "ceiling" },
            "thermostat": { "above": 1.5, "width": 0.1 }
        },
        "place": [
            { "place": "beside", "part": "switch", "opening": "door", "spaces": "#room" },
            { "place": "one", "part": "switch", "spaces": "#zone", "per": "region", "each": true, "near": "door" },
            { "place": "over", "part": "light", "spaces": ["#room", "#zone"] },
            { "place": "one", "part": "thermostat", "spaces": ["#room", "#zone"], "per": "region" }
        ],
        "connect": [{
            "role": "control", "from": "switch", "to": "light", "draw": "routed",
            "via": { "ceilings": ["#room", "#zone", "#circulation"] }
        }]
    })
}

fn with_services(services: Value) -> Solution {
    let mut v = open_flat(rules_with(vec![]));
    v["services"] = services;
    run(v)
}

fn space_info<'a>(s: &'a Solution, name: &str) -> &'a crate::layout::SpaceInfo {
    element(s, name).space.as_ref().unwrap()
}

#[test]
fn regions_join_spaces_with_no_wall_between_them() {
    let s = run(open_flat(rules_with(vec![])));
    let r = element(&s, "region:living+dining");
    let info = r.region.as_ref().unwrap();
    assert_eq!(info.spaces, vec!["living", "dining"]); // biggest first
    assert_eq!(info.label, "living / dining");
    let sum = element(&s, "living").area.unwrap() + element(&s, "dining").area.unwrap();
    assert!((info.area - sum).abs() < 1e-6, "{} vs {sum}", info.area);
    assert_eq!(space_info(&s, "living").region.as_deref(), Some("living+dining"));
    assert_eq!(space_info(&s, "dining").region.as_deref(), Some("living+dining"));
    // The hall is open to both but is circulation; the nook is behind an opening.
    for name in ["hall", "nook", "bath"] {
        assert_eq!(space_info(&s, name).region, None, "{name}");
    }
    assert_eq!(of_kind(&s, ElementKind::Region).len(), 1);
}

#[test]
fn rooms_opened_by_a_rule_form_a_region() {
    let mut v = open_flat(rules_with(vec![
        json!({ "rule": "door", "a": "nook", "b": "bath", "leaf": "none", "width": "full" }),
    ]));
    // Make the bath a neighbour of the nook: pin it beside it.
    v["spaces"]["bath"] = json!({ "at": [9, 3.5], "width": 3, "depth": 2 });
    v["rules"] = json!([
        { "rule": "door", "a": "nook", "b": "dining", "leaf": "none", "width": { "min": 1.5 }, "margin": 0.6 },
        { "rule": "door", "a": "nook", "b": "bath", "leaf": "none", "width": "full" },
    ]);
    let s = run(v);
    assert!(walls_between(&s, "nook", "bath").is_empty());
    assert_eq!(space_info(&s, "nook").region.as_deref(), Some("nook+bath"));
}

#[test]
fn per_region_places_one_part_for_the_region_and_one_per_room() {
    let s = with_services(json!({ "lighting": lighting_by_region() }));
    assert_met(&s);
    let thermostats = parts(&s, "thermostat");
    assert_eq!(thermostats.len(), 3, "nook, bath, and one for living and dining");
    let serving: Vec<Vec<String>> =
        thermostats.iter().map(|e| e.part.as_ref().unwrap().serves.clone()).collect();
    assert!(serving.contains(&vec!["living".to_owned(), "dining".to_owned()]), "{serving:?}");
    assert_eq!(serving.iter().filter(|v| v.is_empty()).count(), 2);
}

#[test]
fn each_puts_a_part_per_space_side_by_side() {
    let s = with_services(json!({ "lighting": lighting_by_region() }));
    let zone: Vec<&crate::layout::Element> =
        parts(&s, "switch").into_iter().filter(|e| !e.part.as_ref().unwrap().serves.is_empty()).collect();
    assert_eq!(zone.len(), 2);
    let served: Vec<&str> = zone.iter().map(|e| e.part.as_ref().unwrap().serves[0].as_str()).collect();
    assert_eq!(served, vec!["living", "dining"]);
    let (a, b) = (zone[0].part.as_ref().unwrap().at, zone[1].part.as_ref().unwrap().at);
    assert!((a.distance(b) - 0.08).abs() < 1e-6, "side by side, a switch's width apart: {a:?} {b:?}");
}

#[test]
fn a_switch_controls_the_lights_of_the_space_it_serves() {
    use std::collections::HashMap;
    let s = with_services(json!({ "lighting": lighting_by_region() }));
    // Group the control runs' points into connected pieces.
    let key = |p: crate::geometry::Point| ((p.x * 1e4).round() as i64, (p.y * 1e4).round() as i64);
    let mut parent: HashMap<(i64, i64), (i64, i64)> = HashMap::new();
    fn find(parent: &mut HashMap<(i64, i64), (i64, i64)>, k: (i64, i64)) -> (i64, i64) {
        let p = *parent.entry(k).or_insert(k);
        if p == k {
            k
        } else {
            let r = find(parent, p);
            parent.insert(k, r);
            r
        }
    }
    for e in of_kind(&s, ElementKind::Run) {
        let r = e.run.as_ref().unwrap();
        if !r.control || r.draw != "routed" {
            continue;
        }
        for w in e.points.windows(2) {
            let (a, b) = (find(&mut parent, key(w[0])), find(&mut parent, key(w[1])));
            parent.insert(a, b);
        }
    }
    let at = |space: &str, part: &str, serves: Option<&str>| {
        parts(&s, part)
            .into_iter()
            .map(|e| e.part.as_ref().unwrap())
            .find(|p| match serves {
                Some(x) => p.serves.first().map(String::as_str) == Some(x),
                None => p.space == space,
            })
            .unwrap()
            .at
    };
    let living_light = find(&mut parent, key(at("living", "light", None)));
    let living_switch = find(&mut parent, key(at("", "switch", Some("living"))));
    let dining_switch = find(&mut parent, key(at("", "switch", Some("dining"))));
    let dining_light = find(&mut parent, key(at("dining", "light", None)));
    assert_eq!(living_light, living_switch, "the living room's switch is on the dining wall");
    assert_eq!(dining_light, dining_switch);
    assert_ne!(living_light, dining_light);
}

#[test]
fn every_shared_edge_says_what_is_built_and_why() {
    let s = run(open_flat(rules_with(vec![])));
    let edges: Vec<&crate::layout::EdgeInfo> =
        of_kind(&s, ElementKind::Edge).into_iter().map(|e| e.edge.as_ref().unwrap()).collect();
    let find = |a: &str, b: &str| {
        edges
            .iter()
            .find(|e| e.between.contains(&a.to_owned()) && e.between.contains(&b.to_owned()))
            .unwrap_or_else(|| panic!("no edge between {a} and {b}"))
    };
    let e = find("living", "dining");
    assert_eq!((e.treatment.as_str(), e.rule.as_deref()), ("open", None));
    assert!(e.why.contains("zone"), "{}", e.why);
    let e = find("nook", "dining");
    assert_eq!((e.treatment.as_str(), e.rule.as_deref()), ("opening", Some("/rules/1")));
    let e = find("bath", "hall");
    assert_eq!((e.treatment.as_str(), e.rule.as_deref()), ("door", Some("/rules/0")));
    assert_eq!(find("living", "hall").treatment, "open");
    // Not exported: it's for inspecting the drawing.
    assert!(of_kind(&s, ElementKind::Edge).iter().all(|e| !e.exported()));
}
