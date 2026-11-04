import "@fontsource-variable/archivo/standard.css";
import "@fontsource-variable/jetbrains-mono/index.css";
import "@fontsource-variable/jetbrains-mono/wght-italic.css";
import "./style.css";

import * as api from "./api";
import { GeometryEdits } from "./editor/geometryEdits";
import { rulesOf, spanFor, valueAt } from "./editor/sources";
import { withSystem } from "./editor/system";
import { defaultUnits, modeOf, template, type StartMode } from "./editor/templates";
import { ConfigEngine } from "./engine/engine";
import { download, pickFile, pickZip, takeLegacyDraft } from "./files";
import { openMenu, type MenuItem } from "./menu";
import { libraryCard } from "./library";
import { nextView, summarize, type ServiceSummary } from "./services";
import { place } from "./reference/insert";
import type { Cursor } from "./reference/locate";
import { ReferencePanel } from "./reference/panel";
import { catalog, suggest, thumbnail } from "./symbols";
import { caseLabel, caseLiteral, casePointer, previewCases, sidesOf } from "./preview";
import type {
  Diagnostic,
  Element,
  ExportFormat,
  Layout,
  Point,
  PreviewView,
  Report,
  SlotInfo,
  SourceSpan,
  System,
} from "./types";
import { Lengths } from "./viewer/format";
import { Viewer, type ContextTarget } from "./viewer/viewer";
import { Workbench } from "./workbench";
import { importersOf, libraryKind, retargetImports } from "./plates";
import { closePalette, askText, isPaletteOpen, showPalette, type PaletteItem } from "./palette";
import {
  byRecent,
  cleanProjectName,
  formatRoute,
  parseRoute,
  planCopy,
  pushRecent,
  timeAgo,
} from "./projects";
import { adoptLegacyDrafts, importsOf, isPlan, isPlate, WorkspaceStore } from "./workspace";

const $ = <T extends HTMLElement = HTMLElement>(id: string) => document.getElementById(id) as T;

// ------------------------------------------------------------------ state

let lastLayout: Layout | null = null;
/** The last configuration that evaluated cleanly, as data. Exports use it. */
let lastConfig: api.ConfigData | null = null;
/** The files the plate in the drawing reads (itself and its imports), as of the last solve. */
let mainFiles = new Set<string>();
/** The last evaluation that succeeded: the texts it read, and where each value came from. `plate`
 * is false when the file in the drawing is a plan shown on its own. */
let lastEval: {
  files: Record<string, string>;
  sources: SourceSpan[];
  config: api.ConfigData;
  plate: boolean;
} | null = null;
/** What was last sent to the solver (the configuration as named for the drawing, and the view). */
let lastSent: { config: api.ConfigData; view: PreviewView } | null = null;
/** Notes from the solver that aren't problems (e.g. a plate changes the plan it previews). */
let notes: string[] = [];

/**
 * What a plan file in the drawing is shown in: one of its preview cases, or the slot a plate placed
 * it in (opened from that plate's drawing). Remembered per file for the session.
 */
type PlanView =
  { case: number } | { slot: SlotInfo; from: { plate: string; space: string; typical?: string } };
const planViews = new Map<string, PlanView>();
const planView = (path: string): PlanView => planViews.get(path) ?? { case: 0 };
let solveSeq = 0;
let inflight: AbortController | null = null;
let debounce = 0;
/** False while the workspace loads and the last session's tabs reopen. */
let ready = false;

// ------------------------------------------------------------------ workspace, editors, viewer

const store = new WorkspaceStore();
const viewer = new Viewer($("canvas"));
const configEngine = new ConfigEngine();
const geo = new GeometryEdits((file) => store.get(file)?.text);

const workbench = new Workbench(
  store,
  {
    groups: $("groups"),
    explorer: $("explorer"),
    reference: $("reference"),
    activity: $("activity"),
    sidebar: $("sidebar"),
    sash: $("sidebar-sash"),
  },
  { save: () => void save(), open: () => void openFromComputer(), format: () => formatActive() },
  {
    onActive: (path) => {
      if (!ready) return;
      // Looking at a plate shows it. Looking at a plan keeps the plate in the drawing if it uses
      // the plan (so editing the plan re-solves it there), and otherwise shows the plan on its own.
      if (ready && path && path !== mainPath() && !drawing) {
        const text = store.get(path)?.text.value ?? "";
        const own = isPlan(text) || libraryKind(text) !== null;
        if (isPlate(text) || (own && !mainFiles.has(path))) showPlate(path);
      }
      updateTitle();
      referencePanel.follow();
    },
    onShowPlate: (path) => showPlate(path),
    onCursor: () => referencePanel.follow(),
  },
  {
    switchProject: () => void openProjectSwitcher(),
    copyToProject: (path) => void copyToProject(path),
  },
);

const mainPath = () => workbench.mainPath;

// ------------------------------------------------------------------ reference panel

/** A position in a workspace file, with what the last evaluation knows about it. */
function cursorIn(path: string, line: number, column: number): Cursor | null {
  const doc = store.get(path);
  if (!doc) return null;
  const text = doc.text.value;
  const offset = doc.text.model.getOffsetAt({ lineNumber: line, column });
  // The source map is only good for the text it was made from.
  const fresh = lastEval && lastEval.files[path] === text && lastEval.sources.some((s) => s.file === path);
  return {
    text,
    offset,
    plate: isPlate(text),
    evaluated: fresh
      ? { file: path, sources: lastEval!.sources, config: lastEval!.config, plate: lastEval!.plate }
      : undefined,
  };
}

const referencePanel: ReferencePanel = new ReferencePanel($("reference"), {
  cursor() {
    const path = workbench.activePath();
    const pos = workbench.activeEditor().getPosition();
    return path && pos ? cursorIn(path, pos.lineNumber, pos.column) : null;
  },
  cursorAt(uri, line, column) {
    const doc = [...store.docs.values()].find((d) => d.text.model.uri.toString() === uri);
    return doc ? cursorIn(doc.path, line, column) : null;
  },
  insert(snippet, kind) {
    if (drawing) return { message: "Finish or cancel the drawing first.", error: true };
    const editor = workbench.activeEditor();
    const model = editor.getModel();
    const pos = editor.getPosition();
    if (!model || !pos) return { message: "Open a file to insert into.", error: true };
    const placed = place(model.getValue(), model.getOffsetAt(pos), snippet, kind);
    editor.focus();
    if (placed.existing) {
      const a = model.getPositionAt(placed.existing.start);
      const b = model.getPositionAt(placed.existing.end);
      editor.setSelection({
        startLineNumber: a.lineNumber,
        startColumn: a.column,
        endLineNumber: b.lineNumber,
        endColumn: b.column,
      });
      editor.revealLineInCenterIfOutsideViewport(a.lineNumber);
      return {
        message: `It's already there: ${model.getValueInRange({ startLineNumber: a.lineNumber, startColumn: a.column, endLineNumber: b.lineNumber, endColumn: b.column })}`,
      };
    }
    if (placed.end !== undefined) {
      // Replacing a value: the snippet goes over the selection.
      const a = model.getPositionAt(placed.offset);
      const b = model.getPositionAt(placed.end);
      editor.setSelection({
        startLineNumber: a.lineNumber,
        startColumn: a.column,
        endLineNumber: b.lineNumber,
        endColumn: b.column,
      });
    } else editor.setPosition(model.getPositionAt(placed.offset));
    const controller = editor.getContribution("snippetController2") as unknown as {
      insert(template: string, options?: { adjustWhitespace?: boolean }): void;
    } | null;
    controller?.insert(placed.text, { adjustWhitespace: false });
    return null;
  },
  reveal: () => workbench.sidebar.show("reference"),
  toast: (message, error) => toast(message, error),
});
workbench.sidebar.onChange((s) => referencePanel.setVisible(s.open && s.view === "reference"));

store.subscribe((change) => {
  if (!ready) return;
  if (change.renamed) void followMove(change.renamed.from, change.renamed.to);
  if (change.list) {
    const main = mainPath();
    if (main && !store.get(main)) {
      showPlate(firstPlate());
    } else {
      // A file the plate imports may have appeared, moved or gone.
      window.clearTimeout(debounce);
      debounce = window.setTimeout(runSolve, 90);
    }
  }
  if (change.path && !drawing) {
    const path = change.path;
    // A file that gains a `start` while it's being edited becomes the plate in the drawing.
    if (path === workbench.activePath() && path !== mainPath() && isPlate(store.get(path)!.text.value)) {
      showPlate(path);
    } else if (mainFiles.has(path) || path === mainPath() || !lastLayout) {
      window.clearTimeout(debounce);
      // While a point is dragged the drawing is already up to date; solve once at the end.
      if (!geo.dragging) debounce = window.setTimeout(runSolve, 90);
    }
  }
  updateTitle();
});

/**
 * After a file moves, point imports at its new place (and the moved file's own imports, from its
 * new folder). Files without other unsaved changes are saved, so the folder stays consistent.
 */
async function followMove(from: string, to: string): Promise<void> {
  const updated: string[] = [];
  for (const path of store.paths()) {
    const doc = store.get(path)!;
    const next = retargetImports(
      doc.text.value,
      path === to ? from : path,
      path,
      { from, to },
      (p) => !!store.get(p),
    );
    if (next === null) continue;
    const clean = !store.isDirty(path);
    doc.text.setValue(next);
    if (clean) await store.save(path).catch(() => undefined);
    updated.push(path);
  }
  if (updated.length) toast(`Updated the import${updated.length > 1 ? "s" : ""} in ${updated.join(", ")}`);
}

/** Make `path` the plate (or plan) in the drawing (`null`: none). */
function showPlate(path: string | null): void {
  if (path === mainPath()) return;
  workbench.setMain(path);
  resetSheet();
  viewer.setLayout(null);
  void runSolve();
}

/** Show plan file `path` on its own, in `view`. */
function showPlan(path: string, view: PlanView): void {
  planViews.set(path, view);
  if (path === mainPath()) void runSolve();
  else showPlate(path);
}

/** A plate to show when there's no other choice: the open one, else the first in the folder. */
function firstPlate(): string | null {
  const active = workbench.activePath();
  if (active && isPlate(store.get(active)?.text.value ?? "")) return active;
  return store.paths().find((p) => isPlate(store.get(p)!.text.value)) ?? null;
}

