import stdlibSource from "../../../schema/plate.ncl?raw";
import {
  deref,
  helpers,
  literal,
  root as schemaRoot,
  snippet,
  summarize,
  valuesOf,
  valueSnippet,
  type Helper,
  type Schema,
} from "../editor/schema";

/**
 * The reference: every topic the side panel can show, generated from the same sources the
 * engine and the editor use, so it can't drift from them:
 *
 * - `schema/plate.schema.json`: every field, its type, values, default and docs, plus the
 *   non-standard `x-examples` (Nickel source to show and insert) and `x-docs` (guides to read);
 * - `schema/plate.ncl`: the `plate.*` helpers and their `doc` metadata;
 * - `docs/reference/*.md`: guides for ideas that aren't one field (imports, typicals, ports…).
 *
 * Plain data and no Monaco, so it can be tested.
 */

export type EntryKind = "guide" | "plate" | "record" | "union" | "variant" | "value" | "helper";

export interface ValueDoc {
  value: string;
  doc?: string;
}

export interface FieldRow {
  name: string;
  /** Short type, e.g. `LengthRange`, `'room | 'circulation`, `Array Point`. */
  type: string;
  /** The entry that explains the type, when it has one. */
  typeLink?: string;
  doc: string;
  default?: string;
  required: boolean;
  /** Allowed values, when there are a few. */
  values: ValueDoc[];
  examples: Example[];
  /** Snippet (with tab stops) that writes the field. */
  snippet: string;
  guides: string[];
}

export interface Example {
  code: string;
  /** Inserted text, with tab stops; `code` (escaped) when absent. */
  snippet?: string;
}

export interface Entry {
  id: string;
  kind: EntryKind;
  section: string;
  title: string;
  /** Shown under the title in code type: a signature or a tag. */
  subtitle?: string;
  /** First sentence, for lists and search results. */
  summary: string;
  /** Markdown. */
  body: string;
  fields: FieldRow[];
  /** The forms a union can take (a starting mode, a rule kind). */
  variants: string[];
  /** What a value accepts (a length is a number or a string). */
  accepts: { label: string; link?: string }[];
  values: ValueDoc[];
  examples: Example[];
  /** Snippet (with tab stops) that writes one. */
  template?: string;
  /** Helpers that write it (rule entries), or rules a helper writes (helper entries). */
  helpers: string[];
  rules: string[];
  guides: string[];
  usedBy: { entry: string; field: string }[];
}

export interface Section {
  title: string;
  ids: string[];
}

export interface Reference {
  entries: Map<string, Entry>;
  sections: Section[];
  /** Which entry explains a schema definition (`Spaces` is explained by `Space`). */
  entryFor(def: string): string | undefined;
  helperByName: Map<string, Helper>;
}

export interface Guide {
  slug: string;
  source: string;
}

const SECTIONS = ["Guides", "Plate", "Starting geometry", "Spaces and plans", "Rules", "Helpers", "Values"];

/** Where a definition is listed, in this order; unknown ones go under Values or Spaces and plans by shape. */
const SECTION_OF: Record<string, string> = {
  Start: "Starting geometry",
  Space: "Spaces and plans",
  Plan: "Spaces and plans",
  Needs: "Spaces and plans",
  Engine: "Spaces and plans",
  Goals: "Spaces and plans",
  Rules: "Rules",
  Length: "Values",
  LengthRange: "Values",
  AreaRange: "Values",
  Count: "Values",
  Point: "Values",
  Selector: "Values",
  Port: "Values",
  EdgeRoles: "Values",
};

/** Definitions explained as part of another: a dictionary of spaces, a rule-or-list, an enum. */
const FOLD: Record<string, string> = {
  Spaces: "Space",
  RuleItem: "Rules",
  Rule: "Rules",
  PortNeed: "Needs",
};

const ROOT_ID = "def:#";

export const defId = (name: string) => (name === "#" ? ROOT_ID : `def:${name}`);
export const helperId = (name: string) => `helper:${name}`;
export const guideId = (slug: string) => `guide:${slug}`;

const LISTED = Object.keys(SECTION_OF).map(defId);

const firstSentence = (text: string) => {
  const flat = text.replace(/\s+/g, " ").trim();
  const m = /^(.+?[.!?])(\s|$)/.exec(flat);
  return m ? m[1] : flat;
};

