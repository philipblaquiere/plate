import type { Point, System, Units } from "../types";

/**
 * Starting files for the three modes. Boundary and spine are the two modes the solver
 * understands; free mode is presented separately but written as a spine with one point.
 *
 * Each comes with a starter program sized to what was drawn, so the solver has something to
 * place straight away: an apartment (or a studio, or a workplace) inside a boundary, a hotel
 * floor along a spine, a small house around an origin.
 */

export type StartMode = "boundary" | "spine" | "free";

const DECIMALS: Record<Units, number> = { mm: 0, cm: 1, m: 3, in: 3, ft: 4 };

/** A coordinate as written in a file: rounded to a sensible precision, no trailing zeros. */
export function formatNumber(v: number, units: Units): string {
  const s = v.toFixed(DECIMALS[units]);
  const trimmed = s.includes(".") ? s.replace(/0+$/, "").replace(/\.$/, "") : s;
  return trimmed === "-0" ? "0" : trimmed;
}

export function defaultUnits(system: System): Units {
  return system === "imperial" ? "ft" : "m";
}

const INTRO: Record<StartMode, string> = {
  boundary:
    "# Boundary mode: everything the solver places stays inside this closed outline.\n" +
    "# Drag the corners in the drawing, or edit the coordinates here. The spaces below are a\n" +
    "# starting program: change sizes and rules and watch it re-solve.",
  spine:
    "# Spine mode: the solver works along this line.\n" +
    "# Drag the points in the drawing, or edit the coordinates here. Below, rooms are stacked\n" +
    "# along a corridor, and each room is solved by its own plan.",
  free:
    "# Free mode: just an origin for the solver to work from.\n" +
    "# (To the solver this is a spine with a single point.) The spaces are packed around it.",
};

const POINTS_COMMENT: Record<StartMode, string> = {
  boundary: "# Corners, in order, as [x, y]. The last connects back to the first.",
  spine: "# Points along the spine, in order, as [x, y].",
  free: "# The origin, as [x, y].",
};

/** Sizes for programs, written in the file's units: metres, or rounded feet. */
function sizes(system: System) {
  const imperial = system === "imperial";
  // An area in m² or a length in m, converted and rounded for the file.
  const a = (m2: number) => (imperial ? String(Math.round((m2 * 10.7639) / 5) * 5) : String(m2));
  const l = (m: number) => (imperial ? String(Math.round((m / 0.3048) * 2) / 2) : String(m));
  return { a, l, m2: imperial ? 10.7639 : 1 };
}

function areaOf(points: Point[]): number {
  let s = 0;
  for (let i = 0; i < points.length; i++) {
    const p = points[i];
    const q = points[(i + 1) % points.length];
    s += p.x * q.y - q.x * p.y;
  }
  return Math.abs(s / 2);
}

function lengthOf(points: Point[]): number {
  let s = 0;
  for (let i = 1; i < points.length; i++)
    s += Math.hypot(points[i].x - points[i - 1].x, points[i].y - points[i - 1].y);
  return s;
}

