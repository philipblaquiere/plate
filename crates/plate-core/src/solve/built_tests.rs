//! Assemblies and services, solved end to end.

use serde_json::{json, Value};

use super::*;
use crate::geometry::Bounds;

fn run(v: Value) -> Solution {
    solve(&Config::from_value(v).unwrap())
}

fn messages(s: &Solution) -> Vec<(Option<String>, String)> {
    s.diagnostics.iter().map(|d| (d.pointer.clone(), d.message.clone())).collect()
}

fn of_kind(s: &Solution, kind: ElementKind) -> Vec<&Element> {
    s.layout.elements.iter().filter(|e| e.kind == kind).collect()
}

fn element<'a>(s: &'a Solution, id: &str) -> &'a Element {
    s.layout.elements.iter().find(|e| e.id == id).unwrap_or_else(|| panic!("no {id}"))
}

fn walls_between<'a>(s: &'a Solution, a: &str, b: &str) -> Vec<&'a Element> {
    of_kind(s, ElementKind::Wall)
        .into_iter()
        .filter(|e| {
            let w = e.wall.as_ref().unwrap();
            w.between.iter().any(|n| n == a) && w.between.iter().any(|n| n == b)
        })
        .collect()
}

/// A 12 × 8 flat between neighbours: a living room, a bedroom, a bath, a hall that takes the rest.
fn flat(extra: Value) -> Value {
    let mut v = json!({
        "seed": 1,
        "start": { "mode": "boundary", "points": [[0, 0], [12, 0], [12, 8], [0, 8]], "ports": ["facade", "blank", "facade", "blank"] },
        "assemblies": {
            "exterior": { "on": "facade", "thickness": 0.3, "cavity": 0.14 },
            "party": { "on": "blank", "thickness": 0.25, "fire": 1 },
            "partition": { "on": ["partition", "link", "entrance"], "thickness": 0.1, "cavity": 0.09 },
            "wet": { "on": "wet", "thickness": 0.15, "cavity": 0.14 },
        },
        "spaces": {
            "hall": { "kind": "circulation", "fill": true, "area": { "min": 8, "max": 40 }, "side": { "min": 1.2 } },
            "living": { "area": { "min": 28, "max": 40 }, "side": { "min": 4 } },
            "bedroom": { "area": { "min": 12, "max": 16 }, "side": { "min": 3 }, "tags": ["sleep"] },
            "bath": { "area": { "min": 4.5, "max": 7 }, "side": { "min": 1.8 }, "walls": "wet", "tags": ["wet"] },
        },
        "rules": [
            { "rule": "door", "a": ["living", "#sleep", "#wet"], "b": "hall" },
            { "rule": "window", "a": ["living", "#sleep"] },
        ]
    });
    if let (Some(extra), Some(obj)) = (extra.as_object(), v.as_object_mut()) {
        for (k, val) in extra {
            obj.insert(k.clone(), val.clone());
        }
    }
    v
}

#[test]
fn without_assemblies_nothing_is_built() {
    let mut v = flat(json!({}));
    v.as_object_mut().unwrap().remove("assemblies");
    let s = run(v);
    assert!(of_kind(&s, ElementKind::Wall).is_empty());
    assert!(of_kind(&s, ElementKind::Space).iter().all(|e| e.space.as_ref().unwrap().gross.is_none()));
}

#[test]
fn without_assemblies_a_rule_that_asks_for_a_wall_is_said() {
    let with_rule = |rule: Value| {
        let mut v = flat(json!({}));
        v.as_object_mut().unwrap().remove("assemblies");
        v["rules"].as_array_mut().unwrap().push(rule);
        run(v)
    };
    let s = with_rule(json!({ "rule": "wall", "a": "bath", "port": "wall", "cavity": 0.14, "length": 1.5 }));
    assert!(
        messages(&s).iter().any(|(p, m)| p.as_deref() == Some("/rules/2") && m.contains("a cavity of 0.14")),
        "{:?}",
        messages(&s)
    );
    assert!(s.report.unmet > 0);
    // A selector that matches nothing, and a rule that asks for nothing: no wall is missing.
    for rule in [
        json!({ "rule": "wall", "a": "#nothing", "port": "wall", "cavity": 0.14 }),
        json!({ "rule": "wall", "a": "bath", "port": "wall" }),
    ] {
        let s = with_rule(rule);
        assert!(!messages(&s).iter().any(|(_, m)| m.contains("no assembly")), "{:?}", messages(&s));
    }
}

