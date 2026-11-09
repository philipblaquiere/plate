//! The configuration, as the engine sees it after Nickel has been evaluated and checked
//! against `schema/plate.schema.json`.
//!
//! The JSON Schema is the source of truth for the format; these types mirror it so the solver
//! gets typed data. Checks the schema can't express (geometry: repeated points, zero area, a
//! boundary crossing itself) live here and report the path of the offending value, which the
//! editor turns into a squiggle.

use std::collections::BTreeMap;

use serde::Deserialize;

use crate::assembly::Assembly;
use crate::geometry::{segments_cross, Point, Polygon};
use crate::plan::{Engine, Goals, LengthSer, Needs, Plan, Previews, RuleItem, Space};
use crate::service::Service;
use crate::units::{Length, System, Units};

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    /// Human-readable name of the floor plate. Shown in the viewer's title block and exports.
    #[serde(default = "default_name")]
    pub name: String,

    /// How lengths are presented: decimal metres or feet-and-inches.
    #[serde(default)]
    pub system: System,

    /// Unit of bare numbers. Defaults to metres (metric) or feet (imperial).
    #[serde(default)]
    units: Option<Units>,

    /// The geometry the solver starts from. A file without one is a plan: it is shown on its own
    /// in a preview slot (see [`crate::solve::View`]).
    #[serde(default)]
    pub start: Option<Start>,

    /// Seed for the solver: same file and seed, same layout.
    #[serde(default = "default_seed")]
    pub seed: u64,

    /// Grid spaces snap to. Defaults from `system`.
    #[serde(default)]
    pub grid: Option<LengthSer>,

    /// Which way the grid runs (`'auto`: square to the outline's walls).
    #[serde(default)]
    pub align: Option<crate::plan::Align>,

    /// The root plan's fields (a plan can also sit inside any space).
    #[serde(default)]
    pub engine: Option<Engine>,
    #[serde(default)]
    pub spaces: BTreeMap<String, Space>,
    #[serde(default)]
    pub rules: Vec<RuleItem>,
    #[serde(default)]
    pub goals: Goals,
    #[serde(default)]
    pub assemblies: BTreeMap<String, Assembly>,
    #[serde(default)]
    pub services: BTreeMap<String, Service>,

    /// Floor to ceiling. Services use it for the cable that runs up a wall to the ceiling.
    /// Defaults to 2.4 m (metric) or 8 ft (imperial).
    #[serde(default)]
    pub height: Option<LengthSer>,

    /// A plan file's own fields: what it needs from its space, and how to preview it.
    #[serde(default)]
    pub needs: Needs,
    #[serde(default)]
    pub preview: Option<Previews>,
}

fn default_seed() -> u64 {
    1
}

/// The role(s) of one boundary edge.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(untagged)]
pub enum EdgeRoles {
    One(String),
    Many(Vec<String>),
}

impl EdgeRoles {
    pub fn roles(&self) -> Vec<String> {
        match self {
            EdgeRoles::One(r) => vec![r.clone()],
            EdgeRoles::Many(v) => v.clone(),
        }
    }
}

fn default_name() -> String {
    "Untitled plate".to_owned()
}

/// A point written `[x, y]`; each coordinate is a number or a length string.
#[derive(Debug, Clone, Copy, Deserialize)]
pub struct PointInput(pub Length, pub Length);

impl PointInput {
    pub fn to(self, units: Units) -> Point {
        Point::new(self.0.to(units), self.1.to(units))
    }
}

/// The two starting modes the solver understands. (The editor's "free" mode is a spine with a
/// single point: an origin.)
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "mode", rename_all = "lowercase", deny_unknown_fields)]
pub enum Start {
    /// A closed boundary; everything the solver places stays inside it. `ports` gives each edge
    /// a role (`'facade` when left out).
    Boundary {
        points: Vec<PointInput>,
        #[serde(default)]
        ports: Vec<EdgeRoles>,
    },
    /// A line (or a single origin point) the solver works around.
    Spine { points: Vec<PointInput> },
}

impl Start {
    pub fn points(&self) -> &[PointInput] {
        match self {
            Start::Boundary { points, .. } | Start::Spine { points } => points,
        }
    }
}

/// A schema error, located by its path in the evaluated value (e.g. `["start", "points", "3"]`).
#[derive(Debug, Clone, thiserror::Error)]
#[error("{message}")]
pub struct ConfigError {
    pub path: Vec<String>,
    pub message: String,
}

impl ConfigError {
    fn at(path: &[&str], message: impl Into<String>) -> Self {
        ConfigError { path: path.iter().map(|s| s.to_string()).collect(), message: message.into() }
    }

