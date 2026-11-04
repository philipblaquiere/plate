import { describe, expect, it } from "vitest";
import { importPath, importsOf, isPlan, isPlate, libraryKind, retargetImports } from "./plates";

describe("workspace files", () => {
  it("tells plates from plans", () => {
    expect(isPlate('{\n  name = "x",\n  start = { kind = \'free }\n}')).toBe(true);
    expect(isPlate("{ spaces = {} }")).toBe(false);
    // Plans are shown on their own; libraries of bits to import aren't.
    expect(isPlan("{ spaces = {} }")).toBe(true);
    expect(isPlan("# a room\n{\n  needs.width.min = 3,\n}")).toBe(true);
    expect(isPlan("{\n  bath = { area.min = 4 },\n}")).toBe(false);
    expect(isPlan("{\n  start = { mode = 'spine },\n  spaces = {},\n}")).toBe(false);
  });

  it("follows imports relative to the importing file", () => {
    const files = {
      "hotel.ncl": 'let r = import "plans/room.ncl" in { start = 1 }',
      "plans/room.ncl":
        '# use it: let r = import "nope.ncl" in\nlet b = import "../lib/bath.ncl" in let c = import "closet.ncl" in {}',
      "plans/nope.ncl": "{}",
      "plans/closet.ncl": "{}",
      "lib/bath.ncl": 'import "../hotel.ncl"',
      "other.ncl": "{}",
    };
    expect([...importsOf("hotel.ncl", files)].sort()).toEqual([
      "hotel.ncl",
      "lib/bath.ncl",
      "plans/closet.ncl",
      "plans/room.ncl",
    ]);
  });
});

describe("moving files", () => {
  const exists = (p: string) =>
    ["hotel.ncl", "plans/room.ncl", "lib/bath.ncl", "plans/hotel.ncl"].includes(p);

  it("writes the shortest import path", () => {
    expect(importPath("", "plans/room.ncl")).toBe("plans/room.ncl");
    expect(importPath("plans/", "lib/bath.ncl")).toBe("../lib/bath.ncl");
    expect(importPath("plans", "plans/room.ncl")).toBe("room.ncl");
  });

  it("points imports at a moved file", () => {
    const text = 'let r = import "hotel-room.ncl" in let s = import "spine.ncl" in {}';
    expect(
      retargetImports(
        text,
        "hotel.ncl",
        "hotel.ncl",
        { from: "hotel-room.ncl", to: "plans/room.ncl" },
        exists,
      ),
    ).toBe('let r = import "plans/room.ncl" in let s = import "spine.ncl" in {}');
    expect(
      retargetImports(text, "hotel.ncl", "hotel.ncl", { from: "x.ncl", to: "y.ncl" }, exists),
    ).toBeNull();
  });

  it("leaves comments alone", () => {
    const text = '# let r = import "hotel-room.ncl" in\nlet r = import "hotel-room.ncl" in r';
    expect(
      retargetImports(
        text,
        "hotel.ncl",
        "hotel.ncl",
        { from: "hotel-room.ncl", to: "plans/room.ncl" },
        exists,
      ),
    ).toBe('# let r = import "hotel-room.ncl" in\nlet r = import "plans/room.ncl" in r');
  });

  it("keeps a moved file's own imports working", () => {
    const text = 'import "plans/room.ncl"';
    expect(
      retargetImports(
        text,
        "hotel.ncl",
        "plans/hotel.ncl",
        { from: "hotel.ncl", to: "plans/hotel.ncl" },
        exists,
      ),
    ).toBe('import "room.ncl"');
  });

  it("knows a library from a plan", () => {
    expect(libraryKind('{\n  label = "Lighting",\n  parts = { light = {} },\n}')).toBe("service");
    expect(libraryKind("fun x => {\n  place = [],\n}")).toBe("service");
    expect(libraryKind("{\n  partition = { on = 'partition, thickness = 0.1 },\n}")).toBe("assemblies");
    expect(libraryKind('{ partition = { thickness = "100 mm" } }')).toBe("assemblies");
    expect(libraryKind("let g = 1 in\n{\n  wet = {\n    layers = [g],\n  },\n}")).toBe("assemblies");
    expect(libraryKind("{\n  spaces = { a = {} },\n}")).toBeNull();
    expect(libraryKind("{ start = { mode = 'boundary } }")).toBeNull();
    expect(libraryKind("let helper = fun x => x in helper")).toBeNull();
  });
});
