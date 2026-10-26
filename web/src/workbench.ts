import { createCodeEditor, type CodeEditor, type EditorCommands, type ViewState } from "./editor/editor";
import { openMenu } from "./menu";
import { importPath } from "./plates";
import { SideBar, type SideState } from "./sidebar";
import { isPlan, isPlate, type WorkspaceStore } from "./workspace";

/**
 * The editor side of the app, laid out like VS Code: a side bar (the file explorer, or the
 * reference), and up to two editor groups side by side, each with its own tabs. Every tab shows a document of the workspace
 * store; the same file can be open in both groups.
 */

const LAYOUT_KEY = "plate:layout";
const MAX_GROUPS = 2;
const TAB_MIME = "application/x-plate-tab";

export interface WorkbenchEvents {
  /** The focused tab changed (or `null`: nothing open). */
  onActive(path: string | null): void;
  /** Make this file the plate shown in the drawing. */
  onShowPlate(path: string): void;
  /** The cursor moved in the focused editor. */
  onCursor?(): void;
}

class Group {
  readonly el: HTMLDivElement;
  readonly tabsEl: HTMLDivElement;
  readonly editor: CodeEditor;
  readonly emptyEl: HTMLDivElement;
  tabs: string[] = [];
  active: string | null = null;
  readonly views = new Map<string, ViewState>();

  constructor(parent: HTMLElement, commands: EditorCommands) {
    this.el = document.createElement("div");
    this.el.className = "group";
    this.el.innerHTML = `
      <div class="tabs" role="tablist">
        <div class="tab-list"></div>
        <div class="tab-actions">
          <button class="tab-action" data-act="split" title="Open to the side (Ctrl+\\)">
            <svg viewBox="0 0 16 16" aria-hidden="true"><rect x="2" y="3" width="12" height="10" rx="1.5"/><path d="M8 3v10"/></svg>
          </button>
          <button class="tab-action" data-act="format" title="Format document (Shift+Alt+F)">
            <svg viewBox="0 0 16 16" aria-hidden="true"><path d="M3 4h10M3 7h7M3 10h10M3 13h5"/></svg>
          </button>
        </div>
      </div>
      <div class="group-editor"></div>
      <div class="group-empty" hidden>Open a file from the explorer, or press <b>New</b> to draw a plate.</div>
      <div class="group-drop" hidden></div>`;
    parent.append(this.el);
    this.tabsEl = this.el.querySelector(".tab-list")!;
    this.emptyEl = this.el.querySelector(".group-empty")!;
    this.editor = createCodeEditor(this.el.querySelector(".group-editor")!, commands);
  }
}

export class Workbench {
  readonly groups: Group[] = [];
  activeGroup = 0;
  /** The plate shown in the drawing, marked in tabs and the explorer. */
  mainPath: string | null = null;
  readonly sidebar: SideBar;
  private readonly groupsEl: HTMLElement;
  private readonly explorer: Explorer;

  constructor(
    private readonly store: WorkspaceStore,
    els: {
      groups: HTMLElement;
      explorer: HTMLElement;
      reference: HTMLElement;
      activity: HTMLElement;
      sidebar: HTMLElement;
      sash: HTMLElement;
    },
    private readonly commands: EditorCommands & { format(): void },
    private readonly events: WorkbenchEvents,
  ) {
    this.groupsEl = els.groups;
    this.sidebar = new SideBar({
      activity: els.activity,
      sidebar: els.sidebar,
      sash: els.sash,
      views: { files: els.explorer, reference: els.reference },
    });
    this.sidebar.onChange(() => this.persist());
    this.addGroup();
    this.explorer = new Explorer(store, els.explorer, this);
    store.subscribe((change) => {
      if (change.renamed) this.renamed(change.renamed.from, change.renamed.to);
      if (change.list) {
        // Close tabs whose files are gone.
        for (const g of this.groups) for (const p of [...g.tabs]) if (!store.get(p)) this.closeIn(g, p);
        this.explorer.render();
      }
      if (change.dirty || change.list) this.render();
      if (change.dirty) this.explorer.render();
    });
  }

  // ---------------------------------------------------------------- groups and tabs