// ------------------------------------------------------------------ solving

/**
 * Every edit runs two steps:
 *  1. In the browser: Nickel evaluates the plate with the files it imports and the schema
 *     contract checks it (worker). Errors stop here, with exact positions, without involving
 *     the server.
 *  2. On the server: the checked configuration data is validated again and solved.
 */
async function runSolve(): Promise<void> {
  if (drawing) return;
  const main = mainPath();
  const seq = ++solveSeq;
  inflight?.abort();
  inflight = new AbortController();
  const signal = inflight.signal;
  const slow = window.setTimeout(() => seq === solveSeq && setStatus("busy", "Solving…"), 200);
  try {
    const doc = main ? store.get(main) : undefined;
    if (!main || !doc) {
      showNothing(nothingMessage());
      return;
    }
    if (!doc.text.value.trim()) {
      showNothing(`${main} is empty. Press New to draw a plate.`);
      return;
    }
    const files = store.files();
    mainFiles = importsOf(main, files);
    const evaluated = await configEngine.evaluate({ main, files });
    if (seq !== solveSeq) return;
    if (!evaluated.ok) {
      showErrors(evaluated.diagnostics);
      return;
    }
    lastConfig = evaluated.config;
    const plate = "start" in evaluated.config;
    // A library (a service, a set of assemblies) has nothing to draw: say what it is.
    if (evaluated.kind === "service" || evaluated.kind === "assemblies") {
      showLibrary(evaluated.kind, main, evaluated.config, files);
      return;
    }
    $("library").hidden = true;
    lastEval = { files, sources: evaluated.sources, config: evaluated.config, plate };
    referencePanel.follow();

    // A plan file is solved in its view, and named after its file in the drawing.
    const sent = plate ? { config: evaluated.config, view: "default" as PreviewView } : planRequest(main);
    lastSent = sent;
    const res = await api.solve(sent.config, signal, sent.view);
    if (seq !== solveSeq) return;
    if (res.ok) {
      lastLayout = res.layout;
      geo.update(evaluated.sources, res.layout.units, main);
      // The solver says which value a finding is about; the source map says where that is.
      // Its findings (requirements unmet, preferences missed, floor left over) are problems;
      // other information is a note on the preview.
      const located = res.diagnostics
        .filter((d) => d.severity !== "info" || d.code)
        .map((d) => {
          if (!d.pointer || d.span) return d;
          const at = spanFor(d.pointer, evaluated.sources);
          return at ? { ...d, span: at.span, file: at.file } : d;
        });
      notes = res.diagnostics.filter((d) => d.severity === "info" && !d.code).map((d) => d.message);
      setDiagnostics(located);
      viewer.setEdgeRoles(edgeRoles(evaluated.config));
      // A plan's preview bar sits under the file chip: keep the drawing clear of it.
      viewer.insets.top = plate ? 84 : 120;
      viewer.setLayout(res.layout);
      updateViews(res.layout, located);
      setStale(null);
      const ms = evaluated.ms + res.timings.solveMs;
      const took = `${ms < 1 ? "<1" : Math.round(ms)} ms`;
      const report = res.report;
      const unmet = located.filter((d) => d.severity !== "info").length;
      if (report) {
        const state = { perfect: "perfect", solved: "ok", incomplete: "warn", empty: "idle" } as const;
        setStatus(state[report.status], `${verdictText(report, res.layout)} · ${took}`);
      } else {
        setStatus(unmet ? "warn" : "ok", `Solved in ${took}${unmet ? ` · ${unmet} unmet` : ""}`);
      }
      showVerdict(report ?? null, res.layout);
      $("status").title =
        `Checked in the browser in ${Math.round(evaluated.ms)} ms, solved in ${res.timings.solveMs.toFixed(1)} ms`;
      $("sb-engine").textContent = `${res.solver} solver`;
      $("empty").hidden = true;
      updateTitleBlock();
    } else {
      showErrors(res.diagnostics);
    }
  } catch (err) {
    if ((err as Error).name === "AbortError" || seq !== solveSeq) return;
    // The file is valid; only the solver is unreachable (it may still be compiling).
    setDiagnostics([]);
    setStatus("offline", "Waiting for engine");
    $("sb-engine").textContent = "";
    if (!lastLayout) showEmpty((err as Error).message);
    window.clearTimeout(debounce);
    debounce = window.setTimeout(runSolve, 1500);
  } finally {
    window.clearTimeout(slow);
    updateSheetFile();
    updatePreviewBar();
  }
}

// ------------------------------------------------------------------ views and libraries

/** The service shown over the plan (S cycles), or none: the plan on its own. */
let serviceView: string | null = null;
let services: ServiceSummary[] = [];

/** The views a solved plate has (the plan, and each of its services), with their problems. */
function updateViews(layout: Layout | null, diagnostics: Diagnostic[]): void {
  const declared = (lastConfig?.services ?? {}) as Record<string, { label?: string }>;
  const labels = Object.fromEntries(
    Object.entries(declared)
      .map(([k, v]) => [k, v?.label ?? ""])
      .filter(([, v]) => v),
  );
  services = layout ? summarize(layout, diagnostics, labels) : [];
  if (serviceView && !services.some((s) => s.name === serviceView)) serviceView = null;
  $("views").hidden = services.length === 0;
  const bar = $("views-bar");
  bar.replaceChildren();
  const chip = (name: string | null, label: string, issues = 0) => {
    const b = document.createElement("button");
    b.className = "view-chip";
    b.setAttribute("aria-pressed", String(serviceView === name));
    b.textContent = label;
    b.title = name ? `Show ${label} over the plan (S)` : "The plan on its own (S)";
    if (issues) {
      const badge = document.createElement("span");
      badge.className = "view-badge";
      badge.textContent = String(issues);
      badge.title = `${issues} not met`;
      b.append(badge);
    }
    b.onclick = () => setServiceView(name);
    bar.append(b);
  };
  chip(null, "Plan");
  for (const s of services) chip(s.name, s.label, s.issues);
  viewer.setServiceView(serviceView);
  renderLegend();
}

function setServiceView(name: string | null): void {
  serviceView = name;
  updateViews(lastLayout, lastDiagnostics());
}

viewer.onCycleView = () =>
  setServiceView(
    nextView(
      serviceView,
      services.map((s) => s.name),
    ),
  );

