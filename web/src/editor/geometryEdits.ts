import type { Element, Point, SourceSpan, Span, Units } from "../types";
import { feetInches } from "../viewer/format";
import type { TextDoc } from "./editor";
import { place } from "../reference/insert";
import { formatNumber } from "./templates";

/**
 * Writes edits made in the drawing back into the configuration text.
 *
 * Every value of the evaluated configuration comes with the span of text that produced it
 * (the source map, from Nickel). Moving a point rewrites exactly its two coordinates, even when
 * they live in a `let` binding further up the file; inserting or removing a point edits the
 * `points` array where it is written. Values computed from expressions (`w * 2`) can't be
 * rewritten in place, so those edits are refused with a pointer to the line.
 */

export type EditResult = { ok: true; note?: string } | { ok: false; reason: string };

type Kind = SourceSpan["kind"];

interface Drag {
  doc: TextDoc;
  kinds: [Kind, Kind];
  originals: [string, string];
}

/** A coordinate as it's written: one text span may hold several (a `let` used twice). */
interface Written {
  source: SourceSpan;
  original: string;
}

/** An edge being moved: its four coordinates (x, y of each end) and where each is written. */
interface EdgeMoveState {
  doc: TextDoc;
  /** Distinct spans, in the order they're highlighted. */
  written: Written[];
  /** For x and y of each end: which of `written` holds it, and its value before the move. */
  coords: { at: number; value: number }[];
}

export class GeometryEdits {
  private sources: SourceSpan[] = [];
  private units: Units = "m";
  private drag: Drag | null = null;
  private edge: EdgeMoveState | null = null;
  private main = "";
  /** The document being edited: the file of the value an operation starts from. */
  private doc!: TextDoc;

  /** `docFor` gives the open document of a workspace file. */
  constructor(private readonly docFor: (file: string) => TextDoc | undefined) {}

  /** The latest source map and units, from the last successful evaluation of plate `main`. */
  update(sources: SourceSpan[], units: Units, main: string): void {
    this.sources = sources;
    this.units = units;
    this.main = main;
  }

  /** Edit the file `s` is written in. False when that file isn't open (it was deleted). */
  private use(s: SourceSpan): boolean {
    const doc = this.docFor(s.file);
    if (doc) this.doc = doc;
    return !!doc;
  }

  /** "line 12", or "line 12 of hotel-room.ncl" for a value from another file. */
  private where(s: SourceSpan): string {
    return s.file && s.file !== this.main
      ? `line ${s.span.startLine} of ${s.file}`
      : `line ${s.span.startLine}`;
  }

  /** The source map from the last successful evaluation. */
  sourcesSnapshot(): SourceSpan[] {
    return this.sources;
  }

  get dragging(): boolean {
    return this.drag !== null;
  }

  private find(pointer: string): SourceSpan | undefined {
    return this.sources.find((s) => s.pointer === pointer);
  }

  /** Is `s` written in the document being edited? */
  private sameFile(s: SourceSpan): boolean {
    return this.docFor(s.file) === this.doc;
  }

  private text(span: Span): string {
    const t = this.doc.value;
    return t.slice(
      this.doc.offsetOf(span.startLine, span.startCol),
      this.doc.offsetOf(span.endLine, span.endCol),
    );
  }

  /** How a coordinate is written back: as a number, or as a length string in its original unit. */
  private format(v: number, kind: Kind, original: string): string {
    if (kind !== "string") return formatNumber(v, this.units);
    const mmPerUnit = { mm: 1, cm: 10, m: 1000, in: 25.4, ft: 304.8 }[this.units];
    const metric = /(mm|cm|m)\s*"?\s*$/.exec(original)?.[1] as "mm" | "cm" | "m" | undefined;
    if (metric) {
      const value = (v * mmPerUnit) / { mm: 1, cm: 10, m: 1000 }[metric];
      return `"${formatNumber(value, metric)} ${metric}"`;
    }
    // Feet and inches, without the closing inch mark so no escaping is needed: "98'-6 1/2".
    return `"${feetInches((v * mmPerUnit) / 25.4, 16).replace(/"$/, "")}"`;
  }

  // ---------------------------------------------------------------- moving a point