#[test]
fn walls_are_built_from_the_roles_either_side() {
    let s = run(flat(json!({})));
    assert_eq!(s.report.unmet, 0, "{:?}", messages(&s));
    let walls = of_kind(&s, ElementKind::Wall);
    assert!(!walls.is_empty());
    let built = |e: &Element| e.wall.as_ref().unwrap().assembly.clone();
    // The plate's own edges.
    assert!(walls.iter().any(|e| built(e) == "exterior"));
    assert!(walls.iter().any(|e| built(e) == "party"));
    // Between rooms: the bath's walls are wet to its neighbours, so the wet assembly (it meets
    // both the partition and the wet wall's needs, and is the thinnest that does).
    let bath = walls_between(&s, "bath", "hall");
    assert!(!bath.is_empty(), "{:?}", walls.iter().map(|w| &w.id).collect::<Vec<_>>());
    assert!(
        bath.iter().all(|e| built(e) == "wet"),
        "{:?}",
        bath.iter().map(|e| built(e)).collect::<Vec<_>>()
    );
    let living = walls_between(&s, "living", "hall");
    assert!(living.iter().all(|e| built(e) == "partition"));
    // Footprints are as thick as their assembly.
    for e in walls {
        let w = e.wall.as_ref().unwrap();
        let len = w.line[0].distance(w.line[1]);
        assert!(
            (e.area.unwrap() - len * w.thickness).abs() <= w.thickness * w.thickness * 2.0 + 1e-9,
            "{}",
            e.id
        );
    }
}

#[test]
fn spaces_are_drawn_to_their_walls_faces() {
    let s = run(flat(json!({})));
    let bedroom = element(&s, "bedroom");
    let info = bedroom.space.as_ref().unwrap();
    let gross = info.gross.expect("walls take floor");
    assert!(bedroom.area.unwrap() < gross - 0.5, "{} vs {gross}", bedroom.area.unwrap());
    // Every wall stays clear of every room's clear floor.
    let b = Bounds::from_points(&bedroom.points);
    for w in of_kind(&s, ElementKind::Wall) {
        let wb = Bounds::from_points(&w.points);
        let overlap_x = b.min_x.max(wb.min_x) < b.max_x.min(wb.max_x) - 1e-6;
        let overlap_y = b.min_y.max(wb.min_y) < b.max_y.min(wb.max_y) - 1e-6;
        assert!(!(overlap_x && overlap_y), "{} overlaps the bedroom", w.id);
    }
}

#[test]
fn plans_inside_are_solved_in_the_clear() {
    let mut v = flat(json!({}));
    v["spaces"]["bedroom"]["plan"] = json!({
        "spaces": { "bed": { "kind": "fixture", "width": 1.53, "depth": 2.03, "against": "wall", "clearance": { "front": 0.6 } } }
    });
    let s = run(v);
    assert_eq!(s.report.unmet, 0, "{:?}", messages(&s));
    let bed = element(&s, "bedroom.bed");
    let room = Bounds::from_points(&element(&s, "bedroom").points);
    let b = Bounds::from_points(&bed.points);
    let e = 1e-6;
    assert!(
        b.min_x >= room.min_x - e
            && b.max_x <= room.max_x + e
            && b.min_y >= room.min_y - e
            && b.max_y <= room.max_y + e
    );
    // Its back is against a wall's face, not its centreline.
    let touches = [b.min_x - room.min_x, room.max_x - b.max_x, b.min_y - room.min_y, room.max_y - b.max_y];
    assert!(touches.iter().any(|d| d.abs() < 1e-6), "{touches:?}");
    // The slot the plan was solved in is the clear one.
    let slot = element(&s, "bedroom").space.as_ref().unwrap().slot.clone().unwrap();
    assert!((slot.width * slot.depth - element(&s, "bedroom").area.unwrap()).abs() < 1e-6);
}

#[test]
fn rules_ask_walls_for_more() {
    let mut v = flat(json!({}));
    v["assemblies"]["quiet"] = json!({ "thickness": 0.16, "cavity": 0.09, "acoustic": 50 });
    v["rules"]
        .as_array_mut()
        .unwrap()
        .push(json!({ "rule": "wall", "a": "#sleep", "port": "partition", "acoustic": 45 }));
    let s = run(v);
    assert_eq!(s.report.unmet, 0, "{:?}", messages(&s));
    for w in walls_between(&s, "bedroom", "hall") {
        assert_eq!(w.wall.as_ref().unwrap().assembly, "quiet");
        assert!(w.wall.as_ref().unwrap().because.contains("/rules/2"));
    }
    // Nothing is good enough: said, and pointed at.
    let mut v = flat(json!({}));
    v["rules"]
        .as_array_mut()
        .unwrap()
        .push(json!({ "rule": "wall", "a": "bedroom", "b": "hall", "fire": 2 }));
    let s = run(v);
    assert!(
        messages(&s).iter().any(|(_, m)| m.contains("no assembly is") && m.contains("2 h fire")),
        "{:?}",
        messages(&s)
    );
}