/** What the shown service placed, and how it's connected. */
function renderLegend(): void {
  const legend = $("legend");
  const s = services.find((x) => x.name === serviceView);
  legend.hidden = !s;
  if (!s) return;
  const esc = (t: string) => t.replace(/[&<>"]/g, (c) => `&#${c.charCodeAt(0)};`);
  const parts = s.parts
    .map((p) => {
      const thumb = p.symbol ? (thumbnail(p.symbol, 18) ?? "") : "";
      return `<li><span class="legend-thumb">${thumb}</span>${esc(p.label)}<span class="legend-count">${p.count}</span></li>`;
    })
    .join("");
  const nets = s.networks
    .map(
      (n) =>
        `<div class="legend-net">${esc(n.name)}${n.groups ? ` · ${n.groups} group${n.groups > 1 ? "s" : ""}` : ""}</div>`,
    )
    .join("");
  legend.innerHTML = `<ul>${parts || "<li>Nothing placed</li>"}</ul>${nets}`;
}

/** A library file: what it is, and which plates use it, instead of a drawing. */
function showLibrary(
  kind: "service" | "assemblies",
  main: string,
  config: Record<string, unknown>,
  files: Record<string, string>,
): void {
  resetSheet();
  lastConfig = config;
  viewer.setLayout(null);
  const name = main
    .split("/")
    .pop()!
    .replace(/\.ncl$/, "");
  const card = $("library");
  card.innerHTML = libraryCard(kind, name, config, {
    usedBy: importersOf(main, files),
    thumbnail: (symbol) => thumbnail(symbol, 36),
  });
  card.hidden = false;
  setStatus("ok", kind === "service" ? "A service, to use in a plate" : "Assemblies, to use in a plate");
}

/** The configuration and view a plan file in the drawing is solved with. */
function planRequest(main: string): { config: api.ConfigData; view: PreviewView } {
  const config = lastConfig!;
  const stem = main
    .split("/")
    .pop()!
    .replace(/\.ncl$/, "");
  const v = planView(main);
  if ("slot" in v)
    return { config: { ...config, name: `${stem} in ${v.from.space}` }, view: { slot: v.slot } };
  const cases = previewCases(config);
  const index = Math.min(v.case, Math.max(0, cases.length - 1));
  const name = cases.length > 1 ? `${stem} · ${caseLabel(cases[index], index)}` : stem;
  return { config: { ...config, name }, view: { case: index } };
}

function nothingMessage(): string {
  const active = workbench.activePath();
  if (active) return `${active} isn't a plate or a plan to draw. Open one, or press New.`;
  return store.paths().length
    ? "Open a plate from the files (marked with a diagonal), or press New to draw one."
    : "Press New to draw a plate.";
}

/** Forget the drawing and its checks: the sheet is blank (no plate, or about to draw). */
function resetSheet(): void {
  solveSeq++;
  inflight?.abort();
  window.clearTimeout(debounce);
  lastLayout = null;
  lastConfig = null;
  lastSent = null;
  notes = [];
  mainFiles = new Set();
  setDiagnostics([]);
  setStale(null);
  $("empty").hidden = true;
  $("titleblock").hidden = true;
  $("library").hidden = true;
  $("sb-engine").textContent = "";
  updateViews(null, []);
  showVerdict(null, null);
  updateSheetFile();
  updatePreviewBar();
}

function showNothing(message: string): void {
  resetSheet();
  viewer.setLayout(null);
  setStatus("idle", mainPath() ? "Empty file" : "No plate");
  showEmpty(message);
}

function showErrors(diagnostics: Diagnostic[]): void {
  setDiagnostics(diagnostics);
  const n = diagnostics.filter((d) => d.severity === "error").length;
  setStatus("error", n === 1 ? "1 error" : `${n} errors`);
  $("status").title = "";
  setStale(lastLayout ? diagnostics[0] : null);
  if (!lastLayout) showEmpty("Fix the error in the configuration to see the drawing.");
}

/** The verdict in words: what holds, and what keeps it from being perfect. */
function verdictText(report: Report, layout: Layout): string {
  const parts: string[] = [];
  if (report.unmet) parts.push(`${report.unmet} unmet`);
  if (report.preferences)
    parts.push(`${report.preferences} preference${report.preferences === 1 ? "" : "s"} missed`);
  if (report.leftover > 1e-9) parts.push(`${Lengths.for(layout).area(report.leftover)} left over`);
  const head = { perfect: "Perfect", solved: "Solved", incomplete: "Incomplete", empty: "Nothing to place" }[
    report.status
  ];
  return [head, ...parts].join(" · ");
}

/** The verdict on the drawing: perfect, solved with something to improve, or incomplete. It
 * opens the problems (each with what to change) and, for a plate, can try other seeds. */
function showVerdict(report: Report | null, layout: Layout | null): void {
  const el = $("verdict");
  el.hidden = !report || !layout || report.status === "empty";
  if (el.hidden) return;
  const status = report!.status;
  el.dataset.status = status;
  const icon = { perfect: "✓", solved: "◐", incomplete: "!", empty: "" }[status];
  const detail =
    status === "perfect"
      ? "Every rule met, no floor left over"
      : status === "incomplete"
        ? "See what isn't met, and what to change"
        : "See what keeps it from perfect";
  el.innerHTML =
    `<button class="verdict-main"><span class="verdict-icon"></span><span class="verdict-text"></span>` +
    `<span class="verdict-detail"></span></button>`;
  el.querySelector(".verdict-icon")!.textContent = icon;
  el.querySelector(".verdict-text")!.textContent = verdictText(report!, layout!);
  el.querySelector(".verdict-detail")!.textContent = detail;
  el.querySelector<HTMLButtonElement>(".verdict-main")!.onclick = () => {
    const panel = $("problems");
    if (panel.hidden) return;
    panel.scrollTop = 0;
    panel.classList.remove("is-flashing");
    void panel.offsetWidth;
    panel.classList.add("is-flashing");
    panel.querySelector<HTMLButtonElement>(".problem")?.focus();
  };
  // The search is stochastic: another seed may do better. Plates only (a plan's preview
  // seed lives in its preview case).
  if (status !== "perfect" && lastEval?.plate) {
    const more = document.createElement("button");
    more.className = "verdict-action";
    more.textContent = "Try other seeds";
    more.title = "Solve with the next few seeds and offer the best";
    more.onclick = () => void trySeeds(more, report!);
    el.append(more);
  }
}

/** Seeds tried at a time by "Try other seeds". */
const SEED_TRIES = 6;

/** Solve the plate with the next few seeds; offer the best one if it beats this one. */
async function trySeeds(button: HTMLButtonElement, current: Report): Promise<void> {
  const config = lastConfig;
  if (!config) return;
  const seq = solveSeq;
  const base = Number(config.seed ?? 1);
  const rank = (r: Report) =>
    [
      { perfect: 0, solved: 1, incomplete: 2, empty: 3 }[r.status],
      r.unmet,
      r.preferences,
      r.leftover,
    ] as const;
  const better = (a: Report, b: Report) => {
    const [x, y] = [rank(a), rank(b)];
    for (let i = 0; i < x.length; i++) if (x[i] !== y[i]) return x[i] < y[i];
    return false;
  };
  button.disabled = true;
  let best: { seed: number; report: Report; layout: Layout } | null = null;
  for (let k = 1; k <= SEED_TRIES; k++) {
    const seed = base + k;
    button.textContent = `Trying seed ${seed}…`;
    const res = await api.solve({ ...config, seed }, undefined, "default").catch(() => null);
    if (seq !== solveSeq) return; // the file changed meanwhile
    if (!res?.ok || !res.report) continue;
    if (better(res.report, best?.report ?? current)) best = { seed, report: res.report, layout: res.layout };
    if (best?.report.status === "perfect") break;
  }
  if (!best) {
    button.textContent = `No better seed up to ${base + SEED_TRIES}`;
    toast("No better seed nearby: the problems say what to change in the file.");
    return;
  }
  const found = best;
  button.disabled = false;
  button.classList.add("is-found");
  button.textContent = `Use seed ${found.seed}: ${verdictText(found.report, found.layout)}`;
  button.onclick = () =>
    reportEdit(geo.setField("", "seed", String(found.seed)), `The plate's seed is ${found.seed} now`);
}

function setStatus(
  state: "idle" | "busy" | "ok" | "perfect" | "warn" | "error" | "offline",
  text: string,
): void {
  const el = $("status");
  el.dataset.state = state;
  el.textContent = text;
}

/** The file a diagnostic is about: the plate unless it says otherwise. */
const fileOf = (d: Diagnostic) => d.file ?? mainPath() ?? "";

/** Squiggles in each file, and the problems list under the editors. */
/** The problems last shown. */
let shown: Diagnostic[] = [];
const lastDiagnostics = () => shown;

function setDiagnostics(diagnostics: Diagnostic[]): void {
  shown = diagnostics;
  const byFile = new Map<string, Diagnostic[]>();
  for (const d of diagnostics) byFile.set(fileOf(d), [...(byFile.get(fileOf(d)) ?? []), d]);
  for (const [path, doc] of store.docs) doc.text.setDiagnostics(byFile.get(path) ?? []);
  showProblems(diagnostics);
}

function revealDiagnostic(d: Diagnostic): void {
  const file = fileOf(d);
  if (store.get(file)) workbench.reveal(file, d.span?.startLine ?? 1, d.span?.startCol ?? 1);
}

function showProblems(diagnostics: Diagnostic[]): void {
  const panel = $("problems");
  panel.replaceChildren();
  panel.hidden = diagnostics.length === 0;
  // Requirements first, then preferences and floor left over.
  const rank = (d: Diagnostic) => (d.severity === "error" ? 0 : d.severity === "warning" ? 1 : 2);
  for (const d of [...diagnostics].sort((a, b) => rank(a) - rank(b))) {
    const row = document.createElement("button");
    row.className = `problem problem-${d.severity}${d.code ? ` problem-${d.code}` : ""}`;
    const other = d.file && d.file !== mainPath();
    const where = d.span
      ? `${other ? `${d.file}:${d.span.startLine}` : `Ln ${d.span.startLine}, Col ${d.span.startCol}`}`
      : other
        ? d.file!
        : "";
    const kind = { unmet: "Unmet", preference: "Preference", leftover: "Left over" } as const;
    row.innerHTML =
      `<span class="problem-body"><span class="problem-msg"></span><span class="problem-hint"></span></span>` +
      `<span class="problem-where"></span>`;
    const msg = row.querySelector(".problem-msg")!;
    if (d.code) {
      const tag = document.createElement("span");
      tag.className = "problem-kind";
      tag.textContent = kind[d.code];
      msg.append(tag);
    }
    msg.append(d.message);
    // What to change: the solver's suggestion, right under the problem.
    const hint = row.querySelector<HTMLElement>(".problem-hint")!;
    hint.textContent = d.notes?.join(" ") ?? "";
    hint.hidden = !d.notes?.length;
    row.querySelector(".problem-where")!.textContent = where;
    row.addEventListener("click", () => revealDiagnostic(d));
    panel.append(row);
  }
}

function setStale(d: Diagnostic | null): void {
  const el = $<HTMLButtonElement>("stale");
  el.hidden = !d;
  $("canvas").classList.toggle("is-stale", !!d);
  if (!d) return;
  const file = d.file && d.file !== mainPath() ? d.file : "";
  const line = d.span ? `line ${d.span.startLine}` : "";
  const where = file && line ? `${file}, ${line}: ` : file ? `${file}: ` : line ? `L${line.slice(1)}: ` : "";
  el.textContent = `Showing the last valid drawing. ${where}${d.message}`;
  el.onclick = () => revealDiagnostic(d);
}

function showEmpty(message: string): void {
  const el = $("empty");
  el.hidden = false;
  el.textContent = message;
}

// ------------------------------------------------------------------ title block + status bar

const MODE_LABEL: Record<StartMode, string> = { boundary: "Boundary", spine: "Spine", free: "Free" };
let scaleText = "";

function updateTitleBlock(): void {
  const layout = lastLayout;
  $("titleblock").hidden = !layout;
  if (!layout) return;
  const lengths = Lengths.for(layout);
  const mode = modeOf(lastConfig);
  const element = layout.elements[0];
  $("tb-name").textContent = layout.name;
  $("tb-mode").textContent = element?.kind === "slot" ? "Plan" : mode ? MODE_LABEL[mode] : "";

  const facts: [string, string][] = [];
  const spaces = layout.elements.filter((e) => e.kind === "space");
  const top = spaces.filter((e) => e.space?.depth === 0);
  const placed = top.reduce((sum, e) => sum + (e.area ?? 0), 0);
  if (element?.kind === "slot") {
    const xs = element.points.map((p) => p.x);
    const ys = element.points.map((p) => p.y);
    const size = (v: number[]) => lengths.length(Math.max(...v) - Math.min(...v));
    facts.push(["Slot", `${size(xs)} × ${size(ys)}`]);
    const used = element.area ? Math.round((100 * placed) / element.area) : 0;
    facts.push(["Spaces", `${spaces.length} · ${used}% used`]);
  } else if (element?.kind === "boundary") {
    facts.push(["Area", lengths.area(element.area ?? 0)]);
    if (spaces.length) {
      const used = element.area ? Math.round((100 * placed) / element.area) : 0;
      facts.push(["Spaces", `${spaces.length} · ${used}% used`]);
    } else {
      facts.push(["Perimeter", lengths.length(element.length)]);
    }
  } else if (element?.kind === "spine") {
    facts.push(["Length", lengths.length(element.length)]);
    facts.push(
      spaces.length
        ? ["Spaces", `${spaces.length} · ${lengths.area(placed)}`]
        : ["Points", String(element.points.length)],
    );
  } else if (element?.kind === "origin") {
    const p = element.points[0];
    if (spaces.length) facts.push(["Spaces", `${spaces.length} · ${lengths.area(placed)}`]);
    else facts.push(["Origin", `${lengths.length(p.x, true)}, ${lengths.length(p.y, true)}`]);
  }
  facts.push(["Scale", scaleText]);
  const dl = $("tb-facts");
  dl.replaceChildren();
  for (const [label, value] of facts) {
    const dt = document.createElement("dt");
    const dd = document.createElement("dd");
    dt.textContent = label;
    dd.textContent = value;
    if (label === "Scale") dd.id = "tb-scale";
    dl.append(dt, dd);
  }
  syncSystemSwitch(layout.system);
}

viewer.onStatus = (s) => {
  $("sb-cursor").textContent = s.cursor
    ? `x ${s.lengths.length(s.cursor.x, true)}    y ${s.lengths.length(s.cursor.y, true)}`
    : " ";
  $("sb-tool").textContent =
    s.tool === "draw"
      ? s.snapStep
        ? `Draw: snapping to ${s.lengths.length(s.snapStep)}`
        : "Draw: snapping off"
      : s.tool === "measure"
        ? "Measure: click two points, Esc to clear"
        : "Pan and inspect";
  scaleText = `1:${s.scale.toLocaleString("en-US")}`;
  const scale = document.getElementById("tb-scale");
  if (scale) scale.textContent = scaleText;
  drawScaleBar(s.pxPerUnit, s.lengths);
  for (const b of document.querySelectorAll<HTMLButtonElement>("[data-tool]")) {
    b.setAttribute("aria-pressed", String(b.dataset.tool === s.tool));
  }
};

/** Alternating black/white scale bar of a round length, like a printed drawing. */
function drawScaleBar(pxPerUnit: number, lengths: Lengths): void {
  const svg = $("tb-bar") as unknown as SVGSVGElement;
  const step = lengths.scaleBar(pxPerUnit);
  const segments = 4;
  const w = step * pxPerUnit;
  const parts: string[] = [];
  for (let i = 0; i < segments; i++) {
    parts.push(
      `<rect x="${1 + (i * w) / segments}" y="2" width="${w / segments}" height="5" class="${i % 2 ? "bar-light" : "bar-dark"}"/>`,
    );
  }
  const unit = lengths.system === "metric" ? ` ${lengths.rulerUnit}` : "";
  parts.push(`<text x="1" y="19" class="bar-label">0</text>`);
  parts.push(
    `<text x="${1 + w}" y="19" text-anchor="end" class="bar-label">${lengths.tick(step)}${unit}</text>`,
  );
  svg.setAttribute("width", String(Math.ceil(w + 2)));
  svg.innerHTML = parts.join("");
}

// ------------------------------------------------------------------ which plate is drawn

/** The chip on the sheet naming the plate file in the drawing; it opens a menu of plates. */
function updateSheetFile(): void {
  const main = mainPath();
  const chip = $("sheet-file");
  chip.hidden = !main || !!drawing;
  const plan = !!main && !isPlate(store.get(main)?.text.value ?? "");
  chip.classList.toggle("is-plan", plan);
  chip.title = plan ? "The plan in the drawing, shown on its own" : "The plate in the drawing";
  $("sheet-file-name").textContent = main ?? "";
}

$("sheet-file").addEventListener("click", (e) => {
  const main = mainPath();
  const texts = (p: string) => store.get(p)!.text.value;
  const plates = store.paths().filter((p) => isPlate(texts(p)));
  const plans = store.paths().filter((p) => isPlan(texts(p)));
  const items: MenuItem[] = [];
  if (main) items.push({ label: `Open ${main}`, action: () => workbench.open(main) }, "-");
  for (const p of plates) items.push({ label: p, checked: p === main, action: () => showPlate(p) });
  if (plans.length) items.push("-");
  for (const p of plans) {
    items.push({ label: p, note: "plan", checked: p === main, action: () => showPlate(p) });
  }
  const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
  openMenu("In the drawing", items, r.left, r.bottom + 4);
});

// ------------------------------------------------------------------ plan previews

const caret = `<svg class="preview-caret" viewBox="0 0 12 12" aria-hidden="true"><path d="M3 4.5 6 7.5 9 4.5" /></svg>`;

function barButton(
  label: string,
  className: string,
  action: (e: MouseEvent) => void,
  title = "",
): HTMLElement {
  const b = document.createElement("button");
  b.className = className;
  b.innerHTML = `<span></span>`;
  b.firstElementChild!.textContent = label;
  if (className.includes("preview-pick")) b.insertAdjacentHTML("beforeend", caret);
  if (title) b.title = title;
  b.addEventListener("click", action);
  return b;
}

function barText(className: string, html: string): HTMLElement {
  const span = document.createElement("span");
  span.className = className;
  span.innerHTML = html;
  return span;
}

const escapeHtml = (s: string) =>
  s.replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]!);

