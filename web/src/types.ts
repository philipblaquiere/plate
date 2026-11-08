// Mirrors the JSON produced by the Rust engine (crates/plate-core).

export type Units = "mm" | "cm" | "m" | "in" | "ft";
export type System = "metric" | "imperial";

export interface Point {
  x: number;
  y: number;
}

export interface Bounds {
  minX: number;
  minY: number;
  maxX: number;
  maxY: number;
}

export type ElementKind =
  | "boundary"
  | "spine"
  | "origin"
  | "space"
  | "door"
  | "window"
  | "slot"
  | "port"
  | "clearance"
  | "symbol"
  | "leftover"
  | "wall"
  | "part"
  | "run";

export type SpaceKind = "room" | "zone" | "circulation" | "void" | "vertical" | "fixture";

/** What a placed space is (elements of kind "space"). */
export interface SpaceInfo {
  /** Instance name within its plan, e.g. `room_3`. */
  name: string;
  /** The space type it is an instance of, e.g. `room`. */
  type: string;
  label: string;
  kind: SpaceKind;
  /** Nesting level: 0 for the root plan's spaces. */
  depth: number;
  /** JSON pointer of the plan that placed it ("" for the root plan). */
  plan: string;
  /** Typical group ("A", "B", …), when several instances share one solution. */
  typical?: string;
  unique: boolean;
  hasPlan: boolean;
  ok: boolean;
  /** The slot its plan was solved in (spaces with a plan): sent back as a view, a plan file
   * shown on its own is solved exactly as it is here. */
  slot?: SlotInfo;
  /** The front edge, from its left end to its right end seen standing in front of it (the space
   * is on the left going from the first point to the second): a fixture's front, or the side a
   * space is entered from. */
  front?: [Point, Point];
  /** The symbol it's drawn with (see `schema/symbols.json`). */
  symbol?: string;
  /** The fill space of its plan: whatever floor the others leave, in any shape. */
  fill?: boolean;
  /** A fill drawn as a material (its symbol's tone) rather than a symbol. */
  tone?: "water" | "plant" | "sand" | "dark";
  /** Its tags (`tags`): what services and rules select it by. */
  tags?: string[];
  /** Floor area to its walls' centrelines, when walls take some of it (`area` is then its clear
   * floor). */
  gross?: number;
}

/** What a wall is built as (elements of kind "wall"). */
export interface WallInfo {
  assembly: string;
  label: string;
  thickness: number;
  cavity: number;
  fire?: number;
  acoustic?: number;
  bearing?: boolean;
  layers?: { material: string; thickness: number; cavity?: boolean }[];
  pattern: "solid" | "hatch" | "cross" | "light";
  /** The port roles either side sees. */
  roles: string[];
  /** The spaces either side; one may be a port role such as `'facade`. */
  between: string[];
  /** Why it's built so. */
  because: string;
  /** Its centreline. */
  line: [Point, Point];
  /** Whether services' runs may pass through it. */
  cross?: "yes" | "firestop" | "no";
}

/** A part a service placed (elements of kind "part"). */
export interface PartInfo {
  service: string;
  part: string;
  label: string;
  mount: "wall" | "ceiling" | "floor" | "cavity";
  above?: number;
  /** The space it's in. */
  space: string;
  /** How it got there, in words. */
  placed: string;
  /** Its group in a network (a circuit), once connected. */
  group?: string;
  /** The network that group is in. */
  network?: string;
  /** The service whose circuit powers it, when that's another service: it's drawn in that
   * service's view too. */
  fedBy?: string;
  /** What it draws. */
  load?: Load;
  /** Where it stands. */
  at: Point;
}

/** What a terminal asks of a service: for power, a current at a voltage. */
export interface Load {
  current?: number;
  watts?: number;
  voltage?: number;
  conductors?: number;
}

/** A point of note on a routed run. */
export interface RunMark {
  at: Point;
  /** `riser`: up or down a wall; `cross`: through a wall; `firestop`: through a fire separation. */
  kind: "riser" | "cross" | "firestop";
  /** Along the wall it passes through. */
  along?: Point;
}

/** One group of a network (a circuit), for a panel schedule. */
export interface Circuit {
  service: string;
  network: string;
  scope: string;
  group: string;
  rating?: number;
  voltage?: number;
  medium?: string;
  devices: [string, number][];
  load: number;
  length: number;
}

