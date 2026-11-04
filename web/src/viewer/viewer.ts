import type { Element, Layout, Point, System, Units } from "../types";
import { degrees, Lengths } from "./format";
import {
  edgeNormal,
  isNoop,
  movedPoints,
  neighbours,
  offsetEdge,
  snapOffset,
  translateEdge,
  type EdgeMove,
} from "./edgeMove";

/**
 * A 2D drawing sheet: pan, zoom, grid with rulers, dimensions, hover inspection, a measure
 * tool, drawing tools for the starting geometry, and direct editing (drag, insert, delete
 * points). Geometry is projected to screen space in JS and drawn as SVG, so line weights, text
 * and hatching stay constant at every zoom level.
 *
 * The viewer never edits the configuration itself: drawing and editing are reported through
 * callbacks, and the app writes them into the file.
 */

const SVG = "http://www.w3.org/2000/svg";
const RULER = 24; // ruler thickness, px
const DIM_OFFSET = 34; // dimension line distance from the geometry, px
const HIT_EDGE = 7;
const HIT_VERTEX = 9;
const SNAP = 10;

export type Tool = "select" | "measure" | "draw";
export type DrawKind = "boundary" | "spine" | "point";

export interface ViewerStatus {
  cursor: Point | null;
  /** Approximate drawing scale at 96 dpi, e.g. 250 for 1:250. */
  scale: number;
  units: Units;
  /** Formats lengths for the current measurement system. */
  lengths: Lengths;
  tool: Tool;
  /** Screen pixels per drawing unit. */
  pxPerUnit: number;
  /** Distance the cursor snaps to, in drawing units (0 when snapping is off). */
  snapStep: number;
}

export interface DrawRequest {
  kind: DrawKind;
  done(points: Point[]): void;
  cancel(): void;
}

export type DragPhase = "start" | "move" | "end" | "cancel";

interface View {
  k: number; // px per unit
  tx: number;
  ty: number;
}

type Hover =
  | { type: "edge"; element: Element; index: number; a: Point; b: Point }
  | { type: "vertex"; element: Element; index: number; p: Point }
  | { type: "face"; element: Element }
  | { type: "space"; element: Element }
  | { type: "wall"; element: Element }
  | { type: "part"; element: Element }
  | null;

/** What a right-click landed on. */
export type ContextTarget =
  | { type: "space"; element: Element }
  | { type: "edge"; element: Element; index: number }
  | { type: "wall"; element: Element }
  | { type: "part"; element: Element };

/**
 * The starting geometry, which the user edits by dragging; everything else is solved. A plan's
 * preview slot is resized by its corners when it's written in the file (it has a `source`).
 */
function editable(e: Element): boolean {
  return e.source === "/start/points" || (e.kind === "slot" && !!e.source);
}

/** What the drawing is framed by: the starting geometry, or a plan's preview slot. */
function framing(e: Element): boolean {
  return editable(e) || e.kind === "slot";
}

interface Measure {
  a: Point | null;
  b: Point | null;
}

interface Drawing {
  request: DrawRequest;
  points: Point[];
}

/** An edge being dragged: a ghost of where it goes, written to the file on release. */
interface EdgeDrag {
  element: Element;
  index: number;
  /** The element's points as they were when the drag started. */
  original: Point[];
  /** Where the edge was picked up, on the edge (drawing units). */
  grab: Point;
  /** Where the pointer went down, on screen: a click doesn't move anything. */
  down: { x: number; y: number };
  pointerId: number;
  moved: boolean;
  move: EdgeMove | null;
  /** An existing point the edge snapped to. */
  snap: Point | null;
}

interface VertexDrag {
  element: Element;
  index: number;
  original: Point;
  pointerId: number;
  moved: boolean;
}

function el<K extends keyof SVGElementTagNameMap>(
  tag: K,
  attrs: Record<string, string | number> = {},
  parent?: SVGElement,
): SVGElementTagNameMap[K] {
  const node = document.createElementNS(SVG, tag);
  for (const [k, v] of Object.entries(attrs)) node.setAttribute(k, String(v));
  parent?.appendChild(node);
  return node;
}

/** Parts of a service are drawn at least this big on screen, as symbols are on paper. */
const PART_MIN_PX = 16;

const prefersReducedMotion = () => window.matchMedia("(prefers-reduced-motion: reduce)").matches;

export class Viewer {
  private readonly root: HTMLElement;
  private readonly svg: SVGSVGElement;
  private readonly tooltip: HTMLDivElement;
  private layout: Layout | null = null;
  /** Units for an empty sheet (while drawing the first shape). */
  private emptyUnits: { system: System; units: Units } = { system: "metric", units: "m" };
  private view: View = { k: 10, tx: 0, ty: 0 };
  private width = 0;
  private height = 0;
  private follow = true;
  private hover: Hover = null;
  private cursor: Point | null = null;
  private screenCursor: { x: number; y: number } | null = null;
  private measure: Measure = { a: null, b: null };
  private snapPoint: Point | null = null;
  private pan: { x: number; y: number; tx: number; ty: number; moved: boolean } | null = null;
  private vertexDrag: VertexDrag | null = null;
  private edgeDrag: EdgeDrag | null = null;
  /** A layout that arrived during an edge drag, shown if the drag is cancelled. */
  private pendingLayout: Layout | null | undefined = undefined;
  private drawing: Drawing | null = null;
  private spaceDown = false;
  private shiftDown = false;
  private altDown = false;
  private frame = 0;
  private animation = 0;

  showDimensions = true;
  showGrid = true;
  tool: Tool = "select";
  /** The service shown over the plan (its parts and networks, the rest faded), or none. */
  serviceView: string | null = null;

  onStatus: (status: ViewerStatus) => void = () => {};
  /** A point of an element is being dragged. `point` is in drawing units. */
  onVertexDrag: (element: Element, index: number, point: Point, phase: DragPhase) => void = () => {};
  /**
   * An edge of the starting geometry is being dragged. `start` when it's picked up (the app can
   * refuse with `abortEdgeDrag`), `end` with where it was dropped, `cancel` when nothing is to be
   * written (Esc, a click, or a drop where the outline would be invalid: `move.problem` says why).
   */
  onEdgeDrag: (
    element: Element,
    index: number,
    move: EdgeMove | null,
    phase: Exclude<DragPhase, "move">,
  ) => void = () => {};
  /** Double-click on an edge: insert a point after `index`. */
  onVertexInsert: (element: Element, index: number, point: Point) => void = () => {};
  /** Right-click (or Delete) on a point. */
  onVertexDelete: (element: Element, index: number) => void = () => {};
  /** Hint text for the status bar while drawing. */
  onHint: (text: string) => void = () => {};
  /** Right-click on a space, a wall, a part or a boundary edge, at client coordinates. */
  onContextMenu: (target: ContextTarget, clientX: number, clientY: number) => void = () => {};
  /** S was pressed in the drawing: show the next view. */
  onCycleView: () => void = () => {};

  /** Port roles of the boundary's edges, for styling and tooltips. */
  private edgeRoles: string[][] = [];

  constructor(root: HTMLElement) {
    this.root = root;
    this.svg = el("svg", { class: "sheet", role: "img", "aria-label": "Floor plate drawing" });
    this.svg.tabIndex = 0;
    this.tooltip = document.createElement("div");
    this.tooltip.className = "tip";
    this.tooltip.hidden = true;
    root.append(this.svg, this.tooltip);

    new ResizeObserver(() => this.resize()).observe(root);
    this.resize();
    this.bindInput();
  }

  // ---------------------------------------------------------------- public API

  setLayout(layout: Layout | null): void {
    const first = !this.layout;
    // Keep the dragged element's local edits until the drag ends.
    if (this.vertexDrag) return;
    if (this.edgeDrag) {
      this.pendingLayout = layout;
      return;
    }
    this.layout = layout;
    this.hover = null;
    if (layout && (this.follow || first)) this.fit(!first);
    this.request();
  }

  /** Show one service over the plan, or none (the plan on its own). */
  setServiceView(name: string | null): void {
    if (this.serviceView === name) return;
    this.serviceView = name;
    this.hover = null;
    this.request();
  }

  /** Port roles of the boundary edges, in order (`facade` when not given). */
  setEdgeRoles(roles: string[][]): void {
    this.edgeRoles = roles;
    this.request();
  }

  /** Units used while the sheet is empty (e.g. drawing the first shape). */
  setEmptyUnits(system: System, units: Units): void {
    this.emptyUnits = { system, units };
    this.request();
    this.emitStatus();
  }

  get units(): Units {
    return this.layout?.units ?? this.emptyUnits.units;
  }

  get lengths(): Lengths {
    return this.layout
      ? Lengths.for(this.layout)
      : new Lengths(this.emptyUnits.system, this.emptyUnits.units);
  }

  fit(animate = true): void {
    this.follow = true;
    const target = this.fitView();
    if (target) this.animateTo(target, animate);
  }

  zoomBy(factor: number, at?: { x: number; y: number }): void {
    const p = at ?? { x: RULER + (this.width - RULER) / 2, y: RULER + (this.height - RULER) / 2 };
    const target = this.zoomed(this.view, factor, p);
    this.follow = false;
    this.animateTo(target, true);
  }

  setTool(tool: Tool): void {
    if (this.drawing && tool !== "draw") this.cancelDrawing();
    this.tool = tool;
    this.measure = { a: null, b: null };
    this.snapPoint = null;
    this.root.dataset.tool = tool;
    this.request();
    this.emitStatus();
  }

  toggleDimensions(on = !this.showDimensions): void {
    this.showDimensions = on;
    this.request();
  }

  toggleGrid(on = !this.showGrid): void {
    this.showGrid = on;
    this.request();
  }

  /** Clear the sheet and frame an empty area around the origin, ready to draw. */
  clear(): void {
    this.layout = null;
    this.hover = null;
    this.follow = true;
    const v = this.fitView();
    if (v) this.view = v;
    this.request();
    this.emitStatus();
  }

  /** Start drawing the starting geometry. The request's callbacks report the result. */
  startDrawing(request: DrawRequest): void {
    this.drawing = { request, points: [] };
    this.tool = "draw";
    this.root.dataset.tool = "draw";
    this.svg.focus({ preventScroll: true });
    this.hint();
    this.request();
    this.emitStatus();
  }

  get isDrawing(): boolean {
    return this.drawing !== null;
  }

  cancelDrawing(): void {
    const d = this.drawing;
    this.drawing = null;
    this.tool = "select";
    this.root.dataset.tool = "select";
    this.request();
    d?.request.cancel();
  }

  /** Abandon a point drag that can't be written to the file, restoring the point. */
  abortVertexDrag(): void {
    const d = this.vertexDrag;
    if (!d) return;
    d.element.points[d.index] = d.original;
    this.svg.releasePointerCapture?.(d.pointerId);
    this.vertexDrag = null;
    this.root.classList.remove("is-dragging");
    this.request();
  }

  /** Abandon an edge drag the app can't write to the file. */
  abortEdgeDrag(): void {
    if (this.edgeDrag) this.finishEdgeDrag(false, false);
  }

  /** Move one point of the displayed geometry (an optimistic preview while dragging). */
  previewPoint(elementId: string, index: number, point: Point): void {
    const element = this.layout?.elements.find((e) => e.id === elementId);
    if (!element) return;
    element.points[index] = point;
    this.request();
  }

  /** Where a drawing point is on screen, in client coordinates (for tools and tests). */
  project(p: Point): { x: number; y: number } {
    const s = this.toScreen(p);
    const r = this.svg.getBoundingClientRect();
    return { x: s.x + r.left, y: s.y + r.top };
  }