  beginDrag(element: Element, index: number): EditResult {
    this.drag = null;
    const base = `${element.source ?? "/start/points"}/${index}`;
    const x = this.find(`${base}/0`);
    const y = this.find(`${base}/1`);
    if (!x || !y) return { ok: false, reason: "This point isn't written as [x, y] in the file." };
    if (x.file !== y.file)
      return { ok: false, reason: "x and y of this point are written in different files." };
    if (!this.use(x)) return { ok: false, reason: `${x.file} isn't in the workspace any more.` };
    for (const c of [x, y]) {
      if (!c.literal) {
        return {
          ok: false,
          reason: `This point is computed on ${this.where(c)} (\`${this.text(c.span).trim()}\`). Edit it there.`,
        };
      }
    }
    if (sameSpan(x.span, y.span)) {
      return { ok: false, reason: `x and y both come from ${this.where(x)}. Edit it there.` };
    }
    // Values shared with other places (a `let` used twice) move those too.
    const shared = this.sources.filter(
      (s) =>
        s.pointer !== x.pointer &&
        s.pointer !== y.pointer &&
        s.file === x.file &&
        (sameSpan(s.span, x.span) || sameSpan(s.span, y.span)),
    ).length;
    this.drag = { doc: this.doc, kinds: [x.kind, y.kind], originals: [this.text(x.span), this.text(y.span)] };
    this.doc.highlight([x.span, y.span]);
    this.doc.pushUndoStop();
    const note = shared
      ? `This point shares its coordinates with ${shared} other value${shared === 1 ? "" : "s"}; they move too.`
      : undefined;
    return { ok: true, note };
  }

  /** Rewrite the dragged point's coordinates. All moves of one drag form one undo step. */
  dragTo(point: Point): void {
    const d = this.drag;
    if (!d) return;
    this.doc = d.doc;
    const [xs, ys] = this.doc.highlighted();
    if (!xs || !ys) return;
    const off = (s: Span) => ({
      start: this.doc.offsetOf(s.startLine, s.startCol),
      end: this.doc.offsetOf(s.endLine, s.endCol),
    });
    this.doc.editOffsets(
      [
        { ...off(xs), text: this.format(point.x, d.kinds[0], d.originals[0]) },
        { ...off(ys), text: this.format(point.y, d.kinds[1], d.originals[1]) },
      ],
      { before: false, after: false },
    );
  }

  endDrag(): void {
    if (this.drag) this.doc = this.drag.doc;
    this.drag = null;
    this.doc.pushUndoStop();
    this.doc.clearHighlight();
  }

  /** Abandon the drag, putting the coordinates back exactly as they were written. */
  cancelDrag(): void {
    const d = this.drag;
    if (!d) return;
    this.doc = d.doc;
    const [xs, ys] = this.doc.highlighted();
    if (xs && ys && (this.text(xs) !== d.originals[0] || this.text(ys) !== d.originals[1])) {
      const off = (s: Span) => ({
        start: this.doc.offsetOf(s.startLine, s.startCol),
        end: this.doc.offsetOf(s.endLine, s.endCol),
      });
      this.doc.editOffsets(
        [
          { ...off(xs), text: d.originals[0] },
          { ...off(ys), text: d.originals[1] },
        ],
        { before: false, after: false },
      );
    }
    this.endDrag();
  }

  // ---------------------------------------------------------------- moving an edge