/** Size of the preview slot in the drawing, e.g. "4 × 7.5 m". */
function slotSize(): { w: number; h: number; text: string } | null {
  const slot = lastLayout?.elements.find((e) => e.kind === "slot");
  if (!slot || !lastLayout) return null;
  const xs = slot.points.map((p) => p.x);
  const ys = slot.points.map((p) => p.y);
  const w = Math.max(...xs) - Math.min(...xs);
  const h = Math.max(...ys) - Math.min(...ys);
  const L = Lengths.for(lastLayout);
  return { w, h, text: `${L.length(w)} × ${L.length(h)}` };
}

/**
 * The bar under the file chip while a plan is in the drawing: which slot it's shown in, the
 * other slots it can be shown in, and the plates that use it.
 */
function updatePreviewBar(): void {
  const bar = $("preview-bar");
  const main = mainPath();
  const show = !!main && !!lastLayout && !drawing && lastLayout.elements[0]?.kind === "slot";
  bar.hidden = !show;
  bar.replaceChildren();
  if (!show || !main) return;
  const size = slotSize();
  const v = planView(main);
  const users = importersOf(main, store.files());
  const sep = () => barText("preview-sep", "");

  if ("slot" in v) {
    const { plate, space, typical } = v.from;
    bar.append(
      barText("preview-tag", "In"),
      barText(
        "preview-what",
        `<b>${escapeHtml(space)}</b> of ${escapeHtml(plate)}${typical ? ` · type ${escapeHtml(typical)}` : ""}`,
      ),
    );
    if (notes.length) bar.append(sep(), barText("preview-note", "Changed by the plate"));
    bar.lastElementChild!.setAttribute("title", notes.join("\n") || `${size?.text ?? ""}`);
    bar.append(
      sep(),
      barButton(
        "Save as preview",
        "preview-action",
        () => saveSlotAsPreview(),
        "Add this slot to the plan's previews",
      ),
      barButton(`Back to ${plate}`, "", () => {
        planViews.delete(main);
        if (store.get(plate)) {
          workbench.open(plate);
          showPlate(plate);
        } else {
          void runSolve();
        }
      }),
    );
    return;
  }

  const cases = previewCases(lastConfig);
  bar.append(barText("preview-tag", "Preview"));
  if (!cases.length) {
    bar.append(
      barText("preview-what", `Sized from needs · ${escapeHtml(size?.text ?? "")}`),
      sep(),
      barButton(
        "Set size",
        "preview-action",
        () => setPreviewSize(),
        "Write this size into the file as `preview`",
      ),
    );
  } else {
    const index = Math.min(v.case, cases.length - 1);
    const label = caseLabel(cases[index], index);
    if (cases.length > 1) {
      bar.append(
        barButton(label, "preview-pick", (e) => {
          const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
          openMenu(
            "Preview",
            cases.map((c, i) => ({
              label: caseLabel(c, i),
              checked: i === index,
              action: () => showPlan(main, { case: i }),
            })),
            r.left,
            r.bottom + 4,
          );
        }),
      );
    }
    bar.append(barText("preview-what", escapeHtml(size?.text ?? "")));
  }
  if (users.length) {
    bar.append(
      sep(),
      barButton(
        users.length === 1 ? `Used in ${users[0]}` : `Used in ${users.length} plates`,
        "preview-pick",
        (e) => {
          const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
          openMenu(
            "Show the plate",
            users.map((p) => ({ label: p, action: () => (workbench.open(p), showPlate(p)) })),
            r.left,
            r.bottom + 4,
          );
        },
      ),
    );
  }
}

/** No preview in the file yet: write the slot it's shown in, so it can be changed. */
function setPreviewSize(): void {
  const size = slotSize();
  if (!size) return;
  reportEdit(
    geo.setPreviewSize("/preview", size.w, size.h),
    "Added a preview to the plan. Drag a corner to resize it.",
  );
}

/** The slot from a plate becomes one of the plan's previews (and the one shown). */
function saveSlotAsPreview(): void {
  const main = mainPath();
  const v = main ? planView(main) : null;
  if (!main || !v || !("slot" in v)) return;
  const { roles, approximate } = sidesOf(v.slot);
  const literal = caseLiteral({
    name: v.from.space,
    width: v.slot.width,
    depth: v.slot.depth,
    units: v.slot.units,
    system: v.slot.system,
    seed: v.slot.seed,
    sides: roles,
  });
  const result = geo.addPreviewCase(literal);
  if (!result.ok) return reportEdit(result);
  planViews.set(main, { case: result.index ?? 0 });
  toast(
    approximate
      ? `Saved ${v.from.space} as a preview. Its sides with more than one port keep the main one.`
      : `Saved ${v.from.space} as a preview.`,
  );
}

/** From a plate's drawing: show the plan of `info` (in `file`) on its own, in that space's slot. */
function openPlanIn(file: string, info: NonNullable<Element["space"]>): void {
  const plate = mainPath();
  if (!plate || !info.slot) return;
  showPlan(file, { slot: info.slot, from: { plate, space: info.name, typical: info.typical } });
  workbench.open(file);
}

/** The preview case a slot element is written as (`/preview`, `/preview/1`). */
function slotBase(element: Element): string {
  return element.source ?? casePointer(lastConfig, 0);
}

/** Smallest slot side when dragging: about a grid step. */
const MIN_SLOT: Record<string, number> = { mm: 500, cm: 50, m: 0.5, in: 18, ft: 1.5 };

function updateTitle(): void {
  const path = workbench.activePath() ?? mainPath();
  const dirty = path ? store.isDirty(path) : false;
  const where = path ? `${dirty ? "● " : ""}${path} – ` : "";
  document.title = `${where}${store.project} – plate`;
  if (ready) syncRoute();
}

// ------------------------------------------------------------------ start screen + drawing

const SYSTEM_KEY = "plate:system";
let startSystem: System = readStoredSystem();

function readStoredSystem(): System {
  try {
    return localStorage.getItem(SYSTEM_KEY) === "imperial" ? "imperial" : "metric";
  } catch {
    return "metric";
  }
}

