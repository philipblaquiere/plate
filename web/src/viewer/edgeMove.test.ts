import { describe, expect, it } from "vitest";
import {
  isNoop,
  movedPoints,
  neighbours,
  offsetEdge,
  selfCrossing,
  signedArea,
  snapOffset,
  translateEdge,
} from "./edgeMove";

const pt = (x: number, y: number) => ({ x, y });
// Counter-clockwise, 10 × 10.
const square = [pt(0, 0), pt(10, 0), pt(10, 10), pt(0, 10)];

describe("offsetEdge", () => {
  it("pushes a wall of a rectangle out, keeping the corners square", () => {
    // Edge 1 is the right side; its left normal points in (−x), so a negative offset goes out.
    const m = offsetEdge(square, true, 1, -2);
    expect(m.a).toEqual(pt(12, 0));
    expect(m.b).toEqual(pt(12, 10));
    expect(m.problem).toBeNull();
    expect(Math.abs(signedArea(movedPoints(square, 1, m)))).toBeCloseTo(120);
  });

  it("slides the ends along slanted neighbours, so they keep their direction", () => {
    // A trapezoid: the top is shorter than the bottom.
    const trap = [pt(0, 0), pt(10, 0), pt(8, 4), pt(2, 4)];
    const m = offsetEdge(trap, true, 2, -2); // the top, moved up by 2
    expect(m.a.x).toBeCloseTo(7);
    expect(m.a.y).toBeCloseTo(6);
    expect(m.b.x).toBeCloseTo(3);
    expect(m.b.y).toBeCloseTo(6);
    expect(m.problem).toBeNull();
  });

  it("refuses to push an edge past the end of its neighbours", () => {
    const m = offsetEdge(square, true, 1, 12); // the right side pushed left beyond the left side
    expect(m.problem).not.toBeNull();
  });

  it("refuses an edge that shrinks to nothing", () => {
    const trap = [pt(0, 0), pt(10, 0), pt(8, 4), pt(2, 4)];
    // The sides meet 10 up; the top pushed up past that would turn around.
    const m = offsetEdge(trap, true, 2, -7);
    expect(m.problem).toMatch(/shrink to nothing/);
  });

  it("moves the end segment of a spine with its open end straight across", () => {
    const spine = [pt(0, 0), pt(10, 0), pt(10, 10)];
    const m = offsetEdge(spine, false, 0, 3);
    expect(m.a).toEqual(pt(0, 3));
    expect(m.b).toEqual(pt(10, 3));
    expect(m.problem).toBeNull();
  });
});

describe("translateEdge", () => {
  it("moves both ends by the same vector", () => {
    const m = translateEdge(square, true, 2, pt(1, 2));
    expect(m.a).toEqual(pt(11, 12));
    expect(m.b).toEqual(pt(1, 12));
    expect(m.free).toBe(true);
    expect(m.problem).toBeNull();
  });

  it("refuses a move that makes the outline cross itself", () => {
    const m = translateEdge(square, true, 2, pt(0, -15));
    expect(m.problem).toMatch(/cross itself|inside out/);
  });
});

describe("snapOffset", () => {
  it("puts a horizontal edge's new y on the grid", () => {
    const [a, b] = [pt(0, 0.3), pt(10, 0.3)];
    // Left normal of a → b is +y.
    expect(0.3 + snapOffset(a, b, 1.1, 0.5)).toBeCloseTo(1.5);
  });

  it("puts a vertical edge's new x on the grid", () => {
    const [a, b] = [pt(10, 0), pt(10, 10)];
    // Left normal is −x: an offset of −2.2 moves it to x = 12.2, snapped to 12.
    expect(10 - snapOffset(a, b, -2.2, 1)).toBeCloseTo(12);
  });

  it("moves a slanted edge by whole steps, and leaves it alone without a grid", () => {
    const [a, b] = [pt(0, 0), pt(3, 4)];
    expect(snapOffset(a, b, 1.3, 0.5)).toBeCloseTo(1.5);
    expect(snapOffset(a, b, 1.3, 0)).toBe(1.3);
  });
});

describe("helpers", () => {
  it("finds the neighbours of an edge, open or closed", () => {
    expect(neighbours(square, true, 0)).toEqual({ prev: 3, next: 2 });
    expect(neighbours([pt(0, 0), pt(1, 0), pt(2, 0)], false, 0)).toEqual({ prev: null, next: 2 });
    expect(neighbours([pt(0, 0), pt(1, 0)], false, 0)).toEqual({ prev: null, next: null });
  });

  it("knows a move that changes nothing", () => {
    expect(isNoop(square, 1, offsetEdge(square, true, 1, 0))).toBe(true);
    expect(isNoop(square, 1, offsetEdge(square, true, 1, 1))).toBe(false);
  });

  it("detects self-crossing", () => {
    expect(selfCrossing(square, true)).toBe(false);
    expect(selfCrossing([pt(0, 0), pt(10, 10), pt(10, 0), pt(0, 10)], true)).toBe(true);
  });
});
