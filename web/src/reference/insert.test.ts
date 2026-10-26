import { describe, expect, it } from "vitest";
import { place, type InsertKind } from "./insert";

/** Apply the placement at `|` and return the new text (tab stops left in). */
function apply(source: string, snippet: string, kind: InsertKind): string {
  const offset = source.indexOf("|");
  const text = source.slice(0, offset) + source.slice(offset + 1);
  const p = place(text, offset, snippet, kind);
  if (p.existing) return `existing ${text.slice(p.existing.start, p.existing.end)}`;
  return text.slice(0, p.offset) + p.text + text.slice(p.end ?? p.offset);
}

const SPACE = [
  "kind",
  "label",
  "count",
  "area",
  "side",
  "width",
  "depth",
  "aspect",
  "plan",
  "typical",
  "mirror",
];
const field = (name: string): InsertKind => ({ field: name, pageFields: SPACE });

describe("placing inserts", () => {
  const spaces = `{
  spaces = {
    kitchen = { area = { min = 9, max = 14 } },
    stair = { kind = 'vertical, count = 2 },
    hall = {
      kind = 'circulation,
    },
  },
}`;

  it("replaces a field's value when asked to (a symbol picked from the catalog)", () => {
    const source = `{ spaces = { bed = { kind = 'fixture, symbol = 'bed_queen, # comment
  width = 1.5 } } }`;
    const pick = { field: "symbol", pageFields: SPACE, replace: true };
    const out = apply(source.replace("kind", "|kind"), "symbol = 'bed_king,", pick);
    expect(out).toContain("symbol = 'bed_king, # comment");
    // Not there yet: added.
    const added = apply(`{ bed = { |kind = 'fixture } }`, "symbol = 'sofa,", pick);
    expect(added).toBe(`{ bed = { kind = 'fixture, symbol = 'sofa } }`);
    // Without `replace`, it points at the field.
    expect(apply(source.replace("kind", "|kind"), "symbol = 'sofa,", field("symbol"))).toBe(
      "existing symbol",
    );
  });

  it("adds a field to a one-line record, from inside one of its values", () => {
    const out = apply(spaces.replace("min = 9", "min = |9"), "aspect = ${1:1},", field("aspect"));
    expect(out).toContain("kitchen = { area = { min = 9, max = 14 }, aspect = ${1:1} },");
  });

  it("adds a field to the record a key holds, when the page is about it", () => {
    const out = apply(spaces.replace("stair =", "st|air ="), "aspect = 1.6,", field("aspect"));
    expect(out).toContain("stair = { kind = 'vertical, count = 2, aspect = 1.6 },");
  });

  it("adds a field on its own line to a multi-line record", () => {
    const out = apply(
      spaces.replace("kind = 'circ", "ki|nd = 'circ"),
      "area = {\n\tmin = ${1:0},\n},",
      field("area"),
    );
    expect(out).toContain(
      "    hall = {\n      kind = 'circulation,\n      area = {\n        min = ${1:0},\n      },\n    },",
    );
  });

  it("points at a field that is already there", () => {
    expect(apply(spaces.replace("count = 2", "count = |2"), "kind = 'room,", field("kind"))).toBe(
      "existing kind",
    );
  });

  it("adds a rule after the current item of a list", () => {
    const rules = `{
  rules = [
    plate.door "a" "b",
    plate.window "a"
  ],
}`;
    expect(
      apply(rules.replace('plate.door "a"', 'plate.do|or "a"'), 'plate.touches "x" "y"', "item"),
    ).toContain('plate.door "a" "b",\n    plate.touches "x" "y",\n    plate.window "a"');
    expect(apply(rules.replace('window "a"', 'window "|a"'), 'plate.touches "x" "y"', "item")).toContain(
      'plate.window "a",\n    plate.touches "x" "y"\n  ],',
    );
    expect(apply(`{ rules = [plate.door "a" "b"|] }`, 'plate.window "c"', "item")).toBe(
      '{ rules = [plate.door "a" "b", plate.window "c"] }',
    );
    expect(apply(`{ rules = [|] }`, 'plate.window "c"', "item")).toBe('{ rules = [plate.window "c"] }');
  });

  it("puts other text at the cursor, or on a new line at the end of one", () => {
    expect(apply("{\n  a = 1,|\n}", "b = 2,", "text")).toBe("{\n  a = 1,\n  b = 2,\n}");
    expect(apply("{\n  |\n}", "b = {\n\tc = 1,\n},", "text")).toBe("{\n  b = {\n    c = 1,\n  },\n}");
  });
});
