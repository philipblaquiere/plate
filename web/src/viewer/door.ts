/** A door drawn in plan, the way architects draw one: the opening in the wall with a jamb at each
 * end, the leaf standing open at 90° on its hinge, and the arc its latch edge sweeps back to the
 * closed position. All in screen coordinates. */

export interface Pt {
  x: number;
  y: number;
}

export interface DoorSymbol {
  /** Hinge and latch ends of the opening, on the wall face the door opens from. */
  hinge: Pt;
  latch: Pt;
  /** The leaf, open: a thin rectangle standing on the hinge. */
  leaf: Pt[];
  /** The swing: from the open leaf's tip back to the latch, centred on the hinge. */
  swing: string;
  /** The floor the leaf sweeps (hinge, open tip, arc, latch). */
  sweep: string;
  /** One short line across the wall at each end of the opening. */
  jambs: [Pt, Pt][];
}

/**
 * `a`, `b`: the opening's ends on the wall's centreline. `room`: the outline of the space the door
 * opens into (null if unknown: it then opens to the left of a→b). `depth`: the wall's thickness
 * (0 for a plain line).
 */
export function doorSymbol(a: Pt, b: Pt, room: Pt[] | null, depth: number): DoorSymbol | null {
  const len = Math.hypot(b.x - a.x, b.y - a.y);
  if (len < 1e-9) return null;
  let u = { x: (b.x - a.x) / len, y: (b.y - a.y) / len };
  let n = { x: -u.y, y: u.x };
  // Open into the room: the normal points to the side the room is on.
  if (room && room.length >= 3) {
    const mid = { x: (a.x + b.x) / 2, y: (a.y + b.y) / 2 };
    const probe = Math.max(depth / 2 + 1, len * 0.05);
    const inside = (s: number) => contains(room, { x: mid.x + n.x * probe * s, y: mid.y + n.y * probe * s });
    if (!inside(1) && inside(-1)) n = { x: -n.x, y: -n.y };
  }
  // Hinge on the end nearer a corner of the room, so the open leaf rests against a wall.
  let [h, l] = [a, b];
  if (room && room.length >= 3 && nearestCorner(room, b) < nearestCorner(room, a) - 1e-6) {
    [h, l] = [b, a];
    u = { x: -u.x, y: -u.y };
  }
  // The door hangs in the wall face on the room's side.
  const f = depth / 2;
  const hinge = { x: h.x + n.x * f, y: h.y + n.y * f };
  const latch = { x: l.x + n.x * f, y: l.y + n.y * f };
  const tip = { x: hinge.x + n.x * len, y: hinge.y + n.y * len };
  const t = Math.min(Math.max(len * 0.045, 1.5), 4);
  const leaf = [
    hinge,
    tip,
    { x: tip.x + u.x * t, y: tip.y + u.y * t },
    { x: hinge.x + u.x * t, y: hinge.y + u.y * t },
  ];
  // From n to u around the hinge: clockwise on screen (y down) when n × u > 0.
  const cw = n.x * u.y - n.y * u.x > 0 ? 1 : 0;
  const arc = `A${len},${len} 0 0 ${cw} ${latch.x},${latch.y}`;
  const swing = `M${tip.x},${tip.y}${arc}`;
  const sweep = `M${hinge.x},${hinge.y}L${tip.x},${tip.y}${arc}Z`;
  const half = Math.max(depth / 2, 3);
  const jamb = (p: Pt): [Pt, Pt] => [
    { x: p.x - n.x * half, y: p.y - n.y * half },
    { x: p.x + n.x * half, y: p.y + n.y * half },
  ];
  return { hinge, latch, leaf, swing, sweep, jambs: [jamb(a), jamb(b)] };
}

function nearestCorner(poly: Pt[], p: Pt): number {
  return Math.min(...poly.map((c) => Math.hypot(c.x - p.x, c.y - p.y)));
}

/** Even-odd point in polygon. */
function contains(poly: Pt[], p: Pt): boolean {
  let inside = false;
  for (let i = 0, j = poly.length - 1; i < poly.length; j = i++) {
    const [pi, pj] = [poly[i], poly[j]];
    if (pi.y > p.y !== pj.y > p.y && p.x < ((pj.x - pi.x) * (p.y - pi.y)) / (pj.y - pi.y) + pi.x)
      inside = !inside;
  }
  return inside;
}

/** An opening (a gap in a wall with no door) drawn in plan: a jamb at each end, and the wall's two
 * faces dashed across the gap for the header above it. All in screen coordinates. */
export interface OpeningSymbol {
  jambs: [Pt, Pt][];
  /** The header: one dashed line along each face of the wall (one line with no wall). */
  head: [Pt, Pt][];
}

export function openingSymbol(a: Pt, b: Pt, depth: number): OpeningSymbol | null {
  const len = Math.hypot(b.x - a.x, b.y - a.y);
  if (len < 1e-9) return null;
  const n = { x: -(b.y - a.y) / len, y: (b.x - a.x) / len };
  const half = Math.max(depth / 2, 3);
  const at = (p: Pt, s: number): Pt => ({ x: p.x + n.x * s, y: p.y + n.y * s });
  const jambs: [Pt, Pt][] = [
    [at(a, -half), at(a, half)],
    [at(b, -half), at(b, half)],
  ];
  const head: [Pt, Pt][] =
    depth > 0
      ? [
          [at(a, -depth / 2), at(b, -depth / 2)],
          [at(a, depth / 2), at(b, depth / 2)],
        ]
      : [[a, b]];
  return { jambs, head };
}