  get element(): SVGSVGElement {
    return this.svg;
  }

  // ------------------------------------------------------------ view maths

  private toScreen(p: Point): { x: number; y: number } {
    const { k, tx, ty } = this.view;
    return { x: p.x * k + tx, y: -p.y * k + ty };
  }

  private toWorld(x: number, y: number): Point {
    const { k, tx, ty } = this.view;
    return { x: (x - tx) / k, y: (ty - y) / k };
  }

  /** Screen space kept clear when fitting: rulers, the tool strip, the title block. */
  insets = { top: RULER + 60, right: 64, bottom: 190, left: RULER + 100 };

  /** Smallest area to frame, so a single point (or nothing) still gets a sensible view. */
  private minSpan(): number {
    const mm = { mm: 1, cm: 10, m: 1000, in: 25.4, ft: 304.8 }[this.units];
    return (this.lengths.system === "imperial" ? 120 * 304.8 : 40_000) / mm;
  }

  private fitView(): View | null {
    if (this.width === 0) return null;
    const b = this.layout?.bounds;
    const min = this.minSpan();
    // An empty sheet frames the origin near the lower left, ready for drawing.
    const box = b ?? { minX: -min * 0.15, minY: -min * 0.15, maxX: min, maxY: min * 0.7 };
    const { top, right, bottom, left } = this.insets;
    const cx = (box.minX + box.maxX) / 2;
    const cy = (box.minY + box.maxY) / 2;
    const w = Math.max(box.maxX - box.minX, b ? min * 0.25 : min);
    const h = Math.max(box.maxY - box.minY, b ? min * 0.25 : min * 0.6);
    const availW = Math.max(80, this.width - left - right);
    const availH = Math.max(80, this.height - top - bottom);
    const k = Math.max(1e-6, Math.min(availW / w, availH / h));
    return { k, tx: left + availW / 2 - cx * k, ty: top + availH / 2 + cy * k };
  }

  private zoomed(view: View, factor: number, at: { x: number; y: number }): View {
    const fitK = this.fitView()?.k ?? view.k;
    const k = Math.min(fitK * 400, Math.max(fitK / 40, view.k * factor));
    const f = k / view.k;
    return { k, tx: at.x - (at.x - view.tx) * f, ty: at.y - (at.y - view.ty) * f };
  }

  private animateTo(target: View, animate: boolean): void {
    cancelAnimationFrame(this.animation);
    if (!animate || prefersReducedMotion()) {
      this.view = target;
      this.request();
      this.emitStatus();
      return;
    }
    const from = { ...this.view };
    const start = performance.now();
    const duration = 220;
    const step = (now: number) => {
      const t = Math.min(1, (now - start) / duration);
      const e = 1 - Math.pow(1 - t, 3);
      // Interpolate scale geometrically so zooms feel even.
      const k = from.k * Math.pow(target.k / from.k, e);
      this.view = { k, tx: from.tx + (target.tx - from.tx) * e, ty: from.ty + (target.ty - from.ty) * e };
      this.render();
      if (t < 1) this.animation = requestAnimationFrame(step);
      else this.emitStatus();
    };
    this.animation = requestAnimationFrame(step);
  }

  private resize(): void {
    const r = this.root.getBoundingClientRect();
    this.width = r.width;
    this.height = r.height;
    this.svg.setAttribute("viewBox", `0 0 ${r.width} ${r.height}`);
    if (this.follow) {
      const v = this.fitView();
      if (v) this.view = v;
    }
    this.request();
  }

  private scaleRatio(): number {
    const mmPerUnit = { mm: 1, cm: 10, m: 1000, in: 25.4, ft: 304.8 }[this.units];
    const mmPerPx = 25.4 / 96;
    const raw = mmPerUnit / (this.view.k * mmPerPx);
    const mag = Math.pow(10, Math.floor(Math.log10(raw)) - 1);
    return Math.round(raw / mag) * mag;
  }

  /** The grid step the cursor snaps to while drawing or dragging; Alt turns snapping off. */
  /**
   * Snap a point on edge a–b to the grid without leaving the edge: the free coordinate of a
   * horizontal or vertical edge, otherwise the distance from a. Never onto an end point.
   */
  private snapOnEdge(p: Point, a: Point, b: Point): Point {
    const step = this.snapStep();
    if (!step) return p;
    const round = (v: number) => Math.round(v / step) * step;
    const len = Math.hypot(b.x - a.x, b.y - a.y);
    const eps = len * 1e-9;
    let q: Point;
    if (Math.abs(b.y - a.y) <= eps) q = { x: round(p.x), y: a.y };
    else if (Math.abs(b.x - a.x) <= eps) q = { x: a.x, y: round(p.y) };
    else {
      const d = round(Math.hypot(p.x - a.x, p.y - a.y)) / len;
      q = { x: a.x + (b.x - a.x) * d, y: a.y + (b.y - a.y) * d };
    }
    const t = ((q.x - a.x) * (b.x - a.x) + (q.y - a.y) * (b.y - a.y)) / (len * len);
    return t > 1e-6 && t < 1 - 1e-6 ? q : p;
  }

  private snapStep(): number {
    return this.altDown ? 0 : this.gridSteps().minor;
  }

  private emitStatus(): void {
    this.onStatus({
      cursor: this.cursor,
      scale: this.scaleRatio(),
      units: this.units,
      lengths: this.lengths,
      tool: this.tool,
      pxPerUnit: this.view.k,
      snapStep: this.tool === "draw" || this.vertexDrag || this.edgeDrag ? this.snapStep() : 0,
    });
  }

  // ------------------------------------------------------------ snapping

  /** Where a point placed at screen (x, y) lands: other points, then angle, then grid. */
  private snapWorld(
    x: number,
    y: number,
    from: Point | null,
    avoid?: { element: Element; index: number },
  ): Point {
    // Existing points (and the first drawn point, to close a boundary).
    const candidates: Point[] = [];
    for (const e of this.layout?.elements ?? []) {
      e.points.forEach((p, i) => {
        if (!(avoid && avoid.element.id === e.id && avoid.index === i)) candidates.push(p);
      });
    }
    if (this.drawing) candidates.push(...this.drawing.points);
    for (const p of candidates) {
      const s = this.toScreen(p);
      if (Math.hypot(s.x - x, s.y - y) <= SNAP) return { ...p };
    }

    let w = this.toWorld(x, y);
    const step = this.snapStep();
    if (from && this.shiftDown) {
      // Constrain to 45° steps, keeping the length on the grid.
      const ang = Math.round(Math.atan2(w.y - from.y, w.x - from.x) / (Math.PI / 4)) * (Math.PI / 4);
      let len = Math.hypot(w.x - from.x, w.y - from.y);
      if (step) len = Math.max(step, Math.round(len / step) * step);
      return { x: tidy(from.x + Math.cos(ang) * len), y: tidy(from.y + Math.sin(ang) * len) };
    }
    if (step) w = { x: tidy(Math.round(w.x / step) * step), y: tidy(Math.round(w.y / step) * step) };
    return w;
  }

  // ------------------------------------------------------------ input

  private bindInput(): void {
    const svg = this.svg;
    const local = (e: MouseEvent) => {
      const rect = svg.getBoundingClientRect();
      return { x: e.clientX - rect.left, y: e.clientY - rect.top };
    };

    svg.addEventListener(
      "wheel",
      (e) => {
        e.preventDefault();
        const at = local(e);
        const speed = e.ctrlKey ? 0.01 : 0.0018;
        const delta = e.deltaMode === 1 ? e.deltaY * 16 : e.deltaY;
        cancelAnimationFrame(this.animation);
        this.view = this.zoomed(this.view, Math.exp(-delta * speed), at);
        this.follow = false;
        this.updateCursor(at.x, at.y);
        this.request();
        this.emitStatus();
      },
      { passive: false },
    );

    svg.addEventListener("pointerdown", (e) => {
      svg.focus({ preventScroll: true });
      this.syncModifiers(e);
      // Start dragging a point of the geometry.
      if (e.button === 0 && this.tool === "select" && !this.spaceDown && this.hover?.type === "vertex") {
        e.preventDefault();
        svg.setPointerCapture(e.pointerId);
        const { element, index, p } = this.hover;
        this.vertexDrag = { element, index, original: { ...p }, pointerId: e.pointerId, moved: false };
        this.root.classList.add("is-dragging");
        this.follow = false; // keep the view still while the user shapes the drawing
        this.onVertexDrag(element, index, p, "start");
        return;
      }
      // Pick up an edge: a ghost shows where it goes until the button is released.
      if (
        e.button === 0 &&
        this.tool === "select" &&
        !this.spaceDown &&
        this.hover?.type === "edge" &&
        edgeMovable(this.hover.element)
      ) {
        e.preventDefault();
        svg.setPointerCapture(e.pointerId);
        const { element, index, a, b } = this.hover;
        const at = local(e);
        this.edgeDrag = {
          element,
          index,
          original: element.points.map((p) => ({ ...p })),
          grab: projectOnSegment(this.toWorld(at.x, at.y), a, b),
          down: at,
          pointerId: e.pointerId,
          moved: false,
          move: null,
          snap: null,
        };
        this.root.classList.add("is-moving-edge");
        return;
      }
      const panning =
        e.button === 1 ||
        this.spaceDown ||
        (e.button === 0 && (this.tool === "select" || this.tool === "draw"));
      if (panning) {
        e.preventDefault();
        svg.setPointerCapture(e.pointerId);
        this.pan = { x: e.clientX, y: e.clientY, tx: this.view.tx, ty: this.view.ty, moved: false };
      }
    });

    svg.addEventListener("pointermove", (e) => {
      this.syncModifiers(e);
      const { x, y } = local(e);
      if (this.edgeDrag) {
        const d = this.edgeDrag;
        if (!d.moved && Math.hypot(x - d.down.x, y - d.down.y) > 3) {
          // Picked up for real (not a click or a double-click): the app may refuse it here.
          d.moved = true;
          this.follow = false;
          this.onEdgeDrag(d.element, d.index, null, "start");
        }
      } else if (this.vertexDrag) {
        const d = this.vertexDrag;
        const prev = d.element.points[(d.index - 1 + d.element.points.length) % d.element.points.length];
        const p = this.snapWorld(x, y, d.element.points.length > 1 ? prev : null, d);
        const current = d.element.points[d.index];
        if (p.x !== current.x || p.y !== current.y) {
          d.moved = true;
          d.element.points[d.index] = p;
          this.onVertexDrag(d.element, d.index, p, "move");
        }
      } else if (this.pan) {
        const dx = e.clientX - this.pan.x;
        const dy = e.clientY - this.pan.y;
        if (Math.abs(dx) + Math.abs(dy) > 3) {
          this.pan.moved = true;
          this.root.classList.add("is-panning");
        }
        if (this.pan.moved) {
          cancelAnimationFrame(this.animation);
          this.view = { ...this.view, tx: this.pan.tx + dx, ty: this.pan.ty + dy };
          this.follow = false;
        }
      }
      this.updateCursor(x, y);
      this.updateEdgeGhost();
      this.request();
      this.emitStatus();
    });

    const end = (e: PointerEvent) => {
      if (this.edgeDrag) {
        this.finishEdgeDrag(e.type === "pointerup");
        return;
      }
      if (this.vertexDrag) {
        const d = this.vertexDrag;
        svg.releasePointerCapture(d.pointerId);
        this.vertexDrag = null;
        this.root.classList.remove("is-dragging");
        this.onVertexDrag(d.element, d.index, d.element.points[d.index], d.moved ? "end" : "cancel");
        this.request();
        return;
      }
      const click = this.pan && !this.pan.moved;
      if (this.pan) {
        svg.releasePointerCapture(e.pointerId);
        this.pan = null;
        this.root.classList.remove("is-panning");
      }
      if (e.type !== "pointerup" || e.button !== 0 || this.spaceDown) return;
      const { x, y } = local(e);
      if (this.tool === "measure") this.measureClick();
      else if (this.tool === "draw" && click) this.drawClick(x, y);
    };
    svg.addEventListener("pointerup", end);
    svg.addEventListener("pointercancel", end);

    svg.addEventListener("pointerleave", () => {
      if (this.pan || this.vertexDrag || this.edgeDrag) return;
      this.cursor = null;
      this.screenCursor = null;
      this.hover = null;
      this.snapPoint = null;
      this.tooltip.hidden = true;
      this.request();
      this.emitStatus();
    });

    svg.addEventListener("dblclick", (e) => {
      if (this.tool === "draw") return; // clicks already handle finishing
      const h = this.hover;
      if (this.tool === "select" && h?.type === "edge" && editable(h.element)) {
        const { x, y } = local(e);
        const p = this.snapOnEdge(projectOnSegment(this.toWorld(x, y), h.a, h.b), h.a, h.b);
        this.follow = false;
        this.onVertexInsert(h.element, h.index, { x: tidy(p.x), y: tidy(p.y) });
        return;
      }
      this.fit();
    });

    svg.addEventListener("contextmenu", (e) => {
      e.preventDefault();
      const h = this.hover;
      if (this.tool === "select" && h?.type === "vertex") {
        this.follow = false;
        this.onVertexDelete(h.element, h.index);
      } else if (this.tool === "draw") {
        this.undoDrawPoint();
      } else if (
        this.tool === "select" &&
        (h?.type === "space" || h?.type === "wall" || h?.type === "part")
      ) {
        this.tooltip.hidden = true;
        this.onContextMenu({ type: h.type, element: h.element }, e.clientX, e.clientY);
      } else if (this.tool === "select" && h?.type === "edge" && h.element.kind === "boundary") {
        this.tooltip.hidden = true;
        this.onContextMenu({ type: "edge", element: h.element, index: h.index }, e.clientX, e.clientY);
      }
    });

    window.addEventListener("keydown", (e) => {
      this.syncModifiers(e);
      if (e.code === "Space" && document.activeElement === svg) {
        this.spaceDown = true;
        this.root.classList.add("is-space");
        e.preventDefault();
      }
    });
    window.addEventListener("keyup", (e) => {
      this.syncModifiers(e);
      if (e.code === "Space") {
        this.spaceDown = false;
        this.root.classList.remove("is-space");
      }
    });
  }

