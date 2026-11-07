import { describe, expect, it } from "vitest";

import { loadText, nextView, schedule, serviceOf, summarize } from "./services";
import type { Circuit, Diagnostic, Element, Layout } from "./types";

const part = (service: string, name: string, room: string): Element => ({
  id: `part:${service}.${name}:${room}#0`,
  kind: "part",
  layer: "PLATE-PART",
  points: [],
  closed: true,
  length: 0,
  part: { service, part: name, label: name, mount: "wall", space: room, placed: "", at: { x: 0, y: 0 } },
});

const run = (service: string, network: string, scope: string, group?: string): Element => ({
  id: `run:${service}:0:${scope}#0`,
  kind: "run",
  layer: "PLATE-RUN",
  points: [],
  closed: false,
  length: 0,
  run: { service, network, group, draw: "arcs" },
});

const layout = (elements: Element[]): Layout => ({
  name: "x",
  system: "metric",
  units: "m",
  elements,
  bounds: { minX: 0, minY: 0, maxX: 1, maxY: 1 },
});

describe("services", () => {
  it("finds the service a pointer is about", () => {
    expect(serviceOf("/services/power/place/2")).toBe("power");
    expect(serviceOf("/spaces/flat/plan/services/heat")).toBe("heat");
    expect(serviceOf("/rules/0")).toBeNull();
    expect(serviceOf(undefined)).toBeNull();
  });

  it("counts parts, groups per scope, and problems", () => {
    const l = layout([
      part("power", "outlet", "a"),
      part("power", "outlet", "b"),
      part("lighting", "light", "a"),
      run("power", "Circuits", "flat_1", "1"),
      run("power", "Circuits", "flat_1", "1"),
      run("power", "Circuits", "flat_2", "1"),
      run("power", "Circuits", "flat_2", "2"),
    ]);
    const diagnostics: Diagnostic[] = [
      { severity: "warning", message: "x", pointer: "/services/power/place/0" },
      { severity: "info", message: "y", pointer: "/services/power/place/1" },
      { severity: "warning", message: "z", pointer: "/services/plumbing/connect/0" },
    ];
    const s = summarize(l, diagnostics, { power: "Power" });
    expect(s.map((x) => x.name)).toEqual(["power", "lighting", "plumbing"]);
    expect(s[0]).toMatchObject({ label: "Power", issues: 1 });
    expect(s[0].parts).toEqual([{ part: "outlet", label: "outlet", symbol: undefined, count: 2 }]);
    expect(s[0].networks).toEqual([{ name: "Circuits", groups: 3 }]);
    expect(s[1].label).toBe("Lighting");
    expect(s[2]).toMatchObject({ parts: [], issues: 1 });
  });

  it("cycles from the plan through each service and back", () => {
    expect(nextView(null, ["a", "b"])).toBe("a");
    expect(nextView("a", ["a", "b"])).toBe("b");
    expect(nextView("b", ["a", "b"])).toBeNull();
    expect(nextView("gone", ["a"])).toBeNull();
    expect(nextView(null, [])).toBeNull();
  });
});

describe("panel schedule", () => {
  const circuit = (scope: string, group: string, extra: Partial<Circuit> = {}): Circuit => ({
    service: "power",
    network: "General",
    scope,
    group,
    devices: [["receptacle", 6]],
    load: 0,
    length: 10,
    ...extra,
  });

  it("lists one scope's circuits in number order, two-pole for 240 V", () => {
    const s = schedule(
      [
        circuit("flat_1", "10", { rating: 30, voltage: 240, medium: "10/3", load: 30 }),
        circuit("flat_1", "2", { rating: 15, medium: "14/2" }),
        circuit("flat_2", "1", { rating: 15, medium: "14/2" }),
        circuit("", "1", { service: "plumbing" }),
      ],
      "power",
    );
    expect(s.scopes).toEqual(["flat_1", "flat_2"]);
    expect(s.rows.map((r) => r.circuit)).toEqual(["2", "10"]);
    expect(s.rows[1]).toMatchObject({ breaker: "30 A 2P", medium: "10/3", load: "30 A" });
    expect(s.rows[0].devices).toBe("receptacle ×6");
    expect(schedule([circuit("flat_2", "1")], "power", "flat_2").scope).toBe("flat_2");
  });

  it("says a load in words", () => {
    expect(loadText({ current: 30, voltage: 240 })).toBe("30 A at 240 V");
    expect(loadText({ watts: 60 })).toBe("60 W at 120 V");
  });
});