function setStartSystem(system: System): void {
  startSystem = system;
  try {
    localStorage.setItem(SYSTEM_KEY, system);
  } catch {
    /* ignore */
  }
  for (const b of document.querySelectorAll<HTMLButtonElement>("[data-start-system]")) {
    b.setAttribute("aria-pressed", String(b.dataset.startSystem === system));
  }
}

const startScreen = $("start");

function showStart(): void {
  const main = mainPath();
  $("start-continue").hidden = !main;
  $("start-continue-name").textContent = main ?? "";
  setStartSystem(startSystem);
  startScreen.hidden = false;
  startScreen.querySelector<HTMLElement>(main ? "#start-continue" : "[data-mode]")?.focus();
}

function hideStart(): void {
  startScreen.hidden = true;
}

/** The drawing in progress: what it's for. The plate shown before stays selected until it's done. */
let drawing: { mode: StartMode; system: System } | null = null;

const DRAW_LABEL: Record<StartMode, string> = {
  boundary: "Draw the boundary",
  spine: "Draw the spine",
  free: "Place the origin",
};

function beginDrawing(mode: StartMode, system = startSystem): void {
  hideStart();
  drawing = { mode, system };
  resetSheet();
  setStatus("idle", "Drawing");
  workbench.setReadOnly(true);
  syncSystemSwitch(system);
  $("toast").hidden = true;

  $("drawbar-mode").textContent = DRAW_LABEL[mode];
  $("drawbar-undo").hidden = mode === "free";
  $("drawbar-done").hidden = mode === "free";
  $("drawbar").hidden = false;

  viewer.setEmptyUnits(system, defaultUnits(system));
  viewer.clear();
  viewer.startDrawing({
    kind: mode === "free" ? "point" : mode,
    done: (points) => void finishDrawing(points),
    cancel: () => cancelDrawing(),
  });
}

function endDrawingUi(): void {
  drawing = null;
  workbench.setReadOnly(false);
  $("drawbar").hidden = true;
  $("sb-hint").textContent = "Scroll to zoom, drag to pan, hover lines for dimensions";
}

/** The drawing becomes a new plate file in the workspace, opened and shown. */
async function finishDrawing(points: Point[]): Promise<void> {
  const d = drawing;
  if (!d) return;
  endDrawingUi();
  try {
    const doc = await store.create(store.freePath("plate.ncl"), template(d.mode, points, d.system));
    workbench.open(doc.path);
    workbench.setMain(doc.path);
    await runSolve();
    toast(
      d.mode === "free"
        ? `Created ${doc.path}. Drag the origin, or edit its coordinates in the file.`
        : `Created ${doc.path}. Drag points to move them, double-click an edge to add one, right-click a point to remove it.`,
    );
  } catch (err) {
    toast(`Couldn't create the plate: ${(err as Error).message}`, true);
    void runSolve();
  }
}

/** Restarting (e.g. switching units mid-drawing) cancels the viewer's drawing; don't treat that as giving up. */
let restarting = false;

function cancelDrawing(): void {
  if (!drawing || restarting) return;
  endDrawingUi();
  void runSolve(); // the plate shown before
  showStart();
}

function restartDrawing(system: System): void {
  const d = drawing;
  if (!d) return;
  restarting = true;
  viewer.cancelDrawing();
  restarting = false;
  setStartSystem(system);
  beginDrawing(d.mode, system);
}

viewer.onHint = (text) => {
  $("drawbar-hint").textContent = text;
  $("sb-hint").textContent = text;
};

const pressKey = (key: string) => viewer.handleKey(new KeyboardEvent("keydown", { key }));
$("drawbar-undo").addEventListener("click", () => pressKey("Backspace"));
$("drawbar-done").addEventListener("click", () => pressKey("Enter"));
$("drawbar-cancel").addEventListener("click", () => pressKey("Escape"));

for (const b of startScreen.querySelectorAll<HTMLButtonElement>("[data-mode]")) {
  b.addEventListener("click", () => beginDrawing(b.dataset.mode as StartMode));
}
for (const b of startScreen.querySelectorAll<HTMLButtonElement>("[data-start-system]")) {
  b.addEventListener("click", () => setStartSystem(b.dataset.startSystem as System));
}
$("start-continue").addEventListener("click", () => {
  hideStart();
  const main = mainPath();
  if (main && !workbench.activePath()) workbench.open(main);
});
$("start-open").addEventListener("click", () => void openFromComputer());
$("start-projects").addEventListener("click", () => void openProjectSwitcher());

// ------------------------------------------------------------------ editing in the drawing

/**
 * Resizing a plan's preview slot by a corner. The corner at the origin stays put; the others set
 * the width, the depth or both. The slot stays a rectangle while dragging and the file is written
 * when the drag ends.
 */
function dragSlot(element: Element, index: number, point: Point, phase: string): void {
  if (phase === "start") {
    const check =
      index === 0
        ? { ok: false as const, reason: "The slot's corner at the origin stays put. Drag another corner." }
        : geo.checkPreviewSize(slotBase(element));
    if (!check.ok) {
      viewer.abortVertexDrag();
      toast(check.reason, true);
    }
    return;
  }
  const min = MIN_SLOT[lastLayout?.units ?? "m"] ?? 0.5;
  const [w0, h0] = [element.points[2].x, element.points[2].y];
  const w = index === 3 ? w0 : Math.max(min, point.x);
  const h = index === 1 ? h0 : Math.max(min, point.y);
  if (phase === "move" || phase === "end") {
    viewer.previewPoint(element.id, 1, { x: w, y: 0 });
    viewer.previewPoint(element.id, 2, { x: w, y: h });
    viewer.previewPoint(element.id, 3, { x: 0, y: h });
  }
  if (phase === "end") {
    const result = geo.setPreviewSize(slotBase(element), w, h);
    if (!result.ok) toast(result.reason, true);
    void runSolve();
  } else if (phase === "cancel") {
    void runSolve();
  }
}

viewer.onVertexDrag = (element, index, point, phase) => {
  if (element.kind === "slot") return dragSlot(element, index, point, phase);
  if (phase === "start") {
    const result = geo.beginDrag(element, index);
    if (!result.ok) {
      viewer.abortVertexDrag();
      toast(result.reason, true);
    } else if (result.note) {
      toast(result.note);
    }
  } else if (phase === "move") {
    geo.dragTo(point);
  } else if (phase === "end") {
    geo.dragTo(point);
    geo.endDrag();
    void runSolve();
  } else {
    geo.cancelDrag();
    void runSolve();
  }
};

/** An edge moved as a whole: the viewer shows a ghost while dragging; the file is written on release. */
viewer.onEdgeDrag = (element, index, move, phase) => {
  if (phase === "start") {
    const result = geo.beginEdgeMove(element, index);
    if (!result.ok) {
      viewer.abortEdgeDrag();
      toast(result.reason, true);
    } else if (result.note) {
      toast(result.note);
    }
  } else if (phase === "end" && move) {
    const result = geo.endEdgeMove(move.a, move.b);
    if (!result.ok) toast(result.reason, true);
    void runSolve();
  } else {
    geo.cancelEdgeMove();
    if (move?.problem) toast(`${move.problem} The edge wasn't moved.`, true);
  }
};

const SLOT_SHAPE = "The preview slot is a rectangle: drag its corners to resize it.";

viewer.onVertexInsert = (element, after, point) => {
  if (element.kind === "slot") return toast(SLOT_SHAPE);
  const result = geo.insertPoint(element, after, point);
  if (!result.ok) toast(result.reason, true);
};

viewer.onVertexDelete = (element, index) => {
  if (element.kind === "slot") return toast(SLOT_SHAPE);
  const result = geo.deletePoint(element, index);
  if (!result.ok) toast(result.reason, true);
};

// ------------------------------------------------------------------ right-click menus

/** Port roles of each boundary edge, from `start.ports` (missing ones are `facade`). */
function edgeRoles(config: api.ConfigData | null): string[][] {
  const ports = ((config?.start as { ports?: unknown[] } | undefined)?.ports ?? []) as (string | string[])[];
  return ports.map((r) => (Array.isArray(r) ? r : [r]));
}

const EDGE_ROLES: [string, string][] = [
  ["facade", "Exterior wall, windows allowed"],
  ["blank", "Exterior wall, no openings"],
  ["party", "Shared with a neighbour"],
  ["outdoor", "Onto a balcony or terrace"],
  ["entrance", "Where the plate is entered"],
  ["service", "Back-of-house access"],
  ["core", "Against the building core"],
];

function reportEdit(
  result: { ok: true; note?: string } | { ok: false; reason: string },
  done?: string,
): void {
  if (!result.ok) toast(result.reason, true);
  else if (done) toast(done);
}

/** Menu items to pick the symbol a space is drawn with: likely ones by name, or the catalog. */
function symbolItems(element: Element): MenuItem[] {
  const info = element.space!;
  const pointer = `${info.plan}/spaces/${info.type}`;
  const items: MenuItem[] = suggest(info.type, info.label, 4)
    .filter((name) => name !== info.symbol)
    .map((name) => ({
      label: `Draw as ${catalog[name].label.toLowerCase()}`,
      note: `'${name}`,
      action: () =>
        reportEdit(geo.setField(pointer, "symbol", `'${name}`), `${info.type} is drawn as '${name}`),
    }));
  items.push({
    label: "Choose a symbol…",
    note: "From the catalog",
    action: () => chooseSymbol(pointer, info.symbol),
  });
  if (info.symbol) {
    items.push({
      label: "Remove its symbol",
      note: `'${info.symbol}`,
      action: () => reportEdit(geo.removeField(pointer, "symbol"), `${info.type} has no symbol now`),
    });
  }
  return items;
}

/** The catalog, with the cursor inside the space so a click there inserts into it. */
function chooseSymbol(pointer: string, current?: string): void {
  const at = spanFor(pointer, geo.sourcesSnapshot());
  if (!at || !store.get(at.file)) return toast("Can't find where that space is written.", true);
  // Just inside its `{`: that's where an Insert from the catalog goes.
  workbench.reveal(at.file, at.span.startLine, at.span.startCol + 1);
  workbench.sidebar.show("reference");
  referencePanel.open("def:Symbol", current);
  toast(`Pick a symbol: it goes into ${pointer.split("/").pop()}`);
}

