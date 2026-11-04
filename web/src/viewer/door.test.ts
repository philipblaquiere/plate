import { describe, expect, it } from "vitest";
import { doorSymbol, type Pt } from "./door";

/** Centre of the circle through the arc's end points, on the side the sweep flag picks. */
function arcCentre(start: Pt, path: string): Pt {
  const m = /A([\d.]+),[\d.]+ 0 0 ([01]) ([-\d.]+),([-\d.]+)/.exec(path)!;
  const [r, cw, x, y] = [Number(m[1]), m[2] === "1", Number(m[3]), Number(m[4])];
  const mid = { x: (start.x + x) / 2, y: (start.y + y) / 2 };
  const half = Math.hypot(x - start.x, y - start.y) / 2;
  const h = Math.sqrt(Math.max(r * r - half * half, 0));
  const [dx, dy] = [(x - start.x) / (2 * half), (y - start.y) / (2 * half)];
  // Clockwise on screen (y down) with the small arc: the centre is to the right of travel.
  const s = cw ? 1 : -1;
  return { x: mid.x - dy * h * s, y: mid.y + dx * h * s };
}

// A room below a wall along y = 0 on screen (y down), from x = 0 to 100.
const room: Pt[] = [
  { x: 0, y: 0 },
  { x: 100, y: 0 },
  { x: 100, y: 80 },
  { x: 0, y: 80 },
];

describe("doorSymbol", () => {
  for (const [name, a, b] of [
    ["a→b", { x: 10, y: 0 }, { x: 30, y: 0 }],
    ["b→a", { x: 30, y: 0 }, { x: 10, y: 0 }],
  ] as const) {
    it(`swings around its hinge into the room (${name})`, () => {
      const d = doorSymbol(a, b, room, 0)!;
      // Hinge at the end nearer the room's corner.
      expect(d.hinge).toEqual({ x: 10, y: 0 });
      // The open leaf stands in the room.
      expect(d.leaf[1].y).toBeCloseTo(20);
      // The arc is centred on the hinge, not on the far corner.
      const tip = d.leaf[1];
      const c = arcCentre(tip, d.swing);
      expect(c.x).toBeCloseTo(10);
      expect(c.y).toBeCloseTo(0);
    });
  }

  it("opens into the room above when the room is on the other side", () => {
    const above = room.map((p) => ({ x: p.x, y: -p.y }));
    const d = doorSymbol({ x: 70, y: 0 }, { x: 90, y: 0 }, above, 0)!;
    expect(d.hinge).toEqual({ x: 90, y: 0 });
    expect(d.leaf[1].y).toBeCloseTo(-20);
    const c = arcCentre(d.leaf[1], d.swing);
    expect(c.x).toBeCloseTo(90);
    expect(c.y).toBeCloseTo(0);
  });

  it("hangs in the wall's face on the room's side", () => {
    const d = doorSymbol({ x: 10, y: 0 }, { x: 30, y: 0 }, room, 6)!;
    expect(d.hinge).toEqual({ x: 10, y: 3 });
    expect(d.jambs[0][0].y).toBeCloseTo(-3);
    expect(d.jambs[0][1].y).toBeCloseTo(3);
  });
});