/** A run of a service's network (elements of kind "run"). */
export interface RunInfo {
  service: string;
  network: string;
  group?: string;
  /** The last leg toward the source: drawn with an arrow (arcs), or numbered where it starts
   * (routed). */
  home?: boolean;
  /** The schematic (`arcs`) or where it runs (`routed`); both are made. */
  draw: "arcs" | "routed";
  /** The drawing its network didn't ask for: left out of exports. */
  alt?: boolean;
  /** Routed: where this piece goes. */
  along?: "wall" | "header" | "ceiling" | "drop";
  /** The cable it's a piece of, and that cable's length. */
  cable?: number;
  length?: number;
  /** What it's made of (`"14/2"`), and its jacket's colour. */
  medium?: string;
  colour?: "white" | "black" | "grey" | "red" | "yellow" | "orange" | "blue";
  /** A switch leg, not a circuit. */
  control?: boolean;
  /** The service whose circuit powers it, when that's another service. */
  fedBy?: string;
  marks?: RunMark[];
}

/** A symbol drawn in a space: flattened paths in drawing units. */
export interface SymbolInfo {
  name: string;
  paths: { points: Point[]; closed: boolean; tone?: "water" | "plant" | "sand" | "dark" }[];
}

/** A slot a plan is solved in, in its canonical frame (entrance along the bottom). */
export interface SlotInfo {
  width: number;
  depth: number;
  ports: { roles: string[]; a: Point; b: Point }[];
  units: Units;
  system: System;
  seed: number;
  grid: number;
  unique?: string;
  /** Fingerprint of the plan as the plate used it. */
  plan: string;
  /** What the plate gave the plan besides its slot (assemblies, services): sent back as is. */
  context?: unknown;
}

/** What a plan file (no `start`) is shown in. Plates ignore it. */
export type PreviewView = "default" | { case: number } | { slot: SlotInfo };

export interface OpeningInfo {
  /** Full names of the spaces on each side; a port role such as `'facade` for the outside. */
  between: string[];
  width: number;
  /** The thickness of the wall it's in, if walls are built. */
  depth?: number;
  /** Doors: "none" for an opening (a gap with no door). */
  leaf?: "swing" | "none";
}

export interface Element {
  id: string;
  kind: ElementKind;
  layer: string;
  /** In the same order as in the configuration file. */
  points: Point[];
  closed: boolean;
  /** Enclosed area, for closed elements. */
  area?: number;
  /** Perimeter (closed) or length (open). */
  length: number;
  /** JSON pointer to where the element comes from in the configuration, e.g. `/start/points`
   * for the starting geometry or `/spaces/kitchen` for a placed space. */
  source?: string;
  space?: SpaceInfo;
  opening?: OpeningInfo;
  /** Port roles, for `port` elements (the sides of a plan's preview slot). */
  roles?: string[];
  /** What a `symbol` element draws. */
  symbol?: SymbolInfo;
  /** Holes in a closed element (a hall that runs all round a core). */
  holes?: Point[][];
  /** Where its label goes, for a shape whose middle may be outside it. */
  labelAt?: Point;
  wall?: WallInfo;
  part?: PartInfo;
  run?: RunInfo;
}

export interface Layout {
  name: string;
  /** How lengths are presented. */
  system: System;
  /** Unit of every coordinate in the layout. */
  units: Units;
  elements: Element[];
  bounds: Bounds;
  /** Every circuit of every network, for a panel schedule. */
  circuits?: Circuit[];
}

export interface Span {
  startLine: number;
  startCol: number;
  endLine: number;
  endCol: number;
}

/** Where a value of the evaluated configuration comes from in the file. */
export interface SourceSpan {
  /** JSON pointer, e.g. `/start/points/2/0`. */
  pointer: string;
  /** The file it's written in, relative to the workspace. */
  file: string;
  span: Span;
  kind: "number" | "string" | "array" | "record" | "other";
  /** The span is the literal itself (so it can be rewritten), not a computed expression. */
  literal: boolean;
}

export interface Diagnostic {
  severity: "error" | "warning" | "info";
  message: string;
  notes?: string[];
  span?: Span;
  /** The configuration value it is about (JSON pointer), when the solver reports it. */
  pointer?: string;
  /** The file `span` is in, relative to the workspace (the plate file when absent). */
  file?: string;
  /** A solver finding: a requirement not met, a preference missed, floor left over. Its notes
   * say what to change. */
  code?: "unmet" | "preference" | "leftover";
}

/** How good a solution is: perfect (everything holds, no floor left over), solved (every
 * requirement holds), incomplete (some don't), empty (nothing to place). */
export type SolveStatus = "perfect" | "solved" | "incomplete" | "empty";

export interface Report {
  status: SolveStatus;
  /** Requirements not met. */
  unmet: number;
  /** Preferences missed. */
  preferences: number;
  /** Floor no space covers, in square layout units. */
  leftover: number;
  /** The floor there is to fill, in square layout units. */
  floor: number;
  /** Spaces placed, at every level. */
  spaces: number;
}

export type SolveResponse =
  | {
      ok: true;
      layout: Layout;
      solver: string;
      timings: { evalMs: number; solveMs: number };
      diagnostics: Diagnostic[];
      report?: Report;
    }
  | { ok: false; diagnostics: Diagnostic[] };

export type ExportFormat = "dxf" | "svg" | "json";
