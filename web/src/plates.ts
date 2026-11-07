// What a workspace file is, and what it imports. Kept free of the editor so tests can use it.

/** Is this file a plate (it has a `start`), as opposed to a plan or library to import? */
export function isPlate(source: string): boolean {
  return /^\s*start\s*=/m.test(source);
}

/**
 * Is this file a plan (spaces, needs or a preview at the top), which can be shown on its own, as
 * opposed to a plate or a library of bits to import?
 */
export function isPlan(source: string): boolean {
  return !isPlate(source) && /^[\s{]*(spaces|needs|preview)\s*[=.|]/m.test(source);
}

/**
 * Is this file a library of the engine's own kinds: a service (it has a service's fields at the
 * top) or a set of assemblies (wall types, each with a thickness or layers)? Such a file is
 * checked and described on its own, though there's nothing to draw.
 */
export function libraryKind(source: string): "service" | "assemblies" | null {
  if (isPlate(source) || isPlan(source)) return null;
  if (/^[\s{]*(parts|place|connect|claims|scope|params)\s*[=.|]/m.test(source)) return "service";
  if (/^[\s{]*[a-z_][a-z0-9_]*\s*=\s*\{[^]*?\b(thickness|layers)\s*=/m.test(source)) return "assemblies";
  return null;
}

/** The plates that import `path`, directly or not. */
export function importersOf(path: string, files: Record<string, string>): string[] {
  return Object.keys(files)
    .filter((p) => p !== path && isPlate(files[p]) && importsOf(p, files).has(path))
    .sort();
}

/**
 * The files a file imports, directly or not (itself included). Imports are resolved like Nickel
 * does: relative to the importing file's folder. Only string-literal imports are seen, which is
 * how imports are written.
 */
export function importsOf(path: string, files: Record<string, string>): Set<string> {
  const seen = new Set<string>();
  const visit = (p: string) => {
    if (seen.has(p) || files[p] === undefined) return;
    seen.add(p);
    const dir = p.includes("/") ? p.slice(0, p.lastIndexOf("/") + 1) : "";
    for (const m of files[p].matchAll(IMPORT)) {
      if (!inComment(files[p], m.index)) visit(normalize(dir + m[2]));
    }
  };
  visit(path);
  return seen;
}

/** The files a file imports itself (not through another), that exist, sorted. */
export function directImports(path: string, files: Record<string, string>): string[] {
  const text = files[path];
  if (text === undefined) return [];
  const dir = folderOf(path);
  const found = new Set<string>();
  for (const m of text.matchAll(IMPORT)) {
    if (inComment(text, m.index)) continue;
    const target = normalize(dir + m[2]);
    if (files[target] !== undefined && target !== path) found.add(target);
  }
  return [...found].sort();
}

/** The files that import this one themselves (not through another), sorted. */
export function directImporters(path: string, files: Record<string, string>): string[] {
  return Object.keys(files)
    .filter((p) => p !== path && directImports(p, files).includes(path))
    .sort();
}

const IMPORT = /(\bimport\s+")([^"]+)"/g;

/** Is position `at` inside a `#` comment? (A `#` outside a string, earlier on the line.) */
function inComment(text: string, at: number): boolean {
  let quoted = false;
  for (let i = text.lastIndexOf("\n", at - 1) + 1; i < at; i++) {
    if (text[i] === "\\") i++;
    else if (text[i] === '"') quoted = !quoted;
    else if (text[i] === "#" && !quoted) return true;
  }
  return false;
}

function normalize(path: string): string {
  const out: string[] = [];
  for (const part of path.split("/")) {
    if (part === "..") out.pop();
    else if (part && part !== ".") out.push(part);
  }
  return out.join("/");
}

/** The import string that reaches `target` from a file in folder `dir` ("" for the top). */
export function importPath(dir: string, target: string): string {
  const from = dir.split("/").filter(Boolean);
  const to = target.split("/");
  let common = 0;
  while (common < from.length && common < to.length - 1 && from[common] === to[common]) common++;
  return [...Array(from.length - common).fill(".."), ...to.slice(common)].join("/");
}

/**
 * A file's text with its imports pointed at where things are after a move (`moved`), or `null`
 * when nothing changes. The file itself may be the one that moved (`oldPath` → `newPath`), in
 * which case its own relative imports are rewritten from its new folder. Imports of files that
 * don't exist (`exists`) are left as written.
 */
export function retargetImports(
  text: string,
  oldPath: string,
  newPath: string,
  moved: { from: string; to: string },
  exists: (path: string) => boolean,
): string | null {
  const oldDir = folderOf(oldPath);
  const newDir = folderOf(newPath);
  let changed = false;
  const out = text.replace(IMPORT, (whole, head: string, spec: string, at: number) => {
    if (inComment(text, at)) return whole;
    let target = normalize(oldDir + spec);
    if (target === moved.from) target = moved.to;
    if (!exists(target)) return whole;
    const next = importPath(newDir, target);
    if (next === spec) return whole;
    changed = true;
    return `${head}${next}"`;
  });
  return changed ? out : null;
}

function folderOf(path: string): string {
  return path.includes("/") ? path.slice(0, path.lastIndexOf("/") + 1) : "";
}
