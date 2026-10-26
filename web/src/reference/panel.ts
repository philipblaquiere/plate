import * as monaco from "monaco-editor/editor/editor.api";

import { REFERENCE_COMMAND } from "../editor/schema";
import type { InsertKind } from "./insert";
import { locate, type Crumb, type Cursor, type Location } from "./locate";
import { inline, renderMarkdown } from "./markdown";
import { byCategory, catalog, thumbnail } from "../symbols";
import {
  guideId,
  helperId,
  plainSnippet,
  reference,
  search,
  snippetText,
  type Entry,
  type Example,
} from "./model";

/**
 * The reference view of the side bar: every field, rule, helper and guide, generated from the
 * schema (see model.ts). It follows the cursor, showing what the text under it is and what else
 * can be written there, unless pinned. Search finds anything by name; examples insert at the
 * cursor.
 */

export interface ReferenceHost {
  /** The cursor of the focused editor, with its file's text. */
  cursor(): Cursor | null;
  /** A position in a file, by Monaco model URI. */
  cursorAt(uri: string, line: number, column: number): Cursor | null;
  /** Insert a snippet where it belongs near the cursor; a message when it can't (or didn't). */
  insert(snippet: string, kind: InsertKind): { message: string; error?: boolean } | null;
  /** Bring the reference view on screen. */
  reveal(): void;
  toast(message: string, error?: boolean): void;
}

interface View {
  entry: string | null;
  field?: string;
  value?: Location["value"];
  trail?: Crumb[];
}

const STATE_KEY = "plate:reference";

const ICONS = {
  back: '<svg viewBox="0 0 16 16" aria-hidden="true"><path d="M9.5 3.5 5 8l4.5 4.5"/></svg>',
  index:
    '<svg viewBox="0 0 16 16" aria-hidden="true"><path d="M3 4h1.5M3 8h1.5M3 12h1.5M6.5 4H13M6.5 8H13M6.5 12H13"/></svg>',
  follow:
    '<svg viewBox="0 0 16 16" aria-hidden="true"><path d="M4 2.5v8.2l2.3-2.1 1.6 3.6 1.5-.7-1.6-3.5h3.1z"/></svg>',
  pin: '<svg viewBox="0 0 16 16" aria-hidden="true"><path d="M6 2.5h4M7 2.5v4L4.5 9h7L9 6.5v-4M8 9v4.5"/></svg>',
};

const KIND_LABEL: Record<Entry["kind"], string> = {
  guide: "Guide",
  plate: "File",
  record: "Record",
  union: "One of",
  variant: "Form",
  value: "Value",
  helper: "Helper",
};

export class ReferencePanel {
  private view: View = { entry: null };
  private readonly history: View[] = [];
  private pinned = false;
  private visible = false;
  /** What the cursor was last found on: the panel only moves when that changes. */
  private lastFound = "";
  private collapsed = new Set<string>();
  private readonly body: HTMLElement;
  private readonly trailEl: HTMLElement;
  private readonly searchInput: HTMLInputElement;
  private readonly backBtn: HTMLButtonElement;
  private readonly pinBtn: HTMLButtonElement;
  private followTimer = 0;

  constructor(
    private readonly el: HTMLElement,
    private readonly host: ReferenceHost,
  ) {
    el.innerHTML = `
      <div class="side-head">
        <button class="side-btn ref-back" title="Back" aria-label="Back" hidden>${ICONS.back}</button>
        <span class="side-title">Reference</span>
        <button class="side-btn ref-index" title="All topics" aria-label="All topics">${ICONS.index}</button>
        <button class="side-btn ref-pin" aria-pressed="false"></button>
      </div>
      <div class="ref-search">
        <input type="search" spellcheck="false" placeholder="Search fields, rules, values…" aria-label="Search the reference" />
      </div>
      <nav class="ref-trail" aria-label="Where the cursor is" hidden></nav>
      <div class="ref-body" tabindex="-1"></div>`;
    this.body = el.querySelector(".ref-body")!;
    this.trailEl = el.querySelector(".ref-trail")!;
    this.searchInput = el.querySelector(".ref-search input")!;
    this.backBtn = el.querySelector(".ref-back")!;
    this.pinBtn = el.querySelector(".ref-pin")!;

    this.load();
    this.backBtn.addEventListener("click", () => this.back());
    el.querySelector(".ref-index")!.addEventListener("click", () => this.go({ entry: null }));
    this.pinBtn.addEventListener("click", () => this.setPinned(!this.pinned));
    this.searchInput.addEventListener("input", () => this.render());
    this.searchInput.addEventListener("keydown", (e) => this.searchKeys(e));
    // Links anywhere in the panel: topics, fields, inserts, copies.
    el.addEventListener("click", (e) => this.click(e));
    el.addEventListener("keydown", (e) => {
      if (e.key === "Backspace" && e.altKey) this.back();
    });

    monaco.editor.registerCommand(
      REFERENCE_COMMAND,
      (_accessor, uri: string, line: number, column: number) => {
        const cursor = this.host.cursorAt(uri, line, column);
        const found = cursor ? locate(cursor) : undefined;
        this.host.reveal();
        if (found) this.go(this.viewOf(found));
      },
    );

    this.setPinned(this.pinned);
    this.render();
  }