  private addGroup(): Group {
    const group = new Group(this.groupsEl, {
      save: () => this.commands.save(),
      open: () => this.commands.open(),
    });
    const index = () => this.groups.indexOf(group);
    group.editor.onDidFocusEditorText(() => this.focusGroup(index()));
    group.editor.onDidChangeCursorPosition(() => {
      if (index() === this.activeGroup) this.events.onCursor?.();
    });
    group.el.addEventListener("pointerdown", () => this.focusGroup(index()), true);
    group.el.querySelector('[data-act="split"]')!.addEventListener("click", () => this.split());
    group.el.querySelector('[data-act="format"]')!.addEventListener("click", () => this.commands.format());
    this.acceptDrops(group);
    this.groups.push(group);
    this.groupsEl.dataset.groups = String(this.groups.length);
    return group;
  }

  private removeGroup(group: Group): void {
    const i = this.groups.indexOf(group);
    if (i < 0 || this.groups.length === 1) return;
    group.editor.dispose();
    group.el.remove();
    this.groups.splice(i, 1);
    this.groupsEl.dataset.groups = String(this.groups.length);
    this.activeGroup = Math.min(this.activeGroup, this.groups.length - 1);
    this.focusGroup(this.activeGroup);
  }

  private focusGroup(i: number): void {
    if (i < 0) return;
    const changed = i !== this.activeGroup;
    this.activeGroup = i;
    this.groups.forEach((g, j) => g.el.classList.toggle("is-active", j === i));
    if (changed) {
      this.events.onActive(this.activePath());
      this.explorer.render();
    }
    this.persist();
  }

  get group(): Group {
    return this.groups[this.activeGroup];
  }

  activePath(): string | null {
    return this.group?.active ?? null;
  }

  activeEditor(): CodeEditor {
    return this.group.editor;
  }

  /** Open a file in a group (the active one by default) and focus it. */
  open(path: string, options: { group?: number; focus?: boolean } = {}): void {
    const doc = this.store.get(path);
    if (!doc) return;
    let gi = options.group ?? this.activeGroup;
    if (gi >= this.groups.length) {
      this.addGroup();
      gi = this.groups.length - 1;
    }
    const g = this.groups[gi];
    if (g.active && g.active !== path) g.views.set(g.active, g.editor.saveViewState());
    if (!g.tabs.includes(path)) {
      const at = g.active ? g.tabs.indexOf(g.active) + 1 : g.tabs.length;
      g.tabs.splice(at, 0, path);
    }
    g.active = path;
    g.editor.setModel(doc.text.model);
    const view = g.views.get(path);
    if (view) g.editor.restoreViewState(view);
    this.activeGroup = gi;
    this.groups.forEach((x, j) => x.el.classList.toggle("is-active", j === gi));
    if (options.focus !== false) g.editor.focus();
    this.render();
    this.explorer?.render();
    this.events.onActive(path);
    this.persist();
  }

  close(path: string, gi = this.activeGroup): void {
    const g = this.groups[gi];
    if (g) this.closeIn(g, path);
  }

  private closeIn(g: Group, path: string): void {
    const i = g.tabs.indexOf(path);
    if (i < 0) return;
    g.tabs.splice(i, 1);
    g.views.delete(path);
    if (g.active === path) {
      const next = g.tabs[Math.min(i, g.tabs.length - 1)] ?? null;
      g.active = null;
      if (next) {
        const gi = this.groups.indexOf(g);
        this.open(next, { group: gi, focus: false });
        return;
      }
      g.editor.setModel(null);
    }
    if (!g.tabs.length && this.groups.length > 1) this.removeGroup(g);
    this.render();
    this.events.onActive(this.activePath());
    this.persist();
  }

  /** Open the active file in the other group too (making a second group if needed). */
  split(): void {
    const path = this.activePath();
    if (!path) return;
    const target = this.activeGroup === 0 ? 1 : 0;
    if (this.groups.length >= MAX_GROUPS || target < this.groups.length) this.open(path, { group: target });
    else this.open(path, { group: this.groups.length });
  }

  /** Show a position in a file: in the group already showing it, else the active group. */
  reveal(path: string, line: number, col: number): void {
    const gi = this.groups.findIndex((g) => g.active === path);
    this.open(path, { group: gi >= 0 ? gi : this.activeGroup });
    const editor = this.group.editor;
    editor.setPosition({ lineNumber: line, column: col });
    editor.revealLineInCenterIfOutsideViewport(line);
    editor.focus();
  }