const capitalize = (s: string) => s.charAt(0).toUpperCase() + s.slice(1);

const list = (v: string | string[] | undefined): string[] =>
  v === undefined ? [] : Array.isArray(v) ? v : [v];

/** Example code, inserted as-is: `$` would start a tab stop. */
export const plainSnippet = (code: string) => code.replace(/[$}\\]/g, (c) => `\\${c}`);

/** A snippet shown as text: tab stops replaced by their placeholder or first choice. */
export function snippetText(s: string): string {
  return s
    .replace(/\$\{\d+\|([^,|}]*)[^}]*\|\}/g, "$1")
    .replace(/\$\{\d+:([^}]*)\}/g, "$1")
    .replace(/\$\{?\d+\}?/g, "")
    .replace(/\t/g, "  ");
}

function guideOf(guide: Guide): { title: string; body: string } {
  const text = guide.source.replace(/\r\n/g, "\n").trim();
  const m = /^#\s+(.+)\n/.exec(text);
  return m ? { title: m[1].trim(), body: text.slice(m[0].length).trim() } : { title: guide.slug, body: text };
}

/** The constant that tells a union's variants apart (`mode`, `rule`), if there is one. */
export function discriminator(alternatives: Schema[]): string | undefined {
  const first = alternatives[0]?.properties;
  if (!first) return undefined;
  return Object.keys(first).find((key) =>
    alternatives.every((alt) => {
      const prop = alt.properties?.[key];
      return prop !== undefined && deref(prop).node.const !== undefined;
    }),
  );
}

/** A definition whose values are records: it gets a page with fields (or variants). */
export function isRecordDef(node: Schema): boolean {
  const { node: n } = deref(node);
  if (n.properties) return true;
  const alts = n.oneOf ?? n.anyOf;
  return !!alts?.length && alts.every((alt) => !!deref(alt).node.properties);
}

function examplesOf(node: Schema, fieldName?: string): Example[] {
  const out: Example[] = (node["x-examples"] ?? []).map((code) => ({ code }));
  for (const v of node.examples ?? []) {
    out.push({ code: fieldName ? `${fieldName} = ${literal(v)},` : literal(v) });
  }
  return out;
}

