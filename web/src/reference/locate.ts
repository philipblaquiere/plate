import { deref, root as schemaRoot, type Helper, type Schema } from "../editor/schema";
import type { SourceSpan } from "../types";
import { defId, discriminator, helperId, isRecordDef, reference, type Reference } from "./model";

/**
 * What the cursor is on, as a reference entry: the field of a space, a rule kind, a helper.
 *
 * Two ways to find the path to the value under the cursor:
 *  1. The source map of the last good evaluation, inverted: the smallest value whose text
 *     contains the cursor gives a JSON pointer (`/spaces/kitchen/area`). It follows imports
 *     and `let` bindings, and knows each union's variant from the evaluated data.
 *  2. While the file doesn't evaluate (or has changed since), a scan of the text: the field
 *     names of the records and lists the cursor is inside.
 * Then the schema is walked along that path.
 */

export interface Crumb {
  label: string;
  entry: string;
  field?: string;
}

export interface Location {
  entry: string;
  /** The field of the entry the cursor is on. */
  field?: string;
  /** The enum value the cursor is on (`facade`), with its doc. */
  value?: { value: string; doc?: string };
  trail: Crumb[];
}

export interface Cursor {
  /** The whole file. */
  text: string;
  /** Character offset of the cursor. */
  offset: number;
  /** The file has a `start` (a plate), else it is a plan. */
  plate: boolean;
  /**
   * Source map and data of the last evaluation, when it still matches this file's text. `plate`
   * says whether that evaluation was of a plate (pointers start at its root) or of a plan file
   * shown on its own (pointers start at the plan); a plate when left out.
   */
  evaluated?: { file: string; sources: SourceSpan[]; config: unknown; plate?: boolean };
}

const word = /[A-Za-z_][\w-]*'*/y;

