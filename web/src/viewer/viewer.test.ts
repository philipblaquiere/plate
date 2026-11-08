// @vitest-environment happy-dom
// The drawing a layout makes, checked on a fake DOM: no browser, no wasm, no solver. The layout
// is a pinned fixture (fixtures/pinned-flat.ncl → web/src/fixtures/pinned-flat.json via
// `npm run fixtures`), so it never moves.
import { describe, expect, it } from "vitest";
import flat from "../fixtures/pinned-flat.json";
import { mount, pathPoints } from "../test/viewer";
import type { Layout } from "../types";

// Typed: tsc checks the fixture against types.ts, so a Rust change the TS types miss shows here.
const layout: Layout = flat as unknown as Layout;
const count = (kind: string) => layout.elements.filter((e) => e.kind === kind).length;

it("the fixture has something of everything", () => {
  for (const kind of ["space", "wall", "door", "window", "part", "run"])
    expect(count(kind)).toBeGreaterThan(0);
});

describe("viewer on the pinned flat", () => {
  it("draws every space, wall and opening", () => {
    const { all } = mount(layout);
    expect(all("path.space")).toHaveLength(count("space"));
    expect(all("path.space-fill")).toHaveLength(1); // the hall
    expect(all("path.wall")).toHaveLength(count("wall"));
    expect(all("g.door")).toHaveLength(count("door"));
  });

  it("fits the plate inside the sheet", () => {
    const { all } = mount(layout, 800, 600);
    for (const p of all("path.space").flatMap((e) => pathPoints(e.getAttribute("d") ?? ""))) {
      expect(p.x).toBeGreaterThanOrEqual(0);
      expect(p.x).toBeLessThanOrEqual(800);
      expect(p.y).toBeGreaterThanOrEqual(0);
      expect(p.y).toBeLessThanOrEqual(600);
    }
  });

  it("names the rooms", () => {
    const { all } = mount(layout);
    const names = all("text.space-name").map((t) => t.textContent);
    for (const room of ["living", "bedroom", "bath", "hall"]) {
      expect(names.some((n) => n?.toLowerCase().includes(room))).toBe(true);
    }
  });

  it("shows no service until one is chosen", () => {
    const { all } = mount(layout);
    expect(all(".part")).toHaveLength(0);
    expect(all("path.run")).toHaveLength(0);
    expect(all("g.elements.halftone")).toHaveLength(0);
  });

  it("shows one service's parts and routed runs, the plan faded under them", () => {
    const { viewer, all, render } = mount(layout);
    viewer.setServiceView("power");
    render();
    const power = layout.elements.filter((e) => e.kind === "part" && e.part?.service === "power");
    expect(power.length).toBeGreaterThan(5);
    expect(all(".part")).toHaveLength(power.length);
    const routed = layout.elements.filter(
      (e) => e.kind === "run" && e.run?.service === "power" && e.run.draw === "routed",
    );
    expect(routed.length).toBeGreaterThan(5);
    expect(all("path.run-routed")).toHaveLength(routed.length);
    expect(all("path.run-arcs")).toHaveLength(0);
    expect(all("g.elements.halftone")).toHaveLength(1);
  });

  it("draws the schematic instead of the routes when asked", () => {
    const { viewer, all, render } = mount(layout);
    viewer.setServiceView("power");
    viewer.setSchematic(true);
    render();
    expect(all("path.run-routed")).toHaveLength(0);
    expect(all("path.run-arcs").length).toBeGreaterThan(0);
    // One numbered home-run arrow per circuit.
    const circuits = (layout.circuits ?? []).filter((c) => c.service === "power");
    expect(circuits).toHaveLength(2);
    expect(all("path.run-head")).toHaveLength(circuits.length);
  });

  it("keeps each service to its own parts", () => {
    const { viewer, all, render } = mount(layout);
    viewer.setServiceView("lighting");
    render();
    const lighting = layout.elements.filter((e) => e.kind === "part" && e.part?.service === "lighting");
    expect(lighting).toHaveLength(6); // a switch and a light in each of three rooms
    expect(all(".part")).toHaveLength(lighting.length);
  });
});
