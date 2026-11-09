// @vitest-environment happy-dom
// A room the outline cuts (fixtures/pinned-angled.ncl): the bedroom is pinned up into the corner
// cut at an angle, so its floor is its 4 × 4.5 m rectangle less that corner.
import { describe, expect, it } from "vitest";
import angled from "../fixtures/pinned-angled.json";
import flat from "../fixtures/pinned-flat.json";
import { mount } from "../test/viewer";
import type { Element, Layout, Point } from "../types";

/** The viewer's own hover state and cursor, to show a card without a mouse. */
interface Inside {
  hover: { type: string; element: Element } | null;
  screenCursor: Point | null;
  toScreen(p: Point): Point;
}

/** The text of the card shown when `id` is hovered. */
function card(layout: Layout, id: string): string {
  const { viewer, render } = mount(layout);
  const v = viewer as unknown as Inside;
  const element = layout.elements.find((e) => e.id === id && e.space)!;
  v.hover = { type: "space", element };
  v.screenCursor = v.toScreen(element.points[0]);
  render();
  // Lengths and areas are written with non-breaking spaces.
  return (document.querySelector(".tip")?.textContent ?? "").replace(/\s/g, " ");
}

describe("a room cut by the outline", () => {
  const layout = angled as unknown as Layout;

  it("knows the rectangle it was placed as", () => {
    const bedroom = layout.elements.find((e) => e.id === "bedroom")!;
    expect(bedroom.space!.extent).toEqual([4, 4.5]);
    // Its floor to the walls' centres: 18 m² less the 4.5 m² corner.
    expect(bedroom.space!.gross).toBeCloseTo(13.5);
    const living = layout.elements.find((e) => e.id === "living")!;
    expect(living.space!.extent).toBeUndefined();
  });

  it("shows its rectangle and how much of it it kept", () => {
    const text = card(layout, "bedroom");
    expect(text).toContain("4.00 m × 4.50 m, cut by the outline");
    expect(text).toContain("13.50 m² of its 18.00 m² rectangle (75 %)");
  });

  it("says nothing of the kind for a room the outline doesn't cut", () => {
    expect(card(flat as unknown as Layout, "bedroom")).not.toContain("cut by the outline");
  });
});