  private syncModifiers(e: { shiftKey: boolean; altKey: boolean }): void {
    if (this.shiftDown !== e.shiftKey || this.altDown !== e.altKey) {
      this.shiftDown = e.shiftKey;
      this.altDown = e.altKey;
      if (this.screenCursor) this.updateCursor(this.screenCursor.x, this.screenCursor.y);
      this.updateEdgeGhost();
      this.request();
      this.emitStatus();
    }
  }

  /** Keyboard shortcuts while the drawing has focus. Returns true when handled. */
  handleKey(e: KeyboardEvent): boolean {
    if (e.ctrlKey || e.metaKey) return false;
    if (this.drawing) {
      switch (e.key) {
        case "Enter":
          this.finishDrawing();
          return true;
        case "Escape":
          this.cancelDrawing();
          return true;
        case "Backspace":
        case "Delete":
          this.undoDrawPoint();
          return true;
      }
    }
    if (this.edgeDrag && e.key === "Escape") {
      this.finishEdgeDrag(false);
      return true;
    }
    if (this.vertexDrag && e.key === "Escape") {
      const d = this.vertexDrag;
      d.element.points[d.index] = d.original;
      this.vertexDrag = null;
      this.root.classList.remove("is-dragging");
      this.onVertexDrag(d.element, d.index, d.original, "cancel");
      this.request();
      return true;
    }
    if ((e.key === "Delete" || e.key === "Backspace") && this.hover?.type === "vertex") {
      this.follow = false;
      this.onVertexDelete(this.hover.element, this.hover.index);
      return true;
    }
    if (e.altKey) return false;
    switch (e.key.toLowerCase()) {
      case "f":
        this.fit();
        return true;
      case "d":
        this.toggleDimensions();
        return true;
      case "g":
        this.toggleGrid();
        return true;
      case "m":
        if (this.drawing) return false;
        this.setTool(this.tool === "measure" ? "select" : "measure");
        return true;
      case "s":
        if (this.drawing) return false;
        this.onCycleView();
        return true;
      case "v":
        if (this.drawing) return false;
        this.setTool("select");
        return true;
      case "escape":
        if (this.measure.a) this.measure = { a: null, b: null };
        else this.setTool("select");
        this.request();
        return true;
      case "+":
      case "=":
        this.zoomBy(1.4);
        return true;
      case "-":
        this.zoomBy(1 / 1.4);
        return true;
    }
    return false;
  }

  // ------------------------------------------------------------ drawing

  private minPoints(kind: DrawKind): number {
    return kind === "boundary" ? 3 : kind === "spine" ? 2 : 1;
  }

  private drawClick(x: number, y: number): void {
    const d = this.drawing;
    if (!d) return;
    const last = d.points[d.points.length - 1] ?? null;
    const p = this.snapWorld(x, y, last);
    const same = (a: Point | undefined) => !!a && Math.hypot(a.x - p.x, a.y - p.y) < 1e-9;
    if (d.request.kind === "point") {
      d.points = [p];
      this.finishDrawing();
      return;
    }
    // Clicking the first point closes a boundary; clicking the last point again finishes.
    if (d.request.kind === "boundary" && d.points.length >= 3 && same(d.points[0])) {
      this.finishDrawing();
      return;
    }
    if (same(last ?? undefined)) {
      this.finishDrawing();
      return;
    }
    d.points.push(p);
    this.hint();
    this.request();
  }

  private undoDrawPoint(): void {
    if (!this.drawing) return;
    this.drawing.points.pop();
    this.hint();
    this.request();
  }

  private finishDrawing(): void {
    const d = this.drawing;
    if (!d) return;
    if (d.points.length < this.minPoints(d.request.kind)) {
      this.hint(`A ${d.request.kind} needs at least ${this.minPoints(d.request.kind)} points.`);
      return;
    }
    this.drawing = null;
    this.tool = "select";
    this.root.dataset.tool = "select";
    this.request();
    this.emitStatus();
    d.request.done(d.points);
  }

  private hint(override?: string): void {
    const d = this.drawing;
    if (!d) return;
    if (override) return this.onHint(override);
    const n = d.points.length;
    const snapping = "Shift snaps angles, Alt turns off grid snap";
    if (d.request.kind === "point") return this.onHint("Click to place the origin.");
    if (d.request.kind === "boundary") {
      return this.onHint(
        n < 3
          ? `Click to place corner ${n + 1}. ${snapping}.`
          : `Click to add corners. Click the first corner or press Enter to close. Backspace undoes.`,
      );
    }
    return this.onHint(
      n < 2
        ? `Click to place point ${n + 1} of the spine. ${snapping}.`
        : `Click to add points. Press Enter or click the last point to finish. Backspace undoes.`,
    );
  }

  private measureClick(): void {
    const p = this.snapPoint ?? this.cursor;
    if (!p) return;
    if (!this.measure.a || this.measure.b) this.measure = { a: p, b: null };
    else this.measure = { a: this.measure.a, b: p };
    this.request();
  }

  private updateCursor(x: number, y: number): void {
    this.screenCursor = { x, y };
    const inSheet = x > RULER && y > RULER;
    this.cursor = inSheet ? this.toWorld(x, y) : null;
    const busy = this.pan?.moved || this.vertexDrag || this.edgeDrag || this.tool === "draw";
    this.hover = inSheet && !busy ? this.hitTest(x, y) : this.vertexDrag || this.edgeDrag ? this.hover : null;
    this.snapPoint = this.tool === "measure" && inSheet ? this.measureSnap(x, y) : null;
    this.root.dataset.hover = this.hover?.type ?? "";
    // The cursor says which way an edge can be pushed.
    const h = this.hover;
    this.root.dataset.edgeAxis =
      h?.type !== "edge"
        ? ""
        : !edgeMovable(h.element)
          ? "fixed"
          : this.edgeDrag && this.shiftDown
            ? "free"
            : axisOf(h.a, h.b);
  }

  // ------------------------------------------------------------ moving an edge

  /** Recompute the ghost from the cursor: a parallel move, or a free one with Shift. */
  private updateEdgeGhost(): void {
    const d = this.edgeDrag;
    const c = this.screenCursor;
    if (!d || !d.moved || !c) return;
    const pts = d.original;
    const j = (d.index + 1) % pts.length;
    const [a, b] = [pts[d.index], pts[j]];
    const w = this.toWorld(c.x, c.y);
    const near = this.altDown ? null : this.nearPoint(c.x, c.y, d);
    d.snap = near;
    if (this.shiftDown) {
      // Free: both ends follow the cursor. Snapping puts the nearer end on a point, or moves by
      // whole grid steps.
      let delta = { x: w.x - d.grab.x, y: w.y - d.grab.y };
      if (near) {
        const da = Math.hypot(a.x + delta.x - near.x, a.y + delta.y - near.y);
        const db = Math.hypot(b.x + delta.x - near.x, b.y + delta.y - near.y);
        const end = da <= db ? a : b;
        delta = { x: near.x - end.x, y: near.y - end.y };
      } else {
        const step = this.snapStep();
        if (step) delta = { x: Math.round(delta.x / step) * step, y: Math.round(delta.y / step) * step };
      }
      d.move = translateEdge(pts, d.element.closed, d.index, delta);
      return;
    }
    // Parallel: only the distance square to the edge counts. A nearby point puts the edge's line
    // through it; otherwise the grid.
    const n = edgeNormal(a, b);
    const offset = near
      ? (near.x - a.x) * n.x + (near.y - a.y) * n.y
      : snapOffset(a, b, (w.x - d.grab.x) * n.x + (w.y - d.grab.y) * n.y, this.snapStep());
    d.move = offsetEdge(pts, d.element.closed, d.index, offset);
  }

  /** An existing point near screen (x, y) to snap the dragged edge to (not its own ends). */
  private nearPoint(x: number, y: number, d: EdgeDrag): Point | null {
    const j = (d.index + 1) % d.original.length;
    let best: Point | null = null;
    let bestD = SNAP;
    for (const e of this.layout?.elements ?? []) {
      if (e.kind !== "boundary" && e.kind !== "spine" && e.kind !== "space" && e.kind !== "slot") continue;
      e.points.forEach((p, i) => {
        if (e.id === d.element.id && (i === d.index || i === j)) return;
        const s = this.toScreen(p);
        const dist = Math.hypot(s.x - x, s.y - y);
        if (dist < bestD) {
          best = p;
          bestD = dist;
        }
      });
    }
    return best;
  }