  /**
   * Pick up edge `index` (from point `index` to the next): find where its four coordinates are
   * written and highlight them. Nothing is written until `endEdgeMove`, so a computed coordinate
   * only stops the move if the move would change it.
   */
  beginEdgeMove(element: Element, index: number): EditResult {
    this.edge = null;
    const base = element.source ?? "/start/points";
    const ends = [index, (index + 1) % element.points.length];
    const found: SourceSpan[] = [];
    for (const i of ends) {
      const x = this.find(`${base}/${i}/0`);
      const y = this.find(`${base}/${i}/1`);
      if (!x || !y) return { ok: false, reason: "This edge's ends aren't written as [x, y] in the file." };
      if (sameSpan(x.span, y.span) && x.file === y.file) {
        return { ok: false, reason: `x and y of an end of this edge both come from ${this.where(x)}.` };
      }
      found.push(x, y);
    }
    if (found.some((s) => s.file !== found[0].file)) {
      return { ok: false, reason: "The ends of this edge are written in different files." };
    }
    if (!this.use(found[0]))
      return { ok: false, reason: `${found[0].file} isn't in the workspace any more.` };
    const written: Written[] = [];
    const coords = found.map((s, k) => {
      let at = written.findIndex((w) => sameSpan(w.source.span, s.span));
      if (at < 0) at = written.push({ source: s, original: this.text(s.span) }) - 1;
      const p = element.points[ends[k >> 1]];
      return { at, value: k % 2 ? p.y : p.x };
    });
    // Values shared with other places (a `let` used elsewhere) move those too.
    const pointers = new Set(found.map((s) => s.pointer));
    const shared = this.sources.filter(
      (s) =>
        !pointers.has(s.pointer) &&
        s.file === found[0].file &&
        s.literal &&
        written.some((w) => sameSpan(w.source.span, s.span)),
    ).length;
    this.edge = { doc: this.doc, written, coords };
    this.doc.highlight(written.map((w) => w.source.span));
    const note = shared
      ? `This edge shares coordinates with ${shared} other value${shared === 1 ? "" : "s"}; they move too.`
      : undefined;
    return { ok: true, note };
  }

  /** Write the edge's new ends `a` and `b`, as one undo step. */
  endEdgeMove(a: Point, b: Point): EditResult {
    const e = this.edge;
    if (!e) return { ok: false, reason: "Nothing is being moved." };
    this.doc = e.doc;
    const spans = this.doc.highlighted();
    if (spans.length !== e.written.length) {
      this.cancelEdgeMove();
      return { ok: false, reason: "The file changed while the edge was moved. Try again." };
    }
    const values = [a.x, a.y, b.x, b.y];
    const texts: (string | undefined)[] = e.written.map(() => undefined);
    for (let k = 0; k < 4; k++) {
      const { at, value } = e.coords[k];
      const w = e.written[at];
      const same = Math.abs(values[k] - value) <= 1e-9 * (1 + Math.abs(value));
      if (!same && !w.source.literal) {
        this.cancelEdgeMove();
        return {
          ok: false,
          reason: `This edge's ${k % 2 ? "y" : "x"} is computed on ${this.where(w.source)} (\`${w.original.trim()}\`). Edit it there, or move the edge the other way.`,
        };
      }
      // Unchanged coordinates keep their text exactly as written.
      const text = same ? w.original : this.format(values[k], w.source.kind, w.original);
      if (texts[at] !== undefined && texts[at] !== text) {
        this.cancelEdgeMove();
        return {
          ok: false,
          reason: `Both ends of this edge use the value on ${this.where(w.source)}, so it can't move that way. Edit it there.`,
        };
      }
      texts[at] = text;
    }
    const edits = spans
      .map((span, i) => ({ ...this.offsets(span), text: texts[i]! }))
      .filter((edit) => this.doc.value.slice(edit.start, edit.end) !== edit.text);
    this.edge = null;
    this.doc.clearHighlight();
    if (edits.length) this.doc.editOffsets(edits);
    return { ok: true };
  }

  /** Put the edge down without writing anything. */
  cancelEdgeMove(): void {
    const e = this.edge;
    if (!e) return;
    this.edge = null;
    e.doc.clearHighlight();
  }

  // ---------------------------------------------------------------- adding and removing points

  private pointsArray(element: Element): EditResult & { pointer?: string } {
    const pointer = element.source ?? "/start/points";
    const array = this.find(pointer);
    if (!array) return { ok: false, reason: "The points aren't written as a list in the file." };
    if (!this.use(array)) return { ok: false, reason: `${array.file} isn't in the workspace any more.` };
    if (!array.literal) {
      return {
        ok: false,
        reason: `The points are computed on ${this.where(array)}. Add or remove them there.`,
      };
    }
    return { ok: true, pointer };
  }

  insertPoint(element: Element, after: number, point: Point): EditResult {
    const check = this.pointsArray(element);
    if (!check.ok) return check;
    const item = this.find(`${check.pointer}/${after}`);
    if (!item || !this.sameFile(item)) return { ok: false, reason: "Can't find that point in the file." };
    const literal = `[${formatNumber(point.x, this.units)}, ${formatNumber(point.y, this.units)}]`;
    const edits = [this.insertAfter(item.span, literal)];
    // The new point splits an edge: its port role is copied to the new edge.
    const role = element.kind === "boundary" ? this.find(`/start/ports/${after}`) : undefined;
    if (role?.literal && this.sameFile(role)) edits.push(this.insertAfter(role.span, this.text(role.span)));
    this.doc.editOffsets(edits);
    return { ok: true };
  }