    fn at_point(index: usize, message: impl Into<String>) -> Self {
        ConfigError {
            path: vec!["start".into(), "points".into(), index.to_string()],
            message: message.into(),
        }
    }

    fn from_pointer((pointer, message): (String, String)) -> Self {
        ConfigError {
            path: pointer.split('/').filter(|s| !s.is_empty()).map(str::to_owned).collect(),
            message,
        }
    }

    /// The path as a JSON pointer, e.g. `/start/points/3`.
    pub fn pointer(&self) -> String {
        self.path.iter().map(|s| format!("/{s}")).collect()
    }
}

impl Config {
    /// Solve with another seed: the plate's, or every preview case's for a plan file.
    pub fn reseed(&mut self, seed: u64) {
        self.seed = seed;
        match &mut self.preview {
            Some(Previews::One(c)) => c.seed = Some(seed),
            Some(Previews::Many(v)) => v.iter_mut().for_each(|c| c.seed = Some(seed)),
            None => {
                self.preview =
                    Some(Previews::One(crate::plan::PreviewCase { seed: Some(seed), ..Default::default() }))
            }
        }
    }

    pub fn from_value(value: serde_json::Value) -> Result<Config, ConfigError> {
        let config: Config = serde_path_to_error::deserialize(value).map_err(|err| {
            let path = err
                .path()
                .iter()
                .filter_map(|seg| match seg {
                    serde_path_to_error::Segment::Map { key } => Some(key.clone()),
                    serde_path_to_error::Segment::Seq { index } => Some(index.to_string()),
                    _ => None,
                })
                .collect();
            ConfigError { path, message: err.inner().to_string() }
        })?;
        config.validate()?;
        Ok(config)
    }

    /// The unit bare numbers are measured in, which is also the unit of the output geometry.
    pub fn units(&self) -> Units {
        self.units.unwrap_or_else(|| self.system.default_units())
    }

    /// Floor to ceiling, in `units`.
    pub fn height(&self) -> f64 {
        let units = self.units();
        match self.height {
            Some(h) => h.0.to(units),
            None => match self.system {
                System::Metric => 2400.0 / units.to_mm(),
                System::Imperial => 2438.4 / units.to_mm(),
            },
        }
    }

    /// A plan file (no `start`), shown on its own rather than as a plate.
    pub fn is_plan(&self) -> bool {
        self.start.is_none()
    }

    /// The root plan. For a plan file, the plan exactly as a plate importing it would see it.
    pub fn plan(&self) -> Plan {
        Plan {
            engine: self.engine,
            grid: self.grid,
            align: self.align,
            needs: self.needs.clone(),
            spaces: self.spaces.clone(),
            rules: self.rules.clone(),
            goals: self.goals,
            preview: self.preview.clone(),
            assemblies: self.assemblies.clone(),
            services: self.services.clone(),
            origins: Vec::new(),
        }
    }

    /// Port roles of the boundary's edges (`facade` unless given).
    pub fn edge_roles(&self) -> Vec<Vec<String>> {
        let n = self.start_input().len();
        let given: &[EdgeRoles] = match &self.start {
            Some(Start::Boundary { ports, .. }) => ports,
            _ => &[],
        };
        (0..n)
            .map(|i| given.get(i).map(EdgeRoles::roles).unwrap_or_else(|| vec!["facade".to_owned()]))
            .collect()
    }

    fn start_input(&self) -> &[PointInput] {
        self.start.as_ref().map_or(&[], Start::points)
    }

    /// The starting points, resolved to `units()`.
    pub fn start_points(&self) -> Vec<Point> {
        let units = self.units();
        self.start_input().iter().map(|p| p.to(units)).collect()
    }

    fn validate(&self) -> Result<(), ConfigError> {
        let Some(start) = &self.start else {
            self.validate_preview()?;
            return self.plan().validate("").map_err(ConfigError::from_pointer);
        };
        self.validate_start(start)?;
        if let Start::Boundary { ports, points } = start {
            if ports.len() > points.len() {
                return Err(ConfigError::at(
                    &["start", "ports"],
                    format!("{} port roles for {} edges", ports.len(), points.len()),
                ));
            }
        }
        self.plan().validate("").map_err(ConfigError::from_pointer)
    }

    /// Preview slots need a positive size.
    fn validate_preview(&self) -> Result<(), ConfigError> {
        let Some(previews) = &self.preview else { return Ok(()) };
        let many = matches!(previews, Previews::Many(_));
        for (i, case) in previews.cases().into_iter().enumerate() {
            let units = case.units.unwrap_or_else(|| case.system.unwrap_or_default().default_units());
            for (field, value) in [("width", case.width), ("depth", case.depth)] {
                let Some(v) = value else { continue };
                let v = v.0.to(units);
                if !(v.is_finite() && v > units.tolerance()) {
                    let mut path = vec!["preview".to_owned()];
                    if many {
                        path.push(i.to_string());
                    }
                    path.push(field.to_owned());
                    return Err(ConfigError {
                        path,
                        message: format!("the preview's {field} must be more than 0"),
                    });
                }
            }
        }
        Ok(())
    }

