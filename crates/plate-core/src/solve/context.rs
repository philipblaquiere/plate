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

use super::Issue;
use crate::plan::{escape_pointer, tag_of, Plan, RuleItem, Selector, Space};

/// The space a plan is placed in, as its parent sees it.
#[derive(Clone, Copy)]
pub struct Within<'a> {
    pub name: &'a str,
    pub key: &'a str,
    pub space: &'a Space,
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
            let here =
                s.in_scope || within.zip(s.scope.as_ref()).is_some_and(|(w, scope)| w.selected_by(scope));
            if !p.services.contains_key(name) {
                let mut s = s.clone();
                s.in_scope = here;
                p.services.insert(name.clone(), s);
            }
            let (Some(w), true) = (within, here) else { continue };
            let decl = s.source.clone().unwrap_or_default();
            for (sel, fragment) in &s.claims.inside {
                if !w.selected_by(&Selector::One(sel.clone())) {
                    continue;
                }
                let at = format!("{decl}/claims/inside/{}", escape_pointer(sel));
                merge(&mut p, fragment, &at, &mut issues);
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
fn merge(p: &mut Plan, fragment: &Plan, at: &str, issues: &mut Vec<Issue>) {
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
    let needs = &fragment.needs;
    p.needs.width = p.needs.width.or(needs.width);
    p.needs.depth = p.needs.depth.or(needs.depth);
    p.needs.area = p.needs.area.or(needs.area);
    for (role, need) in &needs.ports {
        p.needs.ports.entry(role.clone()).or_insert(*need);
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
        let within = Within { name: "flat", key: "flat", space: &flat };
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
            Some(Within { name: "flat", key: "flat", space: &flat }),
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
            Some(Within { name: "utility", key: "utility", space: &utility }),
            "/x",
        );
        assert!(up.spaces.contains_key("heater"));
        assert_eq!(
            origin(&up, "/spaces/heater/width"),
            "@/services/plumbing/claims/inside/#utility/spaces/heater/width"
        );

        // The shop isn't a dwelling: nothing.
        let shop = root.spaces["shop"].clone();
        let (sp, _) =
            prepare(&flat_plan, Some(&root), Some(Within { name: "shop", key: "shop", space: &shop }), "/s");
        assert!(sp.rules.is_empty());
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
