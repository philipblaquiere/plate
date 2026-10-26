import { scan } from "./locate";

/**
 * Where an Insert from the reference goes, so it lands where it means something:
 *
 * - a field (`aspect = 1.6,`) goes into the record the reference page is about: the record
 *   around the cursor, or the record the cursor's field holds (`stair = { … }`);
 * - a rule (`plate.door "a" "b"`) goes into the list around the cursor, after the current item;
 * - anything else goes at the cursor, on a line of its own when the cursor ends a line.
 *
 * Returns the offset to insert at and the text, already indented (tab stops kept).
 */

export interface Placement {
  offset: number;
  text: string;
  /** For a field the record already has: where it is, instead of inserting. */
  existing?: { start: number; end: number };
  /** `text` replaces the text from `offset` to here. */
  end?: number;
}

export type InsertKind =
  | {
      field: string;
      /** The fields of the page's record: a record that is one of their values is inside it. */
      pageFields: string[];
      /** When the field is already there, replace its value (a tag, string or number) instead
       * of pointing at it. */
      replace?: boolean;
    }
  | "item"
  | "text";

const lineStart = (text: string, offset: number) => text.lastIndexOf("\n", offset - 1) + 1;
const indentAt = (text: string, offset: number) => /^[ \t]*/.exec(text.slice(lineStart(text, offset)))![0];

/** Re-indent a snippet (tabs mark nesting) under a base indentation. */
function indented(snippet: string, base: string, unit = "  "): string {
  return snippet
    .split("\n")
    .map((line, i) => {
      const tabs = /^\t*/.exec(line)![0].length;
      return (i ? base : "") + unit.repeat(tabs) + line.slice(tabs);
    })
    .join("\n");
}

export function place(text: string, offset: number, snippet: string, kind: InsertKind): Placement {
  if (kind === "item") return placeItem(text, offset, snippet) ?? placeText(text, offset, snippet);
  if (kind !== "text") return placeField(text, offset, snippet, kind) ?? placeText(text, offset, snippet);
  return placeText(text, offset, snippet);
}

export function placeText(text: string, offset: number, snippet: string): Placement {
  const start = lineStart(text, offset);
  const end = text.indexOf("\n", offset) < 0 ? text.length : text.indexOf("\n", offset);
  const before = text.slice(start, offset);
  const after = text.slice(offset, end);
  const indent = indentAt(text, offset);
  if (!before.trim() && !after.trim()) {
    return { offset, text: indented(snippet, indent) };
  }
  if (!after.trim()) {
    return { offset: end, text: `\n${indent}${indented(snippet, indent)}` };
  }
  return { offset, text: indented(snippet, indent) + (/\s/.test(after[0]) ? "" : " ") };
}

function placeItem(text: string, offset: number, snippet: string): Placement | undefined {
  const top = scan(text);
  // The innermost list around the cursor.
  let list: ReturnType<typeof scan> | undefined;
  for (let f: ReturnType<typeof scan> | undefined = top; f;) {
    if (f.open === "[") list = f;
    f = f.children.find((c) => c.start < offset && offset <= c.end);
  }
  if (!list) return undefined;
  const item = snippet.trimEnd().endsWith(",") ? snippet.trimEnd() : `${snippet},`;
  const items = list.items;
  const i = Math.max(
    0,
    items.findIndex((it) => it.start <= offset && offset <= it.end),
  );
  const current = items[i];
  const multiline = text.slice(list.start, list.end).includes("\n");
  if (!multiline) {
    // `[a, b]`: after the current item, on the same line.
    const end = current.end === Infinity ? list.end : current.end;
    const hasComma = text[end] === ",";
    const empty = !text.slice(list.start + 1, list.end).trim();
    if (empty) return { offset: list.start + 1, text: item.replace(/,$/, "") };
    const contentEnd = current.start + text.slice(current.start, end).trimEnd().length;
    return hasComma
      ? { offset: end + 1, text: ` ${item.replace(/,$/, "")},` }
      : { offset: contentEnd, text: `, ${item.replace(/,$/, "")}` };
  }
  // One item per line: a new line after the current item's line.
  const empty = !text.slice(list.start + 1, list.end).trim();
  const closeIndent = indentAt(text, list.end);
  if (empty) {
    const indent = `${closeIndent}  `;
    return { offset: list.start + 1, text: `\n${indent}${indented(item, indent)}` };
  }
  const itemStart = text.slice(current.start).search(/\S/) + current.start;
  const indent = indentAt(text, itemStart);
  const end = current.end === Infinity ? list.end : current.end;
  if (text[end] === ",") {
    const eol = text.indexOf("\n", end);
    const at = eol < 0 || eol > list.end ? end + 1 : eol;
    return { offset: at, text: `\n${indent}${indented(item, indent)}` };
  }
  // The last item, without a trailing comma: after its text.
  const contentEnd = current.start + text.slice(current.start, end).trimEnd().length;
  return { offset: contentEnd, text: `,\n${indent}${indented(item, indent)}`.replace(/,$/, "") };
}

