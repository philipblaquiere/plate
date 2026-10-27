import type { Point } from "../types";

/**
 * Moving one edge of the starting geometry (a boundary or a spine).
 *
 * By default the edge keeps its direction and its ends slide along the neighbouring edges, so
 * those keep theirs too: on a rectilinear plate that's a plain push or pull of one wall. A free
 * move translates both ends by the same vector instead, and the neighbours turn to follow.
 *
 * Pure geometry, no DOM: the viewer draws the result as a ghost and reports it on release.
 */

export interface EdgeMove {
  /** The edge's new ends. */
  a: Point;
  b: Point;
  /** Free move (both ends translated) rather than a parallel offset. */
  free: boolean;
  /** Signed distance along the edge's left normal (parallel moves). */
  offset: number;
  /** How far the edge moved, for the marker: from the middle of the old edge to the new one. */
  from: Point;
  to: Point;
  /** Why the move can't be written (the outline would be invalid), or null. */
  problem: string | null;
}

/** Unit normal on the left of a → b (in world space, y up). */
export function edgeNormal(a: Point, b: Point): Point {
  const len = Math.hypot(b.x - a.x, b.y - a.y) || 1;
  return { x: -(b.y - a.y) / len, y: (b.x - a.x) / len };
}

/** The points either side of edge `index` (null at the open ends of a spine). */
export function neighbours(
  points: Point[],
  closed: boolean,
  index: number,
): { prev: number | null; next: number | null } {
  const n = points.length;
  const j = (index + 1) % n;
  const prev = index > 0 || closed ? (index - 1 + n) % n : null;
  const next = j < n - 1 || closed ? (j + 1) % n : null;
  // A triangle's neighbours share a point; a two-point outline has none.
  return { prev: prev === j ? null : prev, next: next === index ? null : next };
}

/**
 * Snap a parallel offset to the grid. A horizontal or vertical edge lands with its free
 * coordinate on the grid (so an edge that was off-grid snaps onto it); any other edge moves by
 * whole grid steps.
 */
export function snapOffset(a: Point, b: Point, offset: number, step: number): number {
  if (!step) return offset;
  const n = edgeNormal(a, b);
  const round = (v: number) => Math.round(v / step) * step;
  if (Math.abs(n.y) > 1 - 1e-9) return (round(a.y + n.y * offset) - a.y) / n.y;
  if (Math.abs(n.x) > 1 - 1e-9) return (round(a.x + n.x * offset) - a.x) / n.x;
  return round(offset);
}

/** Move edge `index` parallel to itself by `offset`, its ends sliding along their neighbours. */
export function offsetEdge(points: Point[], closed: boolean, index: number, offset: number): EdgeMove {
  const n = points.length;
  const j = (index + 1) % n;
  const [a, b] = [points[index], points[j]];
  const len = Math.hypot(b.x - a.x, b.y - a.y);
  const e = { x: (b.x - a.x) / len, y: (b.y - a.y) / len };
  const nrm = edgeNormal(a, b);
  const delta = { x: nrm.x * offset, y: nrm.y * offset };
  const { prev, next } = neighbours(points, closed, index);
  const slide = (p: Point, q: number | null): Point => {
    const target = { x: p.x + delta.x, y: p.y + delta.y };
    if (q === null) return target;
    // Where the neighbour's line meets the moved edge's line.
    const o = points[q];
    const u = { x: p.x - o.x, y: p.y - o.y };
    const den = cross(u, e);
    if (Math.abs(den) < 1e-9 * Math.hypot(u.x, u.y)) return target; // in line with the edge
    const t = cross({ x: target.x - o.x, y: target.y - o.y }, e) / den;
    return { x: o.x + u.x * t, y: o.y + u.y * t };
  };
  const mid = { x: (a.x + b.x) / 2, y: (a.y + b.y) / 2 };
  const move: EdgeMove = {
    a: tidyPoint(slide(a, prev)),
    b: tidyPoint(slide(b, next)),
    free: false,
    offset,
    from: mid,
    to: { x: mid.x + delta.x, y: mid.y + delta.y },
    problem: null,
  };
  move.problem = problemOf(points, closed, index, move);
  return move;
}

/** Move edge `index` by `delta`: both ends translated, the neighbours turn to follow. */
export function translateEdge(points: Point[], closed: boolean, index: number, delta: Point): EdgeMove {
  const j = (index + 1) % points.length;
  const [a, b] = [points[index], points[j]];
  const mid = { x: (a.x + b.x) / 2, y: (a.y + b.y) / 2 };
  const nrm = edgeNormal(a, b);
  const move: EdgeMove = {
    a: tidyPoint({ x: a.x + delta.x, y: a.y + delta.y }),
    b: tidyPoint({ x: b.x + delta.x, y: b.y + delta.y }),
    free: true,
    offset: delta.x * nrm.x + delta.y * nrm.y,
    from: mid,
    to: { x: mid.x + delta.x, y: mid.y + delta.y },
    problem: null,
  };
  move.problem = problemOf(points, closed, index, move);
  return move;
}

/** The outline with the edge moved. */
export function movedPoints(points: Point[], index: number, move: EdgeMove): Point[] {
  const out = points.map((p) => ({ ...p }));
  out[index] = move.a;
  out[(index + 1) % points.length] = move.b;
  return out;
}

