import { fuzzyFilter } from "./fuzzy";

/**
 * A search box over a list, opened from the keyboard: quick open (Ctrl+P), the project switcher
 * (Ctrl+Alt+P), and asking for a name. Type to narrow, arrows to move, Enter to pick, Esc to leave.
 */

export interface PaletteItem {
  id: string;
  label: string;
  /** Searched too, shown in a quieter style. */
  detail?: string;
  /** A heading above this item, when it starts a group (hidden while searching). */
  group?: string;
  /** A picture on the left: the address of an image. */
  image?: string;
  /** A marker after the label: unsaved, current, …. */
  badge?: string;
  /** Actions for this row, in a menu (the ⋯ button, or right-click). */
  menu?: (x: number, y: number) => void;
  pick: () => void;
}

export interface PaletteOptions {
  placeholder: string;
  items: PaletteItem[];
  /** Keys shown at the right of the search box. */
  hint?: string;
  /** Always-there actions under the list. */
  footer?: { label: string; action: () => void }[];
  /** What to say when nothing matches. */
  empty?: string;
  /** Tall rows with pictures. */
  tall?: boolean;
  /** Search the detail too. */
  searchDetail?: boolean;
}

let open: Palette | null = null;

/** Close whatever's open, without picking. */
export function closePalette(): void {
  open?.close();
}

export function isPaletteOpen(): boolean {
  return !!open;
}

/** Show a list to pick from. */
export function showPalette(options: PaletteOptions): void {
  open?.close();
  open = new Palette(options);
}

/** Ask for one line of text. `null` when cancelled. */
export function askText(options: {
  title: string;
  value?: string;
  placeholder?: string;
  action: string;
}): Promise<string | null> {
  open?.close();
  return new Promise((resolve) => {
    const palette = new Palette({ placeholder: options.placeholder ?? "", items: [] }, options, resolve);
    open = palette;
  });
}

class Palette {
  private readonly root: HTMLDivElement;
  private readonly input: HTMLInputElement;
  private readonly list: HTMLDivElement;
  private shown: { item: PaletteItem; positions: number[] }[] = [];
  private selected = 0;
  private readonly restoreFocus = document.activeElement as HTMLElement | null;
  private closed = false;

  constructor(
    private readonly options: PaletteOptions,
    private readonly asking?: { title: string; value?: string; action: string },
    private readonly answer?: (value: string | null) => void,
  ) {
    this.root = document.createElement("div");
    this.root.className = "palette-veil";
    this.root.innerHTML = `
      <div class="palette${options.tall ? " is-tall" : ""}" role="dialog" aria-modal="true">
        <div class="palette-title" hidden></div>
        <div class="palette-search">
          <input spellcheck="false" autocomplete="off" />
          <span class="palette-hint"></span>
        </div>
        <div class="palette-list" role="listbox"></div>
        <div class="palette-foot" hidden></div>
      </div>`;
    this.input = this.root.querySelector("input")!;
    this.list = this.root.querySelector(".palette-list")!;
    this.input.placeholder = options.placeholder;
    this.input.setAttribute("aria-label", asking?.title ?? options.placeholder);
    this.root.querySelector(".palette-hint")!.textContent = asking ? "Enter" : (options.hint ?? "");

    if (asking) {
      const title = this.root.querySelector<HTMLElement>(".palette-title")!;
      title.textContent = asking.title;
      title.hidden = false;
      this.input.value = asking.value ?? "";
      this.list.hidden = true;
    }
    const foot = this.root.querySelector<HTMLElement>(".palette-foot")!;
    for (const f of options.footer ?? []) {
      const b = document.createElement("button");
      b.textContent = f.label;
      b.addEventListener("click", () => {
        this.close();
        f.action();
      });
      foot.append(b);
      foot.hidden = false;
    }

    this.root.addEventListener("pointerdown", (e) => {
      if (e.target === this.root) this.close();
    });
    this.input.addEventListener("input", () => {
      this.selected = 0;
      this.render();
    });
    this.root.addEventListener("keydown", (e) => this.key(e));
    document.body.append(this.root);
    this.render();
    this.input.focus();
    this.input.select();
  }