  /** After a rename: tabs follow the file. */
  renamed(from: string, to: string): void {
    for (const g of this.groups) {
      const i = g.tabs.indexOf(from);
      if (i < 0) continue;
      g.tabs[i] = to;
      if (g.active === from) {
        g.active = to;
        g.editor.setModel(this.store.get(to)!.text.model);
      }
    }
    if (this.mainPath === from) this.mainPath = to;
    this.render();
    this.explorer.render();
    this.persist();
  }

  setReadOnly(on: boolean): void {
    for (const g of this.groups) {
      g.editor.updateOptions({
        readOnly: on,
        readOnlyMessage: { value: "Finish or cancel the drawing first." },
      });
    }
  }

  /** "Show in the drawing" for a plate or a plan that isn't shown. */
  showItems(path: string): { label: string; action: () => void }[] {
    const text = this.store.get(path)?.text.value ?? "";
    if (this.mainPath === path || !(isPlate(text) || isPlan(text))) return [];
    return [{ label: "Show in the drawing", action: () => this.events.onShowPlate(path) }];
  }

  setMain(path: string | null): void {
    this.mainPath = path;
    this.render();
    this.explorer.render();
    this.persist();
  }

  /** Ctrl+B: hide or show the side bar, whichever view it has. */
  toggleSidebar(): void {
    this.sidebar.setOpen(!this.sidebar.open);
  }

  render(): void {
    const names = new Map<string, number>();
    for (const g of this.groups)
      for (const p of g.tabs) names.set(basename(p), (names.get(basename(p)) ?? 0) + 1);
    this.groups.forEach((g, gi) => {
      g.tabsEl.replaceChildren(
        ...g.tabs.map((path) => {
          const tab = document.createElement("div");
          tab.className = "tab";
          tab.setAttribute("role", "tab");
          tab.draggable = true;
          tab.title = path;
          tab.dataset.path = path;
          tab.setAttribute("aria-selected", String(g.active === path));
          tab.classList.toggle("is-dirty", this.store.isDirty(path));
          tab.classList.toggle("is-main", this.mainPath === path);
          const name = document.createElement("span");
          name.className = "tab-name";
          name.textContent = basename(path);
          tab.append(name);
          // Same name in two folders: show where.
          if ((names.get(basename(path)) ?? 0) > 1 && path.includes("/")) {
            const dir = document.createElement("span");
            dir.className = "tab-dir";
            dir.textContent = path.slice(0, path.lastIndexOf("/"));
            tab.append(dir);
          }
          const close = document.createElement("button");
          close.className = "tab-close";
          close.title = "Close (Ctrl+W)";
          close.setAttribute("aria-label", `Close ${basename(path)}`);
          close.addEventListener("click", (e) => {
            e.stopPropagation();
            this.close(path, gi);
          });
          tab.append(close);
          tab.addEventListener("mousedown", (e) => {
            if (e.button === 1) {
              e.preventDefault();
              this.close(path, gi);
            }
          });
          tab.addEventListener("click", () => this.open(path, { group: gi }));
          tab.addEventListener("contextmenu", (e) => {
            e.preventDefault();
            openMenu(
              basename(path),
              [
                { label: "Close", action: () => this.close(path, gi) },
                {
                  label: "Close others",
                  action: () => g.tabs.filter((p) => p !== path).forEach((p) => this.close(p, gi)),
                },
                "-",
                { label: "Open to the side", action: () => this.open(path, { group: gi === 0 ? 1 : 0 }) },
                ...this.showItems(path),
              ],
              e.clientX,
              e.clientY,
            );
          });
          tab.addEventListener("dragstart", (e) => {
            e.dataTransfer?.setData(TAB_MIME, JSON.stringify({ path, from: gi }));
            e.dataTransfer!.effectAllowed = "move";
          });
          return tab;
        }),
      );
      g.emptyEl.hidden = g.tabs.length > 0;
      g.el.classList.toggle("is-empty", g.tabs.length === 0);
    });
  }