  // ---------------------------------------------------------------- following the cursor

  setVisible(visible: boolean): void {
    const was = this.visible;
    this.visible = visible;
    if (visible && !was) {
      this.lastFound = "";
      this.follow(true);
    }
  }

  focusSearch(): void {
    this.searchInput.focus();
    this.searchInput.select();
  }

  /** The cursor moved (or the file was re-evaluated): show what it's on. */
  follow(now = false): void {
    if (!this.visible || this.pinned) return;
    window.clearTimeout(this.followTimer);
    const run = () => {
      const cursor = this.host.cursor();
      const found = cursor ? locate(cursor) : undefined;
      if (!found) return; // keep the page: whitespace and comments say nothing new
      const key = `${found.entry}#${found.field ?? ""}#${found.value?.value ?? ""}#${found.trail.map((c) => c.label).join("/")}`;
      if (key === this.lastFound) return;
      this.lastFound = key;
      // Following replaces the page; it doesn't fill the back history.
      this.view = this.viewOf(found);
      if (!this.searchInput.value) this.render();
    };
    if (now) run();
    else this.followTimer = window.setTimeout(run, 140);
  }

  private viewOf(found: Location): View {
    return { entry: found.entry, field: found.field, value: found.value, trail: found.trail };
  }

  private setPinned(pinned: boolean): void {
    this.pinned = pinned;
    this.pinBtn.setAttribute("aria-pressed", String(pinned));
    this.pinBtn.innerHTML = pinned ? ICONS.pin : ICONS.follow;
    this.pinBtn.title = pinned
      ? "Pinned: this page stays. Click to follow the cursor again"
      : "Following the cursor. Click to pin this page";
    this.pinBtn.setAttribute("aria-label", pinned ? "Follow the cursor" : "Pin this page");
    this.el.classList.toggle("is-pinned", pinned);
    this.save();
    if (!pinned) {
      this.lastFound = "";
      this.follow(true);
    }
  }

  // ---------------------------------------------------------------- navigation

  /** Show a page and pin it, so moving the cursor (to say where an Insert goes) keeps it. */
  open(entry: string, value?: string): void {
    this.setPinned(true);
    this.go({ entry, value: value ? { value } : undefined });
  }

  /** Show a page (or the index), remembering the current one for Back. */
  go(view: View): void {
    if (this.view.entry !== view.entry || this.view.field !== view.field || this.view.value !== view.value)
      this.history.push(this.view);
    if (this.history.length > 50) this.history.shift();
    this.view = view;
    this.searchInput.value = "";
    this.render();
    this.body.focus({ preventScroll: true });
  }

  private back(): void {
    const prev = this.history.pop();
    if (!prev) return;
    this.view = prev;
    this.render();
  }

