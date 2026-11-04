import * as api from "./api";
import { TextDoc } from "./editor/editor";

/**
 * The open project's files, as documents. Every file is loaded (they're small), so a plate can be
 * evaluated with everything it imports, including unsaved edits in other tabs.
 *
 * Projects stay loaded once visited, so going back is instant and undo survives; a background
 * refresh picks up anything that changed on disk. Unsaved text is also kept in the browser, per
 * project, so a reload or a switch never loses work: a file whose draft differs from what's on
 * disk comes back dirty.
 */

const DRAFTS_KEY = "plate:drafts";
/** How many projects stay loaded. Others are read again when they're next opened. */
const LOADED = 6;

const draftsKey = (project: string) => `${DRAFTS_KEY}:${project}`;

export interface Doc {
  path: string;
  text: TextDoc;
  /** The text on disk (as last loaded or saved). */
  saved: string;
}

export { importsOf, isPlan, isPlate } from "./plates";

/** What changed: an edit (`path`), dirty states (`dirty`), the file list (`list`, with `renamed`). */
export interface Change {
  path?: string;
  dirty?: boolean;
  list?: boolean;
  renamed?: { from: string; to: string };
}

/** Everything held for one project. */
interface Loaded {
  name: string;
  root: string;
  docs: Map<string, Doc>;
  /** Folders that hold no file yet (folders with files show through their files' paths). */
  folders: Set<string>;
  /** Each file's dirty state as last announced, to tell listeners only when it flips. */
  shownDirty: Map<string, boolean>;
}

export class WorkspaceStore {
  /** Most recently used last. */
  private readonly loaded = new Map<string, Loaded>();
  private current: Loaded = {
    name: "",
    root: "",
    docs: new Map(),
    folders: new Set(),
    shownDirty: new Map(),
  };
  private listeners = new Set<(change: Change) => void>();
  private draftTimer = 0;

  /** The open project's name ("" before one is open). */
  get project(): string {
    return this.current.name;
  }

  get root(): string {
    return this.current.root;
  }

  get docs(): Map<string, Doc> {
    return this.current.docs;
  }

  get folders(): Set<string> {
    return this.current.folders;
  }

  private dirtyChanged(project: Loaded, path: string): boolean {
    const doc = project.docs.get(path);
    const dirty = !!doc && doc.text.value !== doc.saved;
    const changed = project.shownDirty.get(path) !== dirty;
    project.shownDirty.set(path, dirty);
    return changed;
  }

  /** Called on edits (`path`), dirty-state changes (`dirty`) and file list changes (`list`). */
  subscribe(listener: (change: Change) => void): void {
    this.listeners.add(listener);
  }

  private emit(change: Change): void {
    for (const l of this.listeners) l(change);
  }

  /**
   * Make a project the open one. Tell listeners with a `list` change when `announce` is left on;
   * the workbench switches tabs first and asks for it to be quiet.
   */
  async open(name: string, options: { announce?: boolean } = {}): Promise<void> {
    this.flushDrafts();
    const cached = this.loaded.get(name);
    let project = cached;
    if (!project) project = this.adopt(name, await api.readProject(name));
    else void this.refresh(cached!);
    // Most recently used goes last; the oldest are let go (their drafts are safe in the browser).
    this.loaded.delete(name);
    this.loaded.set(name, project);
    this.current = project;
    for (const [other, state] of this.loaded) {
      if (this.loaded.size <= LOADED) break;
      if (other !== name) this.forget(state);
    }
    if (options.announce !== false) this.emit({ list: true });
  }

  private adopt(name: string, contents: api.ProjectContents): Loaded {
    const project: Loaded = {
      name,
      root: contents.root,
      docs: new Map(),
      folders: new Set(contents.folders),
      shownDirty: new Map(),
    };
    const drafts = readDrafts(name);
    for (const f of contents.files) this.add(project, f.path, f.text, drafts[f.path]);
    return project;
  }

  /** Pick up what changed on disk behind a project that was already loaded. Unsaved text stays. */
  private async refresh(project: Loaded): Promise<void> {
    const contents = await api.readProject(project.name).catch(() => null);
    if (!contents || this.loaded.get(project.name) !== project) return;
    let listChanged = false;
    const onDisk = new Set(contents.files.map((f) => f.path));
    for (const f of contents.files) {
      const doc = project.docs.get(f.path);
      if (!doc) {
        this.add(project, f.path, f.text);
        listChanged = true;
      } else if (doc.saved !== f.text) {
        const clean = doc.text.value === doc.saved;
        doc.saved = f.text;
        if (clean) doc.text.setValue(f.text);
        this.dirtyChanged(project, f.path);
        if (project === this.current) this.emit({ dirty: true });
      }
    }
    for (const [path, doc] of [...project.docs]) {
      if (!onDisk.has(path) && doc.text.value === doc.saved) {
        project.docs.delete(path);
        project.shownDirty.delete(path);
        doc.text.dispose();
        listChanged = true;
      }
    }
    project.folders = new Set(contents.folders);
    if (listChanged && project === this.current) this.emit({ list: true });
  }