#[test]
fn a_named_assembly_and_one_stretch() {
    let mut v = flat(json!({}));
    v["assemblies"]["chase"] = json!({ "thickness": 0.2, "cavity": 0.18 });
    v["rules"]
        .as_array_mut()
        .unwrap()
        .push(json!({ "rule": "wall", "a": "living", "port": "wall", "assembly": "chase", "length": 1.2 }));
    let s = run(v);
    assert_eq!(s.report.unmet, 0, "{:?}", messages(&s));
    let chases: Vec<&Element> = of_kind(&s, ElementKind::Wall)
        .into_iter()
        .filter(|e| e.wall.as_ref().unwrap().assembly == "chase")
        .collect();
    assert_eq!(chases.len(), 1, "one stretch, not every wall");
    // An unknown assembly is an issue on the rule.
    let mut v = flat(json!({}));
    v["rules"]
        .as_array_mut()
        .unwrap()
        .push(json!({ "rule": "wall", "a": "living", "b": "hall", "assembly": "nope" }));
    let s = run(v);
    assert!(messages(&s)
        .iter()
        .any(|(p, m)| p.as_deref() == Some("/rules/2/assembly") && m.contains("nope")));
}

#[test]
fn a_named_assembly_must_still_meet_what_rules_ask() {
    let with_rules = |rules: Value| {
        let mut v = flat(json!({}));
        v["assemblies"]["plain"] = json!({ "thickness": 0.1 });
        v["rules"].as_array_mut().unwrap().extend(rules.as_array().unwrap().iter().cloned());
        run(v)
    };
    let named = json!({ "rule": "wall", "a": "bedroom", "b": "hall", "assembly": "plain" });
    let rated = json!({ "rule": "wall", "a": "bedroom", "b": "hall", "fire": 1 });
    let unmet = |s: &Solution| {
        messages(s).iter().any(|(p, m)| p.as_deref() == Some("/rules/2/assembly") && m.contains("1 h fire"))
    };
    // Another rule on that wall, and a number on the same rule.
    assert!(unmet(&with_rules(json!([named, rated]))));
    let on_itself = json!({ "rule": "wall", "a": "bedroom", "b": "hall", "assembly": "plain", "fire": 1 });
    assert!(unmet(&with_rules(json!([on_itself]))));
    // What its roles would ask is what naming it overrides.
    let s = with_rules(json!([named]));
    assert_eq!(s.report.unmet, 0, "{:?}", messages(&s));
}

#[test]
fn a_door_to_a_tag_no_space_has_asks_for_nothing() {
    let mut v = flat(json!({}));
    v["rules"].as_array_mut().unwrap().push(json!({ "rule": "door", "a": "hall", "b": "#missing" }));
    let s = run(v);
    assert_eq!(s.report.unmet, 0, "{:?}", messages(&s));
}

#[test]
fn doors_and_windows_sit_in_their_walls() {
    let s = run(flat(json!({})));
    for o in of_kind(&s, ElementKind::Door).into_iter().chain(of_kind(&s, ElementKind::Window)) {
        let depth = o.opening.as_ref().unwrap().depth;
        assert!(depth.is_some_and(|d| d > 0.0), "{} isn't in a wall", o.id);
    }
}

#[test]
fn nested_plans_build_their_own_walls_and_inherit() {
    // A building of two flats; the flat plan builds its partitions with the building's assemblies.
    let flat_plan = json!({
        "needs": { "ports": { "facade": "required" } },
        "spaces": {
            "living": { "area": { "min": 20 }, "side": { "min": 3.5 } },
            "bedroom": { "area": { "min": 10, "max": 14 }, "side": { "min": 3 }, "tags": ["sleep"] },
        },
        "rules": [{ "rule": "door", "a": "#sleep", "b": "living" }, { "rule": "window", "a": ["living", "#sleep"] }]
    });
    let v = json!({
        "start": { "mode": "boundary", "points": [[0, 0], [16, 0], [16, 8], [0, 8]] },
        "assemblies": {
            "exterior": { "on": "facade", "thickness": 0.3 },
            "party": { "on": "party", "thickness": 0.25 },
            "partition": { "on": ["partition", "link"], "thickness": 0.1 },
        },
        "spaces": { "flat": { "count": 2, "width": 8, "depth": 8, "plan": flat_plan, "tags": ["dwelling"] } },
    });
    let s = run(v);
    assert_eq!(s.report.unmet, 0, "{:?}", messages(&s));
    let party = walls_between(&s, "flat_1", "flat_2");
    assert_eq!(party.len(), 1);
    assert_eq!(party[0].wall.as_ref().unwrap().assembly, "party");
    let inner = walls_between(&s, "flat_1.living", "flat_1.bedroom");
    assert_eq!(
        inner.len(),
        1,
        "{:?}",
        of_kind(&s, ElementKind::Wall).iter().map(|w| &w.id).collect::<Vec<_>>()
    );
    assert_eq!(inner[0].wall.as_ref().unwrap().assembly, "partition");
    assert_eq!(inner[0].source.as_deref(), Some("/assemblies/partition"));
    // Both flats are the same typical.
    let typical = |n: &str| element(&s, n).space.as_ref().unwrap().typical.clone();
    assert!(typical("flat_1").is_some());
    assert_eq!(typical("flat_1"), typical("flat_2"));
}

