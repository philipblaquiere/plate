// @vitest-environment happy-dom
// Regions and edges on a pinned open flat (fixtures/pinned-open.ncl): living and dining are
// zones with no wall between them, the hall is open to both, the nook has an opening.
import { describe, expect, it } from "vitest";
import open from "../fixtures/pinned-open.json";
import { mount } from "../test/viewer";
import type { Element, Layout, Point } from "../types";

const layout: Layout = open as unknown as Layout;

/** The viewer's own hit-testing and projection, for checking what the cursor finds. */
interface Inside {
  toScreen(p: Point): Point;
  hitTest(x: number, y: number): { type: string; element: Element } | null;
  hover: { type: string; element: Element } | null;
}

const between = (a: string, b: string) =>
  layout.elements.find((e) => e.edge && e.edge.between.includes(a) && e.edge.between.includes(b))!;

describe("regions and edges", () => {
  it("has one region, living and dining, and an edge for each pair that meets", () => {
    const regions = layout.elements.filter((e) => e.kind === "region");
    expect(regions.map((r) => r.region!.spaces)).toEqual([["living", "dining"]]);
    expect(between("living", "dining").edge!.treatment).toBe("open");
    expect(between("nook", "dining").edge!.treatment).toBe("opening");
    expect(between("bath", "hall").edge!.treatment).toBe("door");
  });

  it("draws the region's outline", () => {
    const { all } = mount(layout);
    expect(all("path.region")).toHaveLength(1);
  });

  it("finds an open edge under the cursor, and the wall over a walled one", () => {
    const { viewer } = mount(layout);
    const v = viewer as unknown as Inside;
    const at = (e: Element) => {
      const [a, b] = e.points;
      return v.toScreen({ x: (a.x + b.x) / 2, y: (a.y + b.y) / 2 });
    };
    const open = at(between("living", "dining"));
    expect(v.hitTest(open.x, open.y)).toMatchObject({ type: "shared" });
    const wall = at(between("bath", "hall"));
    expect(v.hitTest(wall.x, wall.y)?.type).not.toBe("shared");
    expect(viewer.edgeBetween(["dining", "living"])?.edge?.treatment).toBe("open");
  });

  it("outlines a space's region when the space is hovered", () => {
    const { viewer, all, render } = mount(layout);
    const living = layout.elements.find((e) => e.id === "living")!;
    (viewer as unknown as Inside).hover = { type: "space", element: living };
    render();
    expect(all("path.hover-region")).toHaveLength(1);
    const bath = layout.elements.find((e) => e.id === "bath")!;
    (viewer as unknown as Inside).hover = { type: "space", element: bath };
    render();
    expect(all("path.hover-region")).toHaveLength(0);
  });
});