  /**
   * End an edge drag. `commit`: write where it was dropped (when it moved, and the outline is
   * valid). `report`: tell the app (not when the app itself aborted the drag).
   */
  private finishEdgeDrag(commit: boolean, report = true): void {
    const d = this.edgeDrag;
    if (!d) return;
    this.svg.releasePointerCapture?.(d.pointerId);
    this.edgeDrag = null;
    this.root.classList.remove("is-moving-edge");
    const m = d.move;
    const write = commit && d.moved && m && !m.problem && !isNoop(d.original, d.index, m);
    report &&= d.moved; // the app only hears about drags it saw start
    const pending = this.pendingLayout;
    this.pendingLayout = undefined;
    if (write) {
      // Show it where it was dropped until the new solve comes back.
      const moved = movedPoints(d.original, d.index, m);
      d.element.points.splice(0, d.element.points.length, ...moved);
      if (report) this.onEdgeDrag(d.element, d.index, m, "end");
    } else {
      if (report) this.onEdgeDrag(d.element, d.index, commit && d.moved ? m : null, "cancel");
      if (pending !== undefined) this.setLayout(pending);
    }
    if (this.screenCursor) this.updateCursor(this.screenCursor.x, this.screenCursor.y);
    this.request();
    this.emitStatus();
  }

  private hitTest(x: number, y: number): Hover {
    if (!this.layout) return null;
    let best: Hover = null;
    let bestD = Infinity;
    // Points and edges of the starting geometry: these can be dragged and edited.
    for (const element of this.layout.elements.filter(editable)) {
      element.points.forEach((p, index) => {
        const s = this.toScreen(p);
        const d = Math.hypot(s.x - x, s.y - y);
        if (d <= HIT_VERTEX && d - 100 < bestD) {
          best = { type: "vertex", element, index, p };
          bestD = d - 100; // vertices win over edges
        }
      });
      edgesOf(element).forEach(([a, b], index) => {
        const d = segmentDistance({ x, y }, this.toScreen(a), this.toScreen(b));
        if (d <= HIT_EDGE && d < bestD) {
          best = { type: "edge", element, index, a, b };
          bestD = d;
        }
      });
    }
    if (best) return best;
    const w = this.toWorld(x, y);
    // A service's parts, when it's shown: small, so they're found within a few pixels.
    if (this.serviceView) {
      let part: Element | null = null;
      let near = 9;
      for (const e of this.layout.elements) {
        if (e.kind !== "part" || e.part?.service !== this.serviceView) continue;
        // As drawn: grown from where it stands (see renderService).
        const shape = this.partShape(e);
        const inside = pointInPolygon({ x, y }, shape);
        const c = centroid(shape);
        const d = inside ? 0 : Math.hypot(c.x - x, c.y - y);
        if (d < near) {
          near = d;
          part = e;
        }
      }
      if (part) return { type: "part", element: part };
    }
    // Walls take floor between the spaces' clear outlines.
    for (const e of this.layout.elements) {
      if (e.kind === "wall" && pointInPolygon(w, e.points)) return { type: "wall", element: e };
    }
    // The innermost space under the cursor.
    let space: Element | null = null;
    for (const e of this.layout.elements) {
      if (e.kind !== "space" || !pointInPolygon(w, e.points)) continue;
      if (!space || (e.space?.depth ?? 0) >= (space.space?.depth ?? 0)) space = e;
    }
    if (space) return { type: "space", element: space };
    const face = this.layout.elements.find((e) => e.closed && editable(e) && pointInPolygon(w, e.points));
    return face ? { type: "face", element: face } : null;
  }

  private measureSnap(x: number, y: number): Point | null {
    if (!this.layout) return null;
    let best: Point | null = null;
    let bestD = SNAP;
    for (const e of this.layout.elements) {
      const candidates = [
        ...e.points,
        ...edgesOf(e).map(([a, b]) => ({ x: (a.x + b.x) / 2, y: (a.y + b.y) / 2 })),
      ];
      for (const p of candidates) {
        const s = this.toScreen(p);
        const d = Math.hypot(s.x - x, s.y - y);
        if (d < bestD) {
          best = p;
          bestD = d;
        }
      }
    }
    return best;
  }

  // ------------------------------------------------------------ rendering

  private request(): void {
    if (this.frame) return;
    this.frame = requestAnimationFrame(() => {
      this.frame = 0;
      this.render();
    });
  }

  private render(): void {
    const svg = this.svg;
    svg.replaceChildren();
    const defs = el("defs", {}, svg);
    const hatch = el(
      "pattern",
      { id: "hatch", width: 7, height: 7, patternUnits: "userSpaceOnUse", patternTransform: "rotate(45)" },
      defs,
    );
    el("line", { x1: 0, y1: 0, x2: 0, y2: 7, class: "hatch-line" }, hatch);
    // Floor no space covers: hatched in red, so a solution that isn't perfect shows where.
    const left = el(
      "pattern",
      {
        id: "leftover",
        width: 6,
        height: 6,
        patternUnits: "userSpaceOnUse",
        patternTransform: "rotate(-45)",
      },
      defs,
    );
    el("line", { x1: 0, y1: 0, x2: 0, y2: 6, class: "leftover-line" }, left);
    // Wall hatches: lines for a framed wall, a cross for masonry or a rated one.
    for (const [id, d] of [
      ["wall-hatch", "M0,0V5"],
      ["wall-cross", "M0,0V5M0,0H5"],
    ]) {
      const p = el(
        "pattern",
        { id, width: 5, height: 5, patternUnits: "userSpaceOnUse", patternTransform: "rotate(45)" },
        defs,
      );
      el("rect", { width: 5, height: 5, class: "wall-ground" }, p);
      el("path", { d, class: "wall-line" }, p);
    }
    const clip = el("clipPath", { id: "sheet-clip" }, defs);
    el(
      "rect",
      {
        x: RULER,
        y: RULER,
        width: Math.max(0, this.width - RULER),
        height: Math.max(0, this.height - RULER),
      },
      clip,
    );

    const sheet = el("g", { "clip-path": "url(#sheet-clip)" }, svg);
    if (this.showGrid) this.renderGrid(sheet);
    if (this.layout) {
      this.renderElements(sheet);
      if (this.showDimensions) this.renderDimensions(sheet);
      this.renderHover(sheet);
      if (this.edgeDrag) this.renderEdgeGhost(sheet);
    }
    if (this.drawing) this.renderDrawing(sheet);
    if (this.tool === "measure") this.renderMeasure(sheet);
    this.renderRulers(svg);
    this.renderTooltip();
  }

  private gridSteps(): { minor: number; major: number } {
    return this.lengths.gridSteps(this.view.k);
  }

  private renderGrid(parent: SVGElement): void {
    const { minor, major } = this.gridSteps();
    const tl = this.toWorld(RULER, RULER);
    const br = this.toWorld(this.width, this.height);
    const g = el("g", { class: "grid" }, parent);
    const minorPath: string[] = [];
    const majorPath: string[] = [];
    const isMajor = (v: number) => Math.abs(v / major - Math.round(v / major)) < 1e-6;

    for (let x = Math.floor(tl.x / minor) * minor; x <= br.x; x += minor) {
      const sx = Math.round(this.toScreen({ x, y: 0 }).x) + 0.5;
      (isMajor(x) ? majorPath : minorPath).push(`M${sx},${RULER}V${this.height}`);
    }
    for (let y = Math.floor(br.y / minor) * minor; y <= tl.y; y += minor) {
      const sy = Math.round(this.toScreen({ x: 0, y }).y) + 0.5;
      (isMajor(y) ? majorPath : minorPath).push(`M${RULER},${sy}H${this.width}`);
    }
    el("path", { d: minorPath.join(""), class: "grid-minor" }, g);
    el("path", { d: majorPath.join(""), class: "grid-major" }, g);

    const o = this.toScreen({ x: 0, y: 0 });
    el("path", { d: `M${o.x - 9},${o.y}H${o.x + 9}M${o.x},${o.y - 9}V${o.y + 9}`, class: "origin" }, g);
  }

  private renderElements(parent: SVGElement): void {
    // With a service shown, the plan is faded under it.
    const g = el("g", { class: this.serviceView ? "elements halftone" : "elements" }, parent);
    const elements = this.layout!.elements;
    const start = elements.filter(framing);
    // Fixtures last: they stand on the floor of their room, over their clearance.
    const rank = (e: Element) => (e.space?.depth ?? 0) + (e.space?.kind === "fixture" ? 1000 : 0);
    const spaces = elements.filter((e) => e.kind === "space").sort((a, b) => rank(a) - rank(b));

    // The starting boundary's hatch first: whatever the spaces leave uncovered shows it.
    for (const element of start) {
      if (element.closed) {
        const d = pathOf(
          element.points.map((p) => this.toScreen(p)),
          true,
        );
        el("path", { d, class: spaces.length ? "face face-under" : "face", fill: "url(#hatch)" }, g);
      }
    }

    let clearances = false;
    const symbols = new Map(elements.filter((e) => e.kind === "symbol").map((e) => [e.id, e]));
    for (const s of spaces) {
      const info = s.space!;
      if (info.kind === "fixture" && !clearances) {
        clearances = true;
        this.renderClearances(g);
      }
      const d = this.shapePath(s);
      const cls = [
        "space",
        `space-${info.kind}`,
        `space-depth-${Math.min(info.depth, 2)}`,
        info.hasPlan ? "space-parent" : "",
        info.ok ? "" : "space-failed",
        info.symbol ? "space-has-symbol" : "",
        info.fill ? "space-fill" : "",
        info.tone ? `space-tone-${info.tone}` : "",
      ].join(" ");
      el("path", { d, class: cls, "fill-rule": "evenodd" }, g);
      const symbol = symbols.get(`symbol:${s.id}`);
      if (symbol) this.renderSymbol(g, symbol);
      if (info.kind === "fixture" && info.front && !info.symbol) {
        // The front: where it's used from.
        const [a, b] = info.front.map((p) => this.toScreen(p));
        el("path", { d: `M${a.x},${a.y}L${b.x},${b.y}`, class: "fixture-front" }, g);
      }
    }
    if (!clearances) this.renderClearances(g);
    this.renderWalls(g);
    this.renderLeftover(g);

    this.renderOpenings(g);

    for (const element of start) {
      const pts = element.points.map((p) => this.toScreen(p));
      if (element.kind === "origin") {
        const p = pts[0];
        el("circle", { cx: p.x, cy: p.y, r: 7, class: "origin-ring" }, g);
        el(
          "path",
          { d: `M${p.x - 16},${p.y}H${p.x + 16}M${p.x},${p.y - 16}V${p.y + 16}`, class: "origin-cross" },
          g,
        );
        continue;
      }
      const d = pathOf(pts, element.closed);
      el("path", { d, class: `edge edge-${element.kind}` }, g);
      // Port roles other than facade are drawn over their edge.
      if (element.kind === "boundary") {
        edgesOf(element).forEach(([a, b], i) => {
          const roles = this.edgeRoles[i] ?? ["facade"];
          const role = roles.find((r) => r !== "facade");
          if (!role) return;
          const sa = this.toScreen(a);
          const sb = this.toScreen(b);
          el("path", { d: `M${sa.x},${sa.y}L${sb.x},${sb.y}`, class: `port port-${role.split(".")[0]}` }, g);
        });
      }
      if (element.kind === "slot") this.renderSlotPorts(g);
      if (!editable(element)) continue;
      // Points are always shown: they're what you drag.
      for (const p of pts) {
        if (element.kind === "spine") el("circle", { cx: p.x, cy: p.y, r: 3.5, class: "node" }, g);
        else el("rect", { x: p.x - 3, y: p.y - 3, width: 6, height: 6, class: "node node-corner" }, g);
      }
    }

    this.renderSpaceLabels(g, spaces);
    if (this.serviceView) this.renderService(parent, this.serviceView);
  }

  /** How much a part is grown on screen, and from where: no smaller than a symbol on paper,
   * grown from where it stands (its wall's face). */
  private partGrowth(p: Element): { grow: number; at: Point } {
    const pts = p.points.map((q) => this.toScreen(q));
    const size = Math.max(...pts.map((q) => Math.hypot(q.x - pts[0].x, q.y - pts[0].y)));
    const grow = size > 0 ? Math.max(1, PART_MIN_PX / size) : 1;
    return { grow, at: this.toScreen(p.part!.at) };
  }

