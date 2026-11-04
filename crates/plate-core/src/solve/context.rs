//! The context pass: a plan as it will be solved, with what it inherits from the plans around it.
//!
//! A pure `Plan → Plan`, run before a plan's fingerprint is taken, so what a plan inherits is
//! part of its identity (a typical) and engines need to know nothing about it:
//!
//! - its own assemblies and services learn where they're written (for diagnostics);
//! - the parent's assemblies and services it doesn't define itself are inherited (nearest
//!   definition wins, by name);
//! - `claims.inside` of the parent's services that select the space it's placed in are merged in;
//! - `claims.rules` of every service it's in the scope of are added;
//! - tags in selectors (`"#sleep"`) become the names of the space types that have them.
//!
//! What it adds is recorded in [`Plan::origins`], so a problem with it points at the claim that
//! asked for it, not at a rule the file doesn't have.

use super::{range, Issue};
use crate::plan::{
    escape_pointer, tag_of, AreaRange, Bounds1, LengthRange, LengthSer, Plan, PortNeed, RuleItem, Selector,
    Space,
};
use crate::units::{Length, Units};

/// The space a plan is placed in, as its parent sees it.
#[derive(Clone, Copy)]
pub struct Within<'a> {
    pub name: &'a str,
    pub key: &'a str,
    pub space: &'a Space,
    pub units: Units,
}

impl Within<'_> {
    fn selected_by(&self, sel: &Selector) -> bool {
        sel.matches_tagged(self.name, self.key, &self.space.tags, self.space.kind)
    }
}

/// `plan` as it will be solved. `abs` is its absolute pointer in the configuration (`""` at the
/// root). Problems (a claim adding a space the plan already has) come back as issues with
/// absolute pointers.
pub fn prepare(plan: &Plan, parent: Option<&Plan>, within: Option<Within>, abs: &str) -> (Plan, Vec<Issue>) {
    let mut p = plan.clone();
    let mut issues = Vec::new();
    for (name, a) in p.assemblies.iter_mut() {
        a.source.get_or_insert_with(|| format!("{abs}/assemblies/{}", escape_pointer(name)));
    }
    for (name, s) in p.services.iter_mut() {
        s.source.get_or_insert_with(|| format!("{abs}/services/{}", escape_pointer(name)));
        s.in_scope = s.scope.is_none();
    }

    if let Some(parent) = parent {
        for (name, a) in &parent.assemblies {
            p.assemblies.entry(name.clone()).or_insert_with(|| a.clone());
        }
        for (name, s) in &parent.services {
            // A plan that names one the same replaces it for itself, claims and all.
            if p.services.contains_key(name) {
                continue;
            }
            let here =
                s.in_scope || within.zip(s.scope.as_ref()).is_some_and(|(w, scope)| w.selected_by(scope));
            let mut inherited = s.clone();
            inherited.in_scope = here;
            p.services.insert(name.clone(), inherited);
            let (Some(w), true) = (within, here) else { continue };
            let decl = s.source.clone().unwrap_or_default();
            for (sel, fragment) in &s.claims.inside {
                if !w.selected_by(&Selector::One(sel.clone())) {
                    continue;
                }
                let at = format!("{decl}/claims/inside/{}", escape_pointer(sel));
                merge(&mut p, fragment, &at, w.units, &mut issues);
            }
        }
    }

    // Rules every plan in scope gets.
    let claims: Vec<(String, Vec<RuleItem>)> = p
        .services
        .values()
        .filter(|s| s.in_scope && !s.claims.rules.is_empty())
        .map(|s| (format!("{}/claims/rules", s.source.clone().unwrap_or_default()), s.claims.rules.clone()))
        .collect();
    for (at, rules) in claims {
        for (j, rule) in rules.into_iter().enumerate() {
            p.origins.push((format!("/rules/{}", p.rules.len()), format!("{at}/{j}")));
            p.rules.push(rule);
        }
    }

    resolve_tags(&mut p);
    (p, issues)
}