  deletePoint(element: Element, index: number): EditResult {
    const min = element.closed ? 3 : 1;
    if (element.kind === "origin")
      return { ok: false, reason: "The origin can't be removed. Drag it, or start a new plate." };
    if (element.points.length <= min) {
      return {
        ok: false,
        reason: `A ${element.closed ? "boundary" : "spine"} needs at least ${min} point${min > 1 ? "s" : ""}.`,
      };
    }
    const check = this.pointsArray(element);
    if (!check.ok) return check;
    const item = this.find(`${check.pointer}/${index}`);
    if (!item || !this.sameFile(item)) return { ok: false, reason: "Can't find that point in the file." };
    const edits = [this.removal(item.span)];
    // Two edges become one: drop the port role of the edge that starts at the removed point.
    const role = element.kind === "boundary" ? this.find(`/start/ports/${index}`) : undefined;
    if (role?.literal && this.sameFile(role)) edits.push(this.removal(role.span));
    this.doc.editOffsets(edits);
    return { ok: true };
  }

  // ---------------------------------------------------------------- rules and ports

  /** Give boundary edge `index` the port roles `roles` (writes `start.ports`). */
  setEdgeRoles(index: number, roles: string[], edgeCount: number): EditResult {
    const literal = rolesLiteral(roles);
    const ports = this.find("/start/ports");
    if (ports) {
      if (!this.use(ports)) return { ok: false, reason: `${ports.file} isn't in the workspace any more.` };
      if (!ports.literal) {
        return {
          ok: false,
          reason: `The ports are computed on ${this.where(ports)}. Edit them there.`,
        };
      }
      const item = this.find(`/start/ports/${index}`);
      if (item) {
        if (!item.literal || !this.sameFile(item))
          return { ok: false, reason: `That port is computed on ${this.where(item)}.` };
        this.doc.editOffsets([{ ...this.offsets(item.span), text: literal }]);
        return { ok: true };
      }
      const count = this.arrayItems("/start/ports")?.length ?? 0;
      const items = [...Array(Math.max(0, index - count)).fill("'facade"), literal];
      const edit = this.appendItem("/start/ports", items.join(", "));
      if (!edit) return { ok: false, reason: "Can't read the ports list in the file." };
      this.doc.editOffsets([edit]);
      return { ok: true };
    }
    // No `ports` yet: add it after `points`.
    const points = this.find("/start/points");
    if (!points || !this.use(points))
      return { ok: false, reason: "Can't find the boundary's points in the file." };
    const items = [...Array(index).fill("'facade"), literal];
    void edgeCount;
    const field = `ports = [${items.join(", ")}],`;
    this.doc.editOffsets([this.fieldAfter(points.span, field)]);
    return { ok: true };
  }

  /** Add a rule to the plan at `plan` (a JSON pointer, "" for the root). */
  addRule(plan: string, rule: string): EditResult {
    const rules = this.find(`${plan}/rules`);
    if (rules) {
      if (!this.use(rules)) return { ok: false, reason: `${rules.file} isn't in the workspace any more.` };
      if (!rules.literal || rules.kind !== "array") {
        return { ok: false, reason: `The rules are computed on ${this.where(rules)}. Add it there.` };
      }
      const edit = this.appendItem(`${plan}/rules`, rule);
      if (!edit) return { ok: false, reason: "Can't read the rules list in the file." };
      this.doc.editOffsets([edit]);
      return { ok: true };
    }
    const spaces = this.find(`${plan}/spaces`);
    if (!spaces || !this.use(spaces)) return { ok: false, reason: "Can't find where this plan is written." };
    this.doc.editOffsets([this.fieldAfter(spaces.span, `rules = [${rule}],`)]);
    return { ok: true };
  }