/** Code in a helper's doc string: `plate.door "bedroom" "hall"`. */
function helperExamples(h: Helper): Example[] {
  return [...h.doc.matchAll(/`([^`]+)`/g)]
    .map((m) => m[1])
    .filter((code) => code.startsWith("plate."))
    .map((code) => ({ code }));
}

/** The named definition a field's value refers to, looking through lists and alternatives. */
function linkedDef(raw: Schema, depth = 0): string | undefined {
  if (depth > 4) return undefined;
  const { node, name } = deref(raw);
  if (name) return name;
  if (node.items && typeof node.items === "object") return linkedDef(node.items, depth + 1);
  if (node.additionalProperties && typeof node.additionalProperties === "object")
    return linkedDef(node.additionalProperties, depth + 1);
  return undefined;
}

export function buildReference(
  root: Schema = schemaRoot,
  stdlib: string = stdlibSource,
  guides: Guide[] = GUIDES,
): Reference {
  const entries = new Map<string, Entry>();
  const defs = root.$defs ?? {};
  const helperList = helpers(stdlib);
  const helperByName = new Map(helperList.map((h) => [h.name, h]));

  const entryFor = (def: string): string | undefined => {
    let name = def;
    for (let i = 0; i < 4 && FOLD[name]; i++) name = FOLD[name];
    const id = defId(name);
    return entries.has(id) ? id : undefined;
  };

  const blank = (id: string, kind: EntryKind, section: string, title: string): Entry => ({
    id,
    kind,
    section,
    title,
    summary: "",
    body: "",
    fields: [],
    variants: [],
    accepts: [],
    values: [],
    examples: [],
    helpers: [],
    rules: [],
    guides: [],
    usedBy: [],
  });

  // ------------------------------------------------------------ guides
  for (const g of guides) {
    const { title, body } = guideOf(g);
    const e = blank(guideId(g.slug), "guide", "Guides", title);
    e.body = body;
    e.summary = firstSentence(
      body.split(/\n\s*\n/).find((p) => !p.startsWith("#") && !p.startsWith("```")) ?? "",
    );
    entries.set(e.id, e);
  }

  // ------------------------------------------------------------ schema definitions
  // Variants of tagged unions (each starting mode, each rule kind) are listed with their union.
  const variantOf = new Map<string, { union: string; key: string; value: string }>();
  for (const [name, def] of Object.entries(defs)) {
    const alts = (def.oneOf ?? def.anyOf ?? []).map((a) => deref(a));
    const key = discriminator(alts.map((a) => a.node));
    if (!key) continue;
    for (const alt of alts) {
      if (alt.name)
        variantOf.set(alt.name, {
          union: name,
          key,
          value: String(deref(alt.node.properties![key]).node.const),
        });
    }
  }

  const sectionOf = (name: string, node: Schema): string => {
    const variant = variantOf.get(name);
    if (variant) return sectionOf(variant.union, defs[variant.union]);
    if (FOLD[name]) return sectionOf(FOLD[name], defs[FOLD[name]] ?? {});
    if (SECTION_OF[name]) return SECTION_OF[name];
    return isRecordDef(node) ? "Spaces and plans" : "Values";
  };

  const add = (name: string, node: Schema) => {
    const variant = variantOf.get(name);
    const record = isRecordDef(node);
    const union = !node.properties && record;
    const kind: EntryKind =
      name === "#" ? "plate" : variant ? "variant" : union ? "union" : record ? "record" : "value";
    const title = name === "#" ? "Plate file" : variant ? capitalize(variant.value) : name;
    const e = blank(defId(name), kind, name === "#" ? "Plate" : sectionOf(name, node), title);
    if (variant) e.subtitle = `${variant.key} = '${variant.value}`;
    e.body = node.description ?? "";
    e.summary = firstSentence(e.body);
    e.examples = examplesOf(node);
    e.guides = list(node["x-docs"]);
    // A value to insert, where that means something on its own: a starting mode, a rule kind, a tag.
    if (kind === "variant" || kind === "union" || valuesOf(node).length)
      e.template = valueSnippet(node, { i: 1 });
    entries.set(e.id, e);
  };

  add("#", root);
  for (const [name, def] of Object.entries(defs)) {
    if (FOLD[name]) continue;
    add(name, def);
  }

  // Folded definitions add what they know to the entry that explains them.
  for (const [name, target] of Object.entries(FOLD)) {
    const def = defs[name];
    const e = entries.get(defId(target));
    if (!def || !e) continue;
    e.guides.push(...list(def["x-docs"]).filter((g) => !e.guides.includes(g)));
    e.examples.push(...examplesOf(def));
  }

  // Fields, variants, values, accepted forms.
  const fill = (name: string, node: Schema) => {
    const e = entries.get(entryFor(name) ?? "");
    if (!e) return;
    const n = deref(node).node;
    for (const [field, prop] of Object.entries(n.properties ?? {})) {
      if (variantOf.get(name)?.key === field) continue; // the tag itself is the subtitle
      const target = deref(prop);
      const dict = !target.name && target.node.additionalProperties;
      // A dictionary (`ports = { entrance = 'required }`) is described by its values.
      const entryNode = dict && typeof dict === "object" ? dict : undefined;
      const detail = entryNode ? `{ name = ${summarize(entryNode).detail} }` : summarize(prop).detail;
      const link = linkedDef(prop);
      const typeLink = link ? entryFor(link) : undefined;
      const valueNode = entryNode ? deref(entryNode).node : target.node;
      const enumValues = valuesOf(valueNode);
      const docs = valueNode["x-enumDescriptions"] ?? {};
      const values =
        enumValues.length && enumValues.length <= 8
          ? enumValues.map((v) => ({ value: v, doc: docs[v] }))
          : [];
      const doc = [prop.description, target.name && !prop.description ? valueNode.description : undefined]
        .filter(Boolean)
        .join("\n\n");
      e.fields.push({
        name: field,
        type: detail,
        typeLink: typeLink !== e.id ? typeLink : undefined,
        doc,
        // Enum defaults are written as tags: `'room`, not `"room"`.
        default:
          prop.default === undefined
            ? undefined
            : enumValues.includes(String(prop.default))
              ? `'${String(prop.default)}`
              : literal(prop.default),
        required: !!n.required?.includes(field),
        values,
        examples: examplesOf(prop, field),
        snippet: snippet(field, prop),
        guides: list(prop["x-docs"]),
      });
      if (typeLink && typeLink !== e.id) {
        const used = entries.get(typeLink)!;
        if (!used.usedBy.some((u) => u.entry === e.id && u.field === field))
          used.usedBy.push({ entry: e.id, field });
      }
    }
    // Unions: their variants.
    for (const alt of n.oneOf ?? n.anyOf ?? []) {
      const { node: a, name: altName } = deref(alt);
      if (altName && variantOf.get(altName)?.union === name) {
        const id = entryFor(altName);
        if (id && !e.variants.includes(id)) e.variants.push(id);
      } else if (!isRecordDef(n) && !a.properties) {
        const linked = linkedDef(alt);
        e.accepts.push({ label: summarize(alt).detail, link: linked ? entryFor(linked) : undefined });
      } else if (a.properties && !isRecordDef(n)) {
        e.accepts.push({ label: `{ ${Object.keys(a.properties).join(", ")} }` });
      }
    }
    if (FOLD[name]) return; // its values are listed with the field that uses them
    const docs = n["x-enumDescriptions"] ?? {};
    const values = valuesOf(n);
    for (const v of values.length ? values : Object.keys(docs)) {
      if (!e.values.some((x) => x.value === v)) e.values.push({ value: v, doc: docs[v] });
    }
  };
  fill("#", root);
  for (const [name, def] of Object.entries(defs)) fill(name, def);

  // Rules list their kinds: the union behind `Rules` (through `RuleItem`).
  // Folded unions (`Rule`, into `Rules`) give their variants to the entry that explains them.
  for (const [name, target] of Object.entries(FOLD)) {
    const e = entries.get(defId(target));
    if (!e || !defs[name]) continue;
    for (const alt of defs[name].oneOf ?? defs[name].anyOf ?? []) {
      const altName = deref(alt).name;
      if (altName && variantOf.get(altName)?.union === name && entries.has(defId(altName))) {
        if (!e.variants.includes(defId(altName))) e.variants.push(defId(altName));
      }
    }
    if (e.variants.length) {
      e.accepts = [];
      if (e.kind === "value") e.kind = "union";
    }
  }

  // ------------------------------------------------------------ helpers
  for (const h of helperList) {
    const e = blank(helperId(h.name), "helper", "Helpers", `plate.${h.name}`);
    e.subtitle = [`plate.${h.name}`, ...h.params].join(" ");
    e.body = h.doc;
    e.summary = firstSentence(h.doc.replace(/`[^`]*`/g, (c) => (c.startsWith("`plate.") ? "" : c)));
    e.examples = helperExamples(h);
    e.template = `plate.${h.snippet}`;
    entries.set(e.id, e);
  }
  // Each rule kind knows its helpers, and each helper the rule kinds it writes.
  for (const h of helperList) {
    const he = entries.get(helperId(h.name))!;
    const written = [...h.rules.map((k) => ["rule", k]), ...h.places.map((k) => ["place", k])];
    for (const [prop, kind] of written) {
      const ruleEntry = [...entries.values()].find(
        (x) => x.kind === "variant" && x.subtitle === `${prop} = '${kind}`,
      );
      if (!ruleEntry) continue;
      he.rules.push(ruleEntry.id);
      ruleEntry.helpers.push(he.id);
      if (!ruleEntry.examples.length || written.length === 1)
        ruleEntry.examples.push(
          ...he.examples.filter((x) => !ruleEntry.examples.some((y) => y.code === x.code)),
        );
      // Written with the helper of the same name when there is one (`reach`, not `through`).
      if (written.length === 1 && (!ruleEntry.template?.startsWith("plate.") || h.name === kind))
        ruleEntry.template = he.template;
    }
  }
  // Helpers that wrap other rules (`prefer`) belong with every rule kind's options.
  for (const h of helperList.filter((x) => !x.rules.length && !x.places.length)) {
    const rules = entries.get(defId("Rules"));
    if (rules && !rules.helpers.includes(helperId(h.name))) rules.helpers.push(helperId(h.name));
  }

  // ------------------------------------------------------------ sections
  const sections: Section[] = SECTIONS.map((title) => ({ title, ids: [] }));
  for (const e of entries.values()) {
    let s = sections.find((x) => x.title === e.section);
    if (!s) sections.push((s = { title: e.section, ids: [] }));
    s.ids.push(e.id);
  }
  // Unions come before their variants; guides keep their file order.
  for (const s of sections) {
    if (s.title === "Guides" || s.title === "Helpers") continue;
    const order = (id: string) => {
      const e = entries.get(id)!;
      if (e.kind === "plate") return -1;
      const union = [...entries.values()].find((u) => u.variants.includes(id));
      return union ? s.ids.indexOf(union.id) + 0.5 + union.variants.indexOf(id) / 100 : s.ids.indexOf(id);
    };
    const listed = (id: string) =>
      LISTED.includes(id) ? LISTED.indexOf(id) : LISTED.length + s.ids.indexOf(id);
    const original = [...s.ids].sort((a, b) => listed(a) - listed(b));
    s.ids.splice(0, s.ids.length, ...original);
    const rank = new Map(s.ids.map((id) => [id, order(id)]));
    s.ids.sort((a, b) => rank.get(a)! - rank.get(b)!);
  }

  return { entries, sections: sections.filter((s) => s.ids.length), entryFor, helperByName };
}

