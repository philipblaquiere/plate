//! Assemblies: what an edge between two spaces is built as (a wall type).
//!
//! These types mirror `Assembly` in `schema/plate.schema.json`. A plan lists assemblies by name;
//! `on` says which port roles each is for. Every edge a level builds is given the thinnest
//! assembly that meets every requirement on it: those of the assemblies its roles match, and
//! those of `wall` rules (see [`crate::solve::build`]). An assembly takes floor: the spaces on
//! either side are solved in what its thickness leaves them.

use serde::{Deserialize, Serialize};

use crate::plan::{role_matches, LengthSer, WallRoles};
use crate::units::Units;

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Assembly {
    /// Display name. Only how it's shown: out of the fingerprint.
    #[serde(default, skip_serializing)]
    pub label: Option<String>,
    /// The port roles it's for (any of them). None: only used when a `wall` rule asks for it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on: Option<WallRoles>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thickness: Option<LengthSer>,
    /// Depth inside it that a service can run in (a stud bay).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cavity: Option<LengthSer>,
    /// The build-up, from one face to the other. Thickness and cavity follow from it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub layers: Vec<Layer>,
    /// Fire resistance, in hours.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fire: Option<f64>,
    /// Sound transmission class.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub acoustic: Option<f64>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub bearing: bool,
    /// Where the edge runs through it. On the plate's boundary a wall grows inward unless this
    /// is `centre`; between spaces it's centred.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<WallLine>,
    /// How it's hatched. Only how it's drawn.
    #[serde(default, skip_serializing)]
    pub pattern: Option<Pattern>,
    /// CAD layer on export. Only how it's drawn.
    #[serde(default, skip_serializing)]
    pub layer: Option<String>,
    /// Whether services' runs may pass through it. Defaults to `firestop` for a fire-rated wall,
    /// else `yes`. Only where runs go: out of the fingerprint.
    #[serde(default, skip_serializing)]
    pub cross: Option<Cross>,
    /// Where it's written in the configuration (an absolute JSON pointer), set by the host.
    #[serde(skip)]
    pub source: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Layer {
    pub material: String,
    pub thickness: LengthSer,
    /// Open space a service can run in.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub cavity: bool,
}

/// Whether runs may pass through a wall.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Cross {
    /// Freely: drilled studs and plates.
    Yes,
    /// Through a sealed penetration, marked on the drawing (a fire separation).
    Firestop,
    /// Never: nothing runs in it or through it.
    No,
}