/** Open the file a value is written in, at the value. */
function showInFile(pointer: string): void {
  const at = spanFor(pointer, geo.sourcesSnapshot());
  if (at && store.get(at.file)) workbench.reveal(at.file, at.span.startLine, at.span.startCol);
}

viewer.onContextMenu = (target: ContextTarget, x: number, y: number) => {
  if (target.type === "wall") {
    const w = target.element.wall!;
    const source = target.element.source;
    openMenu(
      `${w.label} between ${w.between.join(" and ")}`,
      source
        ? [
            {
              label: `Show ${w.assembly} in the file`,
              note: "Its assembly",
              action: () => showInFile(source),
            },
          ]
        : [{ label: "Its assembly isn't in a file here", disabled: true, action: () => {} }],
      x,
      y,
    );
    return;
  }
  if (target.type === "part") {
    const p = target.element.part!;
    const source = target.element.source;
    const service = source?.match(/^(.*\/services\/[^/]+)/)?.[1];
    const items: MenuItem[] = [];
    if (source)
      items.push({ label: `Show where ${p.label}s go`, note: p.placed, action: () => showInFile(source) });
    if (service) items.push({ label: `Show the ${p.service} service`, action: () => showInFile(service) });
    openMenu(`${p.label} in ${p.space}`, items, x, y);
    return;
  }
  if (target.type === "edge") {
    const current = edgeRoles(lastConfig)[target.index] ?? ["facade"];
    const n = target.element.points.length;
    openMenu(
      `Edge ${target.index + 1}: port role`,
      EDGE_ROLES.map(([role, note]) => ({
        label: `'${role}`,
        note,
        checked: current.length === 1 && current[0] === role,
        action: () =>
          reportEdit(geo.setEdgeRoles(target.index, [role], n), `Edge ${target.index + 1} is now '${role}`),
      })),
      x,
      y,
    );
    return;
  }
  // Right-clicking inside a solved space offers its parent's options too (the nearest space
  // with a plan: that's what can be made unique).
  let element: Element = target.element;
  const own = element.space!;
  if (!own.hasPlan) {
    let id = element.id;
    while (id.includes(".")) {
      id = id.slice(0, id.lastIndexOf("."));
      const parent = lastLayout?.elements.find((e) => e.id === id);
      if (parent?.space?.hasPlan) {
        element = parent;
        break;
      }
    }
  }
  const info = element.space!;
  const items: MenuItem[] = [];
  if (info.hasPlan) {
    const plan = valueAt(lastConfig, info.plan);
    const rule = rulesOf(plan).find(
      ({ rule }) =>
        rule.rule === "unique" &&
        (rule.a === info.name || (Array.isArray(rule.a) && rule.a.length === 1 && rule.a[0] === info.name)),
    );
    if (rule) {
      items.push({
        label: "Use its typical again",
        note: "Removes plate.unique",
        action: () =>
          reportEdit(geo.removeItem(`${info.plan}${rule.pointer}`), `${info.name} is typical again`),
      });
    } else if (!info.unique) {
      items.push({
        label: `Make ${info.name} unique`,
        note: info.typical ? `Leaves type ${info.typical}` : "Solved on its own",
        action: () =>
          reportEdit(
            geo.addRule(info.plan, `plate.unique "${info.name}"`),
            `Added plate.unique "${info.name}"`,
          ),
      });
    }
    items.push("-");
  }
  items.push(...symbolItems(target.element), "-");
  if (element !== target.element) {
    items.push({
      label: `Show ${own.name} in the file`,
      action: () => showInFile(`${own.plan}/spaces/${own.type}`),
    });
  }
  items.push({
    label: `Show ${info.name} in the file`,
    action: () => showInFile(`${info.plan}/spaces/${info.type}`),
  });
  if (info.hasPlan) {
    const at = spanFor(`${info.plan}/spaces/${info.type}/plan/spaces`, geo.sourcesSnapshot());
    const elsewhere = at && at.file !== mainPath() ? at.file : null;
    // A plan in a file of its own can be opened on its own, in exactly this space.
    if (elsewhere && info.slot && !isPlate(store.get(elsewhere)?.text.value ?? "")) {
      items.push({
        label: "Open its plan here",
        note: `${elsewhere}, in this slot`,
        action: () => openPlanIn(elsewhere, info),
      });
    }
    items.push({
      label: elsewhere ? `Open its plan (${elsewhere})` : "Show its plan in the file",
      action: () => showInFile(`${info.plan}/spaces/${info.type}/plan${elsewhere ? "/spaces" : ""}`),
    });
  }
  openMenu(target.element.id, items, x, y);
};

// ------------------------------------------------------------------ measurement system

/**
 * The Metric/Imperial switch edits the plate file, which stays the single source of truth. When
 * the file doesn't say which unit bare numbers are in, the switch pins the current one so the
 * plate keeps its real size; only the presentation changes.
 */
function setSystem(system: System): void {
  if (drawing) {
    if (system !== drawing.system) restartDrawing(system);
    return;
  }
  const main = mainPath();
  const doc = main ? store.get(main) : undefined;
  if (!doc) {
    setStartSystem(system);
    syncSystemSwitch(system);
    viewer.setEmptyUnits(system, defaultUnits(system));
    return;
  }
  const source = doc.text.value;
  const next = withSystem(source, system, lastLayout?.units);
  if (next === source) return;
  doc.text.setValue(next);
  const unitsLine = /^\s*units\s*=\s*'(\w+)/m.exec(next)?.[1];
  const bare = unitsLine ? ` Plain numbers are still in ${unitName(unitsLine)}.` : "";
  toast(
    `${system === "imperial" ? "Imperial" : "Metric"}: lengths shown in ${system === "imperial" ? "feet and inches" : "metres"}.${bare}`,
  );
}

function unitName(u: string): string {
  return (
    (
      { mm: "millimetres", cm: "centimetres", m: "metres", in: "inches", ft: "feet" } as Record<
        string,
        string
      >
    )[u] ?? u
  );
}

function syncSystemSwitch(system: System): void {
  for (const b of document.querySelectorAll<HTMLButtonElement>("[data-system]")) {
    b.setAttribute("aria-pressed", String(b.dataset.system === system));
  }
}

for (const b of document.querySelectorAll<HTMLButtonElement>("[data-system]")) {
  b.addEventListener("click", () => setSystem(b.dataset.system as System));
}

// ------------------------------------------------------------------ files

async function save(all = false): Promise<void> {
  if (drawing) return;
  const paths = all ? store.dirtyPaths() : [workbench.activePath()].filter((p): p is string => !!p);
  if (!paths.length) return;
  try {
    for (const p of paths) await store.save(p);
    toast(paths.length === 1 ? `Saved ${paths[0]}` : `Saved ${paths.length} files`);
  } catch (err) {
    toast(`Couldn't save: ${(err as Error).message}`, true);
  }
}

/** Copy a .ncl file from this computer into the workspace, and open it. */
async function openFromComputer(): Promise<void> {
  const picked = await pickFile().catch((e: Error) => (toast(e.message, true), null));
  if (!picked) return;
  if (drawing) {
    restarting = true;
    viewer.cancelDrawing();
    restarting = false;
    endDrawingUi();
  }
  hideStart();
  try {
    const doc = await store.create(store.freePath(picked.name.replace(/[^\w.-]+/g, "-")), picked.text);
    workbench.open(doc.path);
    toast(`Added ${doc.path} to ${store.project}`);
  } catch (err) {
    toast(`Couldn't add ${picked.name}: ${(err as Error).message}`, true);
  }
}

function formatActive(): void {
  void workbench.activeEditor().getAction("editor.action.formatDocument")?.run();
}

function newPlate(): void {
  if (drawing) return;
  showStart();
}

async function exportAs(format: ExportFormat): Promise<void> {
  if (!lastConfig) {
    toast("Fix the errors in the configuration before exporting.", true);
    return;
  }
  const sent = lastSent ?? { config: lastConfig, view: "default" as PreviewView };
  const res = await api.exportLayout(sent.config, format, sent.view).catch((e: Error) => {
    toast(e.message, true);
    return null;
  });
  if (!res) return;
  if (!res.ok) {
    toast(`Can't export: ${res.diagnostics[0]?.message ?? "the configuration has errors"}`, true);
    return;
  }
  download(res.blob, res.filename);
  toast(`Exported ${res.filename}`);
}

let toastTimer = 0;
function toast(message: string, error = false, undo?: { label: string; action: () => void }): void {
  const el = $("toast");
  el.textContent = message;
  el.classList.toggle("toast-error", error);
  if (undo) {
    const b = document.createElement("button");
    b.className = "toast-action";
    b.textContent = undo.label;
    b.addEventListener("click", () => {
      el.hidden = true;
      undo.action();
    });
    el.append(" ", b);
  }
  el.hidden = false;
  window.clearTimeout(toastTimer);
  // Long enough to reach a button; errors stay long enough to read.
  toastTimer = window.setTimeout(() => (el.hidden = true), undo ? 10000 : error ? 5000 : 3200);
}

// ------------------------------------------------------------------ chrome wiring

$("btn-new").addEventListener("click", newPlate);
$("btn-open").addEventListener("click", () => void openFromComputer());
$("btn-save").addEventListener("click", (e) => void save((e as MouseEvent).shiftKey));

const menu = $("export-menu");
const menuButton = $("btn-export");
const menuList = menu.querySelector<HTMLElement>(".menu-list")!;
function setMenu(open: boolean): void {
  menuList.hidden = !open;
  menuButton.setAttribute("aria-expanded", String(open));
  if (open) menuList.querySelector<HTMLElement>("button")?.focus();
}
menuButton.addEventListener("click", () => setMenu(menuList.hasAttribute("hidden")));
menuList.addEventListener("click", (e) => {
  const item = (e.target as HTMLElement).closest<HTMLElement>("[data-format]");
  if (!item) return;
  setMenu(false);
  void exportAs(item.dataset.format as ExportFormat);
});
document.addEventListener("pointerdown", (e) => {
  if (!menu.contains(e.target as Node)) setMenu(false);
});
menu.addEventListener("keydown", (e) => {
  const items = [...menuList.querySelectorAll<HTMLElement>("button")];
  const i = items.indexOf(document.activeElement as HTMLElement);
  if (e.key === "Escape") (setMenu(false), menuButton.focus());
  if (e.key === "ArrowDown") (e.preventDefault(), items[(i + 1) % items.length]?.focus());
  if (e.key === "ArrowUp") (e.preventDefault(), items[(i - 1 + items.length) % items.length]?.focus());
});

