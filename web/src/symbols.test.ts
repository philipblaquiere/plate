import { describe, expect, it } from "vitest";
import schema from "../../schema/plate.schema.json";
import { byCategory, catalog, suggest, thumbnail } from "./symbols";

describe("symbol catalog", () => {
  it("is the schema's Symbol enum", () => {
    const names = (schema.$defs as unknown as Record<string, { enum: string[] }>).Symbol.enum;
    expect(names).toEqual(Object.keys(catalog));
  });

  it("draws every symbol with every shape", () => {
    for (const name of Object.keys(catalog)) {
      const svg = thumbnail(name, 48)!;
      expect(svg.startsWith("<svg")).toBe(true);
      // One element per shape.
      const count = (svg.match(/<(rect|polyline|polygon|circle|ellipse) /g) ?? []).length;
      expect(count, name).toBe(catalog[name].shapes.length);
      expect(svg).not.toContain("NaN");
    }
    expect(thumbnail("no_such_thing")).toBeNull();
  });

  it("groups by category in catalog order", () => {
    const groups = byCategory();
    expect(groups[0].category).toBe("Bedroom");
    expect(groups.flatMap((g) => g.names).length).toBe(Object.keys(catalog).length);
  });

  it("suggests symbols from a space's name", () => {
    expect(suggest("bed").slice(0, 4)).toEqual(
      expect.arrayContaining(["bed_single", "bed_double", "bed_queen", "bed_king"]),
    );
    expect(suggest("parking")[0]).toBe("parking_space");
    expect(suggest("parking")).toContain("car");
    expect(suggest("pool")[0]).toBe("pool");
    expect(suggest("island", "Kitchen island")[0]).toBe("kitchen_island");
    expect(suggest("room_3")).toEqual([]);
  });
});
