import { describe, expect, it } from "vitest";

import { fuzzyFilter, fuzzyMatch } from "./fuzzy";

describe("fuzzyMatch", () => {
  it("needs every character, in order", () => {
    expect(fuzzyMatch("wng", "hotel/wing.ncl")).not.toBeNull();
    expect(fuzzyMatch("gnw", "hotel/wing.ncl")).toBeNull();
    expect(fuzzyMatch("xyz", "hotel/wing.ncl")).toBeNull();
  });

  it("ignores case and spaces", () => {
    expect(fuzzyMatch("HOTEL wing", "hotel/wing.ncl")).not.toBeNull();
  });

  it("points at what matched", () => {
    const m = fuzzyMatch("wing", "hotel/wing.ncl")!;
    expect(m.positions).toEqual([6, 7, 8, 9]);
  });

  it("matches everything with no query", () => {
    expect(fuzzyMatch("", "anything")).toEqual({ score: 0, positions: [] });
  });
});

describe("fuzzyFilter", () => {
  const paths = ["plans/bedroom.ncl", "bath.ncl", "hotel/bathroom.ncl", "plans/kitchen.ncl"];
  const rank = (q: string) => fuzzyFilter(q, paths, (p) => p).map((r) => r.item);

  it("keeps the order with no query", () => {
    expect(rank("")).toEqual(paths);
  });

  it("drops what doesn't match", () => {
    expect(rank("kit")).toEqual(["plans/kitchen.ncl"]);
  });

  it("prefers a file's name, then the shorter one", () => {
    expect(rank("bath")).toEqual(["bath.ncl", "hotel/bathroom.ncl"]);
  });

  it("prefers a run of characters to scattered ones", () => {
    const found = fuzzyFilter("room", ["r/o/o/m.ncl", "bedroom.ncl"], (p) => p).map((r) => r.item);
    expect(found[0]).toBe("bedroom.ncl");
  });
});
