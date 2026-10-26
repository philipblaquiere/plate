import { describe, expect, it } from "vitest";
import { plateHelpers, schemaFields } from "./schema";

describe("schema-driven editor docs", () => {
  it("reads every field from the JSON Schema, including spaces and rules", () => {
    const keys = [...schemaFields.keys()];
    for (const k of [
      "name",
      "system",
      "units",
      "start",
      "mode",
      "points",
      "ports",
      "spaces",
      "kind",
      "count",
    ])
      expect(keys).toContain(k);
    for (const k of [
      "area",
      "plan",
      "needs",
      "rules",
      "rule",
      "a",
      "b",
      "port",
      "goals",
      "typical",
      "mirror",
    ])
      expect(keys).toContain(k);
  });

  it("knows allowed values, types and defaults", () => {
    expect(schemaFields.get("system")?.values).toEqual(["metric", "imperial"]);
    expect(schemaFields.get("units")?.detail).toBe("'mm | 'cm | 'm | 'in | 'ft");
    expect(schemaFields.get("name")?.doc).toContain('Default: `"Untitled plate"`');
    expect(schemaFields.get("kind")?.values).toEqual([
      "room",
      "zone",
      "circulation",
      "void",
      "vertical",
      "fixture",
    ]);
    expect(schemaFields.get("typical")?.doc).toContain("Default: `true`");
  });

  it("merges fields shared by the variants of a union", () => {
    expect(schemaFields.get("mode")?.values).toEqual(["boundary", "spine"]);
    expect(schemaFields.get("points")?.detail).toBe("Array Point");
    expect(schemaFields.get("points")?.doc).toContain("Corners of the boundary");
    expect(schemaFields.get("points")?.doc).toContain("Points along the spine");
    expect(schemaFields.get("rule")?.values).toContain("door");
  });

  it("builds snippets from the shape of the value", () => {
    expect(schemaFields.get("start")?.snippet).toBe(
      "start = {\n\tmode = '${1|boundary,spine|},\n\tpoints = [\n\t\t[${2:0}, ${3:0}],\n\t],\n},",
    );
    expect(schemaFields.get("spaces")?.snippet).toBe("spaces = {\n\t${1:name} = { $2 },\n},");
    expect(schemaFields.get("a")?.snippet).toBe('a = "${1:name}",');
    expect(schemaFields.get("port")?.snippet).toMatch(/^port = '\$\{1\|entrance,service,/);
  });

  it("reads the plate helpers and their docs from schema/plate.ncl", () => {
    const door = plateHelpers.find((h) => h.name === "door");
    expect(door?.snippet).toBe('door "${1:a}" "${2:b}"');
    expect(door?.doc).toContain('plate.door "bedroom" "hall"');
    expect(plateHelpers.map((h) => h.name)).toEqual(
      expect.arrayContaining(["touches", "window", "between", "along", "unique", "prefer"]),
    );
  });
});
