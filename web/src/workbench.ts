import { createCodeEditor, type CodeEditor, type EditorCommands, type ViewState } from "./editor/editor";
import { Explorer, type ExplorerActions } from "./explorer";
import { openMenu } from "./menu";
import { libraryKind } from "./plates";
import { SideBar, type SideState } from "./sidebar";
import { isPlan, isPlate, type WorkspaceStore } from "./workspace";

/**
 * The editor side of the app, laid out like VS Code: a side bar (the file explorer, or the
 * reference), and up to two editor groups side by side, each with its own tabs. Every tab shows a
 * document of the open project; the same file can be open in both groups. Each project keeps its
 * own tabs, cursors and shown plate; the side bar is the same for all of them.
 */

const LAYOUT_KEY = "plate:layout";
const projectKey = (project: string) => `plate:project:${project}`;
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
      <div class="group-empty" hidden><span>Open a file from the explorer, or press <b>New</b> to draw a plate.</span></div>
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
  readonly explorer: Explorer;
  /** Where each visited project's editors were, for going back. */
  private readonly viewStates = new Map<string, Map<string, ViewState>[]>();

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
    explorerActions: ExplorerActions,
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
    this.explorer = new Explorer(store, els.explorer, this, explorerActions);
    store.subscribe((change) => {
      if (change.renamed) this.renamed(change.renamed.from, change.renamed.to);
      if (change.list) {
        // Close tabs whose files are gone.
        if (!this.switching) {
          for (const g of this.groups) for (const p of [...g.tabs]) if (!store.get(p)) this.closeIn(g, p);
        }
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
    if (this.mainPath === path || !(isPlate(text) || isPlan(text) || libraryKind(text))) return [];
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
      g.tabsEl
        .querySelector('[aria-selected="true"]')
        ?.scrollIntoView({ block: "nearest", inline: "nearest" });
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

  // ---------------------------------------------------------------- projects

  private legacyLayout: string | null = null;

  /** True while tabs are being swapped: nothing shown belongs to the open project yet. */
  private switching = false;

  /**
   * Put away the open project's editors, before another is opened. Cursors and scroll positions
   * stay in memory for coming back; tabs are in the browser already.
   */
  leaveProject(): void {
    // Already on the way to another project: there's nothing on screen to put away.
    if (this.switching) return;
    this.persist();
    this.switching = true;
    const project = this.store.project;
    for (const g of this.groups) {
      if (g.active) g.views.set(g.active, g.editor.saveViewState());
    }
    if (project)
      this.viewStates.set(
        project,
        this.groups.map((g) => new Map(g.views)),
      );
    for (const g of [...this.groups]) {
      g.tabs = [];
      g.active = null;
      g.views.clear();
      g.editor.setModel(null);
    }
    while (this.groups.length > 1) this.removeGroup(this.groups[this.groups.length - 1]);
    this.mainPath = null;
  }

  /**
   * The store has a new project: show it as it was left. Returns the plate that was shown, if any.
   */
  enterProject(): string | null {
    const views = this.viewStates.get(this.store.project);
    const main = this.restoreTabs();
    this.groups.forEach((g, i) => {
      for (const [path, view] of views?.[i] ?? []) g.views.set(path, view);
      const view = g.active ? g.views.get(g.active) : undefined;
      if (view) g.editor.restoreViewState(view);
    });
    this.switching = false;
    this.render();
    return main;
  }

  /** A project is gone or renamed: forget where its editors were (the new name, if any, inherits it). */
  forgetProject(name: string, renamedTo?: string): void {
    const views = this.viewStates.get(name);
    this.viewStates.delete(name);
    if (views && renamedTo) this.viewStates.set(renamedTo, views);
    try {
      const saved = localStorage.getItem(projectKey(name));
      if (saved && renamedTo) localStorage.setItem(projectKey(renamedTo), saved);
      localStorage.removeItem(projectKey(name));
    } catch {
      /* ignore */
    }
  }

  // ---------------------------------------------------------------- persistence

  persist(): void {
    if (this.switching) return;
    try {
      localStorage.setItem(LAYOUT_KEY, JSON.stringify({ sidebar: this.sidebar.snapshot() }));
      if (this.store.project) {
        localStorage.setItem(
          projectKey(this.store.project),
          JSON.stringify({
            groups: this.groups.map((g) => ({ tabs: g.tabs, active: g.active })),
            active: this.activeGroup,
            main: this.mainPath,
            collapsed: this.explorer?.collapsedFolders() ?? [],
          }),
        );
      }
    } catch {
      /* ignore */
    }
  }

  /** Put the side bar back as it was. */
  restoreSidebar(): void {
    let saved: {
      /** Before the side bar had views: whether the explorer was open. */
      explorer?: boolean;
      sidebar?: Partial<SideState>;
      /** Before projects: the workspace's one set of tabs. */
      groups?: unknown;
    } = {};
    try {
      const raw = localStorage.getItem(LAYOUT_KEY);
      saved = JSON.parse(raw ?? "{}");
      // Restoring the side bar writes this key over, so what it held is kept for a moment.
      if (saved.groups) this.legacyLayout = raw;
    } catch {
      /* ignore */
    }
    if (saved.sidebar) this.sidebar.restore(saved.sidebar);
    else if (saved.explorer === false) this.sidebar.setOpen(false);
  }

  /** The layout from before projects, with its tabs, belongs to the project its files moved to. */
  adoptLegacyLayout(project: string): void {
    try {
      if (this.legacyLayout && !localStorage.getItem(projectKey(project))) {
        localStorage.setItem(projectKey(project), this.legacyLayout);
      }
    } catch {
      /* ignore */
    }
    this.legacyLayout = null;
  }

  /** Reopen a project's tabs. Returns the plate that was shown, if any. */
  private restoreTabs(): string | null {
    let saved: {
      groups?: { tabs: string[]; active: string | null }[];
      active?: number;
      main?: string | null;
      collapsed?: string[];
    } = {};
    try {
      saved = JSON.parse(localStorage.getItem(projectKey(this.store.project)) ?? "{}");
    } catch {
      /* ignore */
    }
    this.explorer.enterProject(saved.collapsed ?? []);
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

function basename(path: string): string {
  return path.slice(path.lastIndexOf("/") + 1);
}
