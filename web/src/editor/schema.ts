import type * as Monaco from "monaco-editor/editor/editor.api";
import schemaJson from "../../../schema/plate.schema.json";
import stdlibSource from "../../../schema/plate.ncl?raw";

/**
 * Hover docs and completions, read from the configuration JSON Schema
 * (`schema/plate.schema.json`): the same file the engine compiles into Nickel contracts.
 * Nothing here needs updating when the schema grows.
 */

export type Schema = {
  $ref?: string;
  type?: string;
  description?: string;
  default?: unknown;
  enum?: unknown[];
  const?: unknown;
  pattern?: string;
  properties?: Record<string, Schema>;
  required?: string[];
  anyOf?: Schema[];
  oneOf?: Schema[];
  items?: Schema | boolean;
  additionalProperties?: Schema | boolean;
  prefixItems?: Schema[];
  $defs?: Record<string, Schema>;
  "x-enumDescriptions"?: Record<string, string>;
  /** Nickel source to show (and insert) as examples. */
  "x-examples"?: string[];
  /** Guides in docs/reference that explain this, by slug (`typicals`). */
  "x-docs"?: string | string[];
  examples?: unknown[];
  title?: string;
};

export const root = schemaJson as Schema;

export interface FieldDoc {
  name: string;
  detail: string;
  doc: string;
  values: string[];
  snippet: string;
}

export function deref(node: Schema): { node: Schema; name?: string } {
  let name: string | undefined;
  for (let i = 0; i < 16 && node.$ref; i++) {
    name = node.$ref.replace("#/$defs/", "");
    node = { ...(root.$defs?.[name] ?? {}), ...node, $ref: root.$defs?.[name]?.$ref };
  }
  return { node, name };
}

export const tag = (v: unknown) => `'${String(v)}`;

export function literal(v: unknown): string {
  if (typeof v === "string") return JSON.stringify(v);
  if (v && typeof v === "object") {
    return `{ ${Object.entries(v)
      .map(([k, x]) => `${k} = ${literal(x)}`)
      .join(", ")} }`;
  }
  return String(v);
}

export function valuesOf(node: Schema): string[] {
  return (node.enum ?? (node.const !== undefined ? [node.const] : [])).map(String);
}

export function alternativesOf(node: Schema): Schema[] {
  return (node.oneOf ?? node.anyOf ?? []).map((alt) => deref(alt).node);
}

export function summarize(raw: Schema): { detail: string; values: string[] } {
  const { node, name } = deref(raw);
  const values = valuesOf(node);
  if (values.length) return { detail: values.map(tag).join(" | "), values };
  if (name) return { detail: name, values };
  if (node.properties) return { detail: `{ ${Object.keys(node.properties).join(", ")} }`, values };
  if (node.type === "array" && node.items && typeof node.items === "object") {
    return { detail: `Array ${summarize(node.items).detail}`, values };
  }
  const type = node.type === "string" ? "String" : node.type === "number" ? "Number" : (node.type ?? "Dyn");
  return { detail: type, values };
}

/**
 * A snippet that fills in a value of this schema: required fields, a choice between tags, one
 * example item for lists. `n` numbers the tab stops.
 */
