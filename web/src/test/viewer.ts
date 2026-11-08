// A viewer mounted on a fake DOM (happy-dom), for tests: the drawing a layout makes, without a
// browser, the wasm engine or the solver. Use from a test file that starts with
// `// @vitest-environment happy-dom`, with a layout from `web/src/fixtures/` (`npm run fixtures`).
import type { Layout } from "../types";
import { Viewer } from "../viewer/viewer";

export interface Mounted {
  viewer: Viewer;
  svg: SVGSVGElement;
  /** Elements of the drawing matching a CSS selector (`path.space`, `.part`). */
  all: (selector: string) => SVGElement[];
  /** Draw again after changing something (`viewer.setServiceView(…)`). */
  render: () => void;
}

/** Mount a viewer `width` × `height` pixels, show `layout` and draw it once. */
export function mount(layout: Layout, width = 800, height = 600): Mounted {
  const root = document.createElement("div");
  root.getBoundingClientRect = () =>
    ({
      x: 0,
      y: 0,
      left: 0,
      top: 0,
      right: width,
      bottom: height,
      width,
      height,
      toJSON: () => ({}),
    }) as DOMRect;
  document.body.replaceChildren(root);
  const viewer = new Viewer(root);
  const svg = viewer.element;
  svg.getBoundingClientRect = root.getBoundingClientRect;
  viewer.setLayout(layout);
  viewer.fit(false);
  viewer.renderNow();
  return {
    viewer,
    svg,
    all: (selector) => Array.from(svg.querySelectorAll<SVGElement>(selector)),
    render: () => viewer.renderNow(),
  };
}

/** Every coordinate in an SVG path's `d` (pairs of numbers after M, L, H, V…, roughly). */
export function pathPoints(d: string): { x: number; y: number }[] {
  const out: { x: number; y: number }[] = [];
  for (const m of d.matchAll(/[ML](-?[\d.]+(?:e-?\d+)?),(-?[\d.]+(?:e-?\d+)?)/g)) {
    out.push({ x: Number(m[1]), y: Number(m[2]) });
  }
  return out;
}
