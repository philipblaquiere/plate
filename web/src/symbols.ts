import catalogJson from "../../schema/symbols.json";

/**
 * The symbol catalog (`schema/symbols.json`), for the reference gallery and the drawing's
 * suggestions. The engine draws placed symbols itself (as paths in the layout); this module only
 * draws thumbnails and picks likely symbols for a space. Plain data and no DOM, so it can be
 * tested.
 */

export type Tone = "water" | "plant" | "sand" | "dark";

export interface Shape {
  rect?: number[];
  r?: number;
  line?: number[];
  poly?: number[];
  circle?: number[];
  ellipse?: number[];
  arc?: number[];
  tone?: Tone;
}

export interface SymbolDef {
  label: string;
  category: string;
  tags: string[];
  /** Width along the front and depth front to back, in millimetres. */
  size: [number, number];
  fit: "stretch" | "contain";
  shapes: Shape[];
}

export const catalog = (catalogJson as unknown as { symbols: Record<string, SymbolDef> }).symbols;

/** Symbol names in catalog order, grouped by category (in order of first appearance). */
export function byCategory(): { category: string; names: string[] }[] {
  const out: { category: string; names: string[] }[] = [];
  for (const [name, def] of Object.entries(catalog)) {
    let group = out.find((g) => g.category === def.category);
    if (!group) out.push((group = { category: def.category, names: [] }));
    group.names.push(name);
  }
  return out;
}

const fmt = (v: number) => String(Math.round(v * 100) / 100);

/**
 * A symbol as an SVG string, `size` pixels square, front at the bottom. `null` for an unknown
 * name.
 */
export function thumbnail(name: string, size = 64): string | null {
  const def = catalog[name];
  if (!def) return null;
  const [w, d] = def.size;
  const pad = 3;
  const k = (size - 2 * pad) / Math.max(w, d);
  const ox = (size - w * k) / 2;
  const oy = (size - d * k) / 2;
  // y runs from the front (bottom) to the back (top).
  const X = (x: number) => fmt(ox + x * k);
  const Y = (y: number) => fmt(oy + (d - y) * k);
  const parts: string[] = [];
  for (const s of def.shapes) {
    const cls = s.tone ? ` class="tone-${s.tone}"` : "";
    if (s.rect) {
      const [x, y, rw, rh] = s.rect;
      const r = s.r ? ` rx="${fmt(s.r * k)}"` : "";
      parts.push(
        `<rect x="${X(x)}" y="${Y(y + rh)}" width="${fmt(rw * k)}" height="${fmt(rh * k)}"${r}${cls}/>`,
      );
    } else if (s.line || s.poly) {
      const v = (s.line ?? s.poly)!;
      const pts: string[] = [];
      for (let i = 0; i + 1 < v.length; i += 2) pts.push(`${X(v[i])},${Y(v[i + 1])}`);
      parts.push(
        s.line ? `<polyline points="${pts.join(" ")}"/>` : `<polygon points="${pts.join(" ")}"${cls}/>`,
      );
    } else if (s.circle) {
      const [cx, cy, r] = s.circle;
      parts.push(`<circle cx="${X(cx)}" cy="${Y(cy)}" r="${fmt(r * k)}"${cls}/>`);
    } else if (s.ellipse) {
      const [cx, cy, rx, ry] = s.ellipse;
      parts.push(`<ellipse cx="${X(cx)}" cy="${Y(cy)}" rx="${fmt(rx * k)}" ry="${fmt(ry * k)}"${cls}/>`);
    } else if (s.arc) {
      const [cx, cy, r, from, to] = s.arc;
      const steps = Math.max(2, Math.ceil(Math.abs(to - from) / 10));
      const pts: string[] = [];
      for (let i = 0; i <= steps; i++) {
        const a = ((from + ((to - from) * i) / steps) * Math.PI) / 180;
        pts.push(`${X(cx + r * Math.cos(a))},${Y(cy + r * Math.sin(a))}`);
      }
      parts.push(`<polyline points="${pts.join(" ")}"/>`);
    }
  }
  return `<svg class="symbol-thumb" viewBox="0 0 ${size} ${size}" width="${size}" height="${size}" aria-hidden="true">${parts.join("")}</svg>`;
}

const words = (s: string) =>
  s
    .toLowerCase()
    .split(/[^a-z0-9]+/)
    .filter((w) => w.length > 1)
    .map((w) => w.replace(/s$/, ""));

/**
 * Symbols that suit a space, best first: by its type name and label (`bed` → beds, `parking` →
 * car, `pool` → swimming pool). At most `limit`.
 */
export function suggest(type: string, label = "", limit = 5): string[] {
  const wanted = new Set([...words(type), ...words(label)]);
  if (!wanted.size) return [];
  const scored: [string, number][] = [];
  for (const [name, def] of Object.entries(catalog)) {
    let score = 0;
    const own = words(name);
    for (const w of wanted) {
      if (own.includes(w)) score += 3;
      else if (words(def.label).includes(w)) score += 2;
      else if (def.tags.some((t) => words(t).includes(w))) score += 1;
    }
    if (score) scored.push([name, score]);
  }
  return scored
    .sort((a, b) => b[1] - a[1])
    .slice(0, limit)
    .map(([name]) => name);
}
