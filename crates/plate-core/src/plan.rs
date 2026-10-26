//! Plans: the spaces to place, the rules between them and the goals to maximise.
//!
//! These types mirror `Plan`, `Space`, `Rule`, … in `schema/plate.schema.json`. A plan can sit
//! at the root of a file and inside any space (`plan = …`), which is how solving recurses: once a
//! space is placed, its plan is solved inside it.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::config::EdgeRoles;
use crate::units::{Length, System, Units};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Engine {
    Grid,
    SpineStack,
}

impl Engine {
    pub fn name(self) -> &'static str {
        match self {
            Engine::Grid => "grid",
            Engine::SpineStack => "spine_stack",
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub engine: Option<Engine>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grid: Option<LengthSer>,
    #[serde(default)]
    pub needs: Needs,
    #[serde(default)]
    pub spaces: BTreeMap<String, Space>,
    #[serde(default)]
    pub rules: Vec<RuleItem>,
    #[serde(default)]
    pub goals: Goals,
    /// How to show this plan on its own (opened as a file). Ignored when the plan is used inside
    /// a space, and left out of the fingerprint so changing it never changes a solution.
    #[serde(default, skip_serializing)]
    pub preview: Option<Previews>,
}

/// `preview`: one slot to show the plan in, or a list of named ones.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum Previews {
    One(PreviewCase),
    Many(Vec<PreviewCase>),
}

impl Previews {
    pub fn cases(&self) -> Vec<&PreviewCase> {
        match self {
            Previews::One(c) => vec![c],
            Previews::Many(v) => v.iter().collect(),
        }
    }
}

/// A slot to preview a plan in: a rectangle with its entrance along the bottom.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreviewCase {
    #[serde(default)]
    pub name: Option<String>,
    /// Along the bottom edge. Picked from `needs` when left out.
    #[serde(default)]
    pub width: Option<LengthSer>,
    /// Away from the bottom edge. Picked from `needs` when left out.
    #[serde(default)]
    pub depth: Option<LengthSer>,
    #[serde(default)]
    pub ports: SidePorts,
    #[serde(default)]
    pub system: Option<System>,
    #[serde(default)]
    pub units: Option<Units>,
    #[serde(default)]
    pub seed: Option<u64>,
}

/// Port roles of the four sides of a preview slot.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SidePorts {
    #[serde(default)]
    pub bottom: Option<EdgeRoles>,
    #[serde(default)]
    pub right: Option<EdgeRoles>,
    #[serde(default)]
    pub top: Option<EdgeRoles>,
    #[serde(default)]
    pub left: Option<EdgeRoles>,
}

/// The sides of a preview slot, counter-clockwise from the bottom (the order of its corners).
pub const SIDES: [&str; 4] = ["bottom", "right", "top", "left"];

impl SidePorts {
    /// Roles of each side in [`SIDES`] order; sides not given are what a typical bay on a
    /// corridor gets: entrance below, facade above, party walls either side (one of them wet).
    pub fn roles(&self) -> [Vec<String>; 4] {
        let given = [&self.bottom, &self.right, &self.top, &self.left];
        let mut out: [Vec<String>; 4] = Default::default();
        for (i, side) in given.iter().enumerate() {
            out[i] = match side {
                Some(r) => r.roles(),
                None => DEFAULT_SIDES[i].iter().map(|s| s.to_string()).collect(),
            };
        }
        out
    }
}

pub const DEFAULT_SIDES: [&[&str]; 4] = [&["entrance"], &["party"], &["facade"], &["party", "wet"]];

/// A [`Length`] that can be serialised back (for hashing plans).
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(transparent)]
pub struct LengthSer(pub Length);

