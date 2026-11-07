/** Where a space's label goes on screen when parts drawn in a service view would cover it. */

export interface Box {
  x0: number;
  y0: number;
  x1: number;
  y1: number;
}

interface Pt {
  x: number;
  y: number;
}

/**
 * The label's centre: `c` if nothing covers the label there, else just below or above a part
 * in the way (nearest first), still inside the space. `w` and `h` are the label's size; its box
 * runs from `top` above the centre to `h - top` below it. Falls back to `c`.
 */
export function clearOf(c: Pt, w: number, h: number, top: number, parts: Box[], space: Pt[]): Pt {
  const box = (p: Pt): Box => ({ x0: p.x - w / 2, x1: p.x + w / 2, y0: p.y - top, y1: p.y - top + h });
  const hits = (b: Box) => parts.filter((q) => b.x0 < q.x1 && q.x0 < b.x1 && b.y0 < q.y1 && q.y0 < b.y1);
  const first = hits(box(c));
  if (first.length === 0) return c;
  const gap = 3;
  const candidates: Pt[] = [];
  for (const q of first) {
    candidates.push({ x: c.x, y: q.y1 + gap + top }, { x: c.x, y: q.y0 - gap - (h - top) });
  }
  candidates.sort((a, b) => Math.abs(a.y - c.y) - Math.abs(b.y - c.y));
  for (const p of candidates) {
    const b = box(p);
    const corners = [
      { x: b.x0, y: b.y0 },
      { x: b.x1, y: b.y0 },
      { x: b.x1, y: b.y1 },
      { x: b.x0, y: b.y1 },
    ];
    if (hits(b).length === 0 && corners.every((k) => inside(k, space))) return p;
  }
  return c;
}

function inside(p: Pt, pts: Pt[]): boolean {
  let yes = false;
  for (let i = 0, j = pts.length - 1; i < pts.length; j = i++) {
    const a = pts[i];
    const b = pts[j];
    if (a.y > p.y !== b.y > p.y && p.x < ((b.x - a.x) * (p.y - a.y)) / (b.y - a.y) + a.x) yes = !yes;
  }
  return yes;
}