/** Did the move change anything? */
export function isNoop(points: Point[], index: number, move: EdgeMove): boolean {
  const j = (index + 1) % points.length;
  return samePoint(points[index], move.a) && samePoint(points[j], move.b);
}

/** Why the moved outline isn't valid geometry, in the user's words, or null. */
export function problemOf(points: Point[], closed: boolean, index: number, move: EdgeMove): string | null {
  const moved = movedPoints(points, index, move);
  const n = moved.length;
  const j = (index + 1) % n;
  const scale = Math.max(...points.map((p) => Math.max(Math.abs(p.x), Math.abs(p.y))), 1);
  const eps = scale * 1e-9;
  const what = closed ? "boundary" : "spine";

  // The edge itself turned around or shrank away (its ends slid past each other).
  const [a, b] = [points[index], points[j]];
  const along = (move.b.x - move.a.x) * (b.x - a.x) + (move.b.y - move.a.y) * (b.y - a.y);
  if (!move.free && along <= eps * Math.hypot(b.x - a.x, b.y - a.y)) {
    return "The edge would shrink to nothing: its ends meet.";
  }
  // A neighbour turned around: an end slid past the far end of the edge beside it.
  const { prev, next } = neighbours(points, closed, index);
  const flipped = (end: Point, moved: Point, other: number | null) =>
    other !== null &&
    (moved.x - points[other].x) * (end.x - points[other].x) +
      (moved.y - points[other].y) * (end.y - points[other].y) <=
      eps * Math.hypot(end.x - points[other].x, end.y - points[other].y);
  if (!move.free && (flipped(a, move.a, prev) || flipped(b, move.b, next))) {
    return "That's past the end of the edge beside it.";
  }
  // No edge may be (almost) zero long.
  const count = closed ? n : n - 1;
  for (let i = 0; i < count; i++) {
    const p = moved[i];
    const q = moved[(i + 1) % n];
    if (Math.hypot(q.x - p.x, q.y - p.y) <= eps) return `Two corners of the ${what} would meet.`;
  }
  if (selfCrossing(moved, closed)) return `The ${what} would cross itself.`;
  if (closed && Math.sign(signedArea(moved)) !== Math.sign(signedArea(points))) {
    return "The boundary would turn inside out.";
  }
  return null;
}

/** Does a polyline cross itself (edges that aren't neighbours touching or crossing)? */
export function selfCrossing(points: Point[], closed: boolean): boolean {
  const n = points.length;
  const count = closed ? n : n - 1;
  for (let i = 0; i < count; i++) {
    for (let k = i + 1; k < count; k++) {
      const adjacent = k === i + 1 || (closed && i === 0 && k === count - 1);
      if (adjacent) continue;
      if (segmentsTouch(points[i], points[(i + 1) % n], points[k], points[(k + 1) % n])) return true;
    }
  }
  return false;
}

/** Signed area: positive when counter-clockwise (y up). */
export function signedArea(pts: Point[]): number {
  let s = 0;
  for (let i = 0; i < pts.length; i++) {
    const a = pts[i];
    const b = pts[(i + 1) % pts.length];
    s += a.x * b.y - b.x * a.y;
  }
  return s / 2;
}

function segmentsTouch(p1: Point, p2: Point, q1: Point, q2: Point): boolean {
  const d1 = orient(q1, q2, p1);
  const d2 = orient(q1, q2, p2);
  const d3 = orient(p1, p2, q1);
  const d4 = orient(p1, p2, q2);
  if (((d1 > 0 && d2 < 0) || (d1 < 0 && d2 > 0)) && ((d3 > 0 && d4 < 0) || (d3 < 0 && d4 > 0))) return true;
  return (
    (d1 === 0 && within(q1, q2, p1)) ||
    (d2 === 0 && within(q1, q2, p2)) ||
    (d3 === 0 && within(p1, p2, q1)) ||
    (d4 === 0 && within(p1, p2, q2))
  );
}

function orient(a: Point, b: Point, c: Point): number {
  const v = (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x);
  const scale = Math.max(
    Math.abs(b.x - a.x) + Math.abs(b.y - a.y),
    Math.abs(c.x - a.x) + Math.abs(c.y - a.y),
  );
  return Math.abs(v) <= 1e-9 * scale * scale ? 0 : Math.sign(v);
}

function within(a: Point, b: Point, p: Point): boolean {
  return (
    Math.min(a.x, b.x) <= p.x &&
    p.x <= Math.max(a.x, b.x) &&
    Math.min(a.y, b.y) <= p.y &&
    p.y <= Math.max(a.y, b.y)
  );
}

function cross(u: Point, v: Point): number {
  return u.x * v.y - u.y * v.x;
}

function samePoint(p: Point, q: Point): boolean {
  return Math.abs(p.x - q.x) < 1e-9 * (1 + Math.abs(p.x)) && Math.abs(p.y - q.y) < 1e-9 * (1 + Math.abs(p.y));
}

/** Round away floating-point noise (e.g. 0.30000000000000004). */
export function tidy(v: number): number {
  return Math.abs(v) < 1e-12 ? 0 : Number(v.toPrecision(12));
}

function tidyPoint(p: Point): Point {
  return { x: tidy(p.x), y: tidy(p.y) };
}