#[test]
fn a_plan_opened_in_its_slot_gets_what_the_plate_gave_it() {
    let mut v = flat(json!({}));
    v["spaces"]["bedroom"]["plan"] = json!({
        "spaces": {
            "bed": { "kind": "fixture", "width": 1.53, "depth": 2.03, "against": "wall" },
            "desk": { "kind": "fixture", "width": 1.2, "depth": 0.6, "against": "wall" },
        },
        "rules": [{ "rule": "apart", "a": "bed", "port": "window", "wall": true }],
    });
    let plate = run(v.clone());
    let slot = element(&plate, "bedroom").space.as_ref().unwrap().slot.clone().unwrap();
    assert!(slot.context.is_some());
    let plan_file = json!({ "spaces": v["spaces"]["bedroom"]["plan"]["spaces"], "rules": v["spaces"]["bedroom"]["plan"]["rules"] });
    let alone = solve_view(&Config::from_value(plan_file).unwrap(), &View::Slot(slot));
    assert!(
        !messages(&alone).iter().any(|(_, m)| m.contains("changes this plan")),
        "the same plan, the same context: {:?}",
        messages(&alone)
    );
}

#[test]
fn claims_add_rules_and_spaces_in_scope() {
    let mut v = flat(json!({}));
    v["spaces"]["bath"]["plan"] = json!({ "spaces": { "toilet": { "kind": "fixture", "width": 0.4, "depth": 0.7, "against": "wall", "tags": ["water"] } } });
    v["services"] = json!({
        "plumbing": {
            "claims": {
                "rules": [{ "rule": "wall", "a": "#wet", "port": "wall", "assembly": "wet", "length": 1.2 }],
                "inside": {
                    "#wet": {
                        "spaces": { "drain": { "kind": "fixture", "width": 0.2, "depth": 0.2, "tags": ["drain"] } },
                        "rules": [{ "rule": "touch", "a": "#water", "port": "wet" }],
                    }
                }
            }
        }
    });
    let s = run(v);
    assert_eq!(s.report.unmet, 0, "{:?}", messages(&s));
    // The claimed fixture is in the bath, and points at the claim.
    let drain = element(&s, "bath.drain");
    assert_eq!(drain.source.as_deref(), Some("/services/plumbing/claims/inside/#wet/spaces/drain"));
    assert!(!drain.space.as_ref().unwrap().plan.starts_with('@'), "{}", drain.space.as_ref().unwrap().plan);
}

#[test]
fn a_claim_that_cant_be_met_points_at_the_claim() {
    let mut v = flat(json!({}));
    v["services"] = json!({
        "plumbing": { "claims": { "rules": [{ "rule": "wall", "a": "#wet", "port": "wall", "cavity": 0.14, "length": 30 }] } }
    });
    let s = run(v);
    assert!(
        messages(&s).iter().any(|(p, _)| p.as_deref() == Some("/services/plumbing/claims/rules/0")),
        "{:?}",
        messages(&s)
    );
}

// Services: where their parts go, and how they're connected.

fn parts<'a>(s: &'a Solution, part: &str) -> Vec<&'a Element> {
    of_kind(s, ElementKind::Part).into_iter().filter(|e| e.part.as_ref().unwrap().part == part).collect()
}

fn in_space<'a>(of: &[&'a Element], space: &str) -> Vec<&'a Element> {
    of.iter().copied().filter(|e| e.part.as_ref().unwrap().space == space).collect()
}

fn with_service(name: &str, service: Value) -> Value {
    let mut v = flat(json!({}));
    v["services"] = json!({ name: service });
    v
}

fn spaces_of(s: &Solution) -> Vec<(String, Vec<Point>)> {
    of_kind(s, ElementKind::Space).iter().map(|e| (e.id.clone(), e.points.clone())).collect()
}

