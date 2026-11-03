use serde::{Deserialize, Serialize};

use crate::geometry::{path_length, signed_area, Bounds, Point};
use crate::plan::SpaceKind;
use crate::symbols::SymbolPath;
use crate::units::{System, Units};

/// The solver's output: plain geometry, ready for the viewer and the exporters.
#[derive(Debug, Clone, Serialize)]
pub struct Layout {
    pub name: String,
    /// How lengths should be presented.
    pub system: System,
    /// Unit of every coordinate in `elements` and `bounds`.
    pub units: Units,
    pub elements: Vec<Element>,
    pub bounds: Bounds,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ElementKind {
    /// The closed starting boundary.
    Boundary,
    /// The starting spine (an open polyline).
    Spine,
    /// A free origin (a spine of one point).
    Origin,
    /// A space placed by the solver (closed polygon).
    Space,
    /// A door: a segment on a shared wall.
    Door,
    /// A window: a segment on an exterior wall.
    Window,
    /// The rectangle a plan is previewed in (a plan file shown on its own).
    Slot,
    /// A port of the preview slot: a labelled part of its edge (`roles`).
    Port,
    /// The clear floor a fixture keeps around it (closed polygon, fixture included).
    Clearance,
    /// A drawing from the symbol catalog in a space (`symbol`): its paths are in `symbol`, its
    /// points are the frame it's drawn in.
    Symbol,
    /// Floor that no space covers (closed polygon, maybe with `holes`): what keeps a solution
    /// from being perfect.
    Leftover,
    /// A built wall: its footprint (closed polygon), and what it's built as (`wall`).
    Wall,
    /// A part a service placed (`part`): its footprint, drawn with its symbol.
    Part,
    /// A run of a service's network (`run`): an open polyline.
    Run,
}

impl ElementKind {
    /// CAD layer the element is written to on export.
    pub fn layer(self) -> &'static str {
        match self {
            ElementKind::Boundary => "PLATE-BOUNDARY",
            ElementKind::Spine => "PLATE-SPINE",
            ElementKind::Origin => "PLATE-ORIGIN",
            ElementKind::Space => "PLATE-SPACE",
            ElementKind::Door => "PLATE-DOOR",
            ElementKind::Window => "PLATE-WINDOW",
            ElementKind::Slot => "PLATE-SLOT",
            ElementKind::Port => "PLATE-PORT",
            ElementKind::Clearance => "PLATE-CLEARANCE",
            ElementKind::Symbol => "PLATE-SYMBOL",
            ElementKind::Leftover => "PLATE-LEFTOVER",
            ElementKind::Wall => "PLATE-WALL",
            ElementKind::Part => "PLATE-PART",
            ElementKind::Run => "PLATE-RUN",
        }
    }
}

fn space_layer(kind: SpaceKind) -> &'static str {
    match kind {
        SpaceKind::Room => "PLATE-SPACE-ROOM",
        SpaceKind::Zone => "PLATE-SPACE-ZONE",
        SpaceKind::Circulation => "PLATE-SPACE-CIRCULATION",
        SpaceKind::Void => "PLATE-SPACE-VOID",
        SpaceKind::Vertical => "PLATE-SPACE-VERTICAL",
        SpaceKind::Fixture => "PLATE-FIXTURE",
    }
}

/// What a placed space is.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpaceInfo {
    /// Instance name within its plan, e.g. `room_3`.
    pub name: String,
    /// The space type it is an instance of, e.g. `room`.
    #[serde(rename = "type")]
    pub type_key: String,
    pub label: String,
    pub kind: SpaceKind,
    /// Nesting level: 0 for the root plan's spaces.
    pub depth: u32,
    /// JSON pointer of the plan that placed it ("" for the root plan).
    pub plan: String,
    /// Typical group ("A", "B", …) shared by instances solved once.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub typical: Option<String>,
    /// Solved on its own (a `unique` or `instance` rule).
    pub unique: bool,
    /// Has a plan of its own that was solved inside it.
    pub has_plan: bool,
    /// False when its plan couldn't be solved here (see the diagnostics).
    pub ok: bool,
    /// The slot its plan was solved in, so the plan can be opened on its own in exactly this
    /// place (for spaces with a plan).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slot: Option<SlotInfo>,
    /// The front edge, from its left end to its right end as seen standing in front of it (so
    /// the space is on the left going from the first point to the second): a fixture's front,
    /// or the side a space is entered from.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub front: Option<[Point; 2]>,
    /// The symbol it's drawn with, from the catalog.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
    /// Takes the floor the other spaces leave (`fill = true`).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub fill: bool,
    /// For a fill space with a symbol: the colour to fill its shape with (a lawn is green), since
    /// a symbol can't be fitted to a shape that isn't a rectangle.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tone: Option<crate::symbols::Tone>,
    /// Its tags (`tags`).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    /// Floor area to its walls' centrelines, when walls take some of it (its own `area` is
    /// then the clear floor).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gross: Option<f64>,
}