// Guides live in docs/reference as numbered markdown files: 01-writing.md is `guide:writing`.
const guideFiles = import.meta.glob("../../../docs/reference/*.md", {
  query: "?raw",
  import: "default",
  eager: true,
}) as Record<string, string>;

export const GUIDES: Guide[] = Object.entries(guideFiles)
  .sort(([a], [b]) => a.localeCompare(b))
  .map(([path, source]) => ({
    slug: path
      .slice(path.lastIndexOf("/") + 1)
      .replace(/\.md$/, "")
      .replace(/^\d+-/, ""),
    source,
  }));

export const reference: Reference = buildReference();

/** Strip markdown for search. */
const plain = (md: string) => md.replace(/```[\s\S]*?```/g, " ").replace(/[`*#>|[\]()_-]+/g, " ");

export interface SearchHit {
  entry: string;
  field?: string;
  /** An enum value (`wet`). */
  value?: string;
  label: string;
  detail: string;
  score: number;
}

/** Fuzzy search over titles, fields and text. Best first. */
export function search(ref: Reference, query: string, limit = 40): SearchHit[] {
  const helperQuery = /^\s*plate\./i.test(query);
  const q = query
    .trim()
    .toLowerCase()
    .replace(/^plate\./, "");
  if (!q) return [];
  const hits: SearchHit[] = [];
  const score = (text: string, base: number): number => {
    const t = text.toLowerCase().replace(/^plate\./, "");
    if (t === q) return base + 100;
    if (t.startsWith(q)) return base + 70;
    const words = t.split(/[^a-z0-9]+/);
    if (words.some((w) => w.startsWith(q))) return base + 55;
    if (t.includes(q)) return base + 40;
    // Letters in order, close together (`lrange` finds `LengthRange`).
    let i = 0;
    let first = -1;
    let last = -1;
    for (let k = 0; k < t.length && i < q.length; k++) {
      if (t[k] !== q[i]) continue;
      if (first < 0) first = k;
      last = k;
      i++;
    }
    const tight = last - first + 1 <= q.length * 2;
    return i === q.length && q.length > 2 && tight ? base + 15 - Math.min(10, t.length - q.length) / 2 : 0;
  };
  for (const e of ref.entries.values()) {
    let best = Math.max(score(e.title, 20), e.subtitle ? score(e.subtitle, 10) : 0);
    if (best && e.kind === "helper" && helperQuery) best += 5;
    if (best) hits.push({ entry: e.id, label: e.title, detail: e.summary, score: best });
    for (const f of e.fields) {
      const s = Math.max(score(f.name, 12), ...f.values.map((v) => score(v.value, 0)));
      if (s)
        hits.push({
          entry: e.id,
          field: f.name,
          label: f.name,
          detail: `${e.title} · ${firstSentence(f.doc)}`,
          score: s,
        });
    }
    for (const v of e.values) {
      const s = score(v.value, 6);
      if (s)
        hits.push({
          entry: e.id,
          value: v.value,
          label: `'${v.value}`,
          detail: `${e.title} · ${v.doc ?? ""}`,
          score: s,
        });
    }
    if (!best && q.length > 2 && plain(e.body).toLowerCase().includes(q)) {
      hits.push({ entry: e.id, label: e.title, detail: e.summary, score: 8 });
    }
  }
  return hits.sort((a, b) => b.score - a.score || a.label.length - b.label.length).slice(0, limit);
}