/// Merge a plan fragment (`claims.inside`) into `p`: its spaces, rules and needs.
fn merge(p: &mut Plan, fragment: &Plan, at: &str, units: Units, issues: &mut Vec<Issue>) {
    for (key, space) in &fragment.spaces {
        if p.spaces.contains_key(key) {
            issues.push(
                Issue::new(
                    format!("@{at}/spaces/{}", escape_pointer(key)),
                    format!("adds a space named \"{key}\", but the plan already has one"),
                )
                .with_hint("Rename the space the claim adds."),
            );
            continue;
        }
        p.origins.push((
            format!("/spaces/{}", escape_pointer(key)),
            format!("{at}/spaces/{}", escape_pointer(key)),
        ));
        p.spaces.insert(key.clone(), space.clone());
    }
    for (j, rule) in fragment.rules.iter().enumerate() {
        p.origins.push((format!("/rules/{}", p.rules.len()), format!("{at}/rules/{j}")));
        p.rules.push(rule.clone());
    }
    // What the plan needs and what the claim needs both hold. `both` gives what to write, if
    // that's a change.
    let needs = &fragment.needs;
    let origins = &mut p.origins;
    let mut both = |field: &str, mine: Option<Bounds1>, claim: Bounds1| -> Option<Bounds1> {
        let b = mine.map_or(claim, |mine| mine.intersect(claim));
        if b.min > b.max {
            let mine = mine.unwrap_or(Bounds1::ANY);
            issues.push(Issue::new(
                format!("@{at}/needs/{field}"),
                format!("asks for a {field} of {}, but the plan needs {}", range(claim), range(mine)),
            ));
            return None;
        }
        (Some(b) != mine).then(|| {
            origins.push((format!("/needs/{field}"), format!("{at}/needs/{field}")));
            b
        })
    };
    let length = |v: f64| LengthSer(Length::Bare(v));
    for (field, mine, claim) in
        [("width", &mut p.needs.width, needs.width), ("depth", &mut p.needs.depth, needs.depth)]
    {
        let Some(claim) = claim else { continue };
        if let Some(b) = both(field, mine.map(|r| r.to(units)), claim.to(units)) {
            *mine = Some(LengthRange::Range {
                min: (b.min > 0.0).then(|| length(b.min)),
                max: b.max.is_finite().then(|| length(b.max)),
            });
        }
    }
    if let Some(claim) = needs.area {
        if let Some(b) = both("area", p.needs.area.map(AreaRange::bounds), claim.bounds()) {
            p.needs.area = Some(AreaRange::Range {
                min: (b.min > 0.0).then_some(b.min),
                max: b.max.is_finite().then_some(b.max),
            });
        }
    }
    for (role, &claim) in &needs.ports {
        match p.needs.ports.get(role).copied() {
            None | Some(PortNeed::Optional) => {
                p.needs.ports.insert(role.clone(), claim);
            }
            Some(mine) if claim == PortNeed::Optional || claim == mine => {}
            Some(_) => issues.push(Issue::new(
                format!("@{at}/needs/ports/{role}"),
                format!("asks for the opposite of what the plan needs of a '{role} port"),
            )),
        }
    }
}

/// Replace tags in every rule's selectors with the space types that have them.
fn resolve_tags(p: &mut Plan) {
    let spaces: Vec<(String, Space)> = p.spaces.iter().map(|(k, s)| (k.clone(), s.clone())).collect();
    for rule in p.rules_mut() {
        for sel in rule.selectors_mut() {
            if !sel.items().iter().any(|i| tag_of(i).is_some()) {
                continue;
            }
            let mut items: Vec<String> = Vec::new();
            for item in sel.items() {
                match tag_of(item) {
                    Some(tag) => {
                        for (key, space) in &spaces {
                            if space.has_tag(tag) && !items.contains(key) {
                                items.push(key.clone());
                            }
                        }
                    }
                    None => {
                        if !items.iter().any(|i| i == item) {
                            items.push(item.to_owned());
                        }
                    }
                }
            }
            *sel = Selector::Many(items);
        }
    }
}

/// The absolute pointer of `ptr` (a pointer within `plan`) if the host added what it points at:
/// `@` and the pointer in the configuration. Otherwise `ptr` as it is.
pub fn origin(plan: &Plan, ptr: &str) -> String {
    for (rel, abs) in &plan.origins {
        if let Some(rest) = ptr.strip_prefix(rel.as_str()) {
            if rest.is_empty() || rest.starts_with('/') {
                return format!("@{abs}{rest}");
            }
        }
    }
    ptr.to_owned()
}

/// `ptr` seen from a plan `prefix` above: prefixed, unless it's already absolute.
pub fn under(prefix: &str, ptr: &str) -> String {
    if ptr.starts_with('@') {
        ptr.to_owned()
    } else if let Some(abs) = prefix.strip_prefix('@') {
        format!("@{abs}{ptr}")
    } else {
        format!("{prefix}{ptr}")
    }
}