  /** Let a loaded project go: its documents are closed, its drafts stay in the browser. */
  private forget(project: Loaded): void {
    for (const doc of project.docs.values()) doc.text.dispose();
    this.loaded.delete(project.name);
  }

  /** A project was renamed or deleted: forget what's held, and carry its drafts to its new name. */
  dropProject(name: string, renamedTo?: string): void {
    this.flushDrafts();
    const held = this.loaded.get(name);
    if (held) {
      if (held === this.current) {
        this.current = { name: "", root: "", docs: new Map(), folders: new Set(), shownDirty: new Map() };
      }
      this.forget(held);
    }
    try {
      const drafts = localStorage.getItem(draftsKey(name));
      if (drafts && renamedTo) localStorage.setItem(draftsKey(renamedTo), drafts);
      localStorage.removeItem(draftsKey(name));
    } catch {
      /* drafts are a convenience only */
    }
  }

  /** How many unsaved files a project has, loaded or not. */
  dirtyCount(name: string): number {
    const held = this.loaded.get(name);
    if (held) return [...held.docs.values()].filter((d) => d.text.value !== d.saved).length;
    return Object.keys(readDrafts(name)).length;
  }

  /** A file left its folders: they stay on disk, empty or not. */
  private keepFoldersOf(path: string): void {
    const parts = path.split("/");
    for (let d = 1; d < parts.length; d++) this.folders.add(parts.slice(0, d).join("/"));
  }

  /** Every folder: those with files and the empty ones. */
  allFolders(): string[] {
    const out = new Set(this.folders);
    for (const path of this.docs.keys()) {
      const parts = path.split("/");
      for (let d = 1; d < parts.length; d++) out.add(parts.slice(0, d).join("/"));
    }
    return [...out].sort();
  }

  async createFolder(path: string): Promise<void> {
    const clean = path.replace(/^\/+|\/+$/g, "");
    if (!clean) return;
    if (this.allFolders().includes(clean)) throw new Error(`${clean} already exists`);
    await api.makeFolder(this.project, clean);
    const parts = clean.split("/");
    for (let d = 1; d <= parts.length; d++) this.folders.add(parts.slice(0, d).join("/"));
    this.emit({ list: true });
  }

  /** Move a folder and everything in it (imports follow, as for a file). */
  async renameFolder(from: string, to: string): Promise<void> {
    if (from === to) return;
    if (to.startsWith(`${from}/`)) throw new Error(`Can't move ${from} into itself`);
    if (this.allFolders().includes(to)) throw new Error(`${to} already exists`);
    const inside = this.paths().filter((p) => p.startsWith(`${from}/`));
    await api.makeFolder(this.project, to);
    for (const sub of this.allFolders().filter((f) => f.startsWith(`${from}/`))) {
      await api.makeFolder(this.project, `${to}${sub.slice(from.length)}`);
    }
    for (const path of inside) await this.rename(path, `${to}${path.slice(from.length)}`);
    await api.removeFolder(this.project, from);
    for (const f of [...this.folders]) {
      if (f === from || f.startsWith(`${from}/`)) {
        this.folders.delete(f);
        this.folders.add(`${to}${f.slice(from.length)}`);
      }
    }
    const parts = to.split("/");
    for (let d = 1; d <= parts.length; d++) this.folders.add(parts.slice(0, d).join("/"));
    this.emit({ list: true });
  }

  /** Remove a folder with no files left in it. */
  async removeFolder(path: string): Promise<void> {
    if (this.paths().some((p) => p.startsWith(`${path}/`))) throw new Error(`${path} still has files in it`);
    await api.removeFolder(this.project, path);
    for (const f of [...this.folders]) if (f === path || f.startsWith(`${path}/`)) this.folders.delete(f);
    this.emit({ list: true });
  }

  private add(project: Loaded, path: string, saved: string, draft?: string): Doc {
    const doc: Doc = { path, text: new TextDoc(path, draft ?? saved, project.name), saved };
    project.docs.set(path, doc);
    project.shownDirty.set(path, doc.text.value !== saved);
    doc.text.onChange(() => {
      const flipped = this.dirtyChanged(project, path);
      // A document of a project in the background (a refresh) concerns nobody on screen.
      if (project === this.current) this.emit({ path, dirty: flipped || undefined });
      this.scheduleDrafts();
    });
    return doc;
  }