impl Serialize for LengthSer {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self.0 {
            Length::Bare(v) => s.serialize_f64(v),
            Length::Mm(mm) => s.serialize_str(&format!("{mm} mm")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SpaceKind {
    #[default]
    Room,
    /// An area without walls of its own, in open floor (desks, a dining end).
    Zone,
    Circulation,
    Void,
    Vertical,
    /// Something that takes up floor and holds nothing else (a bed, a counter, a pool): the last
    /// level. It has a front, and may keep clear floor around it.
    Fixture,
}

impl SpaceKind {
    pub fn name(self) -> &'static str {
        match self {
            SpaceKind::Room => "room",
            SpaceKind::Zone => "zone",
            SpaceKind::Circulation => "circulation",
            SpaceKind::Void => "void",
            SpaceKind::Vertical => "vertical",
            SpaceKind::Fixture => "fixture",
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Space {
    #[serde(default)]
    pub kind: SpaceKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default)]
    pub count: Count,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub area: Option<AreaRange>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub side: Option<LengthRange>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<LengthRange>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub depth: Option<LengthRange>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aspect: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan: Option<Box<Plan>>,
    /// Fixtures: clear floor kept around it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clearance: Option<Clearance>,
    /// Fixtures: the port role (or `wall`, any edge) its back goes against.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub against: Option<String>,
    /// A drawing from the symbol catalog to show it with. Only how it's drawn: left out of the
    /// fingerprint, so choosing one never changes a solution.
    #[serde(default, skip_serializing)]
    pub symbol: Option<String>,
    #[serde(default = "yes")]
    pub typical: bool,
    #[serde(default = "yes")]
    pub mirror: bool,
    /// Takes the floor the other spaces leave, whatever its shape (a hall, an open-plan floor, a
    /// garden). At most one per plan.
    #[serde(default, skip_serializing_if = "is_false")]
    pub fill: bool,
    /// Fixed places: the corner nearest the origin of each instance, in the plan's frame.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<At>,
    /// Roles this space's walls give the spaces next to it (`'wet` for a bathroom).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub walls: Option<WallRoles>,
    /// Fixtures: the instances stand side by side in one run, fronts in line.
    #[serde(default, skip_serializing_if = "is_false")]
    pub row: bool,
}

fn yes() -> bool {
    true
}

fn is_false(v: &bool) -> bool {
    !*v
}

/// `at`: one corner, or one per instance.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(untagged)]
pub enum At {
    One([LengthSer; 2]),
    Many(Vec<[LengthSer; 2]>),
}

impl At {
    pub fn points(&self, units: Units) -> Vec<crate::geometry::Point> {
        let p = |c: &[LengthSer; 2]| crate::geometry::Point::new(c[0].0.to(units), c[1].0.to(units));
        match self {
            At::One(c) => vec![p(c)],
            At::Many(v) => v.iter().map(p).collect(),
        }
    }

    pub fn len(&self) -> usize {
        match self {
            At::One(_) => 1,
            At::Many(v) => v.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// `walls`: one role or several.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(untagged)]
pub enum WallRoles {
    One(String),
    Many(Vec<String>),
}

impl WallRoles {
    pub fn roles(&self) -> Vec<String> {
        match self {
            WallRoles::One(r) => vec![r.clone()],
            WallRoles::Many(v) => v.clone(),
        }
    }
}

impl Space {
    pub fn is_fixture(&self) -> bool {
        self.kind == SpaceKind::Fixture
    }
}

/// `clearance`: one length all round, or one per side (seen from the front, facing it).
#[derive(Debug, Clone, Copy, PartialEq, Deserialize, Serialize)]
#[serde(untagged)]
pub enum Clearance {
    All(LengthSer),
    Sides {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        front: Option<LengthSer>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        back: Option<LengthSer>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        left: Option<LengthSer>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        right: Option<LengthSer>,
    },
}

impl Clearance {
    /// `[front, right, back, left]` in `units`: counter-clockwise from the front, the way a
    /// rectangle's sides follow each other.
    pub fn sides(self, units: Units) -> [f64; 4] {
        let l = |v: Option<LengthSer>| v.map_or(0.0, |v| v.0.to(units).max(0.0));
        match self {
            Clearance::All(v) => [l(Some(v)); 4],
            Clearance::Sides { front, back, left, right } => [l(front), l(right), l(back), l(left)],
        }
    }
}

/// The value of `against` that means any edge of the space.
pub const ANY_WALL: &str = "wall";

#[derive(Debug, Clone, Copy, PartialEq, Deserialize, Serialize)]
#[serde(untagged)]
pub enum Count {
    Exactly(u32),
    Range {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        min: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        max: Option<u32>,
    },
}

impl Default for Count {
    fn default() -> Self {
        Count::Exactly(1)
    }
}

impl Count {
    pub fn min(self) -> u32 {
        match self {
            Count::Exactly(n) => n,
            Count::Range { min, .. } => min.unwrap_or(0),
        }
    }

    /// `None` means "as many as fit".
    pub fn max(self) -> Option<u32> {
        match self {
            Count::Exactly(n) => Some(n),
            Count::Range { max, .. } => max,
        }
    }

    /// Instances are numbered (`room_1`, …) unless exactly one is asked for.
    pub fn numbered(self) -> bool {
        self != Count::Exactly(1)
    }
}

/// A closed interval; `max` may be infinite.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bounds1 {
    pub min: f64,
    pub max: f64,
}

impl Bounds1 {
    pub const ANY: Bounds1 = Bounds1 { min: 0.0, max: f64::INFINITY };

    pub fn intersect(self, other: Bounds1) -> Bounds1 {
        Bounds1 { min: self.min.max(other.min), max: self.max.min(other.max) }
    }

    /// How far `v` is outside the interval (0 inside).
    pub fn miss(self, v: f64) -> f64 {
        if v < self.min {
            self.min - v
        } else if v > self.max {
            v - self.max
        } else {
            0.0
        }
    }

    pub fn contains(self, v: f64, tolerance: f64) -> bool {
        self.miss(v) <= tolerance
    }

    pub fn is_any(self) -> bool {
        self.min <= 0.0 && self.max.is_infinite()
    }

    /// A value to aim for: the middle of a bounded range, or a bit over the minimum.
    pub fn target(self, fallback: f64) -> f64 {
        match (self.min > 0.0, self.max.is_finite()) {
            (true, true) => (self.min + self.max) / 2.0,
            (true, false) => self.min * 1.1,
            (false, true) => self.max * 0.8,
            (false, false) => fallback,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize, Serialize)]
#[serde(untagged)]
pub enum LengthRange {
    Exactly(LengthSer),
    Range {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        min: Option<LengthSer>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        max: Option<LengthSer>,
    },
}

impl LengthRange {
    pub fn to(self, units: Units) -> Bounds1 {
        match self {
            LengthRange::Exactly(v) => {
                let v = v.0.to(units);
                Bounds1 { min: v, max: v }
            }
            LengthRange::Range { min, max } => Bounds1 {
                min: min.map_or(0.0, |v| v.0.to(units)),
                max: max.map_or(f64::INFINITY, |v| v.0.to(units)),
            },
        }
    }
}

pub fn bounds(range: Option<LengthRange>, units: Units) -> Bounds1 {
    range.map_or(Bounds1::ANY, |r| r.to(units))
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize, Serialize)]
#[serde(untagged)]
pub enum AreaRange {
    Exactly(f64),
    Range {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        min: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        max: Option<f64>,
    },
}

impl AreaRange {
    pub fn bounds(self) -> Bounds1 {
        match self {
            AreaRange::Exactly(v) => Bounds1 { min: v, max: v },
            AreaRange::Range { min, max } => {
                Bounds1 { min: min.unwrap_or(0.0), max: max.unwrap_or(f64::INFINITY) }
            }
        }
    }
}

pub fn area_bounds(range: Option<AreaRange>) -> Bounds1 {
    range.map_or(Bounds1::ANY, AreaRange::bounds)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PortNeed {
    Required,
    Optional,
    Forbidden,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Needs {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<LengthRange>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub depth: Option<LengthRange>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub area: Option<AreaRange>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub ports: BTreeMap<String, PortNeed>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Goals {
    #[serde(default = "one")]
    pub utilization: f64,
    #[serde(default = "compactness")]
    pub compactness: f64,
}

fn one() -> f64 {
    1.0
}
fn compactness() -> f64 {
    0.3
}

impl Default for Goals {
    fn default() -> Self {
        Goals { utilization: 1.0, compactness: 0.3 }
    }
}

/// Space names: a name, a type, a pattern with `*`, or a list of these.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(untagged)]
pub enum Selector {
    One(String),
    Many(Vec<String>),
}

impl Selector {
    pub fn items(&self) -> Vec<&str> {
        match self {
            Selector::One(s) => vec![s.as_str()],
            Selector::Many(v) => v.iter().map(String::as_str).collect(),
        }
    }

    /// Does the selector pick the instance `name` of space type `type_key`?
    pub fn matches(&self, name: &str, type_key: &str) -> bool {
        self.items().iter().any(|p| pattern_matches(p, name) || pattern_matches(p, type_key))
    }
}

/// `*` matches any run of characters.
pub fn pattern_matches(pattern: &str, name: &str) -> bool {
    if !pattern.contains('*') {
        return pattern == name;
    }
    let parts: Vec<&str> = pattern.split('*').collect();
    let mut rest = name;
    for (i, part) in parts.iter().enumerate() {
        if i == 0 {
            match rest.strip_prefix(part) {
                Some(r) => rest = r,
                None => return false,
            }
        } else if i == parts.len() - 1 {
            return rest.ends_with(part);
        } else {
            match rest.find(part) {
                Some(at) => rest = &rest[at + part.len()..],
                None => return false,
            }
        }
    }
    rest.is_empty()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Sides {
    #[default]
    Both,
    Left,
    Right,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "rule", rename_all = "snake_case", deny_unknown_fields)]
pub enum Rule {
    Touch {
        a: Selector,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        b: Option<Selector>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        port: Option<String>,
        /// With `port`: every separate stretch of it, not just one.
        #[serde(default, skip_serializing_if = "is_false")]
        each: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        length: Option<LengthSer>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        weight: Option<f64>,
    },
    Apart {
        a: Selector,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        b: Option<Selector>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        port: Option<String>,
        /// Smallest gap to keep; none means only "shares no wall".
        #[serde(default, skip_serializing_if = "Option::is_none")]
        distance: Option<LengthSer>,
        /// With a port: keep off the whole wall the port is on, not just the port.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        wall: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        weight: Option<f64>,
    },
    Door {
        a: Selector,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        b: Option<Selector>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        port: Option<String>,
        /// With `port`: every separate stretch of it, not just one.
        #[serde(default, skip_serializing_if = "is_false")]
        each: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        width: Option<LengthSer>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        weight: Option<f64>,
    },
    Window {
        a: Selector,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        port: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        width: Option<LengthSer>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        weight: Option<f64>,
    },
    Near {
        a: Selector,
        b: Selector,
        distance: LengthSer,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        weight: Option<f64>,
    },
    /// Each `a` (a fixture) has a `b` in front of it.
    Faces {
        a: Selector,
        b: Selector,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        weight: Option<f64>,
    },
    Reach {
        a: Selector,
        b: Selector,
        via: Selector,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        weight: Option<f64>,
    },
    Along {
        a: Selector,
        b: Selector,
        #[serde(default)]
        sides: Sides,
    },
    Unique {
        a: Selector,
    },
    Instance {
        a: Selector,
        plan: Box<Plan>,
    },
}

impl Rule {
    pub fn name(&self) -> &'static str {
        match self {
            Rule::Touch { .. } => "touch",
            Rule::Apart { .. } => "apart",
            Rule::Door { .. } => "door",
            Rule::Window { .. } => "window",
            Rule::Near { .. } => "near",
            Rule::Faces { .. } => "faces",
            Rule::Reach { .. } => "reach",
            Rule::Along { .. } => "along",
            Rule::Unique { .. } => "unique",
            Rule::Instance { .. } => "instance",
        }
    }

    /// `None` for a required rule.
    pub fn weight(&self) -> Option<f64> {
        match self {
            Rule::Touch { weight, .. }
            | Rule::Apart { weight, .. }
            | Rule::Door { weight, .. }
            | Rule::Window { weight, .. }
            | Rule::Near { weight, .. }
            | Rule::Faces { weight, .. }
            | Rule::Reach { weight, .. } => *weight,
            _ => None,
        }
    }

    /// Every selector in the rule, with its field name.
    pub fn selectors(&self) -> Vec<(&'static str, &Selector)> {
        let mut out = Vec::new();
        match self {
            Rule::Touch { a, b, .. } | Rule::Door { a, b, .. } | Rule::Apart { a, b, .. } => {
                out.push(("a", a));
                if let Some(b) = b {
                    out.push(("b", b));
                }
            }
            Rule::Near { a, b, .. } | Rule::Faces { a, b, .. } | Rule::Along { a, b, .. } => {
                out.push(("a", a));
                out.push(("b", b));
            }
            Rule::Reach { a, b, via, .. } => {
                out.push(("a", a));
                out.push(("b", b));
                out.push(("via", via));
            }
            Rule::Window { a, .. } | Rule::Unique { a } | Rule::Instance { a, .. } => out.push(("a", a)),
        }
        out
    }
}

/// A rule, or a list of rules (helpers such as `plate.between` return several).
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum RuleItem {
    One(Rule),
    Many(Vec<Rule>),
}

impl Plan {
    /// Every rule with its JSON pointer relative to the plan (`/rules/3` or `/rules/3/1`).
    pub fn rules(&self) -> Vec<(String, &Rule)> {
        let mut out = Vec::new();
        for (i, item) in self.rules.iter().enumerate() {
            match item {
                RuleItem::One(rule) => out.push((format!("/rules/{i}"), rule)),
                RuleItem::Many(rules) => {
                    for (j, rule) in rules.iter().enumerate() {
                        out.push((format!("/rules/{i}/{j}"), rule));
                    }
                }
            }
        }
        out
    }

    /// A stable fingerprint of the plan, for recognising typicals.
    pub fn fingerprint(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let text = serde_json::to_string(self).unwrap_or_default();
        let mut h = std::collections::hash_map::DefaultHasher::new();
        text.hash(&mut h);
        h.finish()
    }

    /// Could a selector item name some space of this plan?
    fn knows(&self, item: &str) -> bool {
        self.spaces.iter().any(|(key, space)| {
            if pattern_matches(item, key) {
                return true;
            }
            // `room_7`: an instance of a numbered space.
            let numbered = item
                .strip_prefix(key.as_str())
                .and_then(|r| r.strip_prefix('_'))
                .and_then(|n| n.parse::<u32>().ok());
            match numbered {
                Some(n) if space.count.numbered() => n >= 1 && space.count.max().is_none_or(|m| n <= m),
                _ => item.contains('*') && pattern_matches(item, &format!("{key}_1")),
            }
        })
    }

    /// Checks the format can't express: rules naming spaces that don't exist, touch/door with
    /// neither or both of `b` and `port`, impossible counts. Returns (pointer, message).
    pub fn validate(&self, at: &str) -> Result<(), (String, String)> {
        for (key, space) in &self.spaces {
            let here = format!("{at}/spaces/{key}");
            if let (min, Some(max)) = (space.count.min(), space.count.max()) {
                if min > max {
                    return Err((
                        format!("{here}/count"),
                        format!("count min ({min}) is more than max ({max})"),
                    ));
                }
            }
            if space.is_fixture() {
                if space.plan.is_some() {
                    return Err((
                        format!("{here}/plan"),
                        "a fixture can't have a plan: nothing goes inside it (use kind = 'room to divide it)"
                            .to_owned(),
                    ));
                }
            } else {
                for (field, set) in
                    [("clearance", space.clearance.is_some()), ("against", space.against.is_some())]
                {
                    if set {
                        return Err((
                            format!("{here}/{field}"),
                            format!("`{field}` is for fixtures: add kind = 'fixture"),
                        ));
                    }
                }
            }
            if space.row && (!space.is_fixture() || !matches!(space.count, Count::Exactly(n) if n > 1)) {
                return Err((
                    format!("{here}/row"),
                    "`row` is for a fixture with an exact count of two or more: `count = 3, row = true`"
                        .into(),
                ));
            }
            if space.fill {
                if space.is_fixture() {
                    return Err((
                        format!("{here}/fill"),
                        "a fixture can't be a fill: it has its own size".into(),
                    ));
                }
                if space.count != Count::Exactly(1) {
                    return Err((
                        format!("{here}/count"),
                        "a fill space is the one space that takes what's left: its count is 1".into(),
                    ));
                }
                if space.at.is_some() {
                    return Err((
                        format!("{here}/at"),
                        "a fill space takes what's left: it can't be placed with `at`".into(),
                    ));
                }
                if let Some((other, _)) =
                    self.spaces.iter().find(|(k, s)| s.fill && k.as_str() < key.as_str())
                {
                    return Err((
                        format!("{here}/fill"),
                        format!("only one space per plan can be the fill (\"{other}\" already is)"),
                    ));
                }
            }
            if let Some(at) = &space.at {
                if at.is_empty() {
                    return Err((format!("{here}/at"), "`at` needs at least one corner".into()));
                }
                let n = at.len() as u32;
                let fits = match space.count {
                    Count::Exactly(c) => c == n,
                    Count::Range { min, max } => min.unwrap_or(0) <= n && max.is_none_or(|m| n <= m),
                };
                if !fits {
                    return Err((format!("{here}/at"), format!("`at` places {n}, but count asks for another number: give one corner per instance")));
                }
                let exact = |r: &Option<LengthRange>| matches!(r, Some(LengthRange::Exactly(_)));
                if !exact(&space.width) || !exact(&space.depth) {
                    return Err((
                        format!("{here}/at"),
                        "a space placed with `at` needs an exact width and depth".into(),
                    ));
                }
            }
            if let Some(plan) = &space.plan {
                plan.validate(&format!("{here}/plan"))?;
            }
        }
        for (pointer, rule) in self.rules() {
            let here = format!("{at}{pointer}");
            for (field, selector) in rule.selectors() {
                for (i, item) in selector.items().into_iter().enumerate() {
                    if !self.knows(item) {
                        let names: Vec<&str> = self.spaces.keys().map(String::as_str).collect();
                        let pointer = match selector {
                            Selector::One(_) => format!("{here}/{field}"),
                            Selector::Many(_) => format!("{here}/{field}/{i}"),
                        };
                        let known = if names.is_empty() {
                            "this plan has no spaces".to_owned()
                        } else {
                            format!("spaces here: {}", names.join(", "))
                        };
                        return Err((pointer, format!("no space named \"{item}\" ({known})")));
                    }
                }
            }
            match rule {
                Rule::Touch { b, port, .. } | Rule::Door { b, port, .. } | Rule::Apart { b, port, .. } => {
                    if b.is_some() == port.is_some() {
                        return Err((
                            here,
                            format!("a {} rule needs either `b` (a space) or `port`, not both", rule.name()),
                        ));
                    }
                    if let Rule::Apart { wall: true, port: None, .. } = rule {
                        return Err((
                            format!("{here}/wall"),
                            "`wall` is for a port: `plate.apart \"bed\" 'window & { wall = true }`".into(),
                        ));
                    }
                }
                Rule::Instance { plan, .. } => plan.validate(&format!("{here}/plan"))?,
                _ => {}
            }
        }
        Ok(())
    }
}

/// Does a port with `role` satisfy a need for `wanted`? Dotted roles refine: `facade.storefront`
/// is a `facade`.
pub fn role_matches(role: &str, wanted: &str) -> bool {
    role == wanted || role.strip_prefix(wanted).is_some_and(|r| r.starts_with('.'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patterns() {
        assert!(pattern_matches("bed*", "bedroom_2"));
        assert!(pattern_matches("*room", "bedroom"));
        assert!(pattern_matches("a*c*e", "abcde"));
        assert!(!pattern_matches("bed*", "bath"));
        assert!(pattern_matches("hall", "hall"));
    }

    #[test]
    fn roles_refine() {
        assert!(role_matches("facade.storefront", "facade"));
        assert!(!role_matches("facades", "facade"));
        assert!(role_matches("wet", "wet"));
    }

    #[test]
    fn validation_names_unknown_spaces() {
        let plan: Plan = serde_json::from_value(serde_json::json!({
            "spaces": { "room": { "count": { "min": 2 } }, "hall": {} },
            "rules": [
                { "rule": "door", "a": "room", "b": "hall" },
                { "rule": "unique", "a": "room_7" },
                [{ "rule": "touch", "a": "room*", "b": "hall" }, { "rule": "apart", "a": "room", "b": "kitchn" }],
            ]
        }))
        .unwrap();
        let (pointer, message) = plan.validate("").unwrap_err();
        assert_eq!(pointer, "/rules/2/1/b");
        assert!(message.contains("kitchn"), "{message}");
    }

    #[test]
    fn apart_takes_a_space_or_a_port() {
        let check = |rule: serde_json::Value| {
            let plan: Plan = serde_json::from_value(serde_json::json!({
                "spaces": { "bed": {}, "desk": {} },
                "rules": [rule]
            }))
            .unwrap();
            plan.validate("")
        };
        assert!(check(serde_json::json!({ "rule": "apart", "a": "bed", "b": "desk" })).is_ok());
        assert!(
            check(serde_json::json!({ "rule": "apart", "a": "bed", "port": "window", "wall": true })).is_ok()
        );
        let (_, m) = check(serde_json::json!({ "rule": "apart", "a": "bed" })).unwrap_err();
        assert!(m.contains("either `b`"), "{m}");
        let (p, m) =
            check(serde_json::json!({ "rule": "apart", "a": "bed", "b": "desk", "wall": true })).unwrap_err();
        assert_eq!(p, "/rules/0/wall");
        assert!(m.contains("for a port"), "{m}");
    }
}