  private key(e: KeyboardEvent): void {
    e.stopPropagation();
    if (e.key === "Escape") {
      e.preventDefault();
      this.close();
    } else if (e.key === "Enter") {
      e.preventDefault();
      if (this.asking) this.finish(this.input.value.trim() || null);
      else this.pick(this.selected);
    } else if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      const n = this.shown.length;
      if (n) this.select((this.selected + (e.key === "ArrowDown" ? 1 : n - 1)) % n);
    } else if (e.key === "Tab") {
      // Stay in the box: the footer's buttons are for the mouse, the list for the arrows.
      e.preventDefault();
    }
  }

  private select(i: number): void {
    this.selected = i;
    const rows = this.list.querySelectorAll<HTMLElement>(".palette-row");
    rows.forEach((row, j) => row.setAttribute("aria-selected", String(j === i)));
    rows[i]?.scrollIntoView({ block: "nearest" });
  }

  private pick(i: number): void {
    const found = this.shown[i];
    if (!found) return;
    this.close(false);
    found.item.pick();
  }

  private render(): void {
    if (this.asking) return;
    const query = this.input.value;
    const { searchDetail } = this.options;
    const text = (item: PaletteItem) => (searchDetail ? `${item.label} ${item.detail ?? ""}` : item.label);
    this.shown = fuzzyFilter(query, this.options.items, text).map(({ item, match }) => ({
      item,
      positions: match.positions,
    }));
    const searching = !!query.trim();
    const rows: HTMLElement[] = [];
    this.shown.forEach(({ item, positions }, i) => {
      if (item.group && !searching) {
        const head = document.createElement("div");
        head.className = "palette-group";
        head.textContent = item.group;
        rows.push(head);
      }
      rows.push(this.row(item, positions, i));
    });
    if (!rows.length) {
      const none = document.createElement("div");
      none.className = "palette-none";
      none.textContent = this.options.empty ?? "Nothing matches.";
      rows.push(none);
    }
    this.list.replaceChildren(...rows);
    this.select(Math.min(this.selected, Math.max(0, this.shown.length - 1)));
  }

  private row(item: PaletteItem, positions: number[], i: number): HTMLElement {
    const row = document.createElement("div");
    row.className = "palette-row";
    row.setAttribute("role", "option");
    if (item.image) {
      const img = document.createElement("img");
      img.className = "palette-thumb";
      img.alt = "";
      img.loading = "lazy";
      img.src = item.image;
      // No drawing to show (an empty project, a plate that doesn't solve): keep the frame.
      img.addEventListener("error", () => img.classList.add("is-missing"));
      row.append(img);
    }
    const body = document.createElement("div");
    body.className = "palette-body";
    const label = document.createElement("div");
    label.className = "palette-label";
    highlight(label, item.label, positions);
    if (item.badge) {
      const badge = document.createElement("span");
      badge.className = "palette-badge";
      badge.textContent = item.badge;
      label.append(badge);
    }
    body.append(label);
    if (item.detail) {
      const detail = document.createElement("div");
      detail.className = "palette-detail";
      detail.textContent = item.detail;
      body.append(detail);
    }
    row.append(body);
    if (item.menu) {
      const more = document.createElement("button");
      more.className = "palette-more";
      more.textContent = "⋯";
      more.title = "More actions";
      more.setAttribute("aria-label", `More actions for ${item.label}`);
      more.addEventListener("click", (e) => {
        e.stopPropagation();
        const r = more.getBoundingClientRect();
        item.menu!(r.left, r.bottom + 2);
      });
      row.append(more);
      row.addEventListener("contextmenu", (e) => {
        e.preventDefault();
        item.menu!(e.clientX, e.clientY);
      });
    }
    row.addEventListener("mousemove", () => {
      if (this.selected !== i) this.select(i);
    });
    row.addEventListener("click", () => this.pick(i));
    return row;
  }

  private finish(value: string | null): void {
    this.answer?.(value);
    this.close(false);
  }

  close(refocus = true): void {
    if (this.closed) return;
    this.closed = true;
    if (open === this) open = null;
    this.root.remove();
    this.answer?.(null);
    if (refocus) this.restoreFocus?.focus?.();
  }
}

/** Text with the matched characters marked (only those within this text: the detail isn't). */
function highlight(el: HTMLElement, text: string, positions: number[]): void {
  const hits = new Set(positions);
  let run = "";
  let marked = false;
  const flush = () => {
    if (!run) return;
    if (marked) {
      const m = document.createElement("mark");
      m.textContent = run;
      el.append(m);
    } else {
      el.append(document.createTextNode(run));
    }
    run = "";
  };
  for (let i = 0; i < text.length; i++) {
    const hit = hits.has(i);
    if (hit !== marked) {
      flush();
      marked = hit;
    }
    run += text[i];
  }
  flush();
}
