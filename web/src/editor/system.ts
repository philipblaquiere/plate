import type { System } from "../types";

// Source edits behind the Metric/Imperial switch. Kept separate from the UI so they're easy to
// test and to reuse from the upcoming setup flow.

/** Set (or insert) the top-level `system` field, pinning `units` if the file relies on the default. */
export function withSystem(source: string, system: System, currentUnits?: string): string {
  const systemRe = /^([ \t]*)system\s*=\s*'(metric|imperial)\b/m;
  let out: string;
  if (systemRe.test(source)) {
    out = source.replace(systemRe, (_m, indent: string) => `${indent}system = '${system}`);
  } else {
    out = insertField(source, `system = '${system},`);
  }
  if (currentUnits && !/^\s*units\s*=/m.test(out)) {
    out = insertField(out, `units = '${currentUnits},`, /^([ \t]*)system\s*=.*$/m);
  }
  return out;
}

/** Insert a top-level field line after `after` (if found), else after `name = …`, else after the first `{`. */
function insertField(source: string, line: string, after?: RegExp): string {
  const anchors = [after, /^([ \t]*)name\s*=.*$/m].filter(Boolean) as RegExp[];
  for (const re of anchors) {
    const m = re.exec(source);
    if (m) {
      const end = m.index + m[0].length;
      return `${source.slice(0, end)}\n${m[1]}${line}${source.slice(end)}`;
    }
  }
  const brace = /^\{[ \t]*$/m.exec(source) ?? /\{/.exec(source);
  if (!brace) return source;
  const end = brace.index + brace[0].length;
  return `${source.slice(0, end)}\n  ${line}${source.slice(end)}`;
}