/// A pointer as the configuration knows it (the `@` of absolute pointers dropped).
pub fn resolved(ptr: &str) -> String {
    ptr.trim_start_matches('@').to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::Rule;
    use serde_json::json;

    fn plan(v: serde_json::Value) -> Plan {
        serde_json::from_value(v).unwrap()
    }

    #[test]
    fn tags_become_space_names() {
        let p = plan(json!({
            "spaces": {
                "bedroom": { "tags": ["sleep"] },
                "nursery": { "tags": ["sleep.child"] },
                "hall": { "kind": "circulation" }
            },
            "rules": [
                { "rule": "door", "a": "#sleep", "b": "#circulation" },
                { "rule": "touch", "a": ["#nothing", "hall"], "b": "bedroom" }
            ]
        }));
        let (p, issues) = prepare(&p, None, None, "");
        assert!(issues.is_empty());
        let rules = p.rules();
        let Rule::Door { a, b: Some(b), .. } = rules[0].1 else { panic!() };
        assert_eq!(a.items(), vec!["bedroom", "nursery"]);
        assert_eq!(b.items(), vec!["hall"]);
        let Rule::Touch { a, .. } = rules[1].1 else { panic!() };
        assert_eq!(a.items(), vec!["hall"]);
    }

    #[test]
    fn children_inherit_unless_they_say_otherwise() {
        let parent = plan(json!({
            "assemblies": { "partition": { "on": "partition", "thickness": 0.1 }, "exterior": { "thickness": 0.3 } },
            "spaces": { "flat": {} }
        }));
        let (parent, _) = prepare(&parent, None, None, "");
        let child = plan(json!({ "assemblies": { "partition": { "on": "partition", "thickness": 0.075 } } }));
        let flat = parent.spaces["flat"].clone();
        let within = Within { name: "flat", key: "flat", space: &flat, units: Units::M };
        let (child, _) = prepare(&child, Some(&parent), Some(within), "/spaces/flat/plan");
        assert_eq!(child.assemblies.len(), 2);
        assert_eq!(child.assemblies["exterior"].source.as_deref(), Some("/assemblies/exterior"));
        assert_eq!(
            child.assemblies["partition"].source.as_deref(),
            Some("/spaces/flat/plan/assemblies/partition")
        );
    }

    #[test]
    fn claims_reach_the_plans_in_scope() {
        let root = plan(json!({
            "services": {
                "plumbing": {
                    "scope": "#dwelling",
                    "claims": {
                        "rules": [{ "rule": "touch", "a": "#wet", "port": "wet" }],
                        "inside": { "#utility": { "spaces": { "heater": { "kind": "fixture", "width": 0.6, "depth": 0.6 } } } }
                    }
                }
            },
            "spaces": { "flat": { "tags": ["dwelling"] }, "shop": {} }
        }));
        let (root, _) = prepare(&root, None, None, "");
        // Not in scope at the root: no claim rules there.
        assert!(root.rules.is_empty());

        let flat_plan =
            plan(json!({ "spaces": { "bath": { "tags": ["wet"] }, "utility": { "tags": ["utility"] } } }));
        let flat = root.spaces["flat"].clone();
        let (fp, _) = prepare(
            &flat_plan,
            Some(&root),
            Some(Within { name: "flat", key: "flat", space: &flat, units: Units::M }),
            "/spaces/flat/plan",
        );
        assert_eq!(fp.rules.len(), 1);
        let Rule::Touch { a, .. } = fp.rules()[0].1 else { panic!() };
        assert_eq!(a.items(), vec!["bath"]);
        assert_eq!(origin(&fp, "/rules/0/a"), "@/services/plumbing/claims/rules/0/a");

        // Inside the flat, the utility room gets the heater.
        let utility = fp.spaces["utility"].clone();
        let (up, _) = prepare(
            &Plan::default(),
            Some(&fp),
            Some(Within { name: "utility", key: "utility", space: &utility, units: Units::M }),
            "/x",
        );
        assert!(up.spaces.contains_key("heater"));
        assert_eq!(
            origin(&up, "/spaces/heater/width"),
            "@/services/plumbing/claims/inside/#utility/spaces/heater/width"
        );

        // The shop isn't a dwelling: nothing.
        let shop = root.spaces["shop"].clone();
        let within = Within { name: "shop", key: "shop", space: &shop, units: Units::M };
        let (sp, _) = prepare(&flat_plan, Some(&root), Some(within), "/s");
        assert!(sp.rules.is_empty());
    }

    /// `child` prepared inside a space `flat` of a plate with a service that claims `inside`.
    fn claimed(inside: serde_json::Value, child: serde_json::Value) -> (Plan, Vec<Issue>) {
        let root = plan(json!({
            "services": { "s": { "claims": { "inside": { "flat": inside } } } },
            "spaces": { "flat": {} }
        }));
        let (root, _) = prepare(&root, None, None, "");
        let flat = root.spaces["flat"].clone();
        let within = Within { name: "flat", key: "flat", space: &flat, units: Units::M };
        prepare(&plan(child), Some(&root), Some(within), "/spaces/flat/plan")
    }

    #[test]
    fn a_service_replaced_by_name_claims_nothing() {
        let inside = json!({ "spaces": { "heater": { "kind": "fixture", "width": 0.6, "depth": 0.6 } } });
        let (p, _) = claimed(inside.clone(), json!({}));
        assert!(p.spaces.contains_key("heater"));
        let (p, _) = claimed(inside, json!({ "services": { "s": {} } }));
        assert!(!p.spaces.contains_key("heater"));
    }

    #[test]
    fn claimed_needs_narrow_what_the_plan_needs() {
        let (p, issues) = claimed(
            json!({ "needs": { "width": { "min": 2 }, "depth": { "max": 5 }, "area": { "min": 9 } } }),
            json!({ "needs": { "width": { "min": 1, "max": 4 }, "depth": { "min": 3 } } }),
        );
        assert!(issues.is_empty());
        assert_eq!(p.needs.width.unwrap().to(Units::M), Bounds1 { min: 2.0, max: 4.0 });
        assert_eq!(p.needs.depth.unwrap().to(Units::M), Bounds1 { min: 3.0, max: 5.0 });
        assert_eq!(p.needs.area.unwrap().bounds().min, 9.0);
        assert_eq!(origin(&p, "/needs/width"), "@/services/s/claims/inside/flat/needs/width");

        // A claim that asks for no more than the plan leaves it as written.
        let (p, _) =
            claimed(json!({ "needs": { "width": { "min": 1 } } }), json!({ "needs": { "width": 3 } }));
        assert!(matches!(p.needs.width, Some(LengthRange::Exactly(_))));
        assert_eq!(origin(&p, "/needs/width"), "/needs/width");

        // Nothing is both: said, at the claim, and the plan keeps what it wrote.
        let (p, issues) = claimed(
            json!({ "needs": { "width": { "min": 5 } } }),
            json!({ "needs": { "width": { "max": 4 } } }),
        );
        assert_eq!(issues[0].pointer, "@/services/s/claims/inside/flat/needs/width");
        assert_eq!(p.needs.width.unwrap().to(Units::M).max, 4.0);
    }

    #[test]
    fn claimed_ports_are_required_or_forbidden_over_optional() {
        let (p, issues) = claimed(
            json!({ "needs": { "ports": { "a": "required", "b": "forbidden", "c": "optional" } } }),
            json!({ "needs": { "ports": { "a": "optional", "b": "required", "c": "required" } } }),
        );
        assert_eq!(p.needs.ports["a"], PortNeed::Required);
        assert_eq!(p.needs.ports["b"], PortNeed::Required, "the plan keeps what it wrote");
        assert_eq!(p.needs.ports["c"], PortNeed::Required);
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].pointer, "@/services/s/claims/inside/flat/needs/ports/b");
    }

    #[test]
    fn a_rule_with_nothing_selected_is_left_out_and_the_others_keep_their_pointers() {
        let p = plan(json!({
            "spaces": { "hall": {}, "room": {} },
            "rules": [
                { "rule": "door", "a": "hall", "b": "#missing" },
                { "rule": "door", "a": "#missing", "port": "entrance" },
                { "rule": "door", "a": "hall", "b": "room" }
            ]
        }));
        let (p, _) = prepare(&p, None, None, "");
        assert_eq!(p.rules.len(), 3);
        let rules = p.rules();
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].0, "/rules/2");
    }

    #[test]
    fn pointers_under_a_prefix() {
        assert_eq!(under("/spaces/a/plan", "/rules/0"), "/spaces/a/plan/rules/0");
        assert_eq!(under("/spaces/a/plan", "@/services/x"), "@/services/x");
        assert_eq!(
            under("@/services/x/claims/inside/k/spaces/s/plan", "/rules/1"),
            "@/services/x/claims/inside/k/spaces/s/plan/rules/1"
        );
        assert_eq!(resolved("@/services/x"), "/services/x");
    }
}