for (const b of document.querySelectorAll<HTMLButtonElement>(".tools [data-tool]")) {
  b.addEventListener("click", () => viewer.setTool(b.dataset.tool as "select" | "measure"));
}
for (const b of document.querySelectorAll<HTMLButtonElement>(".tools [data-action]")) {
  b.addEventListener("click", () => {
    const action = b.dataset.action;
    if (action === "fit") viewer.fit();
    if (action === "zoom-in") viewer.zoomBy(1.4);
    if (action === "zoom-out") viewer.zoomBy(1 / 1.4);
  });
}
for (const b of document.querySelectorAll<HTMLButtonElement>(".tools [data-toggle]")) {
  b.addEventListener("click", () => {
    const on = b.getAttribute("aria-pressed") !== "true";
    b.setAttribute("aria-pressed", String(on));
    if (b.dataset.toggle === "dims") viewer.toggleDimensions(on);
    if (b.dataset.toggle === "grid") viewer.toggleGrid(on);
  });
}

function syncToggles(): void {
  document.querySelector('[data-toggle="dims"]')?.setAttribute("aria-pressed", String(viewer.showDimensions));
  document.querySelector('[data-toggle="grid"]')?.setAttribute("aria-pressed", String(viewer.showGrid));
}

window.addEventListener("keydown", (e) => {
  const mod = e.ctrlKey || e.metaKey;
  const key = e.key.toLowerCase();
  if (mod && e.altKey && key === "p") {
    e.preventDefault();
    if (isPaletteOpen()) closePalette();
    else void openProjectSwitcher();
    return;
  }
  if (mod && !e.altKey && !e.shiftKey && key === "p") {
    e.preventDefault();
    hideStart();
    if (isPaletteOpen()) closePalette();
    else openQuickOpen();
    return;
  }
  if (!startScreen.hidden) {
    const modes: Record<string, StartMode> = { "1": "boundary", "2": "spine", "3": "free" };
    if (!mod && modes[e.key]) {
      e.preventDefault();
      beginDrawing(modes[e.key]);
    } else if (e.key === "Escape") {
      e.preventDefault();
      hideStart();
    } else if (mod && key === "o") {
      e.preventDefault();
      void openFromComputer();
    }
    return;
  }
  if (mod && key === "s") {
    e.preventDefault();
    void save(e.shiftKey || e.altKey);
    return;
  }
  if (mod && key === "o") {
    e.preventDefault();
    void openFromComputer();
    return;
  }
  if (mod && key === "b") {
    e.preventDefault();
    workbench.toggleSidebar();
    return;
  }
  if (mod && e.shiftKey && (key === "e" || key === "h")) {
    e.preventDefault();
    const view = key === "e" ? "files" : "reference";
    workbench.sidebar.toggle(view);
    if (view === "reference" && workbench.sidebar.showing("reference")) referencePanel.focusSearch();
    return;
  }
  if (mod && e.key === "\\") {
    e.preventDefault();
    workbench.split();
    return;
  }
  const target = e.target as HTMLElement;
  // While drawing, the drawing takes the keys even if the (read-only) editor has focus.
  const typing = !drawing && target.closest(".monaco-editor, input, textarea, [role=menu]");
  if (!typing && viewer.handleKey(e)) {
    e.preventDefault();
    syncToggles();
  }
});

window.addEventListener("beforeunload", (e) => {
  // Unsaved text is kept in the browser too, but say so before leaving.
  if (store.dirtyPaths().length) e.preventDefault();
});

// Resizable split, remembered per browser.
const split = $("split");
const gutter = $("gutter");
const SPLIT_KEY = "plate:split";
const setSplit = (fraction: number) => {
  const f = Math.min(0.78, Math.max(0.22, fraction));
  split.style.setProperty("--code", `${f * 100}%`);
  try {
    localStorage.setItem(SPLIT_KEY, String(f));
  } catch {
    /* ignore */
  }
};
try {
  const stored = Number(localStorage.getItem(SPLIT_KEY));
  if (stored) setSplit(stored);
} catch {
  /* ignore */
}
gutter.addEventListener("pointerdown", (e) => {
  gutter.setPointerCapture(e.pointerId);
  split.classList.add("is-resizing");
  const rect = split.getBoundingClientRect();
  const move = (ev: PointerEvent) => setSplit((ev.clientX - rect.left) / rect.width);
  const up = () => {
    split.classList.remove("is-resizing");
    gutter.removeEventListener("pointermove", move);
    gutter.removeEventListener("pointerup", up);
  };
  gutter.addEventListener("pointermove", move);
  gutter.addEventListener("pointerup", up);
});
gutter.addEventListener("keydown", (e) => {
  const current = parseFloat(split.style.getPropertyValue("--code") || "46") / 100;
  if (e.key === "ArrowLeft") setSplit(current - 0.02);
  if (e.key === "ArrowRight") setSplit(current + 0.02);
});

// ------------------------------------------------------------------ projects

const RECENT_KEY = "plate:recent";

function recentProjects(): string[] {
  try {
    const list = JSON.parse(localStorage.getItem(RECENT_KEY) ?? "[]") as unknown;
    return Array.isArray(list) ? (list as string[]) : [];
  } catch {
    return [];
  }
}

function rememberProject(name: string): void {
  try {
    localStorage.setItem(RECENT_KEY, JSON.stringify(pushRecent(recentProjects(), name)));
  } catch {
    /* ignore */
  }
}

/** The project the address names, else the last one used, else the first; a new user gets Home. */
async function openStartingProject(listing: api.ProjectListing): Promise<{ main: string | null }> {
  const route = parseRoute(location.hash);
  const known = (name: string) => listing.projects.some((p) => p.name === name);
  const wanted = route?.project;
  const example = wanted ? listing.examples.find((e) => e.name === wanted) : undefined;
  if (wanted && !known(wanted) && example) {
    const opened = await api.openExample(example.name);
    await store.open(opened.name, { announce: false });
    pendingFile = route?.path ?? null;
    return { main: example.main };
  }
  const recent = byRecent(listing.projects, recentProjects())[0]?.name;
  const name = wanted && known(wanted) ? wanted : recent;
  if (!name) {
    const first = listing.examples[0];
    const opened = await api.openExample(first.name);
    await store.open(opened.name, { announce: false });
    return { main: first.main };
  }
  await store.open(name, { announce: false });
  if (wanted === name) pendingFile = route?.path ?? null;
  return { main: listing.projects.find((p) => p.name === name)?.main ?? null };
}

/** A file named by the address, to open once the project's up. */
let pendingFile: string | null = null;

/**
 * Show the store's project: its tabs and plate as they were left, or `fallback` on a first visit.
 */
async function showProject(fallback: string | null): Promise<void> {
  ready = false;
  const restored = workbench.enterProject();
  let main = restored ?? (fallback && store.get(fallback) ? fallback : firstPlate());
  const named = pendingFile && store.get(pendingFile) ? pendingFile : null;
  pendingFile = null;
  if (named) {
    if (isPlate(store.get(named)!.text.value)) main = named;
    workbench.open(named, { focus: false });
  } else if (main && !workbench.activePath()) {
    workbench.open(main, { focus: false });
  }
  planViews.clear();
  workbench.setMain(main);
  rememberProject(store.project);
  syncRoute();
  ready = true;
  updateTitle();
  resetSheet();
  viewer.setLayout(null);
  await runSolve();
}

/** Keep the address on the open file. This isn't a step Back would undo; changing project is. */
function syncRoute(): void {
  const hash = formatRoute({ project: store.project, path: workbench.activePath() });
  if (hash !== location.hash) history.replaceState(null, "", hash);
}

let switchSeq = 0;

/** Open a project: instant if it's been opened already, one request if not. */
async function switchProject(
  name: string,
  options: { main?: string | null; file?: string | null; fromAddress?: boolean } = {},
): Promise<void> {
  if (name === store.project) {
    if (options.file && store.get(options.file)) workbench.open(options.file);
    return;
  }
  const seq = ++switchSeq;
  if (drawing) {
    restarting = true;
    viewer.cancelDrawing();
    restarting = false;
    endDrawingUi();
  }
  hideStart();
  window.clearTimeout(debounce);
  inflight?.abort();
  ready = false;
  workbench.leaveProject();
  try {
    await store.open(name, { announce: false });
  } catch (err) {
    toast(`Couldn't open ${name}: ${(err as Error).message}`, true);
    // Stay where we were: its documents are still loaded.
    await store.open(store.project || name, { announce: false }).catch(() => undefined);
    await showProject(null);
    return;
  }
  if (seq !== switchSeq) return;
  pendingFile = options.file ?? null;
  // Going somewhere new is a step Back can undo; following the address already is one.
  if (!options.fromAddress) history.pushState(null, "", formatRoute({ project: name, path: null }));
  await showProject(options.main ?? null);
  workbench.activeEditor().focus();
}

/** Back and forward, or an address typed by hand. */
window.addEventListener("hashchange", () => void followRoute());

async function followRoute(): Promise<void> {
  const route = parseRoute(location.hash);
  if (!route) return;
  if (route.project !== store.project) {
    const listing = await api.listProjects().catch(() => null);
    const example = listing?.examples.find((e) => e.name === route.project);
    const known = listing?.projects.some((p) => p.name === route.project);
    if (!known && !example) return toast(`There's no project called ${route.project}.`, true);
    if (!known && example) await api.openExample(example.name);
    await switchProject(route.project, { file: route.path, main: example?.main, fromAddress: true });
  } else if (route.path && route.path !== workbench.activePath() && store.get(route.path)) {
    workbench.open(route.path);
  }
}

/** Ctrl+P: any file in the project, by name. */
function openQuickOpen(): void {
  const active = workbench.activePath();
  const paths = store.paths().filter((p) => p !== active);
  showPalette({
    placeholder: `Go to a file in ${store.project}`,
    hint: "Ctrl+P",
    empty: "No file here matches.",
    items: paths.map((path) => ({
      id: path,
      label: path,
      badge: store.isDirty(path) ? "unsaved" : undefined,
      pick: () => workbench.open(path),
    })),
  });
}

const plural = (n: number, one: string, many = `${one}s`) => `${n} ${n === 1 ? one : many}`;