#[test]
fn every_puts_parts_along_the_walls_no_further_apart_than_asked() {
    let spacing = 3.6;
    let s = run(with_service(
        "power",
        json!({ "parts": { "outlet": { "above": 0.3 } }, "place": [{ "place": "every", "part": "outlet", "spaces": "#sleep", "spacing": spacing }] }),
    ));
    assert_eq!(s.report.unmet, 0, "{:?}", messages(&s));
    let outlets = in_space(&parts(&s, "outlet"), "bedroom");
    let room = element(&s, "bedroom");
    let perimeter: f64 = (0..room.points.len())
        .map(|i| room.points[i].distance(room.points[(i + 1) % room.points.len()]))
        .sum();
    assert!(
        outlets.len() as f64 >= (perimeter / spacing).floor(),
        "{} outlets for {perimeter}",
        outlets.len()
    );
    // Each stands on the room's edge, at its height.
    let b = Bounds::from_points(&room.points);
    for o in &outlets {
        let p = o.part.as_ref().unwrap();
        let at = p.at;
        let edge = [at.x - b.min_x, b.max_x - at.x, at.y - b.min_y, b.max_y - at.y]
            .iter()
            .fold(f64::MAX, |m, d| m.min(d.abs()));
        assert!(edge < 0.2, "{} is {edge} off the wall", o.id);
        assert_eq!(p.above, Some(0.3));
    }
    // No point along the walls (but in a doorway) is further than half the spacing from one.
    let doors: Vec<Bounds> =
        of_kind(&s, ElementKind::Door).iter().map(|d| Bounds::from_points(&d.points)).collect();
    let n = room.points.len();
    for i in 0..n {
        let (a, c) = (room.points[i], room.points[(i + 1) % n]);
        let len = a.distance(c);
        let steps = (len / 0.1).ceil() as usize;
        for k in 0..=steps {
            let t = k as f64 / steps as f64;
            let p = Point::new(a.x + (c.x - a.x) * t, a.y + (c.y - a.y) * t);
            if doors.iter().any(|d| {
                p.x > d.min_x - 0.3 && p.x < d.max_x + 0.3 && p.y > d.min_y - 0.3 && p.y < d.max_y + 0.3
            }) {
                continue;
            }
            let near =
                outlets.iter().map(|o| o.part.as_ref().unwrap().at.distance(p)).fold(f64::MAX, f64::min);
            assert!(near <= spacing / 2.0 + 0.25, "{p:?} is {near} from an outlet");
        }
    }
    assert!(
        outlets[0].part.as_ref().unwrap().placed.contains("every 3.6"),
        "{}",
        outlets[0].part.as_ref().unwrap().placed
    );
}

#[test]
fn beside_puts_one_by_each_door_on_the_latch_side() {
    let s = run(with_service(
        "lighting",
        json!({ "parts": { "switch": { "above": 1.1 } }, "place": [{ "place": "beside", "part": "switch", "opening": "door", "spaces": ["#sleep", "#wet"] }] }),
    ));
    assert_eq!(s.report.unmet, 0, "{:?}", messages(&s));
    for room in ["bedroom", "bath"] {
        let switches = in_space(&parts(&s, "switch"), room);
        assert_eq!(switches.len(), 1, "{room}");
        let at = switches[0].part.as_ref().unwrap().at;
        let door = of_kind(&s, ElementKind::Door)
            .into_iter()
            .map(|d| Bounds::from_points(&d.points))
            .map(|d| {
                let dx = (d.min_x - at.x).max(at.x - d.max_x).max(0.0);
                let dy = (d.min_y - at.y).max(at.y - d.max_y).max(0.0);
                dx.hypot(dy)
            })
            .fold(f64::MAX, f64::min);
        assert!(door < 0.5, "{room}'s switch is {door} from a door");
    }
    assert!(parts(&s, "switch").iter().all(|e| e.part.as_ref().unwrap().placed.contains("latch")));
}

#[test]
fn over_puts_one_in_the_middle_or_one_per_area() {
    let s = run(with_service(
        "lighting",
        json!({
            "parts": { "light": { "mount": "ceiling" }, "downlight": { "mount": "ceiling" } },
            "place": [
                { "place": "over", "part": "light", "spaces": "#sleep" },
                { "place": "over", "part": "downlight", "spaces": "living", "per": 6 },
            ]
        }),
    ));
    assert_eq!(s.report.unmet, 0, "{:?}", messages(&s));
    let light = parts(&s, "light");
    assert_eq!(light.len(), 1);
    let room = Bounds::from_points(&element(&s, "bedroom").points);
    let at = light[0].part.as_ref().unwrap().at;
    assert!(
        (at.x - (room.min_x + room.max_x) / 2.0).abs() < 1e-6
            && (at.y - (room.min_y + room.max_y) / 2.0).abs() < 1e-6
    );
    let area = element(&s, "living").area.unwrap();
    let down = parts(&s, "downlight");
    assert!(down.len() as f64 >= (area / 6.0).ceil(), "{} for {area} m²", down.len());
    assert!(down.len() as f64 <= (area / 6.0).ceil() + 2.0, "{} for {area} m²", down.len());
}

