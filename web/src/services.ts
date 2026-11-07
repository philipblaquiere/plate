// What a solved plate's services placed, for the views above the drawing and their legend. Kept
// free of the DOM so tests can use it.

import type { Circuit, Diagnostic, Layout, Load } from "./types";

export interface PartCount {
  part: string;
  label: string;
  symbol?: string;
  count: number;
}

export interface NetworkCount {
  name: string;
  /** Groups it was cut into (circuits); 0 when it has none. */
  groups: number;
}

export interface ServiceSummary {
  name: string;
  label: string;
  parts: PartCount[];
  networks: NetworkCount[];
  /** Requirements of this service not met. */
  issues: number;
}

/** The service a pointer is about: `…/services/<name>/…`, or null. */
export function serviceOf(pointer: string | undefined): string | null {
  const m = pointer?.match(/\/services\/([^/]+)(\/|$)/);
  return m ? m[1].replace(/~1/g, "/").replace(/~0/g, "~") : null;
}

/** Every service the layout has parts or runs of (or problems with), in the order of first
 * appearance. `labels` names them as the file does. */
export function summarize(
  layout: Layout | null,
  diagnostics: Diagnostic[],
  labels: Record<string, string> = {},
): ServiceSummary[] {
  const out = new Map<string, ServiceSummary>();
  const get = (name: string) => {
    let s = out.get(name);
    if (!s) {
      s = { name, label: labels[name] ?? title(name), parts: [], networks: [], issues: 0 };
      out.set(name, s);
    }
    return s;
  };
  const groups = new Map<string, Set<string>>();
  for (const e of layout?.elements ?? []) {
    if (e.part) {
      const s = get(e.part.service);
      const p = s.parts.find((x) => x.part === e.part!.part);
      if (p) p.count++;
      else s.parts.push({ part: e.part.part, label: e.part.label, symbol: e.symbol?.name, count: 1 });
    } else if (e.run) {
      const s = get(e.run.service);
      if (!s.networks.some((n) => n.name === e.run!.network))
        s.networks.push({ name: e.run.network, groups: 0 });
      if (e.run.group) {
        // A group is per network and per scope (each flat's circuit 1 is its own).
        const scope = e.id.split(":")[3]?.split("#")[0] ?? "";
        const key = `${e.run.service}\u0000${e.run.network}`;
        const set = groups.get(key) ?? new Set<string>();
        set.add(`${scope}\u0000${e.run.group}`);
        groups.set(key, set);
      }
    }
  }
  for (const s of out.values()) {
    for (const n of s.networks) n.groups = groups.get(`${s.name}\u0000${n.name}`)?.size ?? 0;
  }
  for (const d of diagnostics) {
    const name = serviceOf(d.pointer);
    if (name && d.severity !== "info") get(name).issues++;
  }
  return [...out.values()];
}

/** The next view after `current` (null is the plan), for a key that cycles through them. */
export function nextView(current: string | null, names: string[]): string | null {
  if (!names.length) return null;
  if (current === null) return names[0];
  const i = names.indexOf(current);
  return i < 0 || i === names.length - 1 ? null : names[i + 1];
}

/** A load in words: "30 A at 240 V", "60 W". */
export function loadText(l: Load): string {
  const v = l.voltage ?? 120;
  if (l.current !== undefined) return `${num(l.current)} A at ${num(v)} V`;
  if (l.watts !== undefined) return `${num(l.watts)} W at ${num(v)} V`;
  return `${num(v)} V`;
}

/** One row of a panel schedule. */
export interface ScheduleRow {
  circuit: string;
  network: string;
  breaker: string;
  medium: string;
  devices: string;
  load: string;
  length: number;
}

/** A service's circuits in one scope (the first that has any, unless named), as a panel
 * schedule: one row per circuit, in number order. */
export function schedule(
  circuits: Circuit[] | undefined,
  service: string,
  scope?: string,
): { scope: string; scopes: string[]; rows: ScheduleRow[] } {
  const mine = (circuits ?? []).filter((c) => c.service === service);
  const scopes = [...new Set(mine.map((c) => c.scope))];
  const at = scope !== undefined && scopes.includes(scope) ? scope : (scopes[0] ?? "");
  const rows = mine
    .filter((c) => c.scope === at)
    .sort((a, b) => Number(a.group) - Number(b.group) || a.group.localeCompare(b.group))
    .map((c) => ({
      circuit: c.group,
      network: c.network,
      breaker: c.rating !== undefined ? `${num(c.rating)} A${c.voltage && c.voltage > 150 ? " 2P" : ""}` : "",
      medium: c.medium ?? "",
      devices: c.devices.map(([d, n]) => (n > 1 ? `${d} ×${n}` : d)).join(", "),
      load: c.load > 0 ? `${num(c.load)} A` : "",
      length: c.length,
    }));
  return { scope: at, scopes, rows };
}

function num(v: number): string {
  return String(Math.round(v * 10) / 10);
}

function title(name: string): string {
  const s = name.replace(/[_-]+/g, " ");
  return s.charAt(0).toUpperCase() + s.slice(1);
}
