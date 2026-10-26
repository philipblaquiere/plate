import * as api from "./api";
import { TextDoc } from "./editor/editor";

/**
 * The workspace folder's files, as documents. Every file is loaded (they're small), so a plate
 * can be evaluated with everything it imports, including unsaved edits in other tabs.
 *
 * Unsaved text is also kept in the browser, so a reload never loses work: a file whose draft
 * differs from what's on disk comes back dirty.
 */

const DRAFTS_KEY = "plate:drafts";

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

export class WorkspaceStore {
  readonly docs = new Map<string, Doc>();
  /** Folders that hold no file yet (folders with files show through their files' paths). */
  readonly folders = new Set<string>();
  root = "";
  private listeners = new Set<(change: Change) => void>();
  private draftTimer = 0;
  /** Each file's dirty state as last announced, to tell listeners only when it flips. */
  private readonly shownDirty = new Map<string, boolean>();

  private dirtyChanged(path: string): boolean {
    const dirty = this.isDirty(path);
    const changed = this.shownDirty.get(path) !== dirty;
    this.shownDirty.set(path, dirty);
    return changed;
  }

  /** Called on edits (`path`), dirty-state changes (`dirty`) and file list changes (`list`). */
  subscribe(listener: (change: Change) => void): void {
    this.listeners.add(listener);
  }

  private emit(change: Change): void {
    for (const l of this.listeners) l(change);
  }

  async load(): Promise<void> {
    const listing = await api.listFiles();
    this.root = listing.root;
    const texts = await Promise.all(listing.files.map((f) => api.readFile(f.path)));
    const drafts = readDrafts();
    listing.files.forEach((f, i) => this.add(f.path, texts[i], drafts[f.path]));
    for (const folder of listing.folders ?? []) this.folders.add(folder);
    this.emit({ list: true });
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
    await api.makeFolder(clean);
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
    await api.makeFolder(to);
    for (const sub of this.allFolders().filter((f) => f.startsWith(`${from}/`))) {
      await api.makeFolder(`${to}${sub.slice(from.length)}`);
    }
    for (const path of inside) await this.rename(path, `${to}${path.slice(from.length)}`);
    await api.removeFolder(from);
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
    await api.removeFolder(path);
    for (const f of [...this.folders]) if (f === path || f.startsWith(`${path}/`)) this.folders.delete(f);
    this.emit({ list: true });
  }

  private add(path: string, saved: string, draft?: string): Doc {
    const doc: Doc = { path, text: new TextDoc(path, draft ?? saved), saved };
    this.docs.set(path, doc);
    this.shownDirty.set(path, this.isDirty(path));
    doc.text.onChange(() => {
      this.emit({ path, dirty: this.dirtyChanged(path) || undefined });
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
    const text = doc.text.value;
    await api.writeFile(path, text);
    doc.saved = text;
    this.dirtyChanged(path);
    this.emit({ dirty: true });
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
    await api.writeFile(path, text);
    const doc = this.add(path, text);
    this.emit({ list: true });
    return doc;
  }

  async rename(from: string, to: string): Promise<Doc> {
    const old = this.docs.get(from);
    if (!old) throw new Error(`${from} doesn't exist`);
    if (this.docs.has(to)) throw new Error(`${to} already exists`);
    await api.renameFile(from, to);
    // Models can't change their URI: make a new one with the same (possibly unsaved) text.
    const doc = this.add(to, old.saved);
    if (old.text.value !== old.saved) doc.text.setValue(old.text.value);
    this.docs.delete(from);
    this.shownDirty.delete(from);
    this.keepFoldersOf(from);
    old.text.dispose();
    this.emit({ list: true, renamed: { from, to } });
    this.scheduleDrafts();
    return doc;
  }

  async remove(path: string): Promise<void> {
    const doc = this.docs.get(path);
    if (!doc) return;
    await api.deleteFile(path);
    this.docs.delete(path);
    this.shownDirty.delete(path);
    this.keepFoldersOf(path);
    doc.text.dispose();
    this.emit({ list: true });
    this.scheduleDrafts();
  }

  private scheduleDrafts(): void {
    window.clearTimeout(this.draftTimer);
    this.draftTimer = window.setTimeout(() => {
      const drafts: Record<string, string> = {};
      for (const path of this.dirtyPaths()) drafts[path] = this.docs.get(path)!.text.value;
      try {
        localStorage.setItem(DRAFTS_KEY, JSON.stringify(drafts));
      } catch {
        /* storage unavailable: drafts are a convenience only */
      }
    }, 300);
  }
}

function readDrafts(): Record<string, string> {
  try {
    return JSON.parse(localStorage.getItem(DRAFTS_KEY) ?? "{}") as Record<string, string>;
  } catch {
    return {};
  }
}