  /** Remove the array item at `pointer`, such as a rule (`/rules/3`). */
  removeItem(pointer: string): EditResult {
    const cut = pointer.lastIndexOf("/");
    const index = Number(pointer.slice(cut + 1));
    const array = this.find(pointer.slice(0, cut));
    if (!array || !this.use(array)) return { ok: false, reason: "Can't find that in the file." };
    const items = this.arrayItems(pointer.slice(0, cut));
    const item = items?.[index];
    if (!item) return { ok: false, reason: "Can't find that in the file." };
    this.doc.editOffsets([this.removalAt(item.start, item.end)]);
    return { ok: true };
  }

  // ---------------------------------------------------------------- fields of a record

  /**
   * Set `field` of the record at `record` (a JSON pointer such as `/spaces/bed`) to `literal`:
   * rewritten where it's written, or added to the record.
   */
  setField(record: string, field: string, literal: string): EditResult {
    const have = this.find(`${record}/${field}`);
    if (have) {
      if (!this.use(have)) return { ok: false, reason: `${have.file} isn't in the workspace any more.` };
      // An enum tag (`'bed_queen`) is as good as a literal.
      const tag = /^'[A-Za-z_][\w.]*$/.test(this.text(have.span).trim());
      if (!have.literal && !tag)
        return { ok: false, reason: `Its ${field} is computed on ${this.where(have)}. Edit it there.` };
      this.doc.editOffsets([{ ...this.offsets(have.span), text: literal }]);
      return { ok: true };
    }
    const at = this.recordAt(record);
    if (!at.ok) return at;
    const placed = place(this.doc.value, at.start + 1, `${field} = ${literal},`, { field, pageFields: [] });
    if (placed.existing)
      return { ok: false, reason: `Can't read the ${field} written on ${this.where(at.s)}.` };
    this.doc.editOffsets([{ start: placed.offset, end: placed.offset, text: placed.text }]);
    return { ok: true };
  }

  /** Remove `field` from the record at `record`, where it's written. */
  removeField(record: string, field: string): EditResult {
    const have = this.find(`${record}/${field}`);
    if (!have || !this.use(have)) return { ok: false, reason: `Can't find its ${field} in the file.` };
    const at = this.recordAt(record);
    if (!at.ok || !this.sameFile(at.s))
      return { ok: false, reason: `Its ${field} is set on ${this.where(have)}. Remove it there.` };
    const placed = place(this.doc.value, at.start + 1, `${field} = x,`, { field, pageFields: [] });
    if (!placed.existing)
      return { ok: false, reason: `Its ${field} is set on ${this.where(have)}. Remove it there.` };
    this.doc.editOffsets([this.removalAt(placed.existing.start, this.offsets(have.span).end)]);
    return { ok: true };
  }

  /** The record literal at `pointer`: where its `{` is. */
  private recordAt(
    pointer: string,
  ): { ok: true; start: number; s: SourceSpan } | { ok: false; reason: string } {
    const s = this.find(pointer);
    if (!s || !this.use(s)) return { ok: false, reason: "Can't find where that is written." };
    const start = this.offsets(s.span).start;
    if (!s.literal || s.kind !== "record" || this.doc.value[start] !== "{")
      return { ok: false, reason: `It's computed on ${this.where(s)}. Edit it there.` };
    return { ok: true, start, s };
  }

  // ---------------------------------------------------------------- plan previews

  /** Can the preview case at `base` (`/preview`, `/preview/1`) be resized from the drawing? */
  checkPreviewSize(base: string): EditResult {
    for (const field of ["width", "depth"]) {
      const s = this.find(`${base}/${field}`);
      if (s && !s.literal) {
        this.use(s);
        return {
          ok: false,
          reason: `The preview's ${field} is computed on ${this.where(s)}. Edit it there.`,
        };
      }
    }
    return { ok: true };
  }

