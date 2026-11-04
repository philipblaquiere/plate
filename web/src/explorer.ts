import { fuzzyMatch } from "./fuzzy";
import { openMenu } from "./menu";
import { directImporters, directImports, importPath, isPlan, isPlate, libraryKind } from "./plates";
import { parentsOf, treeRows } from "./tree";
import type { Workbench } from "./workbench";
import type { WorkspaceStore } from "./workspace";

/**
 * The open project's files: its plates pinned at the top, then the folders; a filter; what the
 * active file uses and what uses it. New, rename, move and delete all happen here. Each project
 * remembers which folders were closed.
 */

export interface ExplorerActions {
  /** Open the project switcher. */
  switchProject(): void;
  /** Copy a file (and what it imports) to another project. */
  copyToProject(path: string): void;
}

export class Explorer {
  private readonly list: HTMLElement;
  private readonly related: HTMLElement;
  private readonly filterInput: HTMLInputElement;
  private collapsed = new Set<string>();
  private filter = "";
  /** The file last scrolled to, so the tree moves only when the active file changes. */
  private revealed: string | null = null;
  /** The file the "uses" section was last worked out for, and whether the project changed since. */
  private relatedFor: string | null | undefined;

  constructor(
    private readonly store: WorkspaceStore,
    private readonly el: HTMLElement,
    private readonly bench: Workbench,
    private readonly actions: ExplorerActions,
  ) {
    this.list = el.querySelector(".explorer-list")!;
    this.related = el.querySelector(".explorer-related")!;
    this.filterInput = el.querySelector(".explorer-filter input")!;
    el.querySelector(".explorer-new")?.addEventListener("click", () => this.promptNew(""));
    el.querySelector(".explorer-new-folder")?.addEventListener("click", () => this.promptNewFolder(""));
    el.querySelector(".explorer-collapse")?.addEventListener("click", () => this.collapseAll());
    el.querySelector(".project-button")?.addEventListener("click", () => this.actions.switchProject());
    this.filterInput.addEventListener("input", () => {
      this.filter = this.filterInput.value;
      this.render();
    });
    this.filterInput.addEventListener("keydown", (e) => {
      e.stopPropagation();
      if (e.key === "Escape") {
        this.filterInput.value = "";
        this.filter = "";
        this.render();
        this.bench.activeEditor().focus();
      } else if (e.key === "ArrowDown" || e.key === "Enter") {
        e.preventDefault();
        this.rows()[0]?.focus();
        if (e.key === "Enter") this.rows()[0]?.click();
      }
    });
    this.list.addEventListener("keydown", (e) => this.key(e));
    // Dropping a file on the list's empty space moves it to the top of the project.
    this.dropTarget(this.list, "");
  }

  /** The folders closed in the open project, to remember. */
  collapsedFolders(): string[] {
    return [...this.collapsed];
  }

  /** Another project opened: show its folders as it left them, with no filter. */
  enterProject(collapsed: readonly string[]): void {
    this.collapsed = new Set(collapsed);
    this.filter = "";
    this.filterInput.value = "";
    this.revealed = null;
    this.relatedFor = undefined;
    this.render();
  }

  focusFilter(): void {
    this.bench.sidebar.show("files");
    this.filterInput.focus();
    this.filterInput.select();
  }

  private collapseAll(): void {
    this.collapsed = new Set(this.store.allFolders());
    this.revealed = null;
    this.render();
    this.bench.persist();
  }

  private rows(): HTMLElement[] {
    return [...this.list.querySelectorAll<HTMLElement>(".ex-row:not(.ex-input)")];
  }