#[test]
fn on_puts_one_behind_each_fixture_even_on_its_wall() {
    let mut v = with_service(
        "power",
        json!({
            "parts": { "dedicated": { "above": 0.6 } },
            "place": [{ "place": "on", "part": "dedicated", "fixtures": "#appliance" }]
        }),
    );
    v["spaces"]["living"]["plan"] = json!({
        "spaces": { "fridge": { "kind": "fixture", "width": 0.9, "depth": 0.7, "against": "wall", "tags": ["appliance"] } }
    });
    let s = run(v);
    assert_eq!(s.report.unmet, 0, "{:?}", messages(&s));
    let d = parts(&s, "dedicated");
    assert_eq!(d.len(), 1);
    let fridge = Bounds::from_points(&element(&s, "living.fridge").points);
    let at = d[0].part.as_ref().unwrap().at;
    let e = 0.05;
    assert!(
        at.x >= fridge.min_x - e
            && at.x <= fridge.max_x + e
            && at.y >= fridge.min_y - e
            && at.y <= fridge.max_y + e,
        "{at:?} isn't behind the fridge {fridge:?}"
    );
}

#[test]
fn cavity_parts_go_in_walls_that_have_room_for_them() {
    let s = run(with_service(
        "plumbing",
        json!({
            "parts": { "stack": { "mount": "cavity", "width": 0.1, "depth": 0.1 } },
            "place": [{ "place": "one", "part": "stack", "spaces": "#wet", "wall": { "cavity": 0.12 } }]
        }),
    ));
    assert_eq!(s.report.unmet, 0, "{:?}", messages(&s));
    let stack = parts(&s, "stack");
    assert_eq!(stack.len(), 1);
    let pts = &stack[0].points;
    let at = Point::new(
        pts.iter().map(|p| p.x).sum::<f64>() / pts.len() as f64,
        pts.iter().map(|p| p.y).sum::<f64>() / pts.len() as f64,
    );
    let host = of_kind(&s, ElementKind::Wall).into_iter().find(|w| crate::geometry::contains(&w.points, at));
    let host = host.unwrap_or_else(|| panic!("the stack at {at:?} isn't in a wall"));
    // Not in the room.
    assert!(!crate::geometry::contains(&element(&s, "bath").points, at));
    assert!(host.wall.as_ref().unwrap().cavity >= 0.12, "{}", host.wall.as_ref().unwrap().assembly);
    // Asking for more than any wall has is said, at the placement.
    let s = run(with_service(
        "plumbing",
        json!({
            "parts": { "stack": { "mount": "cavity" } },
            "place": [{ "place": "one", "part": "stack", "spaces": "#wet", "wall": { "cavity": 0.3 } }]
        }),
    ));
    assert!(
        messages(&s)
            .iter()
            .any(|(p, m)| p.as_deref() == Some("/services/plumbing/place/0") && m.contains("no wall")),
        "{:?}",
        messages(&s)
    );
    // As does a part deeper than any cavity: the message says how deep a cavity it needs.
    let s = run(with_service(
        "plumbing",
        json!({ "parts": { "stack": { "mount": "cavity", "depth": 0.2 } }, "place": [{ "place": "one", "part": "stack", "spaces": "#wet" }] }),
    ));
    assert!(
        messages(&s).iter().any(|(_, m)| m.contains("no wall that is a cavity of 0.2 free for a stack")),
        "{:?}",
        messages(&s)
    );
}

#[test]
fn placing_and_connecting_never_move_a_wall() {
    let bare = run(flat(json!({})));
    let lit = run(with_service(
        "power",
        json!({
            "parts": { "panel": {}, "outlet": {}, "light": { "mount": "ceiling" } },
            "place": [
                { "place": "one", "part": "panel", "spaces": "hall", "near": "entrance" },
                { "place": "every", "part": "outlet", "spaces": "#room", "spacing": 2 },
                { "place": "over", "part": "light", "spaces": "#room" },
            ],
            "connect": [{ "from": "panel", "to": ["outlet", "light"], "max": 4 }]
        }),
    ));
    assert!(!parts(&lit, "outlet").is_empty());
    assert!(!of_kind(&lit, ElementKind::Run).is_empty());
    assert_eq!(spaces_of(&bare), spaces_of(&lit));
}

