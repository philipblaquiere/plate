import { describe, expect, it } from "vitest";
import type { SourceSpan } from "../types";
import { locate, pointerAt, scan } from "./locate";

/** Locate with the cursor at `|` (removed from the text). */
function at(
  source: string,
  options: { plate?: boolean; sources?: SourceSpan[]; config?: unknown; evaluatedPlate?: boolean } = {},
) {
  const offset = source.indexOf("|");
  const text = source.slice(0, offset) + source.slice(offset + 1);
  return locate({
    text,
    offset,
    plate: options.plate ?? /\bstart\s*=/.test(text),
    evaluated: options.sources
      ? { file: "a.ncl", sources: options.sources, config: options.config, plate: options.evaluatedPlate }
      : undefined,
  });
}

const APARTMENT = `# comment { not a brace
{
  name = "Two-bedroom",
  start = {
    mode = 'boundary,
    points = [[0, 0], [14, 0], [14, 9]],
    ports = ['facade, 'blank],
  },
  spaces = {
    living = { area.min = 24 },
    hall = { kind = 'circulation, area = { min = 6, max = 12 } },
  },
  rules = [
    plate.touches "kitchen" "living",
    plate.door "a" "b" & { width = 1.2 },
    plate.prefer (plate.window "kitchen"),
  ],
}`;

function cursorIn(text: string, marker: string, shift = 0): string {
  const i = text.indexOf(marker);
  if (i < 0) throw new Error(marker);
  return text.slice(0, i + shift) + "|" + text.slice(i + shift);
}

describe("locating the cursor from the text", () => {
  it("finds root fields and starting modes", () => {
    expect(at(cursorIn(APARTMENT, "name ="))).toMatchObject({ entry: "def:#", field: "name" });
    expect(at(cursorIn(APARTMENT, "mode ="))).toMatchObject({ entry: "def:BoundaryStart" });
    expect(at(cursorIn(APARTMENT, "points =", 2))).toMatchObject({
      entry: "def:BoundaryStart",
      field: "points",
    });
    expect(at(cursorIn(APARTMENT, "14, 9"))).toMatchObject({ entry: "def:BoundaryStart", field: "points" });
  });

  it("knows the spine variant from its tag", () => {
    const loc = at(`{ start = { mode = 'spine, po|ints = [[0, 0]] } }`);
    expect(loc).toMatchObject({ entry: "def:SpineStart", field: "points" });
    expect(loc!.trail.map((c) => c.label)).toEqual(["Plate", "start (Spine)", "points"]);
  });

  it("finds space fields, including dotted names and nested ranges", () => {
    expect(at(cursorIn(APARTMENT, "area.min", 1))).toMatchObject({ entry: "def:Space", field: "area" });
    expect(at(cursorIn(APARTMENT, "min = 6"))).toMatchObject({ entry: "def:Space", field: "area" });
    const kind = at(cursorIn(APARTMENT, "'circulation", 2));
    expect(kind).toMatchObject({ entry: "def:Space", field: "kind", value: { value: "circulation" } });
    expect(kind!.value!.doc).toContain("corridor");
    expect(kind!.trail.map((c) => c.label)).toEqual(["Plate", "spaces", "hall", "kind"]);
  });

  it("documents port roles", () => {
    const loc = at(cursorIn(APARTMENT, "'blank", 2));
    expect(loc).toMatchObject({ entry: "def:BoundaryStart", field: "ports", value: { value: "blank" } });
    expect(loc!.value!.doc).toContain("can't have openings");
  });

  it("finds helpers and the rules they write", () => {
    expect(at(cursorIn(APARTMENT, "touches", 2))).toMatchObject({ entry: "helper:touches" });
    expect(at(cursorIn(APARTMENT, '"kitchen" "living"', 3))).toMatchObject({ entry: "helper:touches" });
    // A record merged into a helper call: the rule's own options.
    expect(at(cursorIn(APARTMENT, "width ="))).toMatchObject({ entry: "def:RuleDoor", field: "width" });
    // Inside `plate.prefer (…)`: the inner helper.
    expect(at(cursorIn(APARTMENT, '"kitchen")', 2))).toMatchObject({ entry: "helper:window" });
    expect(at(cursorIn(APARTMENT, "prefer", 2))).toMatchObject({ entry: "helper:prefer" });
  });

  it("reads a file without a start as a plan", () => {
    const plan = `let x = { a = 1 } in
{
  needs = { ports = { entrance = 'required } },
  spaces = { bath = { area.min = 4 } },
}`;
    expect(at(cursorIn(plan, "entrance ="))).toMatchObject({ entry: "def:Needs", field: "ports" });
    // On a space's name: the page about spaces.
    expect(at(cursorIn(plan, "bath ="))).toMatchObject({ entry: "def:Space" });
    expect(at(cursorIn(plan, "area.min", 1))).toMatchObject({ entry: "def:Space", field: "area" });
    // Inside a `let` binding there is no path from the top; the field name still helps.
    expect(at(cursorIn(plan, "a = 1"))?.entry).not.toBe("def:Plan");
  });

  it("copes with broken text", () => {
    expect(at(`{ spaces = { kitchen = { are|a = `)).toMatchObject({ entry: "def:Space", field: "area" });
    expect(at(`{ start = { mode = 'boundary, poi|`)).toMatchObject({ entry: "def:BoundaryStart" });
  });

  it("says nothing in comments or outside the record", () => {
    expect(at(cursorIn(APARTMENT, "comment {"))).toBeUndefined();
    expect(at(`let w = 4 in| { name = "x" }`)).toBeUndefined();
    expect(at(`{ name = "a # not a comment|" }`, { plate: true })).toMatchObject({
      entry: "def:#",
      field: "name",
    });
  });

  it("scans strings and comments without counting their brackets", () => {
    const top = scan(`{ a = "{[(", # }\n b = m%"}"%, c = { d = 1 } }`);
    expect(top.children).toHaveLength(1);
    expect(top.children[0].fields.map((f) => f.key.join("."))).toEqual(["a", "b", "c"]);
  });
});