impl Cross {
    pub fn name(self) -> &'static str {
        match self {
            Cross::Yes => "yes",
            Cross::Firestop => "firestop",
            Cross::No => "no",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WallLine {
    #[default]
    Centre,
    Inside,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Pattern {
    #[default]
    Solid,
    Hatch,
    Cross,
    Light,
}

impl Pattern {
    pub fn name(self) -> &'static str {
        match self {
            Pattern::Solid => "solid",
            Pattern::Hatch => "hatch",
            Pattern::Cross => "cross",
            Pattern::Light => "light",
        }
    }
}

impl Assembly {
    pub fn thickness(&self, units: Units) -> f64 {
        match self.thickness {
            Some(t) => t.0.to(units),
            None => self.layers.iter().map(|l| l.thickness.0.to(units)).sum(),
        }
    }

    pub fn cavity(&self, units: Units) -> f64 {
        match self.cavity {
            Some(c) => c.0.to(units),
            None => self.layers.iter().filter(|l| l.cavity).map(|l| l.thickness.0.to(units)).sum(),
        }
    }

    /// Every field, those left out of the fingerprint too (to send it somewhere and back).
    pub fn full(&self) -> serde_json::Value {
        let mut v = serde_json::to_value(self).unwrap_or_default();
        if let Some(m) = v.as_object_mut() {
            if let Some(l) = &self.label {
                m.insert("label".into(), l.clone().into());
            }
            if let Some(p) = self.pattern {
                m.insert("pattern".into(), p.name().into());
            }
            if let Some(l) = &self.layer {
                m.insert("layer".into(), l.clone().into());
            }
            if let Some(c) = self.cross {
                m.insert("cross".into(), c.name().into());
            }
        }
        v
    }

    /// Whether runs may pass through it: as given, else a fire-rated wall needs a firestop.
    pub fn crossing(&self) -> Cross {
        self.cross.unwrap_or(if self.fire.is_some_and(|f| f > 0.0) { Cross::Firestop } else { Cross::Yes })
    }

    /// Is it for an edge with one of these roles?
    pub fn matches(&self, roles: &[String]) -> bool {
        self.on
            .as_ref()
            .is_some_and(|on| on.roles().iter().any(|wanted| roles.iter().any(|r| role_matches(r, wanted))))
    }

    /// What it promises, as a requirement other walls must meet.
    pub fn requirement(&self, units: Units) -> Need {
        Need {
            thickness: self.thickness(units),
            cavity: self.cavity(units),
            fire: self.fire.unwrap_or(0.0),
            acoustic: self.acoustic.unwrap_or(0.0),
            bearing: self.bearing,
        }
    }

    pub fn meets(&self, need: &Need, units: Units) -> bool {
        let tol = units.tolerance();
        self.thickness(units) + tol >= need.thickness
            && self.cavity(units) + tol >= need.cavity
            && self.fire.unwrap_or(0.0) + 1e-9 >= need.fire
            && self.acoustic.unwrap_or(0.0) + 1e-9 >= need.acoustic
            && (self.bearing || !need.bearing)
    }

    /// Checks the format can't express. Returns (pointer, message).
    pub fn validate(&self, at: &str) -> Result<(), (String, String)> {
        if self.thickness.is_none() && self.layers.is_empty() {
            return Err((at.to_owned(), "an assembly needs a `thickness`, or `layers` to add up".into()));
        }
        // Plain numbers are in the plate's units, unknown here: compare only like with like.
        fn alike<'a>(mut lengths: impl Iterator<Item = &'a LengthSer>) -> bool {
            let first = lengths.next().map(|l| std::mem::discriminant(&l.0));
            lengths.all(|l| Some(std::mem::discriminant(&l.0)) == first)
        }
        let units = Units::M;
        let sizes = || self.thickness.iter().chain(self.layers.iter().map(|l| &l.thickness));
        if let (Some(t), false, true) = (self.thickness, self.layers.is_empty(), alike(sizes())) {
            let sum: f64 = self.layers.iter().map(|l| l.thickness.0.to(units)).sum();
            if (t.0.to(units) - sum).abs() > units.tolerance() * 10.0 {
                return Err((
                    format!("{at}/thickness"),
                    "`thickness` doesn't match its `layers`: leave one of them out".into(),
                ));
            }
        }
        if alike(sizes().chain(&self.cavity))
            && self.cavity(units) > self.thickness(units) + units.tolerance()
        {
            return Err((format!("{at}/cavity"), "the cavity is deeper than the wall is thick".into()));
        }
        Ok(())
    }
}

/// What a wall must at least be: from the assemblies its roles call for and from `wall` rules.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Need {
    pub thickness: f64,
    pub cavity: f64,
    pub fire: f64,
    pub acoustic: f64,
    pub bearing: bool,
}

impl Need {
    pub fn and(self, other: Need) -> Need {
        Need {
            thickness: self.thickness.max(other.thickness),
            cavity: self.cavity.max(other.cavity),
            fire: self.fire.max(other.fire),
            acoustic: self.acoustic.max(other.acoustic),
            bearing: self.bearing || other.bearing,
        }
    }

