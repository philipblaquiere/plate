// What the file tree shows, as plain data: which rows, in what order, how deep. Kept free of the
// DOM so tests can use it.

export type TreeRow =
  | { kind: "folder"; path: string; depth: number; collapsed: boolean }
  | { kind: "file"; path: string; depth: number };

/** Folders before files at each level, then by name, like VS Code's explorer. */
export function treeOrder(a: string, b: string): number {
  const pa = a.split("/");
  const pb = b.split("/");
  for (let i = 0; i < Math.min(pa.length, pb.length); i++) {
    if (pa[i] === pb[i]) continue;
    const dirA = i < pa.length - 1;
    const dirB = i < pb.length - 1;
    if (dirA !== dirB) return dirA ? -1 : 1;
    return pa[i].localeCompare(pb[i]);
  }
  return pa.length - pb.length;
}

/** The folders a path is inside, outermost first: `a/b/c.ncl` gives `a`, `a/b`. */
export function parentsOf(path: string): string[] {
  const parts = path.split("/");
  return parts.slice(0, -1).map((_, i) => parts.slice(0, i + 1).join("/"));
}

/**
 * The rows of the tree. Files listed in `pinned` are left out (they're shown above the tree), and
 * so is a folder they leave empty. Empty folders sort among the others, as a file in them would.
 * With a `filter`, only rows that match show, every folder open, so nothing found stays hidden.
 */
export function treeRows(
  paths: readonly string[],
  folders: readonly string[],
  collapsed: ReadonlySet<string>,
  options: { pinned?: ReadonlySet<string>; filter?: (path: string) => boolean } = {},
): TreeRow[] {
  const { pinned, filter } = options;
  const shown = paths.filter((p) => !pinned?.has(p) && (!filter || filter(p)));
  const holding = new Set<string>();
  for (const p of paths) for (const dir of parentsOf(p)) holding.add(dir);
  const empty = filter ? [] : folders.filter((f) => !holding.has(f)).map((f) => `${f}/`);

  const rows: TreeRow[] = [];
  const seen = new Set<string>();
  for (const entry of [...shown, ...empty].sort(treeOrder)) {
    const isFolder = entry.endsWith("/");
    const path = isFolder ? entry.slice(0, -1) : entry;
    const dirs = isFolder ? [...parentsOf(path), path] : parentsOf(path);
    let hidden = false;
    for (const [depth, dir] of dirs.entries()) {
      if (!hidden && !seen.has(dir)) {
        seen.add(dir);
        rows.push({ kind: "folder", path: dir, depth, collapsed: !filter && collapsed.has(dir) });
      }
      if (!filter && collapsed.has(dir)) hidden = true;
    }
    if (!isFolder && !hidden) rows.push({ kind: "file", path, depth: dirs.length });
  }
  return rows;
}
