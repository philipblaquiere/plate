//! Services: power, lighting, heat, plumbing, air… What runs through spaces and assemblies.
//!
//! These types mirror `Service` in `schema/plate.schema.json`. A service never draws a wall
//! itself. It has three parts, each used at a different time:
//!
//! - `claims` shape the layout. They're merged into the plans in scope before solving (rules
//!   added to every plan; plan fragments merged into the spaces they name), so they are part of
//!   a plan's fingerprint and the solver makes room for them.
//! - `place` puts `parts` (an outlet, a light, a register) on the walls, floors and ceilings of
//!   the solved spaces ([`crate::solve::fit`]).
//! - `connect` groups parts into networks (circuits, duct runs) and draws them
//!   ([`crate::solve::connect`]).
//!
//! `place` and `connect` never move anything, so they're left out of the fingerprint like
//! `symbol`: changing an outlet's spacing redraws the outlets, never the rooms.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::assembly::Requirement;
use crate::plan::{Count, LengthSer, Plan, RuleItem, Selector};
use crate::units::Units;

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Service {
    #[serde(default, skip_serializing)]
    pub label: Option<String>,
    /// The spaces it lives in, one network each (`"#dwelling"`: a panel per flat). Without it,
    /// the whole plan it's declared in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<Selector>,
    #[serde(default, skip_serializing)]
    pub parts: BTreeMap<String, Part>,
    #[serde(default, skip_serializing_if = "Claims::is_empty")]
    pub claims: Claims,
    #[serde(default, skip_serializing)]
    pub place: Vec<Placement>,
    #[serde(default, skip_serializing)]
    pub connect: Vec<Network>,
    /// What its networks are made of (cables, pipes, ducts), by name: `"14/2"`, `"10/3"`. A
    /// group gets the thinnest one that meets everything asked of it.
    #[serde(default, skip_serializing)]
    pub media: BTreeMap<String, Medium>,
    /// Requirements on how some terminals are served: their own circuit, a breaker, a cable.
    #[serde(default, skip_serializing)]
    pub feeds: Vec<Feed>,
    /// Values the service is written in terms of (a code's numbers), to override by merging:
    /// `(import "power.ncl") & { params = import "nec.ncl" }`. The solver doesn't read them.
    #[serde(default, skip_serializing)]
    pub params: Option<serde_json::Value>,
    /// Where it's written in the configuration (an absolute JSON pointer), set by the host.
    #[serde(skip)]
    pub source: Option<String>,
    /// Whether the plan it's been merged into is inside its scope, set by the host.
    #[serde(skip)]
    pub in_scope: bool,
}

/// What the layout must make room for.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Claims {
    /// Added to every plan in scope. Name spaces by tag (`"#wet"`): a tag that matches nothing
    /// in a plan is simply not there.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rules: Vec<RuleItem>,
    /// Merged into the plan of each space the key selects (`"#utility"`): spaces, rules, needs.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub inside: BTreeMap<String, Plan>,
}

impl Claims {
    pub fn is_empty(&self) -> bool {
        self.rules.is_empty() && self.inside.is_empty()
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Part {
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub mount: Mount,
    /// Along the wall it's on (or across, on a floor or ceiling).
    #[serde(default)]
    pub width: Option<LengthSer>,
    /// Out from the wall.
    #[serde(default)]
    pub depth: Option<LengthSer>,
    /// Height of its centre above the floor (wall parts).
    #[serde(default)]
    pub above: Option<LengthSer>,
    #[serde(default)]
    pub symbol: Option<String>,
    /// What it asks of the network it's on (a heater's watts). A part placed `on` a fixture
    /// without one takes the fixture's.
    #[serde(default)]
    pub load: Option<Load>,
}

/// What a terminal asks of a service: for power, a current at a voltage.
#[derive(Debug, Clone, Copy, Default, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Load {
    /// The circuit it needs, from its spec sheet: `"30 A"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current: Option<Amps>,
    /// Or its power, in watts: the current is `watts / voltage`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub watts: Option<f64>,
    /// In volts. Defaults to 120.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub voltage: Option<f64>,
    /// Conductors its cable needs, not counting the ground: 3 for a range or a dryer (two hots
    /// and a neutral). Defaults to 2.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conductors: Option<u32>,
}

impl Load {
    pub fn volts(&self) -> f64 {
        self.voltage.unwrap_or(120.0)
    }

    /// The current it draws, in amps.
    pub fn amps(&self) -> f64 {
        match (self.current, self.watts) {
            (Some(a), _) => a.0,
            (None, Some(w)) => w / self.volts(),
            (None, None) => 0.0,
        }
    }
}

/// A current in amps: a number, or a string with its unit (`"30 A"`).
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Default, Serialize)]
pub struct Amps(pub f64);