/// What a wall is built as.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WallInfo {
    pub assembly: String,
    pub label: String,
    pub thickness: f64,
    pub cavity: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fire: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub acoustic: Option<f64>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub bearing: bool,
    /// Its build-up, from one face to the other.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub layers: Vec<LayerInfo>,
    pub pattern: &'static str,
    /// The port roles either side sees.
    pub roles: Vec<String>,
    /// The spaces either side; one may be a port role such as `'facade`.
    pub between: Vec<String>,
    /// Why it's built so: the assemblies its roles call for, the rules that ask for more.
    pub because: String,
    /// Its centreline.
    pub line: [Point; 2],
    /// The CAD layer the assembly asks for.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub layer: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LayerInfo {
    pub material: String,
    pub thickness: f64,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub cavity: bool,
}

/// A part a service placed.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PartInfo {
    pub service: String,
    pub part: String,
    pub label: String,
    pub mount: &'static str,
    /// Height above the floor (wall parts).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub above: Option<f64>,
    /// The space it serves.
    pub space: String,
    /// How it got there, in words: "every 3.6 along the walls of #room".
    pub placed: String,
    /// The group of its network it's in (a circuit), once connected.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    /// Where it stands: the middle of its wall side, or of its footprint.
    pub at: Point,
}

/// A run of a network.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunInfo {
    pub service: String,
    pub network: String,
    /// The group it serves (`C1`), if the network has groups.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    /// The last leg back to the source: drawn with an arrow.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub home: bool,
    pub draw: &'static str,
}

/// A symbol drawn in a space.
#[derive(Debug, Clone, Serialize)]
pub struct SymbolInfo {
    pub name: String,
    pub paths: Vec<SymbolPath>,
}

/// A slot a plan is solved in, in its canonical frame (entrance along the bottom), with
/// everything else the solution depends on. Sent back as [`crate::solve::View::Slot`] it
/// reproduces the same solution with the plan file on its own.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SlotInfo {
    pub width: f64,
    pub depth: f64,
    pub ports: Vec<PortInfo>,
    pub units: Units,
    pub system: System,
    /// The plate's seed.
    pub seed: u64,
    pub grid: f64,
    /// Set when the space is solved on its own (`plate.unique`, `plate.instance`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unique: Option<String>,
    /// Fingerprint of the plan as the plate used it.
    pub plan: String,
    /// What the plate gave the plan besides its slot: the assemblies and services it inherits,
    /// and the space it's in (for claims), so the plan opened on its own solves the same.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PortInfo {
    pub roles: Vec<String>,
    pub a: Point,
    pub b: Point,
}

/// What an opening connects.
#[derive(Debug, Clone, Serialize)]
pub struct OpeningInfo {
    /// Full names of the spaces on each side; one side may be a port role such as `'facade`.
    pub between: Vec<String>,
    pub width: f64,
    /// The thickness of the wall it's in, if walls are built.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub depth: Option<f64>,
}

/// A piece of geometry. Points keep the order they have in the configuration, so the editor can
/// map a vertex back to its `[x, y]` in the file.
#[derive(Debug, Clone, Serialize)]
pub struct Element {
    pub id: String,
    pub kind: ElementKind,
    pub layer: &'static str,
    pub points: Vec<Point>,
    pub closed: bool,
    /// Enclosed area, for closed elements.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub area: Option<f64>,
    /// Perimeter (closed) or length (open).
    pub length: f64,
    /// Where the element's points come from in the configuration (a JSON pointer), so the
    /// editor can write edits back.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub space: Option<SpaceInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub opening: Option<OpeningInfo>,
    /// Port roles, for `port` elements.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub roles: Option<Vec<String>>,
    /// What a `symbol` element draws.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol: Option<SymbolInfo>,
    /// Holes in a closed element (clockwise), e.g. a hall that runs all round a core.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub holes: Vec<Vec<Point>>,
    /// Where its label goes, for a shape whose middle may be outside it (a fill, floor left over).
    #[serde(rename = "labelAt", skip_serializing_if = "Option::is_none")]
    pub label_at: Option<Point>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wall: Option<WallInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub part: Option<PartInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run: Option<RunInfo>,
}