    /// In words, for messages: "a cavity of 0.14 and 1 h fire".
    pub fn describe(&self) -> String {
        let n = crate::solve::num;
        let mut parts = Vec::new();
        if self.thickness > 0.0 {
            parts.push(format!("{} thick", n(self.thickness)));
        }
        if self.cavity > 0.0 {
            parts.push(format!("a cavity of {}", n(self.cavity)));
        }
        if self.fire > 0.0 {
            parts.push(format!("{} h fire", n(self.fire)));
        }
        if self.acoustic > 0.0 {
            parts.push(format!("STC {}", n(self.acoustic)));
        }
        if self.bearing {
            parts.push("bearing".into());
        }
        if parts.is_empty() {
            "anything".into()
        } else {
            parts.join(", ")
        }
    }
}

/// `wall` (in a `one` placement): what the wall a part goes on must be. All fields are minimums.
#[derive(Debug, Clone, Copy, Default, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Requirement {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thickness: Option<LengthSer>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cavity: Option<LengthSer>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fire: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub acoustic: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bearing: Option<bool>,
}

impl Requirement {
    pub fn need(&self, units: Units) -> Need {
        Need {
            thickness: self.thickness.map_or(0.0, |v| v.0.to(units)),
            cavity: self.cavity.map_or(0.0, |v| v.0.to(units)),
            fire: self.fire.unwrap_or(0.0),
            acoustic: self.acoustic.unwrap_or(0.0),
            bearing: self.bearing.unwrap_or(false),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::units::Length;

    fn wall(t: f64, cavity: Option<f64>, on: &[&str]) -> Assembly {
        Assembly {
            on: Some(WallRoles::Many(on.iter().map(|s| s.to_string()).collect())),
            thickness: Some(LengthSer(Length::Bare(t))),
            cavity: cavity.map(|c| LengthSer(Length::Bare(c))),
            ..Default::default()
        }
    }

    #[test]
    fn layers_give_thickness_and_cavity() {
        let a: Assembly = serde_json::from_value(serde_json::json!({
            "on": "partition",
            "layers": [
                { "material": "gypsum", "thickness": "13 mm" },
                { "material": "stud", "thickness": "89 mm", "cavity": true },
                { "material": "gypsum", "thickness": "13 mm" }
            ]
        }))
        .unwrap();
        assert!((a.thickness(Units::M) - 0.115).abs() < 1e-9);
        assert!((a.cavity(Units::M) - 0.089).abs() < 1e-9);
        assert!(a.validate("").is_ok());
    }

    #[test]
    fn plain_numbers_and_lengths_with_units_are_not_compared() {
        // In a plate in millimetres, 89 is 89 mm: not a cavity deeper than the wall.
        let mixed: Assembly =
            serde_json::from_value(serde_json::json!({ "thickness": "115 mm", "cavity": 89 })).unwrap();
        assert!(mixed.validate("").is_ok());
        for alike in [
            serde_json::json!({ "thickness": "115 mm", "cavity": "200 mm" }),
            serde_json::json!({ "thickness": 115, "cavity": 200 }),
        ] {
            let a: Assembly = serde_json::from_value(alike).unwrap();
            assert_eq!(a.validate("/a").unwrap_err().0, "/a/cavity");
        }
    }

    #[test]
    fn roles_match_with_refinements() {
        let a = wall(0.2, None, &["facade"]);
        assert!(a.matches(&["facade.storefront".into()]));
        assert!(!a.matches(&["partition".into()]));
        let free = Assembly { on: None, ..wall(0.2, None, &[]) };
        assert!(!free.matches(&["facade".into()]));
    }

    #[test]
    fn meeting_a_need() {
        let wet = wall(0.17, Some(0.14), &["wet"]);
        let partition = wall(0.115, Some(0.089), &["partition"]);
        let need = partition.requirement(Units::M).and(wet.requirement(Units::M));
        assert!(wet.meets(&need, Units::M));
        assert!(!partition.meets(&need, Units::M));
    }

    #[test]
    fn needs_a_size() {
        let (p, m) = Assembly::default().validate("/assemblies/x").unwrap_err();
        assert_eq!(p, "/assemblies/x");
        assert!(m.contains("thickness"), "{m}");
    }
}