  /**
   * Write the size of the preview case at `base`: replaces `width` and `depth` where they're
   * written, adds them when they aren't, and adds `preview` itself to a plan without one.
   */
  setPreviewSize(base: string, width: number, depth: number): EditResult {
    const check = this.checkPreviewSize(base);
    if (!check.ok) return check;
    const size = { width, depth };
    const fields = (["width", "depth"] as const).map((f) => ({ f, s: this.find(`${base}/${f}`) }));
    const present = fields.filter((x) => x.s);
    const missing = fields.filter((x) => !x.s);
    const literal = (f: "width" | "depth") => `${f} = ${formatNumber(size[f], this.units)}`;
    if (!present.length) {
      const record = this.find(base);
      if (!record) {
        return this.addPreviewCase(`{ ${literal("width")}, ${literal("depth")} }`);
      }
      if (!this.use(record) || !record.literal || record.kind !== "record")
        return { ok: false, reason: `The preview is computed on ${this.where(record)}. Edit it there.` };
      const { start, end } = this.offsets(record.span);
      const text = this.doc.value.slice(start, end);
      const inner = text.slice(1, -1).trim();
      const add = `${literal("width")}, ${literal("depth")}`;
      this.doc.editOffsets([
        { start, end, text: inner ? `{ ${add}, ${inner.replace(/,\s*$/, "")} }` : `{ ${add} }` },
      ]);
      return { ok: true };
    }
    if (!this.use(present[0].s!))
      return { ok: false, reason: `${present[0].s!.file} isn't in the workspace any more.` };
    const edits = present.map(({ f, s }, i) => {
      const value = this.format(size[f], s!.kind, this.text(s!.span));
      // A field that isn't written yet goes right after one that is.
      const extra = i === 0 && missing.length ? `, ${literal(missing[0].f)}` : "";
      return { ...this.offsets(s!.span), text: value + extra };
    });
    this.doc.editOffsets(edits);
    return { ok: true };
  }

  /**
   * Add a preview case (a Nickel record literal) to the plan file: as `preview = …` when there
   * is none, else as another item of the list (a single case becomes a list of two). `index` is
   * where the new case ends up.
   */
  addPreviewCase(literal: string): EditResult & { index?: number } {
    const preview = this.find("/preview");
    if (preview) {
      if (!this.use(preview) || !preview.literal)
        return { ok: false, reason: `The preview is computed on ${this.where(preview)}. Add it there.` };
      if (preview.kind === "array") {
        const count = this.arrayItems("/preview")?.length ?? 0;
        const edit = this.appendItem("/preview", literal);
        if (!edit) return { ok: false, reason: "Can't read the preview list in the file." };
        this.doc.editOffsets([edit]);
        return { ok: true, index: count };
      }
      const { start, end } = this.offsets(preview.span);
      const text = this.doc.value;
      const old = text.slice(start, end);
      const lineStart = text.lastIndexOf("\n", start - 1) + 1;
      const indent = /^\s*/.exec(text.slice(lineStart))![0];
      const list =
        old.includes("\n") || old.length + literal.length > 70
          ? `[\n${indent}  ${old.replace(/\n/g, "\n  ")},\n${indent}  ${literal},\n${indent}]`
          : `[${old}, ${literal}]`;
      this.doc.editOffsets([{ start, end, text: list }]);
      return { ok: true, index: 1 };
    }
    // No preview yet: a new field of the plan, after its needs if it has them.
    const anchor = ["/needs", "/spaces", "/rules", "/goals", "/engine", "/grid"]
      .map((p) => this.find(p))
      .find((s) => s && s.file === this.main);
    if (!anchor || !this.use(anchor)) return { ok: false, reason: "Can't find where this plan is written." };
    this.doc.editOffsets([this.fieldAfter(anchor.span, `preview = ${literal},`)]);
    return { ok: true, index: 0 };
  }

  /**
   * The top-level items of the array literal at `pointer`, as offsets in the text. Items
   * written with helpers (`plate.door "a" "b"`) have no position of their own in the source
   * map, so the list is read from its text: split at commas outside brackets, strings and
   * comments.
   */
  private arrayItems(pointer: string): { start: number; end: number }[] | null {
    const array = this.find(pointer);
    if (!array?.literal || array.kind !== "array") return null;
    const text = this.doc.value;
    const { start, end } = this.offsets(array.span);
    if (text[start] !== "[" || text[end - 1] !== "]") return null;
    const items: { start: number; end: number }[] = [];
    let depth = 0;
    let itemStart = -1;
    let itemEnd = -1;
    const close = () => {
      if (itemStart >= 0) items.push({ start: itemStart, end: itemEnd });
      itemStart = -1;
    };
    for (let i = start + 1; i < end - 1; i++) {
      const c = text[i];
      if (c === "#") {
        while (i < end - 1 && text[i] !== "\n") i++;
        continue;
      }
      if (c === '"') {
        if (itemStart < 0) itemStart = i;
        i++;
        while (i < end - 1 && text[i] !== '"') i += text[i] === "\\" ? 2 : 1;
        itemEnd = i + 1;
        continue;
      }
      if (depth === 0 && c === ",") {
        close();
        continue;
      }
      if ("[{(".includes(c)) depth++;
      if ("]})".includes(c)) depth--;
      if (!/\s/.test(c)) {
        if (itemStart < 0) itemStart = i;
        itemEnd = i + 1;
      }
    }
    close();
    return items;
  }