export function valueSnippet(raw: Schema, n: { i: number }, fallback?: unknown): string {
  const { node } = deref(raw);
  const values = valuesOf(node);
  if (values.length > 1) return `'\${${n.i++}|${values.join(",")}|}`;
  if (values.length === 1) return `'${values[0]}`;

  const alternatives = alternativesOf(node);
  if (alternatives.length && alternatives.every((alt) => alt.properties)) {
    // A tagged union: the first variant's fields, offering every variant's tag.
    const first = alternatives[0];
    const lines = Object.entries(first.properties!)
      .filter(([k]) => first.required?.includes(k))
      .map(([k, p]) => {
        const tags = alternatives.flatMap((alt) => {
          const prop = alt.properties?.[k];
          return prop ? valuesOf(deref(prop).node) : [];
        });
        const value = tags.length > 1 ? `'\${${n.i++}|${[...new Set(tags)].join(",")}|}` : valueSnippet(p, n);
        return indent(`${k} = ${value},`);
      });
    return `{\n${lines.join("\n")}\n}`;
  }
  if (alternatives[0]?.type === "string") return `"\${${n.i++}:name}"`;
  if (alternatives.length) return `\${${n.i++}:${literal(fallback ?? 0)}}`;

  if (node.additionalProperties && typeof node.additionalProperties === "object") {
    return `{\n${indent(`\${${n.i++}:name} = ${valueSnippet(node.additionalProperties, n)},`)}\n}`;
  }
  if (node.properties) {
    const lines = Object.entries(node.properties)
      .filter(([k]) => node.required?.includes(k))
      .map(([k, p]) => indent(`${k} = ${valueSnippet(p, n)},`));
    return lines.length ? `{\n${lines.join("\n")}\n}` : `{ \$${n.i++} }`;
  }
  if (node.type === "array") {
    if (node.prefixItems?.length)
      return `[${node.prefixItems.map((item) => valueSnippet(item, n)).join(", ")}]`;
    if (node.items && typeof node.items === "object")
      return `[\n${indent(`${valueSnippet(node.items, n)},`)}\n]`;
    return `[\$${n.i++}]`;
  }
  const tags = Object.keys(node["x-enumDescriptions"] ?? {});
  if (node.type === "string" && tags.length) return `'\${${n.i++}|${tags.join(",")}|}`;
  if (node.type === "string") return `"\${${n.i++}:${String(fallback ?? "")}}"`;
  return `\${${n.i++}:${fallback !== undefined ? literal(fallback) : "0"}}`;
}

const indent = (text: string) => text.replace(/^/gm, "\t");

export function snippet(fieldName: string, raw: Schema): string {
  return `${fieldName} = ${valueSnippet(raw, { i: 1 }, raw.default)},`;
}

/** Combine what different schemas say about fields with the same name (e.g. each variant's `mode`). */
function merge(a: FieldDoc, b: FieldDoc): FieldDoc {
  const values = [...new Set([...a.values, ...b.values])];
  const docs = [...new Set([...a.doc.split("\n\n"), ...b.doc.split("\n\n")])];
  return {
    ...a,
    values,
    detail: values.length ? values.map(tag).join(" | ") : a.detail,
    doc: docs.join("\n\n"),
  };
}

/** Every property in the schema, by name (merged across the variants that share a name), plus enum value docs. */
function collect(): { fields: Map<string, FieldDoc>; valueDocs: Map<string, string> } {
  const fields = new Map<string, FieldDoc>();
  const valueDocs = new Map<string, string>();
  const seen = new Set<Schema>();

  const visit = (node: Schema) => {
    if (seen.has(node)) return;
    seen.add(node);
    const { node: resolved } = deref(node);
    for (const [value, doc] of Object.entries(resolved["x-enumDescriptions"] ?? {}))
      valueDocs.set(value, doc);
    for (const [name, prop] of Object.entries(resolved.properties ?? {})) {
      const { detail, values } = summarize(prop);
      const target = deref(prop);
      const docs = [prop.description, target.name ? target.node.description : undefined].filter(
        (d, i, all): d is string => !!d && all.indexOf(d) === i,
      );
      if (prop.default !== undefined) docs.push(`Default: \`${literal(prop.default)}\``);
      const field = { name, detail, doc: docs.join("\n\n"), values, snippet: snippet(name, prop) };
      const known = fields.get(name);
      fields.set(name, known ? merge(known, field) : field);
      visit(prop);
    }
    for (const alt of [...(resolved.oneOf ?? []), ...(resolved.anyOf ?? [])]) visit(alt);
    for (const child of [resolved.additionalProperties, resolved.items, ...(resolved.prefixItems ?? [])]) {
      if (child && typeof child === "object") visit(child);
    }
  };
  visit(root);
  for (const def of Object.values(root.$defs ?? {})) visit(def);
  return { fields, valueDocs };
}

