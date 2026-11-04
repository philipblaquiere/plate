// Projects as plain data: the address bar, the order the switcher lists them in, a name for a new
// one, and what copying a file to another project takes. Kept free of the DOM so tests can use it.

import { importsOf } from "./plates";

/** Where the app is: a project and, maybe, a file in it. */
export interface Route {
  project: string;
  path: string | null;
}

/** `#/commercial/hotel/wing.ncl` */
export function formatRoute({ project, path }: Route): string {
  const segments = [project, ...(path ? path.split("/") : [])];
  return `#/${segments.map(encodeURIComponent).join("/")}`;
}

export function parseRoute(hash: string): Route | null {
  const raw = hash.replace(/^#\/?/, "");
  if (!raw) return null;
  try {
    const [project, ...rest] = raw.split("/").filter(Boolean).map(decodeURIComponent);
    return project ? { project, path: rest.length ? rest.join("/") : null } : null;
  } catch {
    return null;
  }
}

/** Most recently used first; projects never opened follow, by name. */
export function byRecent<T extends { name: string }>(projects: readonly T[], recent: readonly string[]): T[] {
  const rank = (name: string) => {
    const i = recent.indexOf(name);
    return i < 0 ? recent.length : i;
  };
  return [...projects].sort((a, b) => rank(a.name) - rank(b.name) || a.name.localeCompare(b.name));
}

/** `name` at the front of the list, without repeats, keeping it short. */
export function pushRecent(recent: readonly string[], name: string, kept = 30): string[] {
  return [name, ...recent.filter((n) => n !== name)].slice(0, kept);
}

/** A folder name from what someone typed. Empty when nothing usable is left. */
export function cleanProjectName(typed: string): string {
  return typed
    .trim()
    .replace(/[^\p{L}\p{N} ._()-]+/gu, "-")
    .replace(/^[.\s-]+|[.\s]+$/g, "")
    .slice(0, 64);
}

/** "edited 2 hours ago", from milliseconds since the epoch. */
export function timeAgo(then: number, now: number): string {
  if (!then) return "";
  const minutes = Math.floor((now - then) / 60000);
  if (minutes < 1) return "just now";
  if (minutes < 60) return `${minutes} min ago`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours} h ago`;
  const days = Math.floor(hours / 24);
  if (days < 30) return `${days} d ago`;
  return new Date(then).toLocaleDateString(undefined, { year: "numeric", month: "short", day: "numeric" });
}

export interface CopyPlan {
  /** What to write in the other project, at the same paths so imports keep working. */
  write: { path: string; text: string }[];
  /** Already there, with the same text. */
  same: string[];
  /** Already there, with different text: left alone. */
  clashes: string[];
}

/**
 * What copying a file to another project takes: the file and everything it imports, which must
 * come along, because a project can't import from another.
 */
export function planCopy(
  path: string,
  files: Record<string, string>,
  existing: Record<string, string>,
): CopyPlan {
  const plan: CopyPlan = { write: [], same: [], clashes: [] };
  for (const p of [...importsOf(path, files)].sort()) {
    const there = existing[p];
    if (there === undefined) plan.write.push({ path: p, text: files[p] });
    else if (there === files[p]) plan.same.push(p);
    else plan.clashes.push(p);
  }
  return plan;
}
