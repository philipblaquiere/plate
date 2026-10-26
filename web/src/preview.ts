// Plan previews: a plan file (no `start`) is shown on its own, solved in a slot. Kept free of the
// editor so tests can use it.

import { formatNumber } from "./editor/templates";
import type { SlotInfo, System, Units } from "./types";

/** The sides of a preview slot, counter-clockwise from the bottom (the order of its corners). */
export const SIDES = ["bottom", "right", "top", "left"] as const;
export type Side = (typeof SIDES)[number];

/** Roles of a side that isn't given: a typical room on a corridor. Mirrors `DEFAULT_SIDES` in Rust. */
export const DEFAULT_SIDES: Record<Side, string[]> = {
  bottom: ["entrance"],
  right: ["party"],
  top: ["facade"],
  left: ["party", "wet"],
};

/** A `preview` case as evaluated. */
export interface PreviewCase {
  name?: string;
  width?: number | string;
  depth?: number | string;
}

/** The cases a plan file offers, from its evaluated configuration (empty: none, an automatic slot). */
export function previewCases(config: unknown): PreviewCase[] {
  const preview = (config as { preview?: unknown } | null)?.preview;
  if (!preview || typeof preview !== "object") return [];
  return (Array.isArray(preview) ? preview : [preview]) as PreviewCase[];
}

/** Where case `index` is written: `/preview`, or `/preview/2` in a list. */
export function casePointer(config: unknown, index: number): string {
  const preview = (config as { preview?: unknown } | null)?.preview;
  return Array.isArray(preview) ? `/preview/${index}` : "/preview";
}

/** What a case is called in menus. */
export function caseLabel(c: PreviewCase | undefined, index: number): string {
  return c?.name ?? (index === 0 ? "preview" : `preview ${index + 1}`);
}

/**
 * The roles along each side of a slot, from its port segments. A side split between several
 * ports (part of a wall on a facade, say) takes the roles of its longest part, and `approximate`
 * says so. A side with no port at all is an ordinary `'partition`.
 */
export function sidesOf(slot: SlotInfo): { roles: Record<Side, string[]>; approximate: boolean } {
  const { width: w, depth: h } = slot;
  const tol = 1e-6 * (1 + w + h);
  const on = (side: Side, p: { x: number; y: number }) =>
    side === "bottom"
      ? Math.abs(p.y) < tol
      : side === "top"
        ? Math.abs(p.y - h) < tol
        : side === "left"
          ? Math.abs(p.x) < tol
          : Math.abs(p.x - w) < tol;
  const full = (side: Side) => (side === "bottom" || side === "top" ? w : h);
  let approximate = false;
  const roles = {} as Record<Side, string[]>;
  for (const side of SIDES) {
    const along = slot.ports
      .filter((p) => on(side, p.a) && on(side, p.b))
      .map((p) => ({ roles: p.roles, len: Math.hypot(p.b.x - p.a.x, p.b.y - p.a.y) }))
      .sort((a, b) => b.len - a.len);
    if (!along.length) {
      roles[side] = ["partition"];
      approximate = true;
      continue;
    }
    roles[side] = along[0].roles;
    if (along.length > 1 || along[0].len < full(side) - tol) approximate = true;
  }
  return { roles, approximate };
}

/** `'blank`, `'"facade.storefront"`, or a list of them. */
function roles(r: string[]): string {
  const one = (x: string) => (/^[a-z_][a-z0-9_]*$/.test(x) ? `'${x}` : `'"${x}"`);
  return r.length === 1 ? one(r[0]) : `[${r.map(one).join(", ")}]`;
}

const same = (a: string[], b: string[]) => a.length === b.length && a.every((x, i) => x === b[i]);

/**
 * A preview case as Nickel: `{ name = "room_3", width = 4, depth = 7.5, ports.left = 'facade }`.
 * Only sides that differ from the default are written; units only when they differ from metres,
 * and the seed only when it isn't 1.
 */
export function caseLiteral(c: {
  name?: string;
  width: number;
  depth: number;
  units: Units;
  system?: System;
  seed?: number;
  sides?: Record<Side, string[]>;
}): string {
  const fields: string[] = [];
  if (c.name) fields.push(`name = ${JSON.stringify(c.name)}`);
  fields.push(`width = ${formatNumber(c.width, c.units)}`, `depth = ${formatNumber(c.depth, c.units)}`);
  const changed = SIDES.filter((s) => c.sides && !same(c.sides[s], DEFAULT_SIDES[s]));
  if (changed.length === 1) fields.push(`ports.${changed[0]} = ${roles(c.sides![changed[0]])}`);
  else if (changed.length)
    fields.push(`ports = { ${changed.map((s) => `${s} = ${roles(c.sides![s])}`).join(", ")} }`);
  const system = c.system ?? (c.units === "ft" || c.units === "in" ? "imperial" : "metric");
  if (system === "imperial") fields.push("system = 'imperial");
  if (c.units !== (system === "imperial" ? "ft" : "m")) fields.push(`units = '${c.units}`);
  if (c.seed !== undefined && c.seed !== 1) fields.push(`seed = ${c.seed}`);
  return `{ ${fields.join(", ")} }`;
}