/** The identifier at an offset, with where it starts and ends. */
function wordAt(text: string, offset: number): { word: string; start: number; end: number } | undefined {
  let start = offset;
  while (start > 0 && /[\w'-]/.test(text[start - 1])) start--;
  while (start < offset && !/[A-Za-z_']/.test(text[start])) start++;
  if (text[start] === "'") start++;
  word.lastIndex = start;
  const m = word.exec(text);
  if (!m || start + m[0].length < offset) return undefined;
  return { word: m[0], start, end: start + m[0].length };
}

/** The cursor is in a `#` comment. */
function inComment(text: string, offset: number): boolean {
  const line = text.slice(text.lastIndexOf("\n", offset - 1) + 1, offset);
  let quoted = false;
  for (let i = 0; i < line.length; i++) {
    if (line[i] === "\\") i++;
    else if (line[i] === '"') quoted = !quoted;
    else if (line[i] === "#" && !quoted) return true;
  }
  return false;
}

export function locate(
  cursor: Cursor,
  ref: Reference = reference,
  root: Schema = schemaRoot,
): Location | undefined {
  const { text, offset } = cursor;
  if (inComment(text, offset)) return undefined;
  const at = wordAt(text, offset);

  // A helper name: `plate.door`.
  if (at && text.slice(Math.max(0, at.start - 6), at.start) === "plate.") {
    const h = ref.helperByName.get(at.word);
    if (h) return { entry: helperId(h.name), trail: [{ label: `plate.${h.name}`, entry: helperId(h.name) }] };
  }

  let path: Path | undefined;
  if (cursor.evaluated)
    path = fromSources(
      cursor.evaluated.sources,
      cursor.evaluated.file,
      cursor.evaluated.config,
      text,
      offset,
    );
  // Source map pointers start at what was evaluated: a plate (even in a plan it imports), or a
  // plan file shown on its own.
  const walker = new Walker(ref, root, path ? (cursor.evaluated?.plate ?? true) : cursor.plate);
  if (!path) path = fromText(text, offset, ref);
  if (path) {
    if (path.helper && !path.inRecord) {
      const h = ref.helperByName.get(path.helper);
      if (h) {
        const found = walker.walk(path.segments.slice(0, path.helperDepth), path.valueAt);
        return {
          entry: helperId(h.name),
          trail: [...(found?.location.trail ?? []), { label: `plate.${h.name}`, entry: helperId(h.name) }],
        };
      }
    }
    const found = walker.walk(path.segments, path.valueAt);
    if (found) {
      const tag = text[at ? at.start - 1 : -1] === "'" ? at?.word : undefined;
      if (tag && found.node) {
        const doc = valueDoc(found.node, tag, root);
        if (doc !== null) found.location.value = { value: tag, doc: doc || undefined };
      }
      return found.location;
    }
  }

  // Last resort: a field name anywhere in the schema.
  if (at && /^\s*(\.|\||=(?![=>]))/.test(text.slice(at.end))) {
    for (const e of ref.entries.values()) {
      if (e.kind === "guide" || e.kind === "helper") continue;
      if (e.fields.some((f) => f.name === at.word))
        return {
          entry: e.id,
          field: at.word,
          trail: [
            { label: e.title, entry: e.id },
            { label: at.word, entry: e.id, field: at.word },
          ],
        };
    }
  }
  return undefined;
}

/** The doc of an enum value the schema allows here: `""` when allowed without a doc, `null` when not allowed. */
function valueDoc(node: Schema, value: string, root: Schema, depth = 0): string | null {
  if (depth > 4) return null;
  const { node: n } = deref(node);
  const docs = n["x-enumDescriptions"] ?? {};
  if (docs[value] !== undefined) return docs[value];
  const values = n.enum ?? (n.const !== undefined ? [n.const] : []);
  if (values.map(String).includes(value)) return "";
  if (n.pattern && Object.keys(docs).length) return docs[value.split(".")[0]] ?? "";
  for (const alt of [...(n.anyOf ?? []), ...(n.oneOf ?? [])]) {
    const d = valueDoc(alt, value, root, depth + 1);
    if (d !== null) return d;
  }
  if (n.items && typeof n.items === "object") return valueDoc(n.items, value, root, depth + 1);
  return null;
}

// ------------------------------------------------------------------ the path to the cursor

interface Path {
  /** Field names and list indexes, from the top of the configuration. */
  segments: string[];
  /** The value (or what's known of it: its tags) at the path's first `depth` segments. */
  valueAt(depth: number): unknown;
  /** The cursor is in a helper call (`plate.door "a" "b"`), not in a record merged into it. */
  helper?: string;
  helperDepth?: number;
  inRecord?: boolean;
}

const unescape = (s: string) => s.replace(/~1/g, "/").replace(/~0/g, "~");

function contains(s: SourceSpan, line: number, col: number): boolean {
  const { startLine, startCol, endLine, endCol } = s.span;
  if (line < startLine || line > endLine) return false;
  if (line === startLine && col < startCol) return false;
  if (line === endLine && col > endCol) return false;
  return true;
}

function size(s: SourceSpan): number {
  return (s.span.endLine - s.span.startLine) * 10000 + (s.span.endCol - s.span.startCol);
}

/** 1-based line and column of an offset. */
function lineCol(text: string, offset: number): { line: number; col: number } {
  let line = 1;
  let last = -1;
  for (let i = 0; i < offset; i++) if (text.charCodeAt(i) === 10) (line++, (last = i));
  return { line, col: offset - last };
}

function offsetOf(text: string, line: number, col: number): number {
  let offset = 0;
  for (let l = 1; l < line; l++) {
    const next = text.indexOf("\n", offset);
    if (next < 0) return text.length;
    offset = next + 1;
  }
  return offset + col - 1;
}

/** The value written at the smallest span containing the cursor (the source map, inverted). */
export function pointerAt(
  sources: SourceSpan[],
  file: string,
  line: number,
  col: number,
): SourceSpan | undefined {
  let best: SourceSpan | undefined;
  for (const s of sources) {
    if (s.file !== file || !contains(s, line, col)) continue;
    if (!best || size(s) < size(best) || (size(s) === size(best) && s.pointer.length > best.pointer.length))
      best = s;
  }
  return best;
}

/** The field name being defined at the cursor (`area` in `area.min = 24`), as segments up to the cursor. */
function keyAt(text: string, offset: number): string[] | undefined {
  const at = wordAt(text, offset);
  if (!at || text[at.start - 1] === "'" || text[at.start - 1] === '"') return undefined;
  // The whole dotted name, and the `=` (or `|` annotation) after it.
  let start = at.start;
  while (text[start - 1] === "." && /[\w']/.test(text[start - 2] ?? "")) {
    let s = start - 1;
    while (s > 0 && /[\w'-]/.test(text[s - 1])) s--;
    start = s;
  }
  const after = /^[\w.'-]*\s*(\||=(?![=>]))/.exec(text.slice(at.start));
  if (!after) return undefined;
  const before = text.slice(0, start).trimEnd();
  if (before && !/[{,;]$/.test(before) && !/\b(in|let)$/.test(before)) return undefined;
  if (/\blet$/.test(before)) return undefined;
  return text.slice(start, at.end).split(".");
}

function fromSources(
  sources: SourceSpan[],
  file: string,
  config: unknown,
  text: string,
  offset: number,
): Path | undefined {
  const { line, col } = lineCol(text, offset);
  const hit = pointerAt(sources, file, line, col);
  if (!hit) return undefined;
  const segments = hit.pointer.split("/").slice(1).map(unescape);
  // On a field name inside the record: that field. (Only names written inside its braces.)
  const key = hit.kind === "record" ? keyAt(text, offset) : undefined;
  if (key && offsetOf(text, hit.span.startLine, hit.span.startCol) < offset) segments.push(...key);
  return {
    segments,
    valueAt: (depth) => {
      let v = config;
      for (const part of segments.slice(0, depth)) {
        if (v === null || typeof v !== "object") return undefined;
        v = (v as Record<string, unknown>)[part];
      }
      return v;
    },
  };
}

// ------------------------------------------------------------------ the text, scanned

interface Token {
  type: "ident" | "tag" | "string" | "number" | "punct";
  text: string;
  start: number;
  end: number;
}

/** Nickel tokens, skipping comments; enough to follow records and lists. */
export function tokenize(text: string): Token[] {
  const out: Token[] = [];
  let i = 0;
  while (i < text.length) {
    const c = text[i];
    if (c === "#") {
      while (i < text.length && text[i] !== "\n") i++;
    } else if (/\s/.test(c)) {
      i++;
    } else if (c === '"' || (c === "m" && /^m(%+)"/.test(text.slice(i, i + 8)))) {
      const start = i;
      if (c === "m") {
        const percents = /^m(%+)"/.exec(text.slice(i))![1];
        const close = `"${percents}`;
        const end = text.indexOf(close, i + percents.length + 2);
        i = end < 0 ? text.length : end + close.length;
      } else {
        i++;
        while (i < text.length && text[i] !== '"' && text[i] !== "\n") i += text[i] === "\\" ? 2 : 1;
        i++;
      }
      out.push({
        type: "string",
        text: text.slice(start, Math.min(i, text.length)),
        start,
        end: Math.min(i, text.length),
      });
    } else if (c === "'" && /[A-Za-z_"]/.test(text[i + 1] ?? "")) {
      const m = /^'(?:[A-Za-z_][\w'-]*|"[^"]*")/.exec(text.slice(i))!;
      out.push({ type: "tag", text: m[0].slice(1).replace(/"/g, ""), start: i, end: i + m[0].length });
      i += m[0].length;
    } else if (/[A-Za-z_]/.test(c)) {
      const m = /^[A-Za-z_][\w'-]*/.exec(text.slice(i))!;
      out.push({ type: "ident", text: m[0], start: i, end: i + m[0].length });
      i += m[0].length;
    } else if (/[\d]/.test(c)) {
      const m = /^\d[\d._eE]*/.exec(text.slice(i))!;
      out.push({ type: "number", text: m[0], start: i, end: i + m[0].length });
      i += m[0].length;
    } else {
      const two = text.slice(i, i + 2);
      const p = ["=>", "==", "!=", "<=", ">=", "&&", "||", "|>", "..", "++"].includes(two) ? two : c;
      out.push({ type: "punct", text: p, start: i, end: i + p.length });
      i += p.length;
    }
  }
  return out;
}

interface Field {
  key: string[];
  keyStart: number;
  keyEnd: number;
  end: number;
  /** The value is just a tag: `mode = 'boundary`. */
  tag?: string;
  /** Tokens in the value so far (to spot a lone tag). */
  tokens: number;
}

interface Item {
  start: number;
  end: number;
  helper?: string;
}

interface Frame {
  open: "{" | "[" | "(" | "top";
  start: number;
  end: number;
  /** Path from the parent frame to this one (`["spaces"]`, `["2"]`); `null` when unknown (a `let`). */
  segs: string[] | null;
  parent?: Frame;
  /** Records: the fields; the one being written last. */
  fields: Field[];
  expectKey: boolean;
  pendingKey: { parts: string[]; start: number; end: number } | null;
  /** Lists and parentheses: items, separated by commas. */
  items: Item[];
  lets: number;
  children: Frame[];
}

function frame(open: Frame["open"], start: number, parent?: Frame, segs: string[] | null = []): Frame {
  return {
    open,
    start,
    end: Infinity,
    segs,
    parent,
    fields: [],
    expectKey: open === "{",
    pendingKey: null,
    items: open === "{" ? [] : [{ start: start + 1, end: Infinity }],
    lets: 0,
    children: [],
  };
}

/** The tree of records, lists and parentheses in a Nickel file. */
export function scan(text: string): Frame {
  const top = frame("top", -1);
  let cur = top;
  const tokens = tokenize(text);
  for (let k = 0; k < tokens.length; k++) {
    const t = tokens[k];
    const field = cur.open === "{" && !cur.expectKey ? cur.fields[cur.fields.length - 1] : undefined;
    if (field && !"}],".includes(t.text)) {
      field.tokens++;
      field.tag = field.tokens === 1 && t.type === "tag" ? t.text : undefined;
    }
    if (t.type === "ident" && t.text === "let") {
      cur.lets++;
      continue;
    }
    if (t.type === "ident" && t.text === "in" && cur.lets > 0) {
      cur.lets--;
      continue;
    }
    // Record keys: `name`, `area.min`, `"quoted key"`, then annotations, then `=`.
    if (cur.open === "{" && cur.expectKey && cur.lets === 0) {
      if (t.type === "ident" || t.type === "string") {
        const part = t.type === "string" ? t.text.replace(/^"|"$/g, "") : t.text;
        if (cur.pendingKey && tokens[k - 1]?.text === ".") {
          cur.pendingKey.parts.push(part);
          cur.pendingKey.end = t.end;
        } else if (!cur.pendingKey) {
          cur.pendingKey = { parts: [part], start: t.start, end: t.end };
        }
        continue;
      }
      if (t.text === "=" && cur.pendingKey) {
        cur.fields.push({
          key: cur.pendingKey.parts,
          keyStart: cur.pendingKey.start,
          keyEnd: cur.pendingKey.end,
          end: Infinity,
          tokens: 0,
        });
        cur.pendingKey = null;
        cur.expectKey = false;
        continue;
      }
    }
    if (
      t.type === "ident" &&
      t.text === "plate" &&
      tokens[k + 1]?.text === "." &&
      tokens[k + 2]?.type === "ident"
    ) {
      const item = cur.items[cur.items.length - 1];
      if (item && !item.helper && cur.open !== "{") item.helper = tokens[k + 2].text;
    }
    if (t.text === "{" || t.text === "[" || t.text === "(") {
      let segs: string[] | null = [];
      if (cur.lets > 0) segs = null;
      else if (cur.open === "{")
        segs = cur.expectKey ? null : (cur.fields[cur.fields.length - 1]?.key ?? null);
      else if (cur.open === "[") segs = [String(cur.items.length - 1)];
      const child = frame(t.text as Frame["open"], t.start, cur, segs);
      cur.children.push(child);
      cur = child;
      continue;
    }
    if ((t.text === "}" || t.text === "]" || t.text === ")") && cur.parent) {
      cur.end = t.start;
      const last = cur.fields[cur.fields.length - 1];
      if (last && last.end === Infinity) last.end = t.start;
      const item = cur.items[cur.items.length - 1];
      if (item) item.end = t.start;
      cur = cur.parent;
      continue;
    }
    if (t.text === "," && cur.lets === 0) {
      if (cur.open === "{") {
        const last = cur.fields[cur.fields.length - 1];
        if (last && last.end === Infinity) last.end = t.start;
        cur.expectKey = true;
        cur.pendingKey = null;
      } else {
        cur.items[cur.items.length - 1].end = t.start;
        cur.items.push({ start: t.end, end: Infinity });
      }
    }
  }
  return top;
}

function fromText(text: string, offset: number, ref: Reference): Path | undefined {
  // The frames around the cursor, outermost first.
  const chain: Frame[] = [];
  let f: Frame | undefined = scan(text);
  while (f) {
    chain.push(f);
    f = f.children.find((c) => c.start < offset && offset <= c.end);
  }
  const segments: string[] = [];
  const values = new Map<number, unknown>();
  // What is known of a value: the tags in a record (`mode = 'boundary`), the rule a helper writes.
  const know = (depth: number, v: unknown) => {
    if (v === undefined) return;
    const old = values.get(depth);
    const merge =
      old &&
      typeof old === "object" &&
      !Array.isArray(old) &&
      v &&
      typeof v === "object" &&
      !Array.isArray(v);
    values.set(depth, merge ? { ...v, ...old } : (old ?? v));
  };
  const helperValue = (name: string): unknown => {
    const h: Helper | undefined = ref.helperByName.get(name);
    if (!h?.rules.length) return undefined;
    return h.list ? h.rules.map((rule) => ({ rule })) : { rule: h.rules[0] };
  };
  let helper: string | undefined;
  let helperDepth = 0;
  let inRecord = false;

  chain.forEach((fr, idx) => {
    const inner = chain[idx + 1];
    if (fr.segs === null) return;
    segments.push(...fr.segs);
    if (fr.open === "{") {
      const tags = fr.fields.filter((x) => x.tag && x.key.length === 1).map((x) => [x.key[0], x.tag]);
      know(segments.length, Object.fromEntries(tags));
      inRecord = true;
      return;
    }
    // A list item or a parenthesised call: a helper call gives the rule it writes.
    const at = fr.items.findIndex((it) => it.start <= offset && offset <= it.end);
    const item = fr.items[at];
    if (!item) return;
    const depth = segments.length + (fr.open === "[" ? 1 : 0);
    if (item.helper) know(depth, helperValue(item.helper));
    if (item.helper && (!inner || inner.open === "(")) {
      helper = item.helper;
      helperDepth = depth;
      inRecord = false;
    }
    if (fr.open === "[" && !inner) segments.push(String(at));
  });
  if (chain.some((fr) => fr.segs === null)) return undefined; // inside a `let` value: no path from the top
  if (chain.length === 1) return undefined; // outside the file's record

  // In a record: the field the cursor is on (up to the part under it, in `area.min`).
  const last = chain[chain.length - 1];
  if (last.open === "{") {
    const field = last.fields.find((x) => x.keyStart <= offset && offset <= x.end);
    if (field) {
      let upto = field.keyStart;
      for (const part of field.key) {
        segments.push(part);
        upto += part.length + 1;
        if (offset <= field.keyEnd && offset < upto) break;
      }
    }
  }
  return { segments, valueAt: (d) => values.get(d), helper, helperDepth, inRecord };
}

// ------------------------------------------------------------------ the schema, walked

class Walker {
  constructor(
    private readonly ref: Reference,
    private readonly root: Schema,
    private readonly plate: boolean,
  ) {}

  /** Pick the variant of a union that a value is (by its tag, else by its shape). */
  private resolve(raw: Schema, value: unknown, next?: string): { node: Schema; name?: string } {
    let { node, name } = deref(raw);
    for (let i = 0; i < 4; i++) {
      const alts = (node.oneOf ?? node.anyOf ?? []).map((a) => deref(a));
      if (!alts.length) break;
      const key = discriminator(alts.map((a) => a.node));
      const v = value as Record<string, unknown> | undefined;
      let pick =
        key && v && typeof v === "object" && !Array.isArray(v) && v[key] !== undefined
          ? alts.find((a) => String(deref(a.node.properties![key]).node.const) === String(v[key]))
          : undefined;
      if (!pick && value !== undefined) {
        const shape = Array.isArray(value)
          ? "array"
          : typeof value === "object" && value
            ? "object"
            : typeof value;
        pick = alts.find((a) => {
          const t =
            a.node.type ??
            (a.node.properties || a.node.oneOf
              ? "object"
              : a.node.items || a.node.prefixItems
                ? "array"
                : undefined);
          return (
            t === shape ||
            (shape === "number" && t === "integer") ||
            (shape === "object" && isRecordDef(a.node))
          );
        });
      }
      if (!pick && next !== undefined) {
        pick = /^\d+$/.test(next)
          ? alts.find((a) => a.node.type === "array" || a.node.items)
          : alts.find((a) => a.node.properties?.[next] || (a.node.oneOf ?? []).length);
      }
      if (!pick) break;
      node = pick.node;
      name = pick.name ?? name;
    }
    return { node, name };
  }

  walk(
    segments: string[],
    valueAt: (depth: number) => unknown,
  ): { location: Location; node: Schema } | undefined {
    let raw: Schema = this.plate ? this.root : { $ref: "#/$defs/Plan" };
    let entry = this.plate ? defId("#") : (this.ref.entryFor("Plan") ?? defId("Plan"));
    let field: string | undefined;
    const trail: Crumb[] = [{ label: this.plate ? "Plate" : "Plan", entry }];
    let viaIndex = false;
    for (let i = 0; i <= segments.length; i++) {
      const seg = segments[i];
      const { node, name } = this.resolve(raw, valueAt(i), seg);
      // Records (and unions of records) have pages of their own; values are explained by their field.
      // (The definition, not the alternative picked: a range written `{ min, max }` is still a value.)
      const def = name ? this.root.$defs?.[name] : undefined;
      const id = i > 0 && name && def && isRecordDef(def) ? this.ref.entryFor(name) : undefined;
      if (id && id !== entry) {
        entry = id;
        field = undefined;
        const e = this.ref.entries.get(id)!;
        const last = trail[trail.length - 1];
        if (viaIndex) {
          trail.push({ label: e.title, entry: id });
        } else {
          last.entry = id;
          last.field = undefined;
          if (e.kind === "variant") last.label += ` (${e.title})`;
        }
      }
      if (seg === undefined) return { location: { entry, field, trail }, node };
      viaIndex = false;
      const fields = this.ref.entries.get(entry)?.fields ?? [];
      if (node.properties?.[seg]) {
        // A field of the page we're on is highlighted there; deeper ones keep the field they're in.
        const row = fields.find((x) => x.name === seg);
        if (row) field = seg;
        trail.push(row?.typeLink ? { label: seg, entry: row.typeLink } : { label: seg, entry, field });
        raw = node.properties[seg];
      } else if (node.additionalProperties && typeof node.additionalProperties === "object") {
        trail.push({ label: seg, entry, field });
        raw = node.additionalProperties;
      } else if (node.prefixItems || node.items) {
        viaIndex = true;
        raw = node.prefixItems?.[Number(seg)] ?? (typeof node.items === "object" ? node.items : {});
      } else {
        return { location: { entry, field, trail }, node };
      }
    }
    return undefined;
  }
}