  paths(): string[] {
    return [...this.docs.keys()].sort();
  }

  get(path: string): Doc | undefined {
    return this.docs.get(path);
  }

  isDirty(path: string): boolean {
    const doc = this.docs.get(path);
    return !!doc && doc.text.value !== doc.saved;
  }

  dirtyPaths(): string[] {
    return this.paths().filter((p) => this.isDirty(p));
  }

  /** Current text of every file, saved or not: what a plate is evaluated against. */
  files(): Record<string, string> {
    const out: Record<string, string> = {};
    for (const [path, doc] of this.docs) out[path] = doc.text.value;
    return out;
  }

  async save(path: string): Promise<void> {
    const doc = this.docs.get(path);
    if (!doc) return;
    const project = this.current;
    const text = doc.text.value;
    await api.writeFile(project.name, path, text);
    doc.saved = text;
    this.dirtyChanged(project, path);
    if (project === this.current) this.emit({ dirty: true });
    this.scheduleDrafts();
  }

  /** A path that isn't taken yet: `plate.ncl`, `plate-2.ncl`, … */
  freePath(wanted: string): string {
    const base = wanted.replace(/\.ncl$/, "");
    if (!this.docs.has(`${base}.ncl`)) return `${base}.ncl`;
    for (let i = 2; ; i++) if (!this.docs.has(`${base}-${i}.ncl`)) return `${base}-${i}.ncl`;
  }

  async create(path: string, text: string): Promise<Doc> {
    if (this.docs.has(path)) throw new Error(`${path} already exists`);
    const project = this.current;
    await api.writeFile(project.name, path, text);
    const doc = this.add(project, path, text);
    if (project === this.current) this.emit({ list: true });
    return doc;
  }

  async rename(from: string, to: string): Promise<Doc> {
    const old = this.docs.get(from);
    if (!old) throw new Error(`${from} doesn't exist`);
    if (this.docs.has(to)) throw new Error(`${to} already exists`);
    const project = this.current;
    await api.renameFile(project.name, from, to);
    // Models can't change their URI: make a new one with the same (possibly unsaved) text.
    const doc = this.add(project, to, old.saved);
    if (old.text.value !== old.saved) doc.text.setValue(old.text.value);
    project.docs.delete(from);
    project.shownDirty.delete(from);
    this.keepFoldersOf(from);
    old.text.dispose();
    this.emit({ list: true, renamed: { from, to } });
    this.scheduleDrafts();
    return doc;
  }

  async remove(path: string): Promise<void> {
    const doc = this.docs.get(path);
    if (!doc) return;
    const project = this.current;
    await api.deleteFile(project.name, path);
    project.docs.delete(path);
    project.shownDirty.delete(path);
    this.keepFoldersOf(path);
    doc.text.dispose();
    this.emit({ list: true });
    this.scheduleDrafts();
  }

  private scheduleDrafts(): void {
    window.clearTimeout(this.draftTimer);
    this.draftTimer = window.setTimeout(() => this.flushDrafts(), 300);
  }

  /** Write the unsaved text of every loaded project to the browser. */
  private flushDrafts(): void {
    window.clearTimeout(this.draftTimer);
    for (const project of this.loaded.values()) {
      const drafts: Record<string, string> = {};
      for (const doc of project.docs.values()) {
        if (doc.text.value !== doc.saved) drafts[doc.path] = doc.text.value;
      }
      try {
        if (Object.keys(drafts).length) localStorage.setItem(draftsKey(project.name), JSON.stringify(drafts));
        else localStorage.removeItem(draftsKey(project.name));
      } catch {
        /* storage unavailable: drafts are a convenience only */
      }
    }
  }
}

function readDrafts(project: string): Record<string, string> {
  try {
    return JSON.parse(localStorage.getItem(draftsKey(project)) ?? "{}") as Record<string, string>;
  } catch {
    return {};
  }
}

/** The drafts of the workspace from before projects belong to the project its files moved to. */
export function adoptLegacyDrafts(project: string): void {
  try {
    const legacy = localStorage.getItem(DRAFTS_KEY);
    if (legacy && !localStorage.getItem(draftsKey(project))) localStorage.setItem(draftsKey(project), legacy);
    localStorage.removeItem(DRAFTS_KEY);
  } catch {
    /* drafts are a convenience only */
  }
}
