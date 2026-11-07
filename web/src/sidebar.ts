/**
 * The side bar left of the editors, like VS Code's: an activity bar of icons, and one view at a
 * time (the files, or the reference). Clicking the open view's icon hides the side bar; each view
 * keeps its own width.
 */

export type SideView = "files" | "reference";

export interface SideState {
  open: boolean;
  view: SideView;
  widths: Record<SideView, number>;
}

const MIN = 150;
const MAX = 620;

export class SideBar {
  private state: SideState = { open: true, view: "files", widths: { files: 230, reference: 290 } };
  private readonly listeners = new Set<(state: SideState) => void>();

  constructor(
    private readonly els: {
      activity: HTMLElement;
      sidebar: HTMLElement;
      sash: HTMLElement;
      views: Record<SideView, HTMLElement>;
    },
  ) {
    for (const b of els.activity.querySelectorAll<HTMLButtonElement>("[data-view]")) {
      b.addEventListener("click", () => this.toggle(b.dataset.view as SideView));
    }
    this.resizable();
    this.apply();
  }

  get open(): boolean {
    return this.state.open;
  }

  get view(): SideView {
    return this.state.view;
  }

  /** Is this view on screen? */
  showing(view: SideView): boolean {
    return this.state.open && this.state.view === view;
  }

  onChange(listener: (state: SideState) => void): void {
    this.listeners.add(listener);
  }

  show(view: SideView): void {
    this.set({ open: true, view });
  }

  /** The activity-bar behaviour: show a view, or hide the side bar if it's already showing. */
  toggle(view: SideView = this.state.view): void {
    if (this.showing(view)) this.set({ open: false });
    else this.show(view);
  }

  setOpen(open: boolean): void {
    this.set({ open });
  }

  snapshot(): SideState {
    return { ...this.state, widths: { ...this.state.widths } };
  }

  restore(saved: Partial<SideState>): void {
    const widths = { ...this.state.widths };
    for (const v of ["files", "reference"] as SideView[]) {
      const w = saved.widths?.[v];
      if (typeof w === "number" && w >= MIN && w <= MAX) widths[v] = w;
    }
    this.set({
      open: saved.open ?? this.state.open,
      view: saved.view === "reference" || saved.view === "files" ? saved.view : this.state.view,
      widths,
    });
  }

  private set(change: Partial<SideState>): void {
    this.state = { ...this.state, ...change };
    this.apply();
    for (const l of this.listeners) l(this.snapshot());
  }

  private apply(): void {
    const { open, view, widths } = this.state;
    this.els.sidebar.hidden = !open;
    this.els.sidebar.style.width = `${widths[view]}px`;
    for (const [name, el] of Object.entries(this.els.views)) el.hidden = name !== view;
    for (const b of this.els.activity.querySelectorAll<HTMLButtonElement>("[data-view]")) {
      b.setAttribute("aria-pressed", String(open && b.dataset.view === view));
    }
  }

  private resizable(): void {
    const sash = this.els.sash;
    const setWidth = (w: number, notify: boolean) => {
      const width = Math.round(Math.min(MAX, Math.max(MIN, w)));
      this.state.widths[this.state.view] = width;
      this.els.sidebar.style.width = `${width}px`;
      if (notify) for (const l of this.listeners) l(this.snapshot());
    };
    sash.addEventListener("pointerdown", (e) => {
      sash.setPointerCapture(e.pointerId);
      document.body.classList.add("is-resizing-side");
      const left = this.els.sidebar.getBoundingClientRect().left;
      const move = (ev: PointerEvent) => setWidth(ev.clientX - left, false);
      const up = () => {
        document.body.classList.remove("is-resizing-side");
        sash.removeEventListener("pointermove", move);
        sash.removeEventListener("pointerup", up);
        setWidth(this.els.sidebar.getBoundingClientRect().width, true);
      };
      sash.addEventListener("pointermove", move);
      sash.addEventListener("pointerup", up);
    });
    sash.addEventListener("keydown", (e) => {
      const w = this.state.widths[this.state.view];
      if (e.key === "ArrowLeft") setWidth(w - 16, true);
      if (e.key === "ArrowRight") setWidth(w + 16, true);
    });
  }
}