impl<'de> Deserialize<'de> for Amps {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            N(f64),
            S(String),
        }
        match Raw::deserialize(d)? {
            Raw::N(n) => Ok(Amps(n)),
            Raw::S(s) => {
                let t = s.trim();
                let t = t.strip_suffix('A').unwrap_or(t).trim();
                t.parse::<f64>()
                    .map(Amps)
                    .map_err(|_| serde::de::Error::custom(format!("not a current: {s:?}")))
            }
        }
    }
}

/// One kind of thing a network is made of: a cable, a pipe, a duct.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Medium {
    #[serde(default)]
    pub label: Option<String>,
    /// The most it carries: for a cable, the largest breaker it may be on.
    pub rating: Amps,
    /// Conductors, not counting the ground (`/2`, `/3`).
    #[serde(default = "two")]
    pub conductors: u32,
    /// Wire size (AWG), for the record.
    #[serde(default)]
    pub gauge: Option<f64>,
    /// Its jacket's colour, as it's drawn.
    #[serde(default)]
    pub colour: Option<Colour>,
    /// CAD layer its runs are exported on.
    #[serde(default)]
    pub layer: Option<String>,
}

fn two() -> u32 {
    2
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Colour {
    White,
    Black,
    Grey,
    Red,
    Yellow,
    Orange,
    Blue,
}

impl Colour {
    pub fn name(self) -> &'static str {
        match self {
            Colour::White => "white",
            Colour::Black => "black",
            Colour::Grey => "grey",
            Colour::Red => "red",
            Colour::Yellow => "yellow",
            Colour::Orange => "orange",
            Colour::Blue => "blue",
        }
    }
}

/// A requirement on how the terminals it selects are served.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Feed {
    /// Parts (`"dedicated"`, `"heat.baseboard"`) or the fixtures they're on (`"#cooking"`).
    pub feed: Selector,
    /// Only on this network (its label).
    #[serde(default)]
    pub network: Option<String>,
    /// Their own circuit, one each.
    #[serde(default)]
    pub dedicated: bool,
    /// The breaker their circuit is on.
    #[serde(default)]
    pub rating: Option<Amps>,
    /// The medium their circuit is made of, by name.
    #[serde(default)]
    pub medium: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Mount {
    #[default]
    Wall,
    Ceiling,
    Floor,
    /// Inside a wall's cavity: a stack, a riser. Fixtures can stand in front of it; the wall's
    /// cavity must be as deep as it.
    Cavity,
}

impl Mount {
    pub fn name(self) -> &'static str {
        match self {
            Mount::Wall => "wall",
            Mount::Ceiling => "ceiling",
            Mount::Floor => "floor",
            Mount::Cavity => "cavity",
        }
    }
}

impl Part {
    /// Width and depth in `units`: as given, else the symbol's, else 150 mm.
    pub fn size(&self, units: Units) -> (f64, f64) {
        let symbol = self.symbol.as_deref().and_then(|s| crate::symbols::catalog().get(s));
        let mm = units.to_mm();
        let w = self.width.map(|v| v.0.to(units)).or(symbol.map(|s| s.size[0] / mm)).unwrap_or(150.0 / mm);
        let d = self.depth.map(|v| v.0.to(units)).or(symbol.map(|s| s.size[1] / mm)).unwrap_or(150.0 / mm);
        (w, d)
    }

    /// Height above the floor: as given, else 300 mm on a wall.
    pub fn above(&self, units: Units) -> f64 {
        self.above.map_or(300.0 / units.to_mm(), |v| v.0.to(units))
    }
}

/// Where a service's parts go.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "place", rename_all = "snake_case", deny_unknown_fields)]
pub enum Placement {
    /// Along each wall run of the spaces (their perimeter, cut at doorways), at most `spacing`
    /// apart and half that from each end. On fixtures: along their backs.
    Every {
        part: String,
        spaces: Selector,
        spacing: LengthSer,
        /// Runs shorter than this get none.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        min_run: Option<LengthSer>,
    },
    /// Beside each door (or under each window) of the spaces.
    Beside {
        part: String,
        opening: OpeningKind,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        spaces: Option<Selector>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        side: Option<Side>,
        /// Gap from the opening's edge.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        offset: Option<LengthSer>,
    },
    /// In the spaces (or over fixtures), on the ceiling or floor: one in the middle of the
    /// biggest part of each, or spread over all of it by `count`, `spacing` or `per`.
    Over {
        part: String,
        spaces: Selector,
        /// How many in each space: exactly, or bounds on what `spacing` or `per` gives.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        count: Option<Count>,
        /// Floor area each covers.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        per: Option<f64>,
        /// At most this far apart, and about half that from the walls.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        spacing: Option<LengthSer>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        layout: Option<Arrange>,
        /// Never closer than this to a wall, where there's room. Defaults to 300 mm.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        margin: Option<LengthSer>,
    },
    /// One at the back of each fixture.
    On { part: String, fixtures: Selector },
    /// The best single place in each space.
    One {
        part: String,
        spaces: Selector,
        /// As close as possible to these: port roles (`'entrance`), `'door`, `'window`, `"#tag"`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        near: Option<Selector>,
        /// Away from these (and never on a wall with one of these roles).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        apart: Option<Selector>,
        /// What the wall it goes on must be.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        wall: Option<Requirement>,
        /// What one part is for: each space, or each region.
        #[serde(default, skip_serializing_if = "Per::is_space")]
        per: Per,
        /// Per region: one part for each selected space, side by side at the one place.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        each: bool,
    },
}