  private click(e: MouseEvent): void {
    const target = e.target as HTMLElement;
    const link = target.closest<HTMLElement>("[data-ref]");
    if (link) {
      e.preventDefault();
      const entry = link.dataset.ref || null;
      const value = link.dataset.value;
      const doc = value
        ? reference.entries.get(entry ?? "")?.values.find((v) => v.value === value)?.doc
        : undefined;
      this.go({ entry, field: link.dataset.field, value: value ? { value, doc } : undefined });
      return;
    }
    const insert = target.closest<HTMLElement>("[data-insert]");
    if (insert) {
      const field = insert.dataset.field;
      const page = this.view.entry ? reference.entries.get(this.view.entry) : undefined;
      // A symbol from the gallery goes in the space around the cursor, whatever page this is.
      const pageFields = insert.dataset.pageFields?.split(",") ?? page?.fields.map((f) => f.name) ?? [];
      const kind: InsertKind = field
        ? { field, pageFields, replace: insert.dataset.replace === "true" }
        : insert.dataset.insert!.startsWith("plate.")
          ? "item"
          : "text";
      const result = this.host.insert(insert.dataset.insert!, kind);
      if (result) this.host.toast(result.message, result.error);
      return;
    }
    const copy = target.closest<HTMLElement>("[data-copy]");
    if (copy) {
      void navigator.clipboard?.writeText(copy.dataset.copy!).then(
        () => this.host.toast("Copied"),
        () => this.host.toast("Couldn't copy", true),
      );
      return;
    }
    const section = target.closest<HTMLElement>("[data-section]");
    if (section) {
      const name = section.dataset.section!;
      if (this.collapsed.has(name)) this.collapsed.delete(name);
      else this.collapsed.add(name);
      this.save();
      this.render();
    }
  }

  private searchKeys(e: KeyboardEvent): void {
    const results = [...this.body.querySelectorAll<HTMLElement>(".ref-hit")];
    if (e.key === "Escape") {
      if (this.searchInput.value) {
        this.searchInput.value = "";
        this.render();
      } else this.searchInput.blur();
      e.stopPropagation();
    } else if (e.key === "Enter") {
      results[0]?.click();
    } else if (e.key === "ArrowDown") {
      e.preventDefault();
      results[0]?.focus();
    }
  }

  // ---------------------------------------------------------------- rendering

  private render(): void {
    this.backBtn.hidden = !this.history.length;
    const query = this.searchInput.value.trim();
    this.trailEl.hidden = true;
    if (query) return this.renderSearch(query);
    if (!this.view.entry || !reference.entries.has(this.view.entry)) return this.renderIndex();
    this.renderEntry(reference.entries.get(this.view.entry)!);
  }

  private renderSearch(query: string): void {
    const hits = search(reference, query);
    this.body.innerHTML = hits.length
      ? `<div class="ref-hits" role="list">${hits
          .map(
            (
              h,
            ) => `<button class="ref-hit" role="listitem" data-ref="${esc(h.entry)}"${h.field ? ` data-field="${esc(h.field)}"` : ""}${h.value ? ` data-value="${esc(h.value)}"` : ""}>
              <span class="ref-hit-label${h.field || h.value ? " is-code" : ""}">${esc(h.label)}</span>
              <span class="ref-hit-detail">${inline(h.detail)}</span>
            </button>`,
          )
          .join("")}</div>`
      : `<p class="ref-empty">Nothing called “${esc(query)}”. Try a field (<code>area</code>), a value (<code>'facade</code>) or a rule (<code>door</code>).</p>`;
    const hits$ = [...this.body.querySelectorAll<HTMLElement>(".ref-hit")];
    hits$.forEach((h, i) =>
      h.addEventListener("keydown", (e) => {
        if (e.key === "ArrowDown") (e.preventDefault(), hits$[i + 1]?.focus());
        if (e.key === "ArrowUp") (e.preventDefault(), (hits$[i - 1] ?? this.searchInput).focus());
      }),
    );
    this.body.scrollTop = 0;
  }

  private renderIndex(): void {
    const sections = reference.sections
      .map((s) => {
        const open = !this.collapsed.has(s.title);
        const items = s.ids
          .map((id) => {
            const e = reference.entries.get(id)!;
            const nested = e.kind === "variant" ? " is-nested" : "";
            return `<button class="ref-item${nested}" data-ref="${esc(id)}">
              <span class="ref-item-title">${e.kind === "helper" ? `<code>${esc(e.title)}</code>` : esc(e.title)}</span>
              <span class="ref-item-sum">${inline(e.summary, { codeRef })}</span>
            </button>`;
          })
          .join("");
        return `<section class="ref-section">
          <button class="ref-section-head" data-section="${esc(s.title)}" aria-expanded="${open}">
            <svg class="ex-chevron" viewBox="0 0 12 12" aria-hidden="true"><path d="M4.5 3 7.5 6 4.5 9"/></svg>${esc(s.title)}
            <span class="ref-count">${s.ids.length}</span>
          </button>
          ${open ? `<div class="ref-items">${items}</div>` : ""}
        </section>`;
      })
      .join("");
    const hint = this.pinned
      ? ""
      : `<p class="ref-hint">Click into the file and this panel shows what the cursor is on.</p>`;
    this.body.innerHTML = `${hint}${sections}`;
    this.body.scrollTop = 0;
  }

