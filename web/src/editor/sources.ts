import type { SourceSpan, Span } from "../types";

/**
 * The text a configuration value comes from, for a JSON pointer: the value itself, else the
 * values inside it (a rule written with a helper, `plate.door "a" "b"`, has no literal of its own
 * in the file, but its arguments do), else the closest enclosing value.
 */
export interface Located {
  /** The workspace file the text is in. */
  file: string;
  span: Span;
}

export function spanFor(pointer: string, sources: SourceSpan[]): Located | undefined {
  const exact = sources.find((s) => s.pointer === pointer);
  if (exact) return { file: exact.file, span: exact.span };
  const within = sources.filter((s) => s.pointer.startsWith(`${pointer}/`));
  // Values inside may come from several files (a plan imported into a plate): use the file of
  // the first one written, and only its spans.
  const file = within[0]?.file;
  const inside = within.filter((s) => s.file === file).map((s) => s.span);
  if (file !== undefined && inside.length) {
    const before = (a: Span, b: Span) =>
      a.startLine < b.startLine || (a.startLine === b.startLine && a.startCol <= b.startCol);
    const first = inside.reduce((a, b) => (before(a, b) ? a : b));
    const last = inside.reduce((a, b) =>
      a.endLine > b.endLine || (a.endLine === b.endLine && a.endCol >= b.endCol) ? a : b,
    );
    return {
      file,
      span: {
        startLine: first.startLine,
        startCol: first.startCol,
        endLine: last.endLine,
        endCol: last.endCol,
      },
    };
  }
  let p = pointer;
  while (p.includes("/")) {
    p = p.slice(0, p.lastIndexOf("/"));
    const s = sources.find((x) => x.pointer === p);
    if (s) return { file: s.file, span: s.span };
  }
  return undefined;
}

/** The value at a JSON pointer. */
export function valueAt(root: unknown, pointer: string): unknown {
  let v = root;
  for (const part of pointer.split("/").slice(1)) {
    if (v === null || typeof v !== "object") return undefined;
    v = (v as Record<string, unknown>)[part.replace(/~1/g, "/").replace(/~0/g, "~")];
  }
  return v;
}

/** Rules of a plan with their pointers, flattening lists (`/rules/3`, `/rules/4/1`). */
export function rulesOf(plan: unknown): { pointer: string; rule: Record<string, unknown> }[] {
  const rules = (plan as { rules?: unknown[] } | undefined)?.rules ?? [];
  const out: { pointer: string; rule: Record<string, unknown> }[] = [];
  rules.forEach((r, i) => {
    if (Array.isArray(r)) r.forEach((x, j) => out.push({ pointer: `/rules/${i}/${j}`, rule: x }));
    else out.push({ pointer: `/rules/${i}`, rule: r as Record<string, unknown> });
  });
  return out;
}