/// What a `one` placement counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Per {
    #[default]
    Space,
    /// The spaces joined by open edges (no wall between them).
    Region,
}

impl Per {
    pub fn is_space(&self) -> bool {
        *self == Per::Space
    }
}

impl Placement {
    pub fn part(&self) -> &str {
        match self {
            Placement::Every { part, .. }
            | Placement::Beside { part, .. }
            | Placement::Over { part, .. }
            | Placement::On { part, .. }
            | Placement::One { part, .. } => part,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Placement::Every { .. } => "every",
            Placement::Beside { .. } => "beside",
            Placement::Over { .. } => "over",
            Placement::On { .. } => "on",
            Placement::One { .. } => "one",
        }
    }

    /// Anchored placements go first; the ones that can flow around them after.
    pub fn order(&self) -> u8 {
        match self {
            Placement::On { .. } => 0,
            Placement::Beside { .. } => 1,
            Placement::One { .. } => 2,
            Placement::Every { .. } => 3,
            Placement::Over { .. } => 4,
        }
    }
}

/// How `over` arranges parts in each part of a space.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Arrange {
    /// A grid; a part of the space too narrow for two rows gets a line.
    Grid,
    /// One row along each part.
    Line,
    /// One in the middle of each part.
    Centre,
}

impl From<Arrange> for crate::solve::spread::Arrange {
    fn from(a: Arrange) -> Self {
        match a {
            Arrange::Grid => Self::Grid,
            Arrange::Line => Self::Line,
            Arrange::Centre => Self::Centre,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OpeningKind {
    Door,
    Window,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    /// The side a door opens from (away from its hinges).
    Latch,
    Hinge,
    /// Under it, as wide as it (a window).
    Under,
}

/// A network: parts connected back to a source, in groups.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Network {
    #[serde(default)]
    pub label: Option<String>,
    /// A part (`"panel"`) or a tagged fixture (`"#air"`).
    pub from: Selector,
    /// Parts of this service, of another (`"heat.baseboard"`), or tagged fixtures (`"#drain"`).
    pub to: Selector,
    #[serde(default)]
    pub via: Via,
    /// Most terminals in one group (a circuit).
    #[serde(default)]
    pub max: Option<u32>,
    /// Longest run from the source.
    #[serde(default)]
    pub limit: Option<LengthSer>,
    #[serde(default)]
    pub draw: Draw,
    /// What it does: feed terminals from a source (a circuit), or control them from a switch.
    #[serde(default)]
    pub role: Role,
    /// Each group's breaker. With loads, a group is also cut so it carries at most 80 % of it.
    #[serde(default)]
    pub rating: Option<Amps>,
    /// One terminal per group: dedicated circuits.
    #[serde(default)]
    pub dedicated: bool,
    /// The medium every group is made of, by name. Leave out to pick the thinnest that does.
    #[serde(default)]
    pub medium: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// Groups of terminals, each joined back to the source: circuits.
    #[default]
    Feed,
    /// In each room, the sources (switches) joined to the terminals (lights) they control.
    Control,
}

/// Where a network can run.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Via {
    /// Walls with at least this cavity. Without it, any wall.
    #[serde(default)]
    pub cavity: Option<LengthSer>,
    /// Spaces whose ceilings it can cross.
    #[serde(default)]
    pub ceilings: Option<Selector>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Draw {
    /// The electrical convention: an arc from device to device, a home run to the source.
    Arcs,
    /// Along the walls and ceilings it runs in.
    #[default]
    Routed,
}

impl Service {
    /// Every field, those left out of the fingerprint too (to send it somewhere and back).
    pub fn full(&self) -> serde_json::Value {
        let mut v = serde_json::to_value(self).unwrap_or_default();
        if let Some(m) = v.as_object_mut() {
            if let Some(l) = &self.label {
                m.insert("label".into(), l.clone().into());
            }
            if let Some(p) = &self.params {
                m.insert("params".into(), p.clone());
            }
            for (k, f) in [
                ("parts", serde_json::to_value(&self.parts)),
                ("place", serde_json::to_value(&self.place)),
                ("connect", serde_json::to_value(&self.connect)),
                ("media", serde_json::to_value(&self.media)),
                ("feeds", serde_json::to_value(&self.feeds)),
            ] {
                if let Ok(f) = f {
                    m.insert(k.into(), f);
                }
            }
        }
        v
    }

    /// Checks the format can't express: placements naming parts it doesn't have. Returns
    /// (pointer, message).
    pub fn validate(&self, at: &str) -> Result<(), (String, String)> {
        for (i, p) in self.place.iter().enumerate() {
            if !self.parts.contains_key(p.part()) {
                let names: Vec<&str> = self.parts.keys().map(String::as_str).collect();
                return Err((
                    format!("{at}/place/{i}/part"),
                    format!(
                        "no part named \"{}\" ({})",
                        p.part(),
                        if names.is_empty() {
                            "this service has no parts".into()
                        } else {
                            format!("parts: {}", names.join(", "))
                        }
                    ),
                ));
            }
        }
        for (i, n) in self.connect.iter().enumerate() {
            for (field, sel) in [("from", &n.from), ("to", &n.to)] {
                for item in sel.items() {
                    let other = item.starts_with('#') || item.contains('.');
                    if !other && !self.parts.contains_key(item) {
                        return Err((
                            format!("{at}/connect/{i}/{field}"),
                            format!("no part named \"{item}\": name a part, a tag (\"#air\") or another service's part (\"heat.baseboard\")"),
                        ));
                    }
                }
            }
        }
        // Media named must exist, when the service has its own (else another service's are used).
        let known = |m: &str| self.media.is_empty() || self.media.contains_key(m);
        let unknown = |m: &str| {
            let names: Vec<&str> = self.media.keys().map(String::as_str).collect();
            format!("no medium named \"{m}\" (media: {})", names.join(", "))
        };
        for (i, n) in self.connect.iter().enumerate() {
            if let Some(m) = n.medium.as_deref().filter(|m| !known(m)) {
                return Err((format!("{at}/connect/{i}/medium"), unknown(m)));
            }
        }
        for (i, f) in self.feeds.iter().enumerate() {
            if let Some(m) = f.medium.as_deref().filter(|m| !known(m)) {
                return Err((format!("{at}/feeds/{i}/medium"), unknown(m)));
            }
        }
        // The spaces they name are checked once they're in a plan.
        let claimed = crate::plan::Plan { rules: self.claims.rules.clone(), ..Default::default() };
        for (pointer, rule) in claimed.rules() {
            rule.validate(&format!("{at}/claims{pointer}"))?;
        }
        for (sel, plan) in &self.claims.inside {
            plan.validate(&format!("{at}/claims/inside/{}", crate::plan::escape_pointer(sel)))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placements_are_tagged_records() {
        let s: Service = serde_json::from_value(serde_json::json!({
            "parts": { "outlet": { "symbol": "receptacle" }, "light": { "mount": "ceiling" } },
            "place": [
                { "place": "every", "part": "outlet", "spaces": "#room", "spacing": 3.6 },
                { "place": "over", "part": "light", "spaces": "#room" }
            ],
            "connect": [{ "from": "outlet", "to": ["outlet", "heat.baseboard"], "max": 12 }]
        }))
        .unwrap();
        assert!(s.validate("/services/power").is_ok());
        assert_eq!(s.place[0].order(), 3);
    }

    #[test]
    fn placements_name_known_parts() {
        let s: Service = serde_json::from_value(serde_json::json!({
            "parts": { "outlet": {} },
            "place": [{ "place": "over", "part": "lamp", "spaces": "#room" }]
        }))
        .unwrap();
        let (p, m) = s.validate("/services/power").unwrap_err();
        assert_eq!(p, "/services/power/place/0/part");
        assert!(m.contains("outlet"), "{m}");
    }

    #[test]
    fn claimed_rules_are_checked_too() {
        let s: Service = serde_json::from_value(serde_json::json!({
            "claims": { "rules": [
                { "rule": "wall", "a": "#room", "port": "wall", "fire": 1 },
                { "rule": "wall", "a": "#room", "fire": 1 }
            ] }
        }))
        .unwrap();
        let (p, m) = s.validate("/services/fire").unwrap_err();
        assert_eq!(p, "/services/fire/claims/rules/1");
        assert!(m.contains("`b`"), "{m}");
    }

    #[test]
    fn place_and_connect_stay_out_of_the_fingerprint() {
        let a: Service = serde_json::from_value(serde_json::json!({
            "parts": { "o": {} }, "place": [{ "place": "over", "part": "o", "spaces": "#room" }]
        }))
        .unwrap();
        let b: Service =
            serde_json::from_value(serde_json::json!({ "parts": { "o": { "mount": "floor" } } })).unwrap();
        assert_eq!(serde_json::to_string(&a).unwrap(), serde_json::to_string(&b).unwrap());
    }
}