impl Element {
    pub fn new(id: impl Into<String>, kind: ElementKind, points: Vec<Point>, closed: bool) -> Self {
        Element {
            id: id.into(),
            kind,
            layer: kind.layer(),
            area: closed.then(|| signed_area(&points).abs()),
            length: path_length(&points, closed),
            closed,
            points,
            source: None,
            space: None,
            opening: None,
            roles: None,
            symbol: None,
            holes: Vec::new(),
            label_at: None,
            wall: None,
            part: None,
            run: None,
        }
    }

    /// Cut holes in a closed element; its area goes down by theirs.
    pub fn with_holes(mut self, holes: Vec<Vec<Point>>) -> Self {
        if let Some(a) = self.area.as_mut() {
            *a -= holes.iter().map(|h| signed_area(h).abs()).sum::<f64>();
        }
        self.length += holes.iter().map(|h| path_length(h, true)).sum::<f64>();
        self.holes = holes;
        self
    }

    /// Place the label inside the shape, for shapes that aren't rectangles.
    pub fn with_label_point(mut self) -> Self {
        if self.points.len() > 4 || !self.holes.is_empty() {
            self.label_at = Some(crate::geometry::interior_point(&self.points, &self.holes));
        }
        self
    }

    pub fn space(id: impl Into<String>, points: Vec<Point>, info: SpaceInfo) -> Self {
        let mut e = Element::new(id, ElementKind::Space, points, true);
        e.layer = space_layer(info.kind);
        e.space = Some(info);
        e
    }

    pub fn opening(id: impl Into<String>, kind: ElementKind, a: Point, b: Point, info: OpeningInfo) -> Self {
        let mut e = Element::new(id, kind, vec![a, b], false);
        e.opening = Some(info);
        e
    }

    pub fn with_source(mut self, pointer: impl Into<String>) -> Self {
        self.source = Some(pointer.into());
        self
    }

    /// The CAD layer it's exported on: its kind's, or for a wall its assembly's (`A-WALL-…`), for
    /// a service's parts and runs the service's (`PLATE-POWER`, `PLATE-POWER-RUN`).
    pub fn cad_layer(&self) -> String {
        let upper = |s: &str| s.to_uppercase().replace(['_', ' ', '.'], "-");
        if let Some(w) = &self.wall {
            return w.layer.clone().unwrap_or_else(|| format!("A-WALL-{}", upper(&w.assembly)));
        }
        if let Some(p) = &self.part {
            return format!("PLATE-{}", upper(&p.service));
        }
        if let Some(r) = &self.run {
            return format!("PLATE-{}-RUN", upper(&r.service));
        }
        self.layer.to_owned()
    }
}

/// How good a solution is, at a glance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    /// Every rule and preference holds and every bit of floor belongs to a space.
    Perfect,
    /// Every requirement holds, but a preference is missed or some floor is left over.
    Solved,
    /// Some requirement isn't met.
    Incomplete,
    /// Nothing to solve: no spaces yet.
    Empty,
}

/// The summary behind the status: what keeps a solution from being perfect.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub status: Status,
    /// Requirements not met (rules, sizes, fitting inside).
    pub unmet: usize,
    /// Preferences not met.
    pub preferences: usize,
    /// Floor no space covers, in square `units`.
    pub leftover: f64,
    /// The floor there is to fill, in square `units` (the boundary or preview slot).
    pub floor: f64,
    /// Spaces placed, at every level.
    pub spaces: usize,
}

impl Layout {
    pub fn new(name: String, system: System, units: Units, elements: Vec<Element>) -> Self {
        let bounds = elements
            .iter()
            .map(|e| Bounds::from_points(&e.points))
            .reduce(Bounds::union)
            .unwrap_or(Bounds::EMPTY);
        Layout { name, system, units, elements, bounds }
    }
}