  /** A part's footprint as drawn, on screen. */
  private partShape(p: Element): Point[] {
    const { grow, at } = this.partGrowth(p);
    return p.points.map((q) => {
      const s = this.toScreen(q);
      return { x: at.x + (s.x - at.x) * grow, y: at.y + (s.y - at.y) * grow };
    });
  }

  /** Walls, as poché: filled by their assembly's pattern, between the spaces' clear outlines. */
  private renderWalls(parent: SVGElement): void {
    const g = el("g", { class: "walls" }, parent);
    for (const w of this.layout!.elements) {
      if (w.kind !== "wall") continue;
      const d = pathOf(
        w.points.map((p) => this.toScreen(p)),
        true,
      );
      el("path", { d, class: `wall wall-${w.wall?.pattern ?? "solid"}` }, g);
    }
  }

  /** A service over the plan: its networks, then its parts. */
  private renderService(parent: SVGElement, service: string): void {
    const g = el("g", { class: "service" }, parent);
    for (const r of this.layout!.elements) {
      if (r.kind !== "run" || r.run?.service !== service) continue;
      const pts = r.points.map((p) => this.toScreen(p));
      el(
        "path",
        { d: pathOf(pts, false), class: `run run-${r.run.draw}${r.run.home ? " run-home" : ""}` },
        g,
      );
      if (r.run.home && pts.length >= 2) {
        // A home run: an arrow toward the source, with its group's number.
        const [a, b] = [pts[pts.length - 2], pts[pts.length - 1]];
        const len = Math.hypot(b.x - a.x, b.y - a.y) || 1;
        const [ux, uy] = [(b.x - a.x) / len, (b.y - a.y) / len];
        const head = `M${b.x - ux * 7 - uy * 3.5},${b.y - uy * 7 + ux * 3.5}L${b.x},${b.y}L${b.x - ux * 7 + uy * 3.5},${b.y - uy * 7 - ux * 3.5}`;
        el("path", { d: head, class: "run run-head" }, g);
        if (r.run.group) {
          const t = el("text", { x: b.x + ux * 6, y: b.y + uy * 6 + 3, class: "run-label" }, g);
          t.textContent = r.run.group;
        }
      }
    }
    for (const p of this.layout!.elements) {
      if (p.kind !== "part" || p.part?.service !== service) continue;
      const { grow, at } = this.partGrowth(p);
      const pg = el(
        "g",
        {
          class: `part part-${p.part.mount}`,
          ...(grow > 1
            ? { transform: `translate(${at.x},${at.y}) scale(${grow}) translate(${-at.x},${-at.y})` }
            : {}),
        },
        g,
      );
      el(
        "path",
        {
          d: pathOf(
            p.points.map((q) => this.toScreen(q)),
            true,
          ),
          class: p.symbol ? "part-box" : "part-box part-plain",
        },
        pg,
      );
      if (p.symbol) this.renderSymbol(pg, p);
    }
  }

  /** A closed element as a path, holes included (draw it with `fill-rule: evenodd`). */
  private shapePath(e: Element): string {
    return [e.points, ...(e.holes ?? [])]
      .map((ring) =>
        pathOf(
          ring.map((p) => this.toScreen(p)),
          true,
        ),
      )
      .join("");
  }

  /** Floor no space covers, hatched, with its area: what keeps a solution from being perfect. */
  private renderLeftover(parent: SVGElement): void {
    for (const e of this.layout!.elements) {
      if (e.kind !== "leftover") continue;
      el("path", { d: this.shapePath(e), class: "leftover", "fill-rule": "evenodd" }, parent);
      const pts = e.points.map((p) => this.toScreen(p));
      const xs = pts.map((p) => p.x);
      const ys = pts.map((p) => p.y);
      const [w, h] = [Math.max(...xs) - Math.min(...xs), Math.max(...ys) - Math.min(...ys)];
      const label = `${this.lengths.area(e.area ?? 0)} left over`;
      if (w < label.length * 5.4 + 8 || h < 14) continue;
      const c = e.labelAt ? this.toScreen(e.labelAt) : centroid(pts);
      const t = el("text", { x: c.x, y: c.y + 4, class: "leftover-label" }, parent);
      t.textContent = label;
    }
  }

  /** A symbol from the catalog, already fitted to its space by the engine. */
  private renderSymbol(parent: SVGElement, element: Element): void {
    const g = el("g", { class: "symbol" }, parent);
    for (const path of element.symbol?.paths ?? []) {
      const d = pathOf(
        path.points.map((p) => this.toScreen(p)),
        path.closed,
      );
      el("path", { d, class: path.tone ? `tone-${path.tone}` : "" }, g);
    }
  }

  /** The clear floor around fixtures: dashed, under the fixtures themselves. */
  private renderClearances(parent: SVGElement): void {
    for (const c of this.layout!.elements) {
      if (c.kind !== "clearance") continue;
      const d = pathOf(
        c.points.map((p) => this.toScreen(p)),
        true,
      );
      el("path", { d, class: "clearance" }, parent);
    }
  }

  /** The ports of a plan's preview slot: styled like boundary ports, labelled outside the slot. */
  private renderSlotPorts(parent: SVGElement): void {
    const slot = this.layout!.elements.find((e) => e.kind === "slot");
    const c = slot ? centroid(slot.points.map((p) => this.toScreen(p))) : null;
    for (const port of this.layout!.elements) {
      if (port.kind !== "port" || !port.roles?.length) continue;
      const [a, b] = port.points.map((p) => this.toScreen(p));
      const role = port.roles.find((r) => r !== "facade");
      if (role) {
        el("path", { d: `M${a.x},${a.y}L${b.x},${b.y}`, class: `port port-${role.split(".")[0]}` }, parent);
      }
      const len = Math.hypot(b.x - a.x, b.y - a.y);
      const label = port.roles.map((r) => `'${r}`).join(" ");
      if (len < label.length * 6.2 + 8 || !c) continue;
      // Outside the slot, between the edge and its dimension line.
      let [nx, ny] = [-(b.y - a.y) / len, (b.x - a.x) / len];
      const m = { x: (a.x + b.x) / 2, y: (a.y + b.y) / 2 };
      if ((c.x - m.x) * nx + (c.y - m.y) * ny > 0) [nx, ny] = [-nx, -ny];
      let angle = (Math.atan2(b.y - a.y, b.x - a.x) * 180) / Math.PI;
      if (angle > 90) angle -= 180;
      if (angle <= -90) angle += 180;
      const [x, y] = [m.x + nx * 15, m.y + ny * 15];
      const t = el(
        "text",
        {
          x,
          y,
          class: `port-label port-label-${(role ?? "facade").split(".")[0]}`,
          transform: `rotate(${angle} ${x} ${y})`,
          "dominant-baseline": "middle",
          "text-anchor": "middle",
        },
        parent,
      );
      t.textContent = label;
    }
  }

  /** Doors as a gap with a swing; windows as a thin double line in the wall. */
  private renderOpenings(parent: SVGElement): void {
    const byId = new Map(this.layout!.elements.map((e) => [e.id, e]));
    const g = el("g", { class: "openings" }, parent);
    for (const o of this.layout!.elements) {
      if (o.kind !== "door" && o.kind !== "window") continue;
      const [a, b] = o.points.map((p) => this.toScreen(p));
      const len = Math.hypot(b.x - a.x, b.y - a.y);
      if (len < 3) continue;
      // In a built wall the gap goes right through it.
      const depth = o.opening?.depth ? o.opening.depth * this.view.k + 1 : 0;
      el(
        "path",
        {
          d: `M${a.x},${a.y}L${b.x},${b.y}`,
          class: "opening-gap",
          ...(depth ? { style: `stroke-width:${depth}px` } : {}),
        },
        g,
      );
      if (o.kind === "window") {
        const ux = (b.x - a.x) / len;
        const uy = (b.y - a.y) / len;
        const [nx, ny] = [-uy * 2.5, ux * 2.5];
        el(
          "path",
          {
            d: `M${a.x + nx},${a.y + ny}L${b.x + nx},${b.y + ny}M${a.x - nx},${a.y - ny}L${b.x - nx},${b.y - ny}M${a.x},${a.y}L${b.x},${b.y}`,
            class: "window",
          },
          g,
        );
        continue;
      }
      // The leaf swings into the first space it connects.
      const into = o.opening?.between[0];
      const space = into ? byId.get(into) : undefined;
      const c = space ? centroid(space.points.map((p) => this.toScreen(p))) : null;
      const ux = (b.x - a.x) / len;
      const uy = (b.y - a.y) / len;
      let [nx, ny] = [-uy, ux];
      if (c && (c.x - a.x) * nx + (c.y - a.y) * ny < 0) [nx, ny] = [-nx, -ny];
      const leaf = { x: a.x + nx * len, y: a.y + ny * len };
      const sweep = ux * ny - uy * nx > 0 ? 1 : 0;
      el(
        "path",
        { d: `M${a.x},${a.y}L${leaf.x},${leaf.y}A${len},${len} 0 0 ${sweep} ${b.x},${b.y}`, class: "door" },
        g,
      );
    }
  }

  /** Name and area in each space that has room for them; typical letters on parents. */
  private renderSpaceLabels(parent: SVGElement, spaces: Element[]): void {
    const g = el("g", { class: "space-labels" }, parent);
    const L = this.lengths;
    // Spaces divided into other spaces (not just furnished): a tag in the corner instead, so the
    // labels of the spaces inside stay readable.
    const divided = new Set<string>();
    for (const s of spaces) {
      const dot = s.id.lastIndexOf(".");
      if (dot > 0 && s.space!.kind !== "fixture") divided.add(s.id.slice(0, dot));
    }
    const tags: { x: number; y: number; w: number }[] = [];
    for (const s of spaces) {
      const info = s.space!;
      const pts = s.points.map((p) => this.toScreen(p));
      const xs = pts.map((p) => p.x);
      const ys = pts.map((p) => p.y);
      const [x0, x1, y0, y1] = [Math.min(...xs), Math.max(...xs), Math.min(...ys), Math.max(...ys)];
      const [w, h] = [x1 - x0, y1 - y0];
      if (info.hasPlan && divided.has(s.id)) {
        const tag = info.typical
          ? `${info.label} · ${info.typical}`
          : info.unique
            ? `${info.label} · unique`
            : info.label;
        const tw = tag.length * 6 + 10;
        if (w < tw || h < 18) continue;
        // The top-left corner of its outline (for a fill, its bounding box's may be outside it):
        // the highest vertex, leftmost among those, always has the shape below and to its right.
        const corner = pts.reduce((a, p) =>
          p.y < a.y - 0.5 || (Math.abs(p.y - a.y) <= 0.5 && p.x < a.x) ? p : a,
        );
        const x = corner.x + 5;
        // Below any tag already in that corner (a wing's and its first suite's, say).
        let y = corner.y + 12;
        while (tags.some((t) => x < t.x + t.w && t.x < x + tw && Math.abs(t.y - y) < 12)) y += 12;
        if (y > y1 - 4) continue;
        tags.push({ x, y, w: tw });
        const t = el("text", { x, y, class: "space-tag" }, g);
        t.textContent = tag;
        continue;
      }
      const name = info.label;
      if (info.kind === "fixture") {
        // Name only, and only when it fits: a fixture's area isn't floor space. Its symbol, if
        // it has one, says what it is.
        if (info.symbol || w < name.length * 5.2 + 6 || h < 13) continue;
        const c = centroid(pts);
        const t = el("text", { x: c.x, y: c.y + 4, class: "space-name fixture-name" }, g);
        t.textContent = name;
        continue;
      }
      const area = L.area(s.area ?? 0);
      const big = w > Math.max(name.length, area.length) * 6.4 + 10 && h > 30;
      const small = w > name.length * 5.6 + 6 && h > 14;
      if (!big && !small) continue;
      // A shape that isn't a rectangle (a fill) says where its label fits.
      const c = s.labelAt ? this.toScreen(s.labelAt) : centroid(pts);
      const t = el("text", { x: c.x, y: big ? c.y - 2 : c.y + 4, class: "space-name" }, g);
      t.textContent = name;
      if (big) {
        const a = el("text", { x: c.x, y: c.y + 12, class: "space-area" }, g);
        a.textContent = area;
      }
    }
  }