  /** Tabs dragged onto a group move there; dropped on the right side of a single group, they split. */
  private acceptDrops(g: Group): void {
    const zone = g.el.querySelector<HTMLElement>(".group-drop")!;
    const sideOf = (e: DragEvent) => {
      const r = g.el.getBoundingClientRect();
      return this.groups.length < MAX_GROUPS && e.clientX > r.left + r.width * 0.6 ? "split" : "here";
    };
    g.el.addEventListener("dragover", (e) => {
      if (!e.dataTransfer?.types.includes(TAB_MIME)) return;
      e.preventDefault();
      zone.hidden = false;
      zone.dataset.side = sideOf(e);
    });
    g.el.addEventListener("dragleave", (e) => {
      if (!g.el.contains(e.relatedTarget as Node)) zone.hidden = true;
    });
    g.el.addEventListener("drop", (e) => {
      zone.hidden = true;
      const raw = e.dataTransfer?.getData(TAB_MIME);
      if (!raw) return;
      e.preventDefault();
      const { path, from } = JSON.parse(raw) as { path: string; from: number };
      const here = this.groups.indexOf(g);
      const target = sideOf(e) === "split" ? this.groups.length : here;
      if (target === from) {
        // Reorder within the group: before the tab it was dropped on.
        const over = (e.target as HTMLElement).closest<HTMLElement>(".tab")?.dataset.path;
        if (over && over !== path) {
          g.tabs.splice(g.tabs.indexOf(path), 1);
          g.tabs.splice(g.tabs.indexOf(over), 0, path);
          this.render();
          this.persist();
        }
        return;
      }
      const source = this.groups[from];
      this.open(path, { group: target });
      if (source && source !== this.group) this.closeIn(source, path);
    });
  }

  // ---------------------------------------------------------------- persistence

  persist(): void {
    try {
      localStorage.setItem(
        LAYOUT_KEY,
        JSON.stringify({
          groups: this.groups.map((g) => ({ tabs: g.tabs, active: g.active })),
          active: this.activeGroup,
          main: this.mainPath,
          sidebar: this.sidebar.snapshot(),
        }),
      );
    } catch {
      /* ignore */
    }
  }

  /** Reopen the tabs of the last session. Returns the plate that was shown, if any. */
  restore(): string | null {
    let saved: {
      groups?: { tabs: string[]; active: string | null }[];
      active?: number;
      main?: string | null;
      /** Before the side bar had views: whether the explorer was open. */
      explorer?: boolean;
      sidebar?: Partial<SideState>;
    } = {};
    try {
      saved = JSON.parse(localStorage.getItem(LAYOUT_KEY) ?? "{}");
    } catch {
      /* ignore */
    }
    if (saved.sidebar) this.sidebar.restore(saved.sidebar);
    else if (saved.explorer === false) this.sidebar.setOpen(false);
    (saved.groups ?? []).slice(0, MAX_GROUPS).forEach((g, gi) => {
      const tabs = g.tabs.filter((p) => this.store.get(p));
      for (const p of tabs) this.open(p, { group: gi, focus: false });
      const active = g.active && tabs.includes(g.active) ? g.active : tabs[tabs.length - 1];
      if (active) this.open(active, { group: Math.min(gi, this.groups.length - 1), focus: false });
    });
    if (saved.active !== undefined && saved.active < this.groups.length) this.focusGroup(saved.active);
    const main = saved.main && this.store.get(saved.main) ? saved.main : null;
    return main;
  }
}

/** The workspace's files as a tree, with new, rename and delete. */
class Explorer {
  private readonly list: HTMLElement;
  private readonly collapsed = new Set<string>();

  constructor(
    private readonly store: WorkspaceStore,
    private readonly el: HTMLElement,
    private readonly bench: Workbench,
  ) {
    this.list = el.querySelector(".explorer-list")!;
    el.querySelector(".explorer-new")?.addEventListener("click", () => this.promptNew(""));
    el.querySelector(".explorer-new-folder")?.addEventListener("click", () => this.promptNewFolder(""));
    // Dropping a file on the list's empty space moves it to the top of the workspace.
    this.dropTarget(this.list, "");
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
    const root = this.el.querySelector(".explorer-root");
    if (root) {
      root.textContent = this.store.root.split(/[\\/]/).filter(Boolean).pop() ?? "workspace";
      root.setAttribute("title", this.store.root);
    }
    const rows: HTMLElement[] = [];
    const seen = new Set<string>();
    // Empty folders are listed like a file in them would be, so they sort among the others.
    const empty = this.store
      .allFolders()
      .filter((f) => !this.store.paths().some((p) => p.startsWith(`${f}/`)));
    const entries = [...this.store.paths(), ...empty.map((f) => `${f}/`)].sort(treeOrder);
    for (const path of entries) {
      const parts = path.split("/");
      // Folders along the way.
      for (let d = 1; d < parts.length; d++) {
        const dir = parts.slice(0, d).join("/");
        if (seen.has(dir)) continue;
        seen.add(dir);
        if ([...this.collapsed].some((c) => dir.startsWith(`${c}/`))) continue;
        rows.push(this.folderRow(dir, d - 1));
      }
      if (path.endsWith("/")) continue;
      const hidden = parts.slice(0, -1).some((_, i) => this.collapsed.has(parts.slice(0, i + 1).join("/")));
      if (!hidden) rows.push(this.fileRow(path, parts.length - 1));
    }
    if (!rows.length) {
      const hint = document.createElement("div");
      hint.className = "ex-empty";
      hint.textContent = "No files yet. Press + for a file, or the folder button to organise your studies.";
      rows.push(hint);
    }
    this.list.replaceChildren(...rows);
  }

