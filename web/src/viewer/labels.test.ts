import { describe, expect, it } from "vitest";
import { clearOf } from "./labels";

const room = [
  { x: 0, y: 0 },
  { x: 200, y: 0 },
  { x: 200, y: 150 },
  { x: 0, y: 150 },
];

describe("clearOf", () => {
  it("leaves a label nothing covers where it is", () => {
    expect(clearOf({ x: 100, y: 75 }, 60, 28, 12, [{ x0: 10, y0: 10, x1: 30, y1: 30 }], room)).toEqual({
      x: 100,
      y: 75,
    });
  });

  it("moves a label off a light in the middle of the room, below it", () => {
    const light = { x0: 90, y0: 65, x1: 110, y1: 85 };
    const p = clearOf({ x: 100, y: 75 }, 60, 28, 12, [light], room);
    expect(p.x).toBe(100);
    expect(p.y - 12).toBeGreaterThanOrEqual(light.y1);
  });

  it("goes above when there's no room below", () => {
    const light = { x0: 90, y0: 120, x1: 110, y1: 140 };
    const p = clearOf({ x: 100, y: 130 }, 60, 28, 12, [light], room);
    expect(p.y - 12 + 28).toBeLessThanOrEqual(light.y0);
  });

  it("stays put when nowhere is free", () => {
    const all = { x0: 0, y0: 0, x1: 200, y1: 150 };
    expect(clearOf({ x: 100, y: 75 }, 60, 28, 12, [all], room)).toEqual({ x: 100, y: 75 });
  });
});
