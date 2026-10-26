import { describe, expect, it } from "vitest";
import { feetInches, Lengths } from "./format";

const THIN = " ";

describe("feetInches", () => {
  it.each([
    [1182, `98'-6"`],
    [360, `30'-0"`],
    [6.5, `6 1/2"`],
    [0.5, `1/2"`],
    [0, `0"`],
    [-30, `−2'-6"`],
    [15000 * 12, `15,000'-0"`],
    [11.99, `1'-0"`],
  ])("%d in → %s", (inches, text) => expect(feetInches(inches)).toBe(text));
});

describe("Lengths", () => {
  it("formats imperial", () => {
    const L = new Lengths("imperial", "ft");
    expect(L.length(98.5)).toBe(`98'-6"`);
    expect(L.area(9702.25)).toBe(`9,702${THIN}ft²`);
    expect([L.tick(0), L.tick(20), L.tick(0.5), L.tick(2.5)]).toEqual(["0", "20'", `6"`, `2'-6"`]);
  });

  it("formats metric, omitting the unit on drawing dimensions", () => {
    const L = new Lengths("metric", "m");
    expect(L.length(30)).toBe(`30.00${THIN}m`);
    expect(L.dimension(30)).toBe("30.00");
    expect(L.area(900)).toBe(`900.00${THIN}m²`);
    expect(new Lengths("metric", "mm").area(9e6)).toBe(`9.00${THIN}m²`);
  });

  it("shows metric geometry in imperial", () => {
    expect(new Lengths("imperial", "m").length(30)).toBe(`98'-5 1/8"`);
  });

  it("picks imperial grid steps in feet", () => {
    const { minor, major } = new Lengths("imperial", "ft").gridSteps(10);
    expect([minor, major]).toEqual([2, 10]);
  });
});