  /** Arrows move through the rows and open or close folders; F2 renames; Delete deletes. */
  private key(e: KeyboardEvent): void {
    const row = (e.target as HTMLElement).closest<HTMLElement>(".ex-row");
    if (!row || row.classList.contains("ex-input")) return;
    const rows = this.rows();
    const i = rows.indexOf(row);
    const dir = row.dataset.dir;
    const path = row.dataset.path;
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      e.stopPropagation();
      rows[i + (e.key === "ArrowDown" ? 1 : -1)]?.focus();
    } else if (e.key === "ArrowRight" && dir) {
      e.preventDefault();
      e.stopPropagation();
      if (this.collapsed.has(dir)) this.toggle(dir);
      else rows[i + 1]?.focus();
    } else if (e.key === "ArrowLeft") {
      e.preventDefault();
      e.stopPropagation();
      if (dir && !this.collapsed.has(dir)) {
        this.toggle(dir);
      } else {
        // To the folder this is in.
        const parent = parentsOf(dir ?? path ?? "").pop();
        if (parent) this.list.querySelector<HTMLElement>(`[data-dir="${CSS.escape(parent)}"]`)?.focus();
      }
    } else if (e.key === "F2") {
      e.preventDefault();
      if (path) this.promptRename(path, row);
      else if (dir) this.promptRenameFolder(dir, row);
    } else if (e.key === "Delete") {
      e.preventDefault();
      if (path) void this.delete(path);
      else if (dir) void this.deleteFolder(dir);
    } else if (e.key === "Escape") {
      this.bench.activeEditor().focus();
    }
  }

  private toggle(dir: string): void {
    const focused = document.activeElement === this.list.querySelector(`[data-dir="${CSS.escape(dir)}"]`);
    if (this.collapsed.has(dir)) this.collapsed.delete(dir);
    else this.collapsed.add(dir);
    this.render();
    this.bench.persist();
    if (focused) this.list.querySelector<HTMLElement>(`[data-dir="${CSS.escape(dir)}"]`)?.focus();
  }

  /** Accept files dragged from the tree: they move into `folder` ("" for the top). */
  private dropTarget(target: HTMLElement, folder: string): void {
    target.addEventListener("dragover", (e) => {
      if (!e.dataTransfer?.types.includes(TREE_DRAG)) return;
      e.preventDefault();
      e.stopPropagation();
      e.dataTransfer.dropEffect = "move";
      target.classList.add("is-drop");
    });
    target.addEventListener("dragleave", () => target.classList.remove("is-drop"));
    target.addEventListener("drop", (e) => {
      const from = e.dataTransfer?.getData(TREE_DRAG);
      target.classList.remove("is-drop");
      if (!from) return;
      e.preventDefault();
      e.stopPropagation();
      void this.moveInto(from, folder);
    });
  }

  /** Move a file or folder into `folder` (imports follow). */
  private async moveInto(from: string, folder: string): Promise<void> {
    const name = from.split("/").pop()!;
    const to = folder ? `${folder}/${name}` : name;
    if (to === from) return;
    try {
      if (this.store.get(from)) await this.store.rename(from, to);
      else await this.store.renameFolder(from, to);
    } catch (err) {
      this.flash((err as Error).message);
    }
  }

  render(): void {
    const name = this.el.querySelector(".explorer-root");
    if (name) {
      name.textContent = this.store.project;
      this.el
        .querySelector(".project-button")
        ?.setAttribute("title", `${this.store.root}\nSwitch project (Ctrl+Alt+P)`);
    }
    const paths = this.store.paths();
    const query = this.filter.trim();
    const matches = query ? (p: string) => !!fuzzyMatch(query, p) : undefined;
    // Plates are what a project is for: pinned at the top, wherever they live.
    const plates = paths.filter((p) => isPlate(this.store.get(p)?.text.value ?? ""));
    const pinned = plates.filter((p) => !matches || matches(p));
    const rows: HTMLElement[] = [];
    if (pinned.length) {
      rows.push(this.heading("Plates"));
      for (const p of pinned) rows.push(this.fileRow(p, 0, true));
    }
    const tree = treeRows(paths, this.store.allFolders(), this.collapsed, {
      pinned: new Set(plates),
      filter: matches,
    });
    if (tree.length && pinned.length) rows.push(this.heading("Files"));
    for (const row of tree) {
      rows.push(
        row.kind === "folder" ? this.folderRow(row.path, row.depth) : this.fileRow(row.path, row.depth),
      );
    }
    if (!rows.length) {
      const hint = document.createElement("div");
      hint.className = "ex-empty";
      hint.textContent = query
        ? "No file here matches."
        : "This project is empty. Press New to draw a plate, or + for a file.";
      rows.push(hint);
    }
    this.list.replaceChildren(...rows);
    this.revealActive();
    this.renderRelated();
  }

  private heading(text: string): HTMLElement {
    const head = document.createElement("div");
    head.className = "ex-heading";
    head.textContent = text;
    return head;
  }

  /** Keep the active file in view: its folders open, its row on screen. Only when it changes. */
  private revealActive(): void {
    const active = this.bench.activePath();
    if (active === this.revealed) return;
    this.revealed = active;
    if (!active) return;
    const closed = parentsOf(active).filter((d) => this.collapsed.has(d));
    // A pinned plate is on screen whatever its folder says.
    if (closed.length && !isPlate(this.store.get(active)?.text.value ?? "")) {
      for (const d of closed) this.collapsed.delete(d);
      this.render();
      return;
    }
    // Only if it's out of view, and the tree is on screen to scroll at all.
    const row = this.list.querySelector(`.ex-file[data-path="${CSS.escape(active)}"]`);
    const list = this.list.getBoundingClientRect();
    if (!row || !list.height) return;
    const at = row.getBoundingClientRect();
    if (at.top < list.top || at.bottom > list.bottom) row.scrollIntoView({ block: "nearest" });
  }

  /** What the active file uses, and what uses it: the links a multi-file plate is made of. */
  private renderRelated(): void {
    const active = this.bench.activePath();
    const version = active ? this.store.get(active)?.text.model.getVersionId() : 0;
    const key = active && `${this.store.project}:${active}:${version}:${this.store.paths().length}`;
    if (key === this.relatedFor) return;
    this.relatedFor = key;
    this.related.replaceChildren();
    this.related.hidden = true;
    if (!active) return;
    const files = this.store.files();
    const uses = directImports(active, files);
    const usedBy = directImporters(active, files);
    if (!uses.length && !usedBy.length) return;
    const section = (label: string, paths: string[]) => {
      if (!paths.length) return;
      const head = document.createElement("div");
      head.className = "ex-heading";
      head.textContent = label;
      this.related.append(head);
      for (const path of paths) {
        const b = document.createElement("button");
        b.className = "ex-row ex-link";
        b.title = path;
        b.innerHTML = `<span class="ex-name"></span>`;
        b.querySelector(".ex-name")!.textContent = path.split("/").pop()!;
        b.addEventListener("click", () => this.bench.open(path));
        this.related.append(b);
      }
    };
    section("Uses", uses);
    section("Used by", usedBy);
    this.related.hidden = false;
  }

  private folderRow(dir: string, depth: number): HTMLElement {
    const row = document.createElement("button");
    row.className = "ex-row ex-folder";
    row.style.setProperty("--depth", String(depth));
    row.dataset.dir = dir;
    row.setAttribute("aria-expanded", String(!this.collapsed.has(dir)));
    row.innerHTML = `<svg class="ex-chevron" viewBox="0 0 12 12" aria-hidden="true"><path d="M4.5 3 7.5 6 4.5 9"/></svg><span class="ex-name"></span>`;
    row.querySelector(".ex-name")!.textContent = dir.split("/").pop()!;
    row.addEventListener("click", () => this.toggle(dir));
    row.addEventListener("contextmenu", (e) => {
      e.preventDefault();
      const files = this.store.paths().filter((p) => p.startsWith(`${dir}/`)).length;
      openMenu(
        dir,
        [
          { label: "New file here…", action: () => this.promptNew(`${dir}/`) },
          { label: "New folder here…", action: () => this.promptNewFolder(`${dir}/`) },
          "-",
          { label: "Rename or move…", note: "F2", action: () => this.promptRenameFolder(dir, row) },
          {
            label: files ? "Delete folder (move its files out first)" : "Delete folder",
            disabled: files > 0,
            action: () => void this.deleteFolder(dir),
          },
        ],
        e.clientX,
        e.clientY,
      );
    });
    row.draggable = true;
    row.addEventListener("dragstart", (e) => {
      e.dataTransfer?.setData(TREE_DRAG, dir);
      if (e.dataTransfer) e.dataTransfer.effectAllowed = "move";
    });
    this.dropTarget(row, dir);
    return row;
  }

  private promptNewFolder(prefix: string): void {
    this.bench.sidebar.show("files");
    const input = this.nameInput(prefix, "studies", "Folder name");
    this.list.prepend(input.row);
    input.done.then(async (name) => {
      if (!name) return;
      try {
        await this.store.createFolder(name);
        this.collapsed.delete(name);
      } catch (err) {
        this.flash((err as Error).message);
      }
    });
  }

  private promptRenameFolder(dir: string, row: HTMLElement): void {
    const input = this.nameInput(dir, "", "Folder name");
    row.replaceWith(input.row);
    input.done.then(async (name) => {
      const to = name?.replace(/^\/+|\/+$/g, "");
      if (!to || to === dir) return this.render();
      try {
        await this.store.renameFolder(dir, to);
      } catch (err) {
        this.flash((err as Error).message);
        this.render();
      }
    });
  }

  private async deleteFolder(dir: string): Promise<void> {
    try {
      await this.store.removeFolder(dir);
    } catch (err) {
      this.flash((err as Error).message);
    }
  }

  /** A file's row. Pinned ones show their folder, since they're not under it. */
  private fileRow(path: string, depth: number, pinned = false): HTMLElement {
    const text = this.store.get(path)?.text.value ?? "";
    const plate = isPlate(text);
    const plan = !plate && isPlan(text);
    const library = plate || plan ? null : libraryKind(text);
    const row = document.createElement("button");
    row.className = "ex-row ex-file";
    row.style.setProperty("--depth", String(depth));
    row.dataset.path = path;
    row.dataset.kind = plate ? "plate" : plan ? "plan" : (library ?? "library");
    row.classList.toggle("is-plate", plate);
    row.classList.toggle("is-main", this.bench.mainPath === path);
    row.classList.toggle("is-open", this.bench.activePath() === path);
    row.classList.toggle("is-dirty", this.store.isDirty(path));
    row.title = plate
      ? `${path}: a plate (it has a start)`
      : plan
        ? `${path}: a plan, shown on its own in its preview slot`
        : library === "service"
          ? `${path}: a service, for a plate's services`
          : library === "assemblies"
            ? `${path}: assemblies, for a plate's assemblies`
            : `${path}: a library to import`;
    row.innerHTML = `<span class="ex-icon" aria-hidden="true"></span><span class="ex-name"></span><span class="ex-where"></span><span class="ex-dot" aria-hidden="true"></span>`;
    row.querySelector(".ex-name")!.textContent = path.split("/").pop()!;
    if (pinned && path.includes("/")) {
      row.querySelector(".ex-where")!.textContent = path.slice(0, path.lastIndexOf("/"));
    }
    row.addEventListener("click", () => this.bench.open(path));
    // Drag onto a folder to move it there (tabs and imports follow).
    row.draggable = true;
    row.addEventListener("dragstart", (e) => {
      e.dataTransfer?.setData(TREE_DRAG, path);
      if (e.dataTransfer) e.dataTransfer.effectAllowed = "move";
    });
    row.addEventListener("contextmenu", (e) => {
      e.preventDefault();
      // As written in the file in front of you (imports are relative to the importing file).
      const active = this.bench.activePath() ?? "";
      const spec = importPath(active.slice(0, active.lastIndexOf("/") + 1), path);
      openMenu(
        path,
        [
          { label: "Open", action: () => this.bench.open(path) },
          {
            label: "Open to the side",
            action: () => this.bench.open(path, { group: this.bench.activeGroup === 0 ? 1 : 0 }),
          },
          ...this.bench.showItems(path),
          "-",
          { label: "Rename…", note: "F2", action: () => this.promptRename(path, row) },
          { label: "Duplicate", action: () => void this.duplicate(path) },
          { label: "Copy to project…", action: () => this.actions.copyToProject(path) },
          {
            label: "Copy import line",
            note: `import "${spec}"`,
            action: () => void navigator.clipboard?.writeText(`import "${spec}"`),
          },
          "-",
          { label: "Delete", note: "Del", action: () => void this.delete(path) },
        ],
        e.clientX,
        e.clientY,
      );
    });
    return row;
  }

  /** An inline name box at the top of the list (or in a folder). */
  private promptNew(prefix: string): void {
    this.bench.sidebar.show("files");
    const input = this.nameInput(prefix, "plan.ncl");
    this.list.prepend(input.row);
    input.done.then(async (name) => {
      if (!name) return;
      const path = withExtension(name);
      try {
        const doc = await this.store.create(path, NEW_FILE);
        this.bench.open(doc.path);
      } catch (err) {
        this.flash((err as Error).message);
      }
    });
  }

  private promptRename(path: string, row: HTMLElement): void {
    const input = this.nameInput(path, "");
    row.replaceWith(input.row);
    input.done.then(async (name) => {
      if (!name || withExtension(name) === path) return this.render();
      try {
        const to = withExtension(name);
        await this.store.rename(path, to); // tabs follow (the workbench hears of it)
      } catch (err) {
        this.flash((err as Error).message);
        this.render();
      }
    });
  }

  private async duplicate(path: string): Promise<void> {
    const doc = this.store.get(path);
    if (!doc) return;
    try {
      const copy = await this.store.create(
        this.store.freePath(path.replace(/\.ncl$/, "-copy")),
        doc.text.value,
      );
      this.bench.open(copy.path);
    } catch (err) {
      this.flash((err as Error).message);
    }
  }

  private async delete(path: string): Promise<void> {
    if (!confirm(`Delete ${path}? This removes the file from the project.`)) return;
    try {
      await this.store.remove(path);
    } catch (err) {
      this.flash((err as Error).message);
    }
  }

  private nameInput(
    value: string,
    placeholder: string,
    label = "File name",
  ): { row: HTMLElement; done: Promise<string | null> } {
    const row = document.createElement("div");
    row.className = "ex-row ex-input";
    const input = document.createElement("input");
    input.value = value;
    input.placeholder = placeholder;
    input.spellcheck = false;
    input.setAttribute("aria-label", label);
    row.append(input);
    const done = new Promise<string | null>((resolve) => {
      let settled = false;
      const finish = (v: string | null) => {
        if (settled) return;
        settled = true;
        row.remove();
        resolve(v?.trim() || null);
      };
      input.addEventListener("keydown", (e) => {
        e.stopPropagation();
        if (e.key === "Enter") finish(input.value);
        if (e.key === "Escape") finish(null);
      });
      input.addEventListener("blur", () => finish(input.value === value ? null : input.value));
    });
    queueMicrotask(() => {
      input.focus();
      // Select the name without the extension, like VS Code.
      const end = input.value.lastIndexOf(".");
      input.setSelectionRange(input.value.lastIndexOf("/") + 1, end > 0 ? end : input.value.length);
    });
    return { row, done };
  }

  private flash(message: string): void {
    const note = document.createElement("div");
    note.className = "ex-error";
    note.textContent = message;
    this.list.prepend(note);
    setTimeout(() => note.remove(), 4000);
  }
}

/** What a row dragged in the file tree carries: its path. */
const TREE_DRAG = "application/x-plate-path";

const NEW_FILE = `# A plan to import from a plate: let my_plan = import "this-file.ncl" in …
{
  spaces = {},
  rules = [],
}
`;

function withExtension(name: string): string {
  const clean = name.replace(/\\/g, "/").replace(/^\/+/, "");
  return clean.endsWith(".ncl") ? clean : `${clean}.ncl`;
}