    fn validate_start(&self, start: &Start) -> Result<(), ConfigError> {
        let points = self.start_points();
        for (i, p) in points.iter().enumerate() {
            if !p.x.is_finite() || !p.y.is_finite() {
                return Err(ConfigError::at_point(i, "coordinates must be finite numbers"));
            }
        }
        let closed = matches!(start, Start::Boundary { .. });
        let (min, what) = if closed { (3, "a boundary") } else { (1, "a spine") };
        if points.len() < min {
            return Err(ConfigError::at(
                &["start", "points"],
                format!("{what} needs at least {min} point{}", if min == 1 { "" } else { "s" }),
            ));
        }

        // Consecutive points must differ (a zero-length edge has no direction).
        let tolerance = self.units().tolerance();
        let n = points.len();
        let pairs = if closed { n } else { n.saturating_sub(1) };
        for i in 0..pairs {
            let j = (i + 1) % n;
            if points[i].distance(points[j]) <= tolerance {
                return Err(ConfigError::at_point(
                    j,
                    format!("point {} is in the same place as point {}", j + 1, i + 1),
                ));
            }
        }

        if closed {
            // Non-adjacent edges must not touch or cross.
            for i in 0..n {
                for j in (i + 2)..n {
                    if i == 0 && j == n - 1 {
                        continue; // first and last edges share point 1
                    }
                    let (a, b) = (points[i], points[(i + 1) % n]);
                    let (c, d) = (points[j], points[(j + 1) % n]);
                    if segments_cross(a, b, c, d, tolerance) {
                        return Err(ConfigError::at_point(
                            j,
                            format!(
                                "the boundary crosses itself: edge {}–{} meets edge {}–{}",
                                i + 1,
                                (i + 1) % n + 1,
                                j + 1,
                                (j + 1) % n + 1
                            ),
                        ));
                    }
                }
            }
            // Checked after crossings: a bow tie also has zero (cancelling) area.
            let polygon = Polygon::new(points.clone());
            if polygon.area() <= tolerance * tolerance {
                return Err(ConfigError::at(
                    &["start", "points"],
                    "the boundary has no area: its points are in a line",
                ));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn cfg(v: serde_json::Value) -> Result<Config, ConfigError> {
        Config::from_value(v)
    }

    #[test]
    fn boundary_with_mixed_lengths() {
        let c = cfg(json!({
            "system": "imperial",
            "start": { "mode": "boundary", "points": [[0, 0], ["98'-6", 0], [98.5, "30 m"]] }
        }))
        .unwrap();
        assert_eq!(c.units(), Units::Ft);
        let p = c.start_points();
        assert!((p[1].x - 98.5).abs() < 1e-9);
        assert!((p[2].y - 30_000.0 / 304.8).abs() < 1e-9);
    }

    #[test]
    fn single_point_spine_is_a_free_origin() {
        let c = cfg(json!({ "start": { "mode": "spine", "points": [[5, 5]] } })).unwrap();
        assert_eq!(c.start_points().len(), 1);
    }

    #[test]
    fn rejects_repeated_points_at_the_repeat() {
        let err =
            cfg(json!({ "start": { "mode": "spine", "points": [[0, 0], [1, 1], [1, 1]] } })).unwrap_err();
        assert_eq!(err.pointer(), "/start/points/2");
        assert!(err.message.contains("point 3"), "{}", err.message);
    }

    #[test]
    fn rejects_self_crossing_boundary() {
        // A bow tie: edges 1–2 and 3–4 cross.
        let err =
            cfg(json!({ "start": { "mode": "boundary", "points": [[0, 0], [10, 10], [10, 0], [0, 10]] } }))
                .unwrap_err();
        assert!(err.message.contains("crosses itself"), "{}", err.message);
        assert_eq!(err.pointer(), "/start/points/2");
    }

    #[test]
    fn rejects_flat_boundary() {
        let err =
            cfg(json!({ "start": { "mode": "boundary", "points": [[0, 0], [5, 0], [10, 0]] } })).unwrap_err();
        assert!(err.message.contains("no area"), "{}", err.message);
    }

    #[test]
    fn rejects_unknown_mode() {
        let err = cfg(json!({ "start": { "mode": "blob", "points": [[0, 0]] } })).unwrap_err();
        assert_eq!(err.pointer(), "/start/mode");
    }
}