describe("locating the cursor from the source map", () => {
  const text = `let room = import "room.ncl" in
{
  start = { mode = 'spine, points = [[0, 0], [60, 0]] },
  spaces = { room = { count.min = 1, plan = room } },
}`;
  const span = (
    pointer: string,
    startLine: number,
    startCol: number,
    endLine: number,
    endCol: number,
    kind: SourceSpan["kind"] = "record",
  ) =>
    ({
      pointer,
      file: "a.ncl",
      span: { startLine, startCol, endLine, endCol },
      kind,
      literal: true,
    }) as SourceSpan;
  const sources = [
    span("", 2, 1, 5, 2),
    span("/start", 3, 11, 3, 56),
    span("/start/points", 3, 37, 3, 54, "array"),
    span("/start/points/1/0", 3, 46, 3, 48, "number"),
    span("/spaces", 4, 12, 4, 53),
    span("/spaces/room", 4, 21, 4, 51),
    span("/spaces/room/count/min", 4, 33, 4, 34, "number"),
  ];
  const config = {
    start: {
      mode: "spine",
      points: [
        [0, 0],
        [60, 0],
      ],
    },
    spaces: { room: { count: { min: 1 } } },
  };

  it("inverts the source map: the smallest value around the cursor", () => {
    expect(pointerAt(sources, "a.ncl", 3, 47)?.pointer).toBe("/start/points/1/0");
    expect(pointerAt(sources, "a.ncl", 3, 20)?.pointer).toBe("/start");
    expect(pointerAt(sources, "b.ncl", 3, 47)).toBeUndefined();
  });

  it("uses the evaluated data to pick the variant", () => {
    const loc = at(cursorIn(text, "60, 0", 1), { sources, config });
    expect(loc).toMatchObject({ entry: "def:SpineStart", field: "points" });
  });

  it("adds the field name under the cursor", () => {
    expect(at(cursorIn(text, "plan = room", 1), { sources, config })).toMatchObject({ entry: "def:Plan" });
    expect(at(cursorIn(text, "count.min", 7), { sources, config })).toMatchObject({
      entry: "def:Space",
      field: "count",
    });
  });

  it("walks a plan file shown on its own from the plan", () => {
    const plan = `{
  preview = [{ name = "corner", width = 4, ports.left = 'facade }],
  spaces = {},
}`;
    const planSources = [
      span("", 1, 1, 4, 2),
      span("/preview", 2, 13, 2, 66, "array"),
      span("/preview/0", 2, 14, 2, 65),
      span("/preview/0/width", 2, 41, 2, 42, "number"),
      span("/spaces", 3, 12, 3, 14),
    ];
    const config = { preview: [{ name: "corner", width: 4, ports: { left: "facade" } }], spaces: {} };
    const loc = at(cursorIn(plan, "width = 4", 9), { sources: planSources, config, evaluatedPlate: false });
    expect(loc).toMatchObject({ entry: "def:PreviewCase", field: "width" });
    expect(loc!.trail[0].label).toBe("Plan");
  });
});