  private renderEntry(e: Entry): void {
    const v = this.view;
    // Where the cursor is, when it brought us here.
    if (v.trail && v.trail.length > 1) {
      this.trailEl.hidden = false;
      this.trailEl.innerHTML = v.trail
        .map(
          (c, i) =>
            `${i ? '<span class="ref-crumb-sep">›</span>' : ""}<a href="#" class="ref-crumb" data-ref="${esc(c.entry)}"${c.field ? ` data-field="${esc(c.field)}"` : ""}>${esc(c.label)}</a>`,
        )
        .join("");
    }

    const parts: string[] = [];
    parts.push(`<header class="ref-page-head">
      <div class="ref-kicker">${esc(e.section)}${["Guides", "Helpers", "Plate"].includes(e.section) ? "" : ` · ${KIND_LABEL[e.kind]}`}</div>
      <h2 class="ref-h">${e.kind === "helper" ? `<code>${esc(e.title)}</code>` : esc(e.title)}</h2>
      ${e.subtitle && e.kind !== "helper" ? `<code class="ref-sub">${esc(e.subtitle)}</code>` : ""}
      ${e.kind === "helper" && e.subtitle ? `<code class="ref-sub">${esc(e.subtitle)}</code>` : ""}
    </header>`);

    if (v.value) {
      parts.push(
        `<div class="ref-callout"><code>'${esc(v.value.value)}</code>${
          v.value.doc ? ` <span>${inline(v.value.doc, { codeRef })}</span>` : ""
        }</div>`,
      );
    }

    if (e.body) parts.push(`<div class="ref-md">${renderMarkdown(e.body, { codeRef })}</div>`);

    if (e.variants.length) {
      parts.push(
        section(
          e.id === "def:Rules" ? "Kinds of rule" : "Forms",
          `<div class="ref-links">${e.variants.map((id) => linkCard(id)).join("")}</div>`,
        ),
      );
    }
    if (e.accepts.length) {
      parts.push(
        section(
          "Written as",
          `<ul class="ref-accepts">${e.accepts
            .map(
              (a) =>
                `<li>${a.link ? `<a href="#" class="ref-link" data-ref="${esc(a.link)}">${esc(a.label)}</a>` : `<code>${esc(a.label)}</code>`}</li>`,
            )
            .join("")}</ul>`,
        ),
      );
    }

    if (e.helpers.length) {
      const title = e.kind === "variant" ? "Write it with" : "Helpers";
      parts.push(
        section(title, `<div class="ref-links">${e.helpers.map((id) => linkCard(id, true)).join("")}</div>`),
      );
    }
    if (e.rules.length) {
      parts.push(
        section("Writes", `<div class="ref-links">${e.rules.map((id) => linkCard(id)).join("")}</div>`),
      );
    }

    const fieldsFirst = e.kind === "record" || e.kind === "plate";
    const fields = () => {
      if (e.fields.length) {
        parts.push(
          section(
            e.kind === "variant" && e.section === "Rules" ? "Options" : "Fields",
            e.fields.map((f) => fieldBlock(e, f.name, f.name === v.field)).join(""),
          ),
        );
      }
    };
    if (fieldsFirst) fields();
    // The catalog shows (and inserts) every symbol: no template for one.
    if ((e.examples.length || e.template) && e.id !== "def:Symbol") {
      // A template (with tab stops) when there's no example to start from.
      const examples = e.examples.map((x) => exampleBlock(x));
      if (e.template && !examples.length)
        examples.push(exampleBlock({ code: snippetText(e.template), snippet: e.template }, "Template"));
      parts.push(section(e.examples.length ? "Examples" : "Template", examples.join("")));
    }

    if (!fieldsFirst) fields();

    if (e.id === "def:Symbol") {
      parts.push(section("Catalog", symbolGallery(v.value?.value)));
    } else if (e.values.length) {
      parts.push(
        section(
          "Values",
          `<dl class="ref-values">${e.values
            .map(
              (x) =>
                `<div class="ref-value${v.value?.value === x.value ? " is-current" : ""}"><dt><code>'${esc(x.value)}</code></dt><dd>${inline(x.doc ?? "", { codeRef })}</dd></div>`,
            )
            .join("")}</dl>`,
        ),
      );
    }

    if (e.guides.length) {
      parts.push(
        section(
          "Read more",
          `<div class="ref-links">${e.guides.map((g) => linkCard(guideId(g))).join("")}</div>`,
        ),
      );
    }
    if (e.usedBy.length) {
      parts.push(
        section(
          "Used by",
          `<div class="ref-used">${e.usedBy
            .map((u) => {
              const owner = reference.entries.get(u.entry);
              return `<a href="#" class="ref-link" data-ref="${esc(u.entry)}" data-field="${esc(u.field)}"><code>${esc(owner?.kind === "plate" ? u.field : `${owner?.title ?? ""}.${u.field}`)}</code></a>`;
            })
            .join("")}</div>`,
        ),
      );
    }

    this.body.innerHTML = `<article class="ref-page">${parts.join("")}</article>`;
    void colorizeAll(this.body);
    const current = v.field
      ? this.body.querySelector<HTMLElement>(`.ref-field[data-field="${CSS.escape(v.field)}"]`)
      : this.body.querySelector<HTMLElement>(".ref-value.is-current");
    if (current) current.scrollIntoView({ block: "nearest" });
    else this.body.scrollTop = 0;
  }

  // ---------------------------------------------------------------- state

  private load(): void {
    try {
      const saved = JSON.parse(localStorage.getItem(STATE_KEY) ?? "{}") as {
        pinned?: boolean;
        collapsed?: string[];
      };
      this.pinned = !!saved.pinned;
      this.collapsed = new Set(saved.collapsed ?? []);
    } catch {
      /* ignore */
    }
  }

  private save(): void {
    try {
      localStorage.setItem(
        STATE_KEY,
        JSON.stringify({ pinned: this.pinned, collapsed: [...this.collapsed] }),
      );
    } catch {
      /* ignore */
    }
  }
}

// ------------------------------------------------------------------ pieces

function esc(s: string): string {
  return s.replace(
    /[&<>"']/g,
    (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]!,
  );
}

/** `plate.door` in prose links to the helper. */
function codeRef(code: string): string | undefined {
  const m = /^plate\.(\w+)/.exec(code);
  if (m && reference.helperByName.has(m[1])) return helperId(m[1]);
  return undefined;
}

/** Every symbol, by category: a picture, its name, and a click inserts it into the space. */
function symbolGallery(current?: string): string {
  const spaceFields = reference.entries.get("def:Space")?.fields.map((f) => f.name) ?? [];
  return byCategory()
    .map(
      ({ category, names }) => `<h4 class="ref-gallery-head">${esc(category)}</h4>
      <div class="ref-gallery">${names
        .map((name) => {
          const def = catalog[name];
          const [w, d] = def.size;
          return `<button class="ref-symbol${name === current ? " is-current" : ""}" data-insert="${esc(`symbol = '${name},`)}" data-field="symbol" data-replace="true" data-page-fields="${esc(spaceFields.join(","))}" title="${esc(`${def.label}, ${w} × ${d} mm. Insert symbol = '${name} in the space at the cursor`)}">
            ${thumbnail(name, 56) ?? ""}
            <span class="ref-symbol-label">${esc(def.label)}</span>
            <code>'${esc(name)}</code>
          </button>`;
        })
        .join("")}</div>`,
    )
    .join("");
}

function section(title: string, html: string): string {
  return `<section class="ref-part"><h3 class="ref-part-title">${esc(title)}</h3>${html}</section>`;
}

function linkCard(id: string, code = false): string {
  const e = reference.entries.get(id);
  if (!e) return "";
  const title = code || e.kind === "helper" ? `<code>${esc(e.subtitle ?? e.title)}</code>` : esc(e.title);
  return `<a href="#" class="ref-card" data-ref="${esc(id)}"><span class="ref-card-title">${title}</span><span class="ref-card-sum">${inline(e.summary, { codeRef })}</span></a>`;
}

function exampleBlock(x: Example, label?: string, field?: string): string {
  const insert = x.snippet ?? plainSnippet(x.code);
  const into = field ? ` data-field="${esc(field)}"` : "";
  return `<div class="ref-example">
    ${label ? `<div class="ref-example-label">${esc(label)}</div>` : ""}
    <pre class="ref-code" data-lang="nickel"><code>${esc(x.code)}</code></pre>
    <div class="ref-example-actions">
      <button class="ref-act" data-insert="${esc(insert)}"${into} title="Insert at the cursor">Insert</button>
      <button class="ref-act" data-copy="${esc(x.code)}" title="Copy">Copy</button>
    </div>
  </div>`;
}

function fieldBlock(e: Entry, name: string, current: boolean): string {
  const f = e.fields.find((x) => x.name === name)!;
  const type = f.typeLink
    ? `<a href="#" class="ref-type" data-ref="${esc(f.typeLink)}">${esc(f.type)}</a>`
    : `<span class="ref-type">${esc(f.type)}</span>`;
  const values = f.values.length
    ? `<dl class="ref-values ref-values-inline">${f.values
        .map(
          (x) =>
            `<div class="ref-value"><dt><code>'${esc(x.value)}</code></dt><dd>${inline(x.doc ?? "", { codeRef })}</dd></div>`,
        )
        .join("")}</dl>`
    : "";
  const guides = f.guides.length
    ? `<div class="ref-field-more">${f.guides
        .map((g) => {
          const ge = reference.entries.get(guideId(g));
          return ge ? `<a href="#" class="ref-link" data-ref="${esc(ge.id)}">${esc(ge.title)} →</a>` : "";
        })
        .join(" ")}</div>`
    : "";
  return `<div class="ref-field${current ? " is-current" : ""}" data-field="${esc(f.name)}">
    <div class="ref-field-head">
      <code class="ref-fname">${esc(f.name)}</code>
      ${type}
      ${f.required ? '<span class="ref-badge">required</span>' : ""}
      ${f.default !== undefined ? `<span class="ref-default">default <code>${esc(f.default)}</code></span>` : ""}
      <button class="ref-act ref-field-insert" data-insert="${esc(f.snippet)}" data-field="${esc(f.name)}" title="Insert ${esc(snippetText(f.snippet).split("\n")[0])} at the cursor">Insert</button>
    </div>
    ${f.doc ? `<div class="ref-field-doc">${renderMarkdown(f.doc, { codeRef })}</div>` : ""}
    ${values}
    ${f.examples.map((x) => exampleBlock(x, undefined, f.name)).join("")}
    ${guides}
  </div>`;
}

/** Syntax colours for code blocks, from the editor's own highlighter. */
async function colorizeAll(root: HTMLElement): Promise<void> {
  // Code blocks in guides get the same Insert and Copy as examples.
  for (const pre of root.querySelectorAll<HTMLElement>('.ref-md pre.ref-code[data-lang="nickel"]')) {
    const text = pre.textContent ?? "";
    const wrap = document.createElement("div");
    wrap.className = "ref-example";
    pre.replaceWith(wrap);
    wrap.append(pre);
    wrap.insertAdjacentHTML(
      "beforeend",
      `<div class="ref-example-actions"><button class="ref-act" data-insert="${esc(plainSnippet(text))}" title="Insert at the cursor">Insert</button><button class="ref-act" data-copy="${esc(text)}" title="Copy">Copy</button></div>`,
    );
  }
  const blocks = [...root.querySelectorAll<HTMLElement>('pre.ref-code[data-lang="nickel"] > code')];
  await Promise.all(
    blocks.map(async (code) => {
      const html = await monaco.editor.colorize(code.textContent ?? "", "nickel", { tabSize: 2 });
      // Plain spaces, so long lines can wrap in a narrow panel.
      if (code.isConnected) code.innerHTML = html.replace(/&nbsp;|\u00a0/g, " ");
    }),
  );
}