const { fields: FIELDS, valueDocs: VALUE_DOCS } = collect();

// Rules are written with the `plate` helpers, so suggest one rather than a raw record.
const rulesField = FIELDS.get("rules");
if (rulesField) {
  rulesField.snippet =
    'rules = [\n\tplate.${1|door,touches,window,apart,between,along,unique|} "${2:a}" "${3:b}",\n],';
}

/** A helper of the `plate` library (schema/plate.ncl). */
export interface Helper {
  name: string;
  doc: string;
  snippet: string;
  /** Argument names, as written in the docs (`a`, `b`, `distance`). */
  params: string[];
  /** The `rule` kinds it writes (`door`); empty for helpers that wrap other rules (`prefer`). */
  rules: string[];
  /** The `place` kinds it writes (`every`): a service's placements. */
  places: string[];
  /** It returns a list of rules rather than one. */
  list: boolean;
}

/** Placeholder names for each helper's arguments; strings are quoted. */
const PARAMS: Record<string, string[]> = {
  touches: ['"a"', '"b"'],
  door: ['"a"', '"b"'],
  opening: ['"a"', '"b"'],
  open: ['"a"', '"b"'],
  window: ['"a"'],
  apart: ['"a"', '"b"'],
  near: ['"a"', '"b"', "distance"],
  between: ['"a"', '"mid"'],
  through: ['"a"', '"via"'],
  reach: ['"a"', '"b"', '"via"'],
  along: ['"a"', '"corridor"'],
  unique: ['"a"'],
  instance: ['"a"', "plan"],
  prefer: ["rule"],
  prefer_by: ["weight", "rule"],
  wall: ['"a"', '"b"'],
  every: ['"part"', '"spaces"', "spacing"],
  beside: ['"part"', "opening"],
  over: ['"part"', '"spaces"'],
  on: ['"part"', '"fixtures"'],
  one: ['"part"', '"spaces"'],
};