/** The plan written after `start`, and anything to bind before the file's record. */
function program(mode: StartMode, points: Point[], system: System): { before: string; plan: string } {
  const { a, l, m2 } = sizes(system);
  if (mode === "spine") {
    const before = `# A hotel room. Its plan only talks about ports ('entrance, 'facade, 'wet), never about
# the corridor, so it works in any building that gives it those ports.
let hotel_room = {
  needs = {
    width = { min = ${l(3.4)}, max = ${l(4.5)} },
    depth.min = ${l(7)},
    ports = { entrance = 'required, facade = 'required },
  },
  spaces = {
    entry = { kind = 'circulation, area = { min = ${a(3)}, max = ${a(6)} }, side.min = ${l(1.2)} },
    bath = { area = { min = ${a(4)}, max = ${a(6)} }, side.min = ${l(1.6)} },
    sleep = { area.min = ${a(14)} },
  },
  rules = [
    plate.door "entry" 'entrance,
    plate.door "bath" "entry",
    plate.door "sleep" "entry",
    plate.window "sleep",
    plate.prefer (plate.touches "bath" 'wet),
  ],
}
in
`;
    const short = lengthOf(points) / (system === "imperial" ? 3.2808 : 1) < 20;
    const plan = `
  spaces = {
    corridor = { kind = 'circulation, width = ${l(1.8)} },
    room = { count.min = 1, width = { min = ${l(3.6)}, max = ${l(4.2)} }, depth = ${l(7.5)}, plan = hotel_room },${short ? "" : `\n    stair = { kind = 'vertical, count = 2, width = ${l(3.6)}, depth = ${l(7.5)} },`}
  },

  rules = [
    plate.along "room" "corridor",
  ],
`;
    return { before, plan };
  }

  if (mode === "free") {
    return {
      before: "",
      plan: `
  # A small house, packed around the origin.
  spaces = {
    living = { area.min = ${a(22)} },
    kitchen = { area = { min = ${a(8)}, max = ${a(12)} } },
    bedroom = { area = { min = ${a(11)}, max = ${a(15)} }, side.min = ${l(2.8)} },
    bath = { area = { min = ${a(4.5)}, max = ${a(6)} }, side.min = ${l(1.8)} },
  },

  rules = [
    plate.touches "kitchen" "living",
    plate.door "bedroom" "living",
    plate.door "bath" "living",
    plate.window "living",
    plate.window "bedroom",
  ],
`,
    };
  }

  // Boundary: a program that fits the area drawn (in m²).
  const area = areaOf(points) / m2;
  if (area < 45) {
    return {
      before: "",
      plan: `
  # A studio.
  spaces = {
    living = { area.min = ${a(Math.max(8, Math.round(area * 0.55)))} },
    bath = { area = { min = ${a(3.5)}, max = ${a(6)} }, side.min = ${l(1.6)} },
  },

  rules = [
    plate.door "bath" "living",
    plate.window "living",
  ],
`,
    };
  }
  if (area <= 160) {
    const bedrooms = area < 75 ? 1 : area < 115 ? 2 : 3;
    return {
      before: "",
      plan: `
  # An apartment. Bedrooms open off the hall, and the hall keeps them apart.
  spaces = {
    living = { area.min = ${a(Math.round(area * 0.2))} },
    kitchen = { area = { min = ${a(8)}, max = ${a(14)} } },
    hall = { kind = 'circulation, area = { min = ${a(5)}, max = ${a(6 + 3 * bedrooms)} }, side.min = ${l(1.2)} },
    bedroom = { count = ${bedrooms}, area = { min = ${a(11)}, max = ${a(16)} }, side.min = ${l(2.8)} },
    bath = { area = { min = ${a(4.5)}, max = ${a(7)} }, side.min = ${l(1.8)} },
  },

  rules = [
    plate.touches "kitchen" "living",
    plate.door "living" "hall",
    plate.door "bedroom" "hall",
    plate.door "bath" "hall",${bedrooms > 1 ? `\n    plate.between "bedroom" "hall",` : ""}
    plate.window "bedroom",
    plate.window "living",
    plate.prefer (plate.window "kitchen"),
  ],
`,
    };
  }
  const meetings = Math.max(2, Math.min(6, Math.round(area / 120)));
  return {
    before: "",
    plan: `
  # A workplace: open plan on the facade, meeting rooms beside it, a core.
  spaces = {
    open_plan = { area.min = ${a(Math.round(area * 0.4))} },
    meeting = { count.min = ${meetings}, area = { min = ${a(12)}, max = ${a(24)} }, side.min = ${l(3)} },
    core = { kind = 'vertical, area = { min = ${a(Math.round(Math.max(20, area * 0.06)))} }, side.min = ${l(4)} },
  },

  rules = [
    plate.touches "meeting" "open_plan",
    plate.touches "core" "open_plan",
    plate.window "open_plan",
  ],
`,
  };
}

export function template(mode: StartMode, points: Point[], system: System, name = "Untitled plate"): string {
  const units = defaultUnits(system);
  const list = points
    .map((p) => `      [${formatNumber(p.x, units)}, ${formatNumber(p.y, units)}],`)
    .join("\n");
  const { before, plan } = program(mode, points, system);
  return `${INTRO[mode]}

${before}{
  name = ${JSON.stringify(name)},

  # 'metric or 'imperial: how lengths are shown (metres, or feet and inches).
  system = '${system},
  # Unit of plain numbers. Strings carry their own: "98'-6", "12 ft 6 in", "9.5 m", "950 mm".
  units = '${units},

  start = {
    mode = '${mode === "boundary" ? "boundary" : "spine"},
    ${POINTS_COMMENT[mode]}
    points = [
${list}
    ],
  },
${plan}}
`;
}

/** Which of the three modes a configuration is in, as the user sees it. */
export function modeOf(config: Record<string, unknown> | null): StartMode | null {
  const start = config?.start as { mode?: string; points?: unknown[] } | undefined;
  if (!start) return null;
  if (start.mode === "boundary") return "boundary";
  if (start.mode === "spine") return (start.points?.length ?? 0) <= 1 ? "free" : "spine";
  return null;
}