  private renderDimensions(parent: SVGElement): void {
    const g = el("g", { class: "dims" }, parent);
    for (const element of this.layout!.elements.filter(framing)) {
      // While an edge is being moved its ghost carries the dimensions.
      if (this.edgeDrag?.moved && this.edgeDrag.element === element) continue;
      const flip = element.closed && signedArea(element.points) < 0;
      for (const [a, b] of edgesOf(element)) this.drawDimension(g, a, b, false, flip);
      if (element.kind === "origin") this.originLabel(g, element.points[0]);
    }
  }

  private originLabel(parent: SVGElement, p: Point): void {
    const s = this.toScreen(p);
    const t = el("text", { x: s.x + 14, y: s.y - 14, class: "dim-text origin-text" }, parent);
    t.textContent = `${this.lengths.dimension(p.x)}, ${this.lengths.dimension(p.y)}`;
  }

  /**
   * Aligned dimension beside an edge. For closed shapes it goes outside: edges of a
   * counter-clockwise polygon have the outside on their right; `flip` handles clockwise ones.
   * Open spines put it on the right of the direction of travel.
   */
  private drawDimension(
    parent: SVGElement,
    a: Point,
    b: Point,
    emphasis: boolean,
    flip = false,
    offset = DIM_OFFSET,
  ): void {
    const sa = this.toScreen(a);
    const sb = this.toScreen(b);
    const len = Math.hypot(sb.x - sa.x, sb.y - sa.y);
    if (len < 8) return;
    const ux = (sb.x - sa.x) / len;
    const uy = (sb.y - sa.y) / len;
    // A counter-clockwise polygon (world, y up) is clockwise on screen (y down), so its
    // outward normal is the edge direction rotated +90° in screen space.
    const sign = flip ? -1 : 1;
    const nx = -uy * sign;
    const ny = ux * sign;
    const g = el("g", { class: emphasis ? "dim dim-hot" : "dim" }, parent);
    const p1 = { x: sa.x + nx * offset, y: sa.y + ny * offset };
    const p2 = { x: sb.x + nx * offset, y: sb.y + ny * offset };
    const gap = 5;
    const over = 6;
    el(
      "path",
      {
        d:
          `M${sa.x + nx * gap},${sa.y + ny * gap}L${sa.x + nx * (offset + over)},${sa.y + ny * (offset + over)}` +
          `M${sb.x + nx * gap},${sb.y + ny * gap}L${sb.x + nx * (offset + over)},${sb.y + ny * (offset + over)}`,
        class: "dim-ext",
      },
      g,
    );
    el(
      "path",
      {
        d: `M${p1.x - ux * over},${p1.y - uy * over}L${p2.x + ux * over},${p2.y + uy * over}`,
        class: "dim-line",
      },
      g,
    );
    const t = 5 * 0.7071;
    const tick = (p: { x: number; y: number }) =>
      `M${p.x - (ux + nx) * t},${p.y - (uy + ny) * t}L${p.x + (ux + nx) * t},${p.y + (uy + ny) * t}`;
    el("path", { d: tick(p1) + tick(p2), class: "dim-tick" }, g);

    const label = this.lengths.dimension(Math.hypot(b.x - a.x, b.y - a.y));
    if (len < label.length * 7 + 12) return;
    let angle = (Math.atan2(uy, ux) * 180) / Math.PI;
    if (angle > 90) angle -= 180;
    if (angle <= -90) angle += 180;
    const mx = (p1.x + p2.x) / 2 + nx * 8;
    const my = (p1.y + p2.y) / 2 + ny * 8;
    const text = el(
      "text",
      {
        x: mx,
        y: my,
        class: "dim-text",
        transform: `rotate(${angle} ${mx} ${my})`,
        "dominant-baseline": "middle",
        "text-anchor": "middle",
      },
      g,
    );
    text.textContent = label;
  }

  private renderHover(parent: SVGElement): void {
    const h = this.vertexDrag
      ? {
          type: "vertex" as const,
          element: this.vertexDrag.element,
          index: this.vertexDrag.index,
          p: this.vertexDrag.element.points[this.vertexDrag.index],
        }
      : this.hover;
    if (!h || this.pan?.moved || this.edgeDrag?.moved) return;
    const g = el("g", { class: "hover" }, parent);
    if (h.type === "wall" || h.type === "part") {
      const d = pathOf(
        h.type === "part" ? this.partShape(h.element) : h.element.points.map((p) => this.toScreen(p)),
        true,
      );
      el("path", { d, class: h.type === "wall" ? "hover-wall" : "hover-part" }, g);
      if (h.type === "wall" && h.element.wall) {
        const [a, b] = h.element.wall.line;
        this.drawDimension(g, a, b, true, false, 14);
      }
      return;
    }
    if (h.type === "space") {
      const info = h.element.space!;
      // The same typical elsewhere, lightly, then this one.
      if (info.typical) {
        for (const other of this.layout!.elements) {
          if (
            other !== h.element &&
            other.space?.typical === info.typical &&
            other.space.type === info.type
          ) {
            const d = pathOf(
              other.points.map((p) => this.toScreen(p)),
              true,
            );
            el("path", { d, class: "hover-typical" }, g);
          }
        }
      }
      const d = pathOf(
        h.element.points.map((p) => this.toScreen(p)),
        true,
      );
      el("path", { d, class: "hover-space" }, g);
      const flip = signedArea(h.element.points) < 0;
      for (const [a, b] of edgesOf(h.element)) this.drawDimension(g, a, b, true, flip, 14);
      return;
    }
    if (h.type === "face") {
      el(
        "path",
        {
          d: pathOf(
            h.element.points.map((p) => this.toScreen(p)),
            true,
          ),
          class: "hover-face",
        },
        g,
      );
    } else if (h.type === "edge") {
      const a = this.toScreen(h.a);
      const b = this.toScreen(h.b);
      el("path", { d: `M${a.x},${a.y}L${b.x},${b.y}`, class: "hover-edge" }, g);
      for (const p of [a, b])
        el("rect", { x: p.x - 3.5, y: p.y - 3.5, width: 7, height: 7, class: "grip" }, g);
      const flip = h.element.closed && signedArea(h.element.points) < 0;
      this.drawDimension(g, h.a, h.b, true, flip);
    } else {
      // A point being hovered or dragged: grip, plus live dimensions of its edges.
      const pts = h.element.points;
      const flip = h.element.closed && signedArea(pts) < 0;
      const n = pts.length;
      if (n > 1) {
        const prev = h.index > 0 || h.element.closed ? (h.index - 1 + n) % n : -1;
        const next = h.index < n - 1 || h.element.closed ? (h.index + 1) % n : -1;
        if (prev >= 0) this.drawDimension(g, pts[prev], pts[h.index], true, flip);
        if (next >= 0) this.drawDimension(g, pts[h.index], pts[next], true, flip);
      }
      const p = this.toScreen(h.p);
      el("rect", { x: p.x - 5, y: p.y - 5, width: 10, height: 10, class: "grip grip-hot" }, g);
    }
  }

  /**
   * An edge being moved: the outline as it would be, dashed, with the moved edge in redline, live
   * dimensions, and how far it went. The real geometry stays as it is until the button is
   * released.
   */
  private renderEdgeGhost(parent: SVGElement): void {
    const d = this.edgeDrag!;
    const m = d.move;
    if (!d.moved || !m) return;
    const pts = d.original;
    const n = pts.length;
    const closed = d.element.closed;
    const j = (d.index + 1) % n;
    const g = el("g", { class: m.problem ? "ghost is-invalid" : "ghost" }, parent);

    // Where it was.
    const [oa, ob] = [this.toScreen(pts[d.index]), this.toScreen(pts[j])];
    el("path", { d: `M${oa.x},${oa.y}L${ob.x},${ob.y}`, class: "ghost-from" }, g);

    // Where it goes: the whole outline, then the edge itself.
    const moved = movedPoints(pts, d.index, m);
    const screen = moved.map((p) => this.toScreen(p));
    if (closed) el("path", { d: pathOf(screen, true), class: "ghost-face" }, g);
    el("path", { d: pathOf(screen, closed), class: "ghost-outline" }, g);
    const [na, nb] = [this.toScreen(m.a), this.toScreen(m.b)];
    el("path", { d: `M${na.x},${na.y}L${nb.x},${nb.y}`, class: "ghost-edge" }, g);

    // Dimensions of the outline as it would be; the edges that change are emphasised.
    const { prev, next } = neighbours(pts, closed, d.index);
    const changed = new Set([d.index, prev, next === null ? null : j].filter((i) => i !== null));
    const flip = closed && signedArea(moved) < 0;
    edgesOf({ ...d.element, points: moved }).forEach(([p, q], i) => {
      if (changed.has(i) || this.showDimensions) this.drawDimension(g, p, q, changed.has(i), flip);
    });

    // How far: from the middle of the old edge to the new one.
    const [f, t] = [this.toScreen(m.from), this.toScreen(m.to)];
    const len = Math.hypot(t.x - f.x, t.y - f.y);
    if (len > 6) {
      const [ux, uy] = [(t.x - f.x) / len, (t.y - f.y) / len];
      el("path", { d: `M${f.x},${f.y}L${t.x},${t.y}`, class: "ghost-offset" }, g);
      el(
        "path",
        {
          d: `M${t.x},${t.y}L${t.x - ux * 8 - uy * 4},${t.y - uy * 8 + ux * 4}L${t.x - ux * 8 + uy * 4},${t.y - uy * 8 - ux * 4}Z`,
          class: "ghost-arrow",
        },
        g,
      );
      el("circle", { cx: f.x, cy: f.y, r: 2.5, class: "ghost-arrow" }, g);
      const label = this.lengths.length(Math.hypot(m.to.x - m.from.x, m.to.y - m.from.y), true);
      const w = label.length * 7.4 + 14;
      // Beside the arrow, not on it.
      const [mx, my] = [(f.x + t.x) / 2 - uy * (w / 2 + 6), (f.y + t.y) / 2 + ux * 14];
      el("rect", { x: mx - w / 2, y: my - 10, width: w, height: 20, rx: 3, class: "ghost-tag" }, g);
      const text = el(
        "text",
        { x: mx, y: my, class: "ghost-text", "text-anchor": "middle", "dominant-baseline": "middle" },
        g,
      );
      text.textContent = label;
    }
    for (const p of [na, nb])
      el("rect", { x: p.x - 4, y: p.y - 4, width: 8, height: 8, class: "ghost-grip" }, g);
    if (d.snap) {
      const s = this.toScreen(d.snap);
      el("rect", { x: s.x - 6, y: s.y - 6, width: 12, height: 12, class: "snap" }, g);
    }
  }