#[test]
fn a_scope_gets_its_own_parts_and_networks() {
    let flat_plan = json!({
        "needs": { "ports": { "facade": "required" } },
        "spaces": {
            "living": { "area": { "min": 20 }, "side": { "min": 3.5 } },
            "bedroom": { "area": { "min": 10, "max": 14 }, "side": { "min": 3 }, "tags": ["sleep"] },
        },
        "rules": [{ "rule": "door", "a": "#sleep", "b": "living" }, { "rule": "window", "a": ["living", "#sleep"] }]
    });
    let v = json!({
        "start": { "mode": "boundary", "points": [[0, 0], [16, 0], [16, 8], [0, 8]] },
        "assemblies": { "wall": { "on": ["facade", "party", "partition", "link"], "thickness": 0.15, "cavity": 0.09 } },
        "spaces": { "flat": { "count": 2, "width": 8, "depth": 8, "plan": flat_plan, "tags": ["dwelling"] } },
        "services": {
            "power": {
                "scope": "#dwelling",
                "parts": { "panel": {}, "outlet": {} },
                "place": [
                    { "place": "one", "part": "panel", "spaces": "living" },
                    { "place": "every", "part": "outlet", "spaces": "#sleep", "spacing": 3 },
                ],
                "connect": [{ "from": "panel", "to": "outlet", "max": 3 }]
            }
        }
    });
    let s = run(v);
    assert_eq!(s.report.unmet, 0, "{:?}", messages(&s));
    let panels = parts(&s, "panel");
    assert_eq!(panels.len(), 2, "one per flat");
    let spaces: Vec<&str> = panels.iter().map(|p| p.part.as_ref().unwrap().space.as_str()).collect();
    assert!(spaces.contains(&"flat_1.living") && spaces.contains(&"flat_2.living"), "{spaces:?}");
    // Each flat's outlets are on circuits of its own panel: their runs are the flat's.
    let runs = of_kind(&s, ElementKind::Run);
    for flat in ["flat_1", "flat_2"] {
        assert!(
            runs.iter().any(|r| r.id.contains(&format!(":{flat}#"))),
            "{:?}",
            runs.iter().map(|r| &r.id).collect::<Vec<_>>()
        );
        let panel = panels.iter().find(|p| p.part.as_ref().unwrap().space.starts_with(flat)).unwrap();
        // Home runs are arrows from each circuit toward its own panel.
        let home: Vec<_> = runs
            .iter()
            .filter(|r| r.id.contains(&format!(":{flat}#")) && r.run.as_ref().unwrap().home)
            .collect();
        assert!(!home.is_empty());
        let to = panel.part.as_ref().unwrap().at;
        for r in home {
            let (a, b) = (r.points[0], *r.points.last().unwrap());
            let (ux, uy) = ((b.x - a.x) / a.distance(b), (b.y - a.y) / a.distance(b));
            let (vx, vy) = ((to.x - a.x) / a.distance(to), (to.y - a.y) / a.distance(to));
            assert!(ux * vx + uy * vy > 0.999, "{} doesn't point at its panel", r.id);
        }
    }
    // Groups of at most three.
    let outlets = parts(&s, "outlet");
    let mut groups: BTreeMap<(String, String), usize> = BTreeMap::new();
    for o in &outlets {
        let p = o.part.as_ref().unwrap();
        *groups.entry((p.space.clone(), p.group.clone().expect("connected"))).or_default() += 1;
    }
    assert!(groups.values().all(|&n| n <= 3), "{groups:?}");
}

#[test]
fn a_routed_run_too_long_is_said() {
    let service = |limit: f64| {
        json!({
            "parts": { "stack": { "mount": "cavity", "width": 0.1, "depth": 0.1 }, "drain": { "mount": "floor" } },
            "place": [
                { "place": "one", "part": "stack", "spaces": "#wet", "wall": { "cavity": 0.12 } },
                { "place": "over", "part": "drain", "spaces": ["#wet", "living"] },
            ],
            "connect": [{ "from": "stack", "to": "drain", "draw": "routed", "limit": limit }]
        })
    };
    let s = run(with_service("plumbing", service(100.0)));
    assert_eq!(s.report.unmet, 0, "{:?}", messages(&s));
    let runs = of_kind(&s, ElementKind::Run);
    assert!(!runs.is_empty());
    assert!(runs.iter().all(|r| r.run.as_ref().unwrap().draw == "routed"));
    let s = run(with_service("plumbing", service(1.0)));
    assert!(
        messages(&s)
            .iter()
            .any(|(p, m)| p.as_deref() == Some("/services/plumbing/connect/0") && m.contains("at most 1")),
        "{:?}",
        messages(&s)
    );
}

#[test]
fn a_tag_that_names_nothing_is_only_mentioned() {
    let s = run(with_service(
        "power",
        json!({ "parts": { "charger": {} }, "place": [{ "place": "one", "part": "charger", "spaces": "#garage" }] }),
    ));
    assert_eq!(s.report.unmet, 0, "{:?}", messages(&s));
    assert!(parts(&s, "charger").is_empty());
    let d = s.diagnostics.iter().find(|d| d.message.contains("#garage")).expect("mentioned");
    assert_eq!(d.severity, crate::diagnostic::Severity::Info, "{}", d.message);
}