function placeField(
  text: string,
  offset: number,
  snippet: string,
  { field: name, pageFields, replace }: Exclude<InsertKind, string>,
): Placement | undefined {
  const top = scan(text);
  const chain: ReturnType<typeof scan>[] = [];
  for (let f: ReturnType<typeof scan> | undefined = top; f;) {
    chain.push(f);
    f = f.children.find((c) => c.start < offset && offset <= c.end);
  }
  const records = chain.filter((f) => f.open === "{");
  let r = records.length - 1;
  // Inside one of the page's fields (`area = { min = 6 }`): the page's record is further out.
  while (r > 0 && records[r].segs?.length && pageFields.includes(records[r].segs![0])) r--;
  let record = records[r];
  if (!record) return undefined;
  // On a key that isn't one of the page's fields (`stair = { … }` on the page about spaces):
  // the record it holds.
  const under = record.fields.find((x) => x.keyStart <= offset && offset <= x.end);
  if (under && !pageFields.includes(under.key[0])) {
    const child = record.children.find(
      (c) => c.open === "{" && c.start > under.keyEnd && c.start <= under.end,
    );
    if (child) record = child;
  }
  const have = record.fields.find((x) => x.key[0] === name);
  if (have) {
    const eq = text.indexOf("=", have.keyEnd);
    const written = /^\s*('[\w.]+|"(?:[^"\\]|\\.)*"|-?\d+(?:\.\d+)?)/.exec(text.slice(eq + 1));
    const value = /=\s*([\s\S]*?),?\s*$/.exec(snippet)?.[1];
    if (replace && eq >= 0 && eq < have.end && written && value) {
      const start = eq + 1 + written[0].length - written[1].length;
      return { offset: start, end: start + written[1].length, text: value };
    }
    return { offset: have.keyStart, text: "", existing: { start: have.keyStart, end: have.keyEnd } };
  }

  const field = snippet.trimEnd().endsWith(",") ? snippet.trimEnd() : `${snippet},`;
  const close = record.end;
  const inner = text.slice(record.start + 1, close);
  if (!inner.trim()) return { offset: record.start + 1, text: ` ${indented(field, "").replace(/,$/, "")} ` };
  if (!inner.includes("\n")) {
    // `{ a = 1, b = 2 }`: before the closing brace.
    const body = inner.trimEnd();
    const at = record.start + 1 + body.length;
    const comma = body.endsWith(",") ? "" : ",";
    return { offset: at, text: `${comma} ${field.replace(/,$/, "")}` };
  }
  // One field per line: a new last line, indented like the others.
  const last = record.fields[record.fields.length - 1];
  const indent = last ? indentAt(text, last.keyStart) : `${indentAt(text, close)}  `;
  const lastEnd = inner.trimEnd().length + record.start + 1;
  const comma =
    text
      .slice(record.start + 1, lastEnd)
      .trimEnd()
      .endsWith(",") || !last
      ? ""
      : ",";
  return { offset: lastEnd, text: `${comma}\n${indent}${indented(field, indent)}` };
}