  private renderDrawing(parent: SVGElement): void {
    const d = this.drawing!;
    const g = el("g", { class: "drawing" }, parent);
    const pts = d.points.map((p) => this.toScreen(p));
    const last = d.points[d.points.length - 1] ?? null;
    const sc = this.screenCursor;
    const next = sc && sc.x > RULER && sc.y > RULER ? this.snapWorld(sc.x, sc.y, last) : null;

    if (pts.length > 1) el("path", { d: pathOf(pts, false), class: "draw-line" }, g);
    for (let i = 1; i < d.points.length; i++) this.drawDimension(g, d.points[i - 1], d.points[i], false);

    if (next && d.request.kind !== "point") {
      const sn = this.toScreen(next);
      if (last) {
        const sl = this.toScreen(last);
        el("path", { d: `M${sl.x},${sl.y}L${sn.x},${sn.y}`, class: "draw-rubber" }, g);
        if (Math.hypot(next.x - last.x, next.y - last.y) > 0) this.drawDimension(g, last, next, true);
      }
      // Preview of the closing edge.
      if (d.request.kind === "boundary" && d.points.length >= 2) {
        const s0 = this.toScreen(d.points[0]);
        el("path", { d: `M${sn.x},${sn.y}L${s0.x},${s0.y}`, class: "draw-closing" }, g);
      }
    }
    pts.forEach((p, i) => {
      const closable = i === 0 && d.request.kind === "boundary" && d.points.length >= 3;
      el(
        "rect",
        {
          x: p.x - 4,
          y: p.y - 4,
          width: 8,
          height: 8,
          class: closable ? "draw-node draw-node-first" : "draw-node",
        },
        g,
      );
    });
    if (next) {
      const s = this.toScreen(next);
      if (d.request.kind === "point") {
        el("circle", { cx: s.x, cy: s.y, r: 7, class: "origin-ring draw-ghost" }, g);
        el(
          "path",
          {
            d: `M${s.x - 16},${s.y}H${s.x + 16}M${s.x},${s.y - 16}V${s.y + 16}`,
            class: "origin-cross draw-ghost",
          },
          g,
        );
      } else {
        el(
          "path",
          { d: `M${s.x - 7},${s.y}H${s.x + 7}M${s.x},${s.y - 7}V${s.y + 7}`, class: "draw-cursor" },
          g,
        );
      }
    }
  }

  private renderMeasure(parent: SVGElement): void {
    const g = el("g", { class: "measure" }, parent);
    if (this.snapPoint) {
      const s = this.toScreen(this.snapPoint);
      el("rect", { x: s.x - 5, y: s.y - 5, width: 10, height: 10, class: "snap" }, g);
    }
    const a = this.measure.a;
    const b = this.measure.b ?? this.snapPoint ?? this.cursor;
    if (!a || !b) return;
    const sa = this.toScreen(a);
    const sb = this.toScreen(b);
    el("path", { d: `M${sa.x},${sa.y}L${sb.x},${sb.y}`, class: "measure-line" }, g);
    for (const p of [sa, sb]) el("circle", { cx: p.x, cy: p.y, r: 3.5, class: "measure-pt" }, g);
    const len = Math.hypot(b.x - a.x, b.y - a.y);
    const mx = (sa.x + sb.x) / 2;
    const my = (sa.y + sb.y) / 2;
    const label = this.lengths.length(len, true);
    const w = label.length * 7.4 + 14;
    el("rect", { x: mx - w / 2, y: my - 22, width: w, height: 20, rx: 3, class: "measure-tag" }, g);
    const t = el(
      "text",
      { x: mx, y: my - 12, class: "measure-text", "text-anchor": "middle", "dominant-baseline": "middle" },
      g,
    );
    t.textContent = label;
  }

  private renderRulers(parent: SVGElement): void {
    const g = el("g", { class: "rulers" }, parent);
    el("rect", { x: 0, y: 0, width: this.width, height: RULER, class: "ruler-bg" }, g);
    el("rect", { x: 0, y: 0, width: RULER, height: this.height, class: "ruler-bg" }, g);
    el(
      "path",
      {
        d: `M${RULER - 0.5},${RULER}V${this.height}M${RULER},${RULER - 0.5}H${this.width}`,
        class: "ruler-edge",
      },
      g,
    );

    const { minor, major } = this.gridSteps();
    const tl = this.toWorld(RULER, RULER);
    const br = this.toWorld(this.width, this.height);
    const ticks: string[] = [];
    const isMajor = (v: number) => Math.abs(v / major - Math.round(v / major)) < 1e-6;
    const labels = el("g", { class: "ruler-labels" }, g);

    for (let x = Math.floor(tl.x / minor) * minor; x <= br.x; x += minor) {
      const sx = Math.round(this.toScreen({ x, y: 0 }).x) + 0.5;
      if (sx < RULER) continue;
      const big = isMajor(x);
      ticks.push(`M${sx},${RULER}V${RULER - (big ? 9 : 4)}`);
      if (big) el("text", { x: sx + 3, y: 11 }, labels).textContent = this.lengths.tick(x);
    }
    for (let y = Math.floor(br.y / minor) * minor; y <= tl.y; y += minor) {
      const sy = Math.round(this.toScreen({ x: 0, y }).y) + 0.5;
      if (sy < RULER) continue;
      const big = isMajor(y);
      ticks.push(`M${RULER},${sy}H${RULER - (big ? 9 : 4)}`);
      if (big)
        el("text", { x: 11, y: sy - 3, transform: `rotate(-90 11 ${sy - 3})` }, labels).textContent =
          this.lengths.tick(y);
    }
    el("path", { d: ticks.join(""), class: "ruler-tick" }, g);

    if (this.screenCursor && this.cursor) {
      const { x, y } = this.screenCursor;
      el("path", { d: `M${x},${RULER}l-4,-6h8zM${RULER},${y}l-6,-4v8z`, class: "ruler-cursor" }, g);
    }
    el("rect", { x: 0, y: 0, width: RULER, height: RULER, class: "ruler-corner" }, g);
    const u = el(
      "text",
      {
        x: RULER / 2,
        y: RULER / 2 + 0.5,
        class: "ruler-unit",
        "text-anchor": "middle",
        "dominant-baseline": "middle",
      },
      g,
    );
    u.textContent = this.lengths.rulerUnit;
  }

  /** The tooltip while an edge is moved: how far, which way, and what it does to the outline. */
  private edgeGhostCard(d: EdgeDrag, m: EdgeMove): string {
    const L = this.lengths;
    const len = (v: number) => L.length(v, true);
    const xy = (p: Point) => `${L.length(p.x)}, ${L.length(p.y)}`;
    const pts = d.original;
    const closed = d.element.closed;
    const n = closed ? pts.length : pts.length - 1;
    const what = d.element.kind === "spine" ? "spine segment" : "edge";
    const j = (d.index + 1) % pts.length;
    const rows: [string, string][] = [];
    if (m.free) {
      rows.push(["Δx", len(m.to.x - m.from.x)], ["Δy", len(m.to.y - m.from.y)]);
    } else if (closed && Math.abs(m.offset) > 0) {
      // The left normal points inward on a counter-clockwise outline.
      const outward = m.offset * (signedArea(pts) > 0 ? -1 : 1) > 0;
      rows.push(["Direction", outward ? "outward" : "inward"]);
    }
    const before = Math.hypot(pts[j].x - pts[d.index].x, pts[j].y - pts[d.index].y);
    const after = Math.hypot(m.b.x - m.a.x, m.b.y - m.a.y);
    rows.push(["Length", Math.abs(after - before) < 1e-9 ? len(after) : `${len(before)} → ${len(after)}`]);
    rows.push(["From", xy(m.a)], ["To", xy(m.b)]);
    if (closed) {
      const a0 = Math.abs(signedArea(pts));
      const a1 = Math.abs(signedArea(movedPoints(pts, d.index, m)));
      const diff = a1 - a0;
      const sign = diff > 0 ? "+" : diff < 0 ? "−" : "±";
      rows.push(["Area", `${L.area(a1)} (${sign}${L.area(Math.abs(diff))})`]);
    }
    const hint =
      m.problem ??
      (m.free
        ? "Release Shift to keep it parallel · Alt: no snap · Esc: cancel"
        : "Shift: move freely · Alt: no snap · Esc: cancel");
    return card(
      `Moving ${what} ${d.index + 1} of ${n}`,
      len(Math.hypot(m.to.x - m.from.x, m.to.y - m.from.y)),
      rows,
      hint,
    );
  }