/// A flat whose walls are all too shallow to run in but for the one wall of the bath a rule makes
/// deep, with a stack in that wall and a toilet in the bath.
fn drain_in_the_bath(seed: u64, via: Value) -> Value {
    let mut v = flat(json!({}));
    v["seed"] = json!(seed);
    for name in ["exterior", "partition", "wet"] {
        v["assemblies"][name]["cavity"] = json!(0.05);
    }
    v["assemblies"]["deep"] = json!({ "thickness": 0.3, "cavity": 0.2 });
    v["spaces"]["bath"]["plan"] = json!({
        "spaces": { "toilet": { "kind": "fixture", "width": 0.4, "depth": 0.7, "against": "wet", "tags": ["drain"] } },
    });
    v["rules"]
        .as_array_mut()
        .unwrap()
        .push(json!({ "rule": "wall", "a": "bath", "b": "hall", "assembly": "deep" }));
    v["services"] = json!({ "plumbing": {
        "parts": { "stack": { "mount": "cavity", "width": 0.1, "depth": 0.1 } },
        "place": [{ "place": "one", "part": "stack", "spaces": "bath", "wall": { "cavity": 0.15 } }],
        "connect": [{ "from": "stack", "to": "#drain", "draw": "routed", "via": via }]
    } });
    v
}

/// Is the toilet up against the deep wall?
fn toilet_on_the_deep_wall(s: &Solution) -> bool {
    let toilet = of_kind(s, ElementKind::Space)
        .into_iter()
        .find(|e| e.space.as_ref().unwrap().name == "toilet")
        .unwrap();
    of_kind(s, ElementKind::Wall).iter().any(|w| {
        let line = w.wall.as_ref().unwrap().line;
        w.wall.as_ref().unwrap().assembly == "deep"
            && toilet
                .points
                .iter()
                .any(|p| crate::geometry::point_segment_distance(*p, line[0], line[1]) < 0.2)
    })
}

#[test]
fn a_fixture_on_a_wall_too_shallow_does_not_cross_the_room() {
    let mut tried = 0;
    for seed in 1..12 {
        let s = run(drain_in_the_bath(seed, json!({ "cavity": 0.15 })));
        if parts(&s, "stack").is_empty() || toilet_on_the_deep_wall(&s) {
            continue;
        }
        tried += 1;
        assert!(of_kind(&s, ElementKind::Run).is_empty(), "seed {seed}: {:?}", messages(&s));
        assert!(
            messages(&s).iter().any(|(_, m)| m.contains("can't run in")),
            "seed {seed}: {:?}",
            messages(&s)
        );
    }
    assert!(tried > 0, "no seed put the toilet on a shallow wall");
}

#[test]
fn a_ceiling_it_may_cross_takes_a_fixture_off_a_shallow_wall() {
    let mut tried = 0;
    for seed in 1..12 {
        let s = run(drain_in_the_bath(seed, json!({ "cavity": 0.15, "ceilings": "bath" })));
        if parts(&s, "stack").is_empty() || toilet_on_the_deep_wall(&s) {
            continue;
        }
        tried += 1;
        assert!(!of_kind(&s, ElementKind::Run).is_empty(), "seed {seed}: {:?}", messages(&s));
    }
    assert!(tried > 0, "no seed put the toilet on a shallow wall");
}

#[test]
fn the_source_of_a_network_must_be_on_a_wall_it_can_run_in() {
    let mut v = with_service(
        "power",
        json!({
            "parts": { "panel": {}, "outlet": {} },
            "place": [
                { "place": "one", "part": "panel", "spaces": "hall" },
                { "place": "one", "part": "outlet", "spaces": "living" },
            ],
            "connect": [{ "from": "panel", "to": "outlet", "via": { "cavity": 0.12 } }]
        }),
    );
    for name in ["exterior", "partition", "wet"] {
        v["assemblies"][name]["cavity"] = json!(0.09);
    }
    let s = run(v);
    let said = |what: &str| messages(&s).iter().any(|(_, m)| m.contains(&format!("a {what} is on a wall")));
    assert!(said("panel"), "{:?}", messages(&s));
    assert!(said("outlet"), "{:?}", messages(&s));
}

#[test]
fn a_part_says_which_network_its_group_is_in() {
    let s = run(with_service(
        "power",
        json!({
            "parts": { "panel": {}, "outlet": {}, "counter": {} },
            "place": [
                { "place": "one", "part": "panel", "spaces": "hall" },
                { "place": "one", "part": "outlet", "spaces": "living" },
                { "place": "one", "part": "counter", "spaces": "living" },
            ],
            "connect": [
                { "label": "Circuits", "from": "panel", "to": "outlet", "max": 1 },
                { "label": "Counter circuits", "from": "panel", "to": "counter", "max": 1 },
            ]
        }),
    ));
    let info = |part: &str| parts(&s, part)[0].part.as_ref().unwrap();
    assert_eq!(info("outlet").group.as_deref(), Some("1"));
    assert_eq!(info("counter").group.as_deref(), Some("1"));
    assert_eq!(info("outlet").network.as_deref(), Some("Circuits"));
    assert_eq!(info("counter").network.as_deref(), Some("Counter circuits"));
}