/** Ctrl+Alt+P: every project, recent first, then the examples not opened yet. */
async function openProjectSwitcher(): Promise<void> {
  const listing = await api.listProjects().catch((err: Error) => (toast(err.message, true), null));
  if (!listing) return;
  const now = Date.now();
  const projects = byRecent(listing.projects, recentProjects());
  const items: PaletteItem[] = projects.map((p, i) => {
    const dirty = store.dirtyCount(p.name);
    const mainOfProject = p.name === store.project ? mainPath() : null;
    return {
      id: p.name,
      label: p.name,
      group: i === 0 ? "Your projects" : undefined,
      detail: [plural(p.plates, "plate"), plural(p.files, "file"), timeAgo(p.modified, now)]
        .filter(Boolean)
        .join(" · "),
      image: p.files ? api.projectThumbnailUrl(p.name, mainOfProject) : undefined,
      badge: dirty ? `${dirty} unsaved` : p.name === store.project ? "open" : undefined,
      menu: (x, y) => projectMenu(p, x, y),
      pick: () => void switchProject(p.name, { main: p.main }),
    };
  });
  const missing = listing.examples.filter((e) => !listing.projects.some((p) => p.name === e.name));
  missing.forEach((e, i) => {
    items.push({
      id: `example:${e.name}`,
      label: e.title,
      group: i === 0 ? "Examples" : undefined,
      detail: e.blurb,
      image: api.exampleThumbnailUrl(e.name),
      pick: () => void openExampleProject(e),
    });
  });
  showPalette({
    placeholder: "Switch project",
    hint: "Ctrl+Alt+P",
    tall: true,
    searchDetail: true,
    empty: "No project matches.",
    items,
    footer: [
      { label: "New project…", action: () => void newProject() },
      { label: "Import a zip…", action: () => void importProjectZip() },
    ],
  });
}

async function openExampleProject(example: api.ExampleInfo): Promise<void> {
  try {
    const opened = await api.openExample(example.name);
    await switchProject(opened.name, { main: opened.main });
  } catch (err) {
    toast(`Couldn't open ${example.title}: ${(err as Error).message}`, true);
  }
}

/** Ask for a project name until it's a usable one, or the question's dismissed. */
async function askProjectName(title: string, action: string, value = ""): Promise<string | null> {
  const typed = await askText({ title, value, placeholder: "Riverside flats", action });
  if (typed === null) return null;
  const name = cleanProjectName(typed);
  if (!name) toast("A project name needs a letter or a number in it.", true);
  return name || null;
}

async function newProject(): Promise<void> {
  const name = await askProjectName("Name the new project", "Create");
  if (!name) return;
  try {
    await api.createProject(name);
    await switchProject(name);
    showStart();
  } catch (err) {
    toast(`Couldn't create ${name}: ${(err as Error).message}`, true);
  }
}

async function importProjectZip(): Promise<void> {
  const zip = await pickZip();
  if (!zip) return;
  try {
    const made = await api.importProject(
      cleanProjectName(zip.name.replace(/\.zip$/i, "")) || "imported",
      zip,
    );
    await switchProject(made.name, { main: made.main });
    toast(`Imported ${made.name}.`);
  } catch (err) {
    toast(`Couldn't import ${zip.name}: ${(err as Error).message}`, true);
  }
}

function projectMenu(project: api.ProjectInfo, x: number, y: number): void {
  openMenu(
    project.name,
    [
      { label: "Rename…", action: () => void renameProject(project.name) },
      { label: "Duplicate", action: () => void duplicateProject(project.name) },
      {
        label: "Export as zip",
        disabled: !project.files,
        action: () => {
          closePalette();
          const a = document.createElement("a");
          a.href = api.projectExportUrl(project.name);
          a.download = `${project.name}.zip`;
          a.click();
        },
      },
      "-",
      { label: "Delete", action: () => void deleteProject(project.name) },
    ],
    x,
    y,
  );
}

async function renameProject(name: string): Promise<void> {
  const to = await askProjectName(`Rename ${name}`, "Rename", name);
  if (!to || to === name) return;
  // Save nothing behind the user's back: unsaved text travels with the drafts.
  const here = store.project === name;
  if (here) {
    ready = false;
    workbench.leaveProject();
  }
  try {
    await api.moveProject(name, to);
    store.dropProject(name, to);
    workbench.forgetProject(name, to);
    try {
      localStorage.setItem(RECENT_KEY, JSON.stringify(recentProjects().map((n) => (n === name ? to : n))));
    } catch {
      /* ignore */
    }
    if (here) {
      await store.open(to, { announce: false });
      await showProject(null);
    }
    toast(`Renamed to ${to}.`);
  } catch (err) {
    toast(`Couldn't rename ${name}: ${(err as Error).message}`, true);
    if (here) await showProject(null);
  }
}

async function duplicateProject(name: string): Promise<void> {
  const to = await askProjectName(`Duplicate ${name} as`, "Duplicate", `${name} copy`);
  if (!to) return;
  try {
    await saveIfOpen(name);
    const copy = await api.duplicateProject(name, to);
    await switchProject(copy.name);
    toast(`Made ${copy.name}.`);
  } catch (err) {
    toast(`Couldn't duplicate ${name}: ${(err as Error).message}`, true);
  }
}

/** A copy is of what's on disk: bring the open project's unsaved text there first. */
async function saveIfOpen(name: string): Promise<void> {
  if (name === store.project) for (const p of store.dirtyPaths()) await store.save(p);
}

async function deleteProject(name: string): Promise<void> {
  closePalette();
  const here = store.project === name;
  try {
    if (here) {
      // Somewhere to land first: another project, or an empty one for someone with none left.
      const listing = await api.listProjects();
      const next = byRecent(
        listing.projects.filter((p) => p.name !== name),
        recentProjects(),
      )[0];
      if (next) {
        await switchProject(next.name, { main: next.main });
      } else {
        const landing = name === "my-project" ? "my-project-2" : "my-project";
        await api.createProject(landing);
        await switchProject(landing);
      }
    }
    const { trash } = await api.trashProject(name);
    const kept = keptInBrowser(name);
    store.dropProject(name);
    workbench.forgetProject(name);
    try {
      localStorage.setItem(RECENT_KEY, JSON.stringify(recentProjects().filter((n) => n !== name)));
    } catch {
      /* ignore */
    }
    toast(`Deleted ${name}.`, false, {
      label: "Undo",
      action: () => void undoDelete(trash, kept),
    });
  } catch (err) {
    toast(`Couldn't delete ${name}: ${(err as Error).message}`, true);
  }
}

/** What the browser holds for a project: its unsaved text and its tabs. */
function keptInBrowser(name: string): Record<string, string | null> {
  try {
    return {
      drafts: localStorage.getItem(`plate:drafts:${name}`),
      project: localStorage.getItem(`plate:project:${name}`),
    };
  } catch {
    return {};
  }
}

async function undoDelete(id: string, kept: Record<string, string | null>): Promise<void> {
  try {
    const { name } = await api.restoreProject(id);
    try {
      for (const [key, value] of Object.entries(kept)) {
        if (value) localStorage.setItem(`plate:${key}:${name}`, value);
      }
    } catch {
      /* drafts are a convenience only */
    }
    await switchProject(name);
    toast(`${name} is back.`);
  } catch (err) {
    toast(`Couldn't bring it back: ${(err as Error).message}`, true);
  }
}

/** Copy a file and what it imports to another project, at the same paths. */
async function copyToProject(path: string): Promise<void> {
  const listing = await api.listProjects().catch((err: Error) => (toast(err.message, true), null));
  if (!listing) return;
  const others = byRecent(listing.projects, recentProjects()).filter((p) => p.name !== store.project);
  if (!others.length) return toast("There's no other project to copy to yet.", true);
  showPalette({
    placeholder: `Copy ${path} to…`,
    empty: "No project matches.",
    items: others.map((p) => ({
      id: p.name,
      label: p.name,
      pick: () => void copyInto(path, p.name),
    })),
  });
}

async function copyInto(path: string, project: string): Promise<void> {
  try {
    const there = await api.readProject(project);
    const existing = Object.fromEntries(there.files.map((f) => [f.path, f.text]));
    const plan = planCopy(path, store.files(), existing);
    for (const file of plan.write) await api.writeFile(project, file.path, file.text);
    const kept = plan.clashes.length
      ? ` Left alone, already there and different: ${plan.clashes.join(", ")}.`
      : "";
    toast(`Copied ${plural(plan.write.length, "file")} to ${project}.${kept}`, plan.clashes.length > 0);
  } catch (err) {
    toast(`Couldn't copy to ${project}: ${(err as Error).message}`, true);
  }
}

// ------------------------------------------------------------------ go

/** Open the project to start in, retrying while the server starts. */
async function loadWorkspace(): Promise<{ main: string | null }> {
  for (;;) {
    try {
      const listing = await api.listProjects();
      // Files from before projects moved to "my-files": their drafts and tabs go with them.
      if (listing.projects.some((p) => p.name === "my-files")) {
        adoptLegacyDrafts("my-files");
        workbench.adoptLegacyLayout("my-files");
      }
      return await openStartingProject(listing);
    } catch (err) {
      setStatus("offline", "Waiting for engine");
      showEmpty(`${(err as Error).message} The files are loaded from the workspace folder it serves.`);
      await new Promise((r) => setTimeout(r, 1500));
    }
  }
}

async function start(): Promise<void> {
  setStatus("idle", "Starting…");
  viewer.setEmptyUnits(startSystem, defaultUnits(startSystem));
  workbench.restoreSidebar();
  const openedFirst = await loadWorkspace();
  $("empty").hidden = true;

  // Work left in the single-file editor of earlier versions becomes a file.
  const legacy = takeLegacyDraft();
  if (legacy && !Object.values(store.files()).includes(legacy.text)) {
    const doc = await store.create(store.freePath(legacy.name), legacy.text).catch(() => null);
    if (doc) toast(`Your unsaved plate from before is now ${doc.path} in ${store.project}.`);
  }
  await showProject(openedFirst.main);
  showStart();
}

void start();

// Handy for debugging from the console.
Object.assign(window, {
  plate: {
    store,
    workbench,
    viewer,
    engine: configEngine,
    solve: runSolve,
    beginDrawing,
    layout: () => lastLayout,
  },
});
