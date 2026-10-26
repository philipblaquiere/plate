import { describe, expect, it } from "vitest";
import { importersOf } from "./plates";
import { caseLabel, caseLiteral, casePointer, previewCases, sidesOf } from "./preview";
import type { SlotInfo } from "./types";

const slot = (ports: SlotInfo["ports"]): SlotInfo => ({
  width: 4,
  depth: 7.5,
  ports,
  units: "m",
  system: "metric",
  seed: 1,
  grid: 0.5,
  plan: "0",
});
const P = (x: number, y: number) => ({ x, y });

describe("plan previews", () => {
  it("lists the cases a plan offers", () => {
    expect(previewCases({})).toEqual([]);
    expect(previewCases({ preview: { width: 4 } })).toEqual([{ width: 4 }]);
    const many = { preview: [{ name: "typical" }, {}] };
    expect(previewCases(many).map((c, i) => caseLabel(c, i))).toEqual(["typical", "preview 2"]);
    expect(casePointer(many, 1)).toBe("/preview/1");
    expect(casePointer({ preview: {} }, 0)).toBe("/preview");
    expect(casePointer({}, 0)).toBe("/preview");
  });

  it("reads the sides of a plate's slot", () => {
    const typical = slot([
      { roles: ["entrance"], a: P(0, 0), b: P(4, 0) },
      { roles: ["party"], a: P(4, 0), b: P(4, 7.5) },
      { roles: ["facade"], a: P(4, 7.5), b: P(0, 7.5) },
      { roles: ["party", "wet"], a: P(0, 7.5), b: P(0, 0) },
    ]);
    expect(sidesOf(typical)).toEqual({
      roles: { bottom: ["entrance"], right: ["party"], top: ["facade"], left: ["party", "wet"] },
      approximate: false,
    });
    const split = slot([
      { roles: ["entrance"], a: P(0, 0), b: P(4, 0) },
      { roles: ["facade"], a: P(4, 0), b: P(4, 2) },
      { roles: ["party"], a: P(4, 2), b: P(4, 7.5) },
    ]);
    const s = sidesOf(split);
    expect(s.roles.right).toEqual(["party"]);
    expect(s.roles.top).toEqual(["partition"]);
    expect(s.approximate).toBe(true);
  });

  it("writes a case with only what differs from the defaults", () => {
    expect(caseLiteral({ width: 3.8, depth: 8.2, units: "m" })).toBe("{ width = 3.8, depth = 8.2 }");
    const sides = { bottom: ["entrance"], right: ["party", "wet"], top: ["facade"], left: ["facade"] };
    expect(caseLiteral({ name: "room_1", width: 4, depth: 7.5, units: "m", sides })).toBe(
      `{ name = "room_1", width = 4, depth = 7.5, ports = { right = ['party, 'wet], left = 'facade } }`,
    );
    // Imperial plates: the plan's preview says so, so its numbers are read as feet.
    expect(caseLiteral({ width: 13, depth: 25, units: "ft" })).toBe(
      "{ width = 13, depth = 25, system = 'imperial }",
    );
    expect(caseLiteral({ width: 4, depth: 7.5, units: "m", seed: 7 })).toBe(
      "{ width = 4, depth = 7.5, seed = 7 }",
    );
    expect(caseLiteral({ width: 4000, depth: 7500, units: "mm" })).toBe(
      "{ width = 4000, depth = 7500, units = 'mm }",
    );
  });

  it("finds the plates that use a plan", () => {
    const files = {
      "hotel.ncl": 'let r = import "room.ncl" in {\n  start = 1 }',
      "tower.ncl": 'let h = import "hotel.ncl" in {\n  start = 2 }',
      "other.ncl": "{\n  start = 3 }",
      "room.ncl": "{ spaces = {} }",
    };
    expect(importersOf("room.ncl", files)).toEqual(["hotel.ncl", "tower.ncl"]);
    expect(importersOf("other.ncl", files)).toEqual([]);
  });
});