  private folderRow(dir: string, depth: number): HTMLElement {
    const row = document.createElement("button");
    row.className = "ex-row ex-folder";
    row.style.setProperty("--depth", String(depth));
    row.setAttribute("aria-expanded", String(!this.collapsed.has(dir)));
    row.innerHTML = `<svg class="ex-chevron" viewBox="0 0 12 12" aria-hidden="true"><path d="M4.5 3 7.5 6 4.5 9"/></svg><span class="ex-name"></span>`;
    row.querySelector(".ex-name")!.textContent = dir.split("/").pop()!;
    row.addEventListener("click", () => {
      if (this.collapsed.has(dir)) this.collapsed.delete(dir);
      else this.collapsed.add(dir);
      this.render();
    });
    row.addEventListener("contextmenu", (e) => {
      e.preventDefault();
      const files = this.store.paths().filter((p) => p.startsWith(`${dir}/`)).length;
      openMenu(
        dir,
        [
          { label: "New file here…", action: () => this.promptNew(`${dir}/`) },
          { label: "New folder here…", action: () => this.promptNewFolder(`${dir}/`) },
          "-",
          { label: "Rename or move…", action: () => this.promptRenameFolder(dir, row) },
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

  private fileRow(path: string, depth: number): HTMLElement {
    const row = document.createElement("button");
    row.className = "ex-row ex-file";
    row.style.setProperty("--depth", String(depth));
    row.dataset.path = path;
    const plate = isPlate(this.store.get(path)?.text.value ?? "");
    row.classList.toggle("is-plate", plate);
    row.classList.toggle("is-main", this.bench.mainPath === path);
    row.classList.toggle("is-open", this.bench.activePath() === path);
    row.classList.toggle("is-dirty", this.store.isDirty(path));
    row.title = plate
      ? `${path}: a plate (it has a start)`
      : isPlan(this.store.get(path)?.text.value ?? "")
        ? `${path}: a plan, shown on its own in its preview slot`
        : `${path}: a library to import`;
    row.innerHTML = `<span class="ex-icon" aria-hidden="true"></span><span class="ex-name"></span><span class="ex-dot" aria-hidden="true"></span>`;
    row.querySelector(".ex-name")!.textContent = path.split("/").pop()!;
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
          { label: "Rename…", action: () => this.promptRename(path, row) },
          { label: "Duplicate", action: () => void this.duplicate(path) },
          {
            label: "Copy import line",
            note: `import "${spec}"`,
            action: () => void navigator.clipboard?.writeText(`import "${spec}"`),
          },
          "-",
          { label: "Delete", action: () => void this.delete(path) },
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
    if (!confirm(`Delete ${path}? This removes the file from the workspace folder.`)) return;
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

/** Folders before files at each level, then by name, like VS Code's explorer. */
function treeOrder(a: string, b: string): number {
  const pa = a.split("/");
  const pb = b.split("/");
  for (let i = 0; i < Math.min(pa.length, pb.length); i++) {
    if (pa[i] === pb[i]) continue;
    const dirA = i < pa.length - 1;
    const dirB = i < pb.length - 1;
    if (dirA !== dirB) return dirA ? -1 : 1;
    return pa[i].localeCompare(pb[i]);
  }
  return pa.length - pb.length;
}

function basename(path: string): string {
  return path.slice(path.lastIndexOf("/") + 1);
}

function withExtension(name: string): string {
  const clean = name.replace(/\\/g, "/").replace(/^\/+/, "");
  return clean.endsWith(".ncl") ? clean : `${clean}.ncl`;
}