  /** Add `literal` as the last item of the array at `pointer`, in the list's own layout. */
  private appendItem(pointer: string, literal: string): Edit | null {
    const items = this.arrayItems(pointer);
    if (!items) return null;
    if (items.length)
      return this.insertAfterAt(items[items.length - 1].start, items[items.length - 1].end, literal);
    const { start, end } = this.offsets(this.find(pointer)!.span);
    return { start, end, text: `[${literal}]` };
  }

  // ---------------------------------------------------------------- text edit helpers

  private offsets(span: Span): { start: number; end: number } {
    return {
      start: this.doc.offsetOf(span.startLine, span.startCol),
      end: this.doc.offsetOf(span.endLine, span.endCol),
    };
  }

  /** Insert `literal` as a new array item after the item at `span`, in the same layout. */
  private insertAfter(span: Span, literal: string): Edit {
    const { start, end } = this.offsets(span);
    return this.insertAfterAt(start, end, literal);
  }

  private insertAfterAt(start: number, end: number, literal: string): Edit {
    const text = this.doc.value;
    const lineStart = text.lastIndexOf("\n", start - 1) + 1;
    const leading = text.slice(lineStart, start);
    const sep = leading.trim() === "" ? `\n${leading}` : " ";
    let j = end;
    while (text[j] === " " || text[j] === "\t") j++;
    return text[j] === ","
      ? { start: j + 1, end: j + 1, text: `${sep}${literal},` }
      : { start: end, end, text: `,${sep}${literal}` };
  }

  /** Remove an array item, its comma and (if it had one) its line. */
  private removal(span: Span): Edit {
    const { start, end } = this.offsets(span);
    return this.removalAt(start, end);
  }

  private removalAt(start: number, end: number): Edit {
    const text = this.doc.value;
    let j = end;
    while (text[j] === " " || text[j] === "\t") j++;
    const comma = text[j] === ",";
    if (comma) j++;
    const lineStart = text.lastIndexOf("\n", start - 1) + 1;
    let lineEnd = text.indexOf("\n", j);
    if (lineEnd === -1) lineEnd = text.length;
    if (text.slice(lineStart, start).trim() === "" && text.slice(j, lineEnd).trim() === "") {
      return { start: lineStart, end: Math.min(text.length, lineEnd + 1), text: "" };
    }
    if (comma) {
      let k = j;
      while (text[k] === " ") k++;
      return { start, end: k, text: "" };
    }
    let k = start - 1;
    while (k >= 0 && /\s/.test(text[k])) k--;
    return { start: text[k] === "," ? k : start, end, text: "" };
  }

  /** A new field written after the value at `span` (`points = [...],` → a line below). */
  private fieldAfter(span: Span, field: string): Edit {
    const text = this.doc.value;
    const { start, end } = this.offsets(span);
    const lineStart = text.lastIndexOf("\n", start - 1) + 1;
    const indent = /^\s*/.exec(text.slice(lineStart))![0];
    let j = end;
    while (text[j] === " " || text[j] === "\t") j++;
    return text[j] === ","
      ? { start: j + 1, end: j + 1, text: `\n${indent}${field}` }
      : { start: end, end, text: `,\n${indent}${field.replace(/,$/, "")}` };
  }
}

type Edit = { start: number; end: number; text: string };

/** `'blank`, `'"facade.storefront"`, or a list of them. */
export function rolesLiteral(roles: string[]): string {
  const one = (r: string) => (/^[a-z_][a-z0-9_]*$/.test(r) ? `'${r}` : `'"${r}"`);
  return roles.length === 1 ? one(roles[0]) : `[${roles.map(one).join(", ")}]`;
}

function sameSpan(a: Span, b: Span): boolean {
  return (
    a.startLine === b.startLine &&
    a.startCol === b.startCol &&
    a.endLine === b.endLine &&
    a.endCol === b.endCol
  );
}