  private renderTooltip(): void {
    const tip = this.tooltip;
    const pos = this.screenCursor;
    const L = this.lengths;
    const len = (v: number) => L.length(v, true);
    const xy = (p: Point) => `${L.length(p.x)}, ${L.length(p.y)}`;
    const h = this.vertexDrag
      ? {
          type: "vertex" as const,
          element: this.vertexDrag.element,
          index: this.vertexDrag.index,
          p: this.vertexDrag.element.points[this.vertexDrag.index],
        }
      : this.hover;

    let html = "";
    if (this.edgeDrag?.moved && this.edgeDrag.move && pos) {
      html = this.edgeGhostCard(this.edgeDrag, this.edgeDrag.move);
    } else if (this.drawing && pos) {
      const d = this.drawing;
      const last = d.points[d.points.length - 1];
      const next = this.snapWorld(pos.x, pos.y, last ?? null);
      if (last && d.request.kind !== "point") {
        html = card(`Segment ${d.points.length}`, len(Math.hypot(next.x - last.x, next.y - last.y)), [
          ["Angle", degrees(Math.atan2(next.y - last.y, next.x - last.x))],
          ["To", xy(next)],
        ]);
      } else {
        html = card(d.request.kind === "point" ? "Origin" : "First point", xy(next), []);
      }
    } else if (this.tool === "measure" && this.measure.a && pos) {
      const b = this.measure.b ?? this.snapPoint ?? this.cursor;
      if (b) {
        const a = this.measure.a;
        html = card("Measure", len(Math.hypot(b.x - a.x, b.y - a.y)), [
          ["Δx", len(b.x - a.x)],
          ["Δy", len(b.y - a.y)],
          ["Angle", degrees(Math.atan2(b.y - a.y, b.x - a.x))],
        ]);
      }
    } else if (h?.type === "edge" && h.element.kind === "slot") {
      const side = ["Bottom", "Right", "Top", "Left"][h.index] ?? "Side";
      const roles = this.layout!.elements.filter(
        (e) => e.kind === "port" && onSegment(e.points[0], h.a, h.b) && onSegment(e.points[1], h.a, h.b),
      );
      html = card(
        `${side} of the slot`,
        len(Math.hypot(h.b.x - h.a.x, h.b.y - h.a.y)),
        [["Port", roles.map((e) => (e.roles ?? []).map((r) => `'${r}`).join(", ")).join(" · ") || "none"]],
        "Drag a corner to resize the preview",
      );
    } else if (h?.type === "edge") {
      const n = edgesOf(h.element).length;
      const what = h.element.kind === "spine" ? "Spine segment" : "Edge";
      const rows: [string, string][] = [
        ["Bearing", degrees(Math.atan2(h.b.y - h.a.y, h.b.x - h.a.x))],
        ["From", xy(h.a)],
        ["To", xy(h.b)],
      ];
      const boundary = h.element.kind === "boundary";
      if (boundary)
        rows.unshift(["Port", (this.edgeRoles[h.index] ?? ["facade"]).map((r) => `'${r}`).join(", ")]);
      html = card(
        `${what} ${h.index + 1} of ${n}`,
        len(Math.hypot(h.b.x - h.a.x, h.b.y - h.a.y)),
        rows,
        boundary
          ? "Drag to move, double-click to add a point, right-click to set its port"
          : "Drag to move, double-click to add a point",
      );
    } else if (h?.type === "wall") {
      const w = h.element.wall!;
      const thin = (v: number) => L.thickness(v);
      const rows: [string, string][] = [];
      if (w.cavity > 0) rows.push(["Cavity", thin(w.cavity)]);
      if (w.fire !== undefined) rows.push(["Fire", `${w.fire} h`]);
      if (w.acoustic !== undefined) rows.push(["Sound", `STC ${w.acoustic}`]);
      if (w.bearing) rows.push(["Bearing", "yes"]);
      if (w.layers?.length)
        rows.push(["Build-up", w.layers.map((l) => `${thin(l.thickness)} ${l.material}`).join(" · ")]);
      rows.push(["Between", w.between.join(" and ")]);
      rows.push(["Length", len(Math.hypot(w.line[1].x - w.line[0].x, w.line[1].y - w.line[0].y))]);
      rows.push([
        "Because",
        w.because ? `${w.roles.map((r) => `'${r}`).join(", ")}: ${w.because}` : "asked for",
      ]);
      html = card(
        `${w.label} (${w.assembly})`,
        `${thin(w.thickness)} thick`,
        rows,
        "Right-click: show its assembly",
      );
    } else if (h?.type === "part") {
      const p = h.element.part!;
      const rows: [string, string][] = [["In", p.space]];
      if (p.mount === "wall" && p.above !== undefined)
        rows.push(["Height", `${L.thickness(p.above)} above the floor`]);
      else rows.push(["On", p.mount === "cavity" ? "in the wall" : `the ${p.mount}`]);
      if (p.group) {
        const nets = this.layout!.elements.filter(
          (e) => e.kind === "run" && e.run?.service === p.service && e.run.group === p.group,
        );
        rows.push([nets[0]?.run?.network ?? "Group", p.group]);
      }
      rows.push(["Placed", p.placed]);
      html = card(p.label, p.service, rows, "Right-click: show where it's placed");
    } else if (h?.type === "space") {
      const info = h.element.space!;
      const b = boundsOf(h.element.points);
      const rows: [string, string][] = [["Size", `${L.length(b.w)} × ${L.length(b.h)}`]];
      if (info.gross !== undefined)
        rows.push([
          "Floor",
          `${L.area(h.element.area ?? 0)} clear, ${L.area(info.gross)} to the walls' centres`,
        ]);
      if (info.tags?.length) rows.push(["Tags", info.tags.map((t) => `#${t}`).join(" ")]);
      if (info.front) {
        // Width along its front, depth front to back.
        const [a, f] = info.front;
        const width = Math.hypot(f.x - a.x, f.y - a.y);
        const depth = width > 0 ? (h.element.area ?? 0) / width : 0;
        rows[0] = ["Size", `${L.length(width)} wide × ${L.length(depth)} deep`];
      }
      if (h.element.id !== info.name) {
        // Inside another space: say which, and its typical.
        const parentId = h.element.id.slice(0, h.element.id.lastIndexOf("."));
        const parent = this.layout!.elements.find((e) => e.id === parentId)?.space;
        const tag = parent?.typical ? ` (type ${parent.typical})` : parent?.unique ? " (unique)" : "";
        rows.push(["In", `${parentId}${tag}`]);
      }
      if (info.kind !== "room") rows.push(["Kind", info.kind]);
      if (info.symbol) rows.push(["Symbol", `'${info.symbol}`]);
      if (info.typical) {
        const n = this.layout!.elements.filter(
          (e) => e.space?.typical === info.typical && e.space?.type === info.type,
        ).length;
        rows.push(["Typical", `Type ${info.typical} (${n} alike)`]);
      } else if (info.unique) {
        rows.push(["Typical", "unique: solved on its own"]);
      }
      if (info.hasPlan) {
        const inside = this.layout!.elements.filter(
          (e) => e.kind === "space" && e.id.startsWith(`${h.element.id}.`),
        ).length;
        rows.push(["Inside", info.ok ? `${inside} spaces` : "its plan didn't fit, see problems"]);
      }
      html = card(
        info.label,
        L.area(h.element.area ?? 0),
        rows,
        info.hasPlan ? "Right-click for options" : undefined,
      );
    } else if (h?.type === "vertex" && h.element.kind === "slot") {
      const b = boundsOf(h.element.points);
      html = card(
        "Slot corner",
        `${L.length(b.w)} × ${L.length(b.h)}`,
        [],
        h.index === 0
          ? "The slot's corner at the origin stays put"
          : this.vertexDrag
            ? "Alt: no grid snap, Esc: cancel"
            : "Drag to resize the preview",
      );
    } else if (h?.type === "vertex") {
      const pts = h.element.points;
      const title = h.element.kind === "origin" ? "Origin" : `Point ${h.index + 1} of ${pts.length}`;
      const rows: [string, string][] = [];
      if (h.element.closed && pts.length > 2) {
        const prev = pts[(h.index - 1 + pts.length) % pts.length];
        const next = pts[(h.index + 1) % pts.length];
        let angle = interiorAngle(prev, h.p, next);
        if (signedArea(pts) < 0) angle = Math.PI * 2 - angle;
        rows.push(["Interior angle", degrees(angle)]);
      }
      const removable = h.element.kind !== "origin" && pts.length > (h.element.closed ? 3 : 2);
      html = card(
        title,
        xy(h.p),
        rows,
        this.vertexDrag
          ? "Alt: no grid snap, Esc: cancel"
          : removable
            ? "Drag to move, right-click to remove"
            : "Drag to move",
      );
    } else if (h?.type === "face" && h.element.kind === "slot") {
      const b = boundsOf(h.element.points);
      html = card("Preview slot", L.area(h.element.area ?? 0), [
        ["Size", `${L.length(b.w)} × ${L.length(b.h)}`],
      ]);
    } else if (h?.type === "face") {
      const b = boundsOf(h.element.points);
      html = card("Boundary", L.area(h.element.area ?? 0), [
        ["Perimeter", L.length(h.element.length)],
        ["Extents", `${L.length(b.w)} × ${L.length(b.h)}`],
        ["Corners", String(h.element.points.length)],
      ]);
    }

    if (!html || !pos || this.pan?.moved) {
      tip.hidden = true;
      return;
    }
    tip.classList.toggle("tip-invalid", !!this.edgeDrag?.move?.problem);
    tip.innerHTML = html;
    tip.hidden = false;
    const tw = tip.offsetWidth;
    const th = tip.offsetHeight;
    let x = pos.x + 16;
    let y = pos.y + 18;
    if (x + tw > this.width - 8) x = pos.x - tw - 16;
    if (y + th > this.height - 8) y = pos.y - th - 14;
    tip.style.transform = `translate(${Math.round(x)}px, ${Math.round(y)}px)`;
  }
}

// ---------------------------------------------------------------- helpers

/** Edges that can be dragged: those of the boundary or spine (a preview slot resizes by corners). */
function edgeMovable(e: Element): boolean {
  return editable(e) && e.kind !== "slot" && e.points.length >= 2;
}

/** Which way a parallel move goes, for the cursor: across a horizontal or vertical edge. */
function axisOf(a: Point, b: Point): string {
  const [dx, dy] = [Math.abs(b.x - a.x), Math.abs(b.y - a.y)];
  if (dy <= dx * 1e-6) return "ns";
  if (dx <= dy * 1e-6) return "ew";
  return "diagonal";
}

/** Round away floating-point noise (e.g. 0.30000000000000004). */
function tidy(v: number): number {
  return Math.abs(v) < 1e-12 ? 0 : Number(v.toPrecision(12));
}

function edgesOf(element: Element): [Point, Point][] {
  const pts = element.points;
  const n = pts.length;
  if (n < 2) return [];
  const count = element.closed ? n : n - 1;
  return Array.from({ length: count }, (_, i) => [pts[i], pts[(i + 1) % n]] as [Point, Point]);
}

function card(title: string, value: string, rows: [string, string][], hint?: string): string {
  const esc = (s: string) => s.replace(/[&<>"]/g, (c) => `&#${c.charCodeAt(0)};`);
  return (
    `<div class="tip-title">${esc(title)}</div>` +
    `<div class="tip-value">${esc(value)}</div>` +
    (rows.length ? `<dl>${rows.map(([k, v]) => `<dt>${esc(k)}</dt><dd>${esc(v)}</dd>`).join("")}</dl>` : "") +
    (hint ? `<div class="tip-hint">${esc(hint)}</div>` : "")
  );
}

/** Is `p` on segment a–b (within a hair)? */
function onSegment(p: Point, a: Point, b: Point): boolean {
  const len = Math.hypot(b.x - a.x, b.y - a.y);
  const tol = 1e-6 * (1 + len);
  const cross = Math.abs((b.x - a.x) * (p.y - a.y) - (b.y - a.y) * (p.x - a.x)) / (len || 1);
  const dot = (p.x - a.x) * (b.x - a.x) + (p.y - a.y) * (b.y - a.y);
  return cross < tol && dot >= -tol * len && dot <= len * len + tol * len;
}

function pathOf(points: { x: number; y: number }[], closed: boolean): string {
  return points.map((p, i) => `${i ? "L" : "M"}${p.x},${p.y}`).join("") + (closed ? "Z" : "");
}

function segmentDistance(
  p: { x: number; y: number },
  a: { x: number; y: number },
  b: { x: number; y: number },
): number {
  const q = projectOnSegment(p, a, b);
  return Math.hypot(p.x - q.x, p.y - q.y);
}

function projectOnSegment(p: Point, a: Point, b: Point): Point {
  const dx = b.x - a.x;
  const dy = b.y - a.y;
  const len2 = dx * dx + dy * dy;
  const t = len2 === 0 ? 0 : Math.max(0, Math.min(1, ((p.x - a.x) * dx + (p.y - a.y) * dy) / len2));
  return { x: a.x + t * dx, y: a.y + t * dy };
}

function pointInPolygon(p: Point, pts: Point[]): boolean {
  let inside = false;
  for (let i = 0, j = pts.length - 1; i < pts.length; j = i++) {
    const a = pts[i];
    const b = pts[j];
    if (a.y > p.y !== b.y > p.y && p.x < ((b.x - a.x) * (p.y - a.y)) / (b.y - a.y) + a.x) inside = !inside;
  }
  return inside;
}

function signedArea(pts: Point[]): number {
  let s = 0;
  for (let i = 0; i < pts.length; i++) {
    const a = pts[i];
    const b = pts[(i + 1) % pts.length];
    s += a.x * b.y - b.x * a.y;
  }
  return s / 2;
}

/** Interior angle at `p` for a counter-clockwise polygon. */
function interiorAngle(prev: Point, p: Point, next: Point): number {
  const a1 = Math.atan2(prev.y - p.y, prev.x - p.x);
  const a2 = Math.atan2(next.y - p.y, next.x - p.x);
  let a = a1 - a2;
  while (a < 0) a += Math.PI * 2;
  return a;
}

function centroid(pts: { x: number; y: number }[]): { x: number; y: number } {
  const xs = pts.map((p) => p.x);
  const ys = pts.map((p) => p.y);
  return { x: (Math.min(...xs) + Math.max(...xs)) / 2, y: (Math.min(...ys) + Math.max(...ys)) / 2 };
}

function boundsOf(pts: Point[]): { w: number; h: number } {
  const xs = pts.map((p) => p.x);
  const ys = pts.map((p) => p.y);
  return { w: Math.max(...xs) - Math.min(...xs), h: Math.max(...ys) - Math.min(...ys) };
}