export function helpers(source: string): Helper[] {
  const out: Helper[] = [];
  const re = /^ {2}([a-z_]+)\n\s+\| doc "((?:[^"\\]|\\.)*)"/gm;
  const found = [...source.matchAll(re)];
  found.forEach((m, k) => {
    const name = m[1];
    const doc = m[2].replace(/\\(.)/g, "$1");
    // The body runs to the next helper: which rules it writes, and whether it returns a list.
    const body = source.slice(m.index! + m[0].length, found[k + 1]?.index ?? source.length);
    const value = body.slice(body.indexOf("=>") + 2).trimStart();
    const rules = [...new Set([...body.matchAll(/rule = '(\w+)/g)].map((r) => r[1]))];
    const places = [...new Set([...body.matchAll(/place = '(\w+)/g)].map((r) => r[1]))];
    const params = PARAMS[name] ?? [];
    let i = 1;
    const args = params.map((p) =>
      p.startsWith('"') ? `"\${${i++}:${p.slice(1, -1)}}"` : `\${${i++}:${p}}`,
    );
    out.push({
      name,
      doc,
      snippet: [name, ...args].join(" "),
      params: params.map((p) => p.replace(/"/g, "")),
      rules,
      places,
      list: value.startsWith("["),
    });
  });
  return out;
}

/** Exposed for tests. */
export const plateHelpers: Helper[] = helpers(stdlibSource);

/** Exposed for tests. */
export const schemaFields: ReadonlyMap<string, FieldDoc> = FIELDS;

/** Command that opens the reference panel at a position: `[modelUri, line, column]`. */
export const REFERENCE_COMMAND = "plate.reference.show";

export function registerSchema(monaco: typeof Monaco): void {
  monaco.languages.registerHoverProvider("nickel", {
    provideHover(model, position) {
      const hover = hoverAt(model, position);
      if (!hover) return null;
      const args = encodeURIComponent(
        JSON.stringify([model.uri.toString(), position.lineNumber, position.column]),
      );
      hover.contents.push({
        value: `[Open in reference](command:${REFERENCE_COMMAND}?${args})`,
        isTrusted: { enabledCommands: [REFERENCE_COMMAND] },
      });
      return hover;
    },
  });

  function hoverAt(
    model: Monaco.editor.ITextModel,
    position: Monaco.Position,
  ): { range: Monaco.Range; contents: Monaco.IMarkdownString[] } | null {
    const word = model.getWordAtPosition(position);
    if (!word) return null;
    const range = new monaco.Range(
      position.lineNumber,
      word.startColumn,
      position.lineNumber,
      word.endColumn,
    );
    if (word.word.startsWith("'")) {
      const doc = VALUE_DOCS.get(word.word.slice(1));
      return doc ? { range, contents: [{ value: `\`${word.word}\`` }, { value: doc }] } : null;
    }
    const before = model.getLineContent(position.lineNumber).slice(0, word.startColumn - 1);
    if (before.endsWith("plate.")) {
      const helper = plateHelpers.find((h) => h.name === word.word);
      return helper
        ? { range, contents: [{ value: `**plate.${helper.name}**` }, { value: helper.doc }] }
        : null;
    }
    const field = FIELDS.get(word.word);
    if (!field) return null;
    const after = model
      .getLineContent(position.lineNumber)
      .slice(word.endColumn - 1)
      .trimStart();
    if (!after.startsWith("=") && !after.startsWith("|") && !after.startsWith(".")) return null;
    return {
      range,
      contents: [{ value: `**${field.name}** : \`${field.detail}\`` }, { value: field.doc }],
    };
  }

  monaco.languages.registerCompletionItemProvider("nickel", {
    triggerCharacters: ["'", "."],
    provideCompletionItems(model, position) {
      const word = model.getWordUntilPosition(position);
      const line = model.getLineContent(position.lineNumber).slice(0, position.column - 1);
      const range = new monaco.Range(
        position.lineNumber,
        word.startColumn,
        position.lineNumber,
        word.endColumn,
      );
      const Kind = monaco.languages.CompletionItemKind;

      // After `plate.`: the helper library.
      if (/plate\.\w*$/.test(line)) {
        return {
          suggestions: plateHelpers.map((h) => ({
            label: h.name,
            kind: Kind.Function,
            detail: "plate helper",
            documentation: { value: h.doc },
            insertText: h.snippet,
            insertTextRules: monaco.languages.CompletionItemInsertTextRule.InsertAsSnippet,
            range,
          })),
        };
      }

      // After `=` (or a quote): suggest the field's allowed values.
      if (/=\s*'?[\w]*$/.test(line)) {
        const typed = /'[\w]*$/.test(line);
        const start = typed ? line.lastIndexOf("'") + 1 : word.startColumn;
        const valueRange = new monaco.Range(position.lineNumber, start, position.lineNumber, position.column);
        const name = /([\w-]+)\s*=\s*'?\w*$/.exec(line)?.[1] ?? "";
        const values = FIELDS.get(name)?.values ?? [];
        return {
          suggestions: values.map((v) => ({
            label: tag(v),
            kind: Kind.EnumMember,
            insertText: typed ? v : tag(v),
            documentation: VALUE_DOCS.get(v),
            range: valueRange,
          })),
        };
      }

      return {
        suggestions: [...FIELDS.values()].map((f) => ({
          label: f.name,
          kind: Kind.Field,
          detail: f.detail,
          documentation: { value: f.doc },
          insertText: f.snippet,
          insertTextRules: monaco.languages.CompletionItemInsertTextRule.InsertAsSnippet,
          range,
        })),
      };
    },
  });
}
