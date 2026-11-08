import { describe, expect, it } from "vitest";
import { edgeActions, namesOnly, ruleText } from "./edges";
import type { EdgeInfo } from "./types";

const edge = (over: Partial<EdgeInfo>): EdgeInfo => ({
  between: ["living", "dining"],
  treatment: "open",
  why: "living is a zone and dining is a zone: open floor",
  default: "open",
  plan: "",
  names: ["living", "dining"],
  ...over,
});

describe("ruleText", () => {
  it("writes the helper for each treatment", () => {
    expect(ruleText("open", "a", "b")).toBe('plate.open "a" "b"');
    expect(ruleText("opening", "a", "b")).toBe('plate.opening "a" "b"');
    expect(ruleText("door", "a", "b")).toBe('plate.door "a" "b"');
    expect(ruleText("wall", "a", "b")).toBe('plate.wall "a" "b"');
  });
});

describe("namesOnly", () => {
  it("is true for a rule naming exactly the two spaces, either way round", () => {
    expect(namesOnly({ rule: "door", a: "dining", b: "living" }, ["living", "dining"])).toBe(true);
    expect(namesOnly({ rule: "door", a: ["living"], b: "dining" }, ["living", "dining"])).toBe(true);
  });
  it("is false for a rule that covers more", () => {
    expect(namesOnly({ rule: "door", a: ["bedroom", "bath"], b: "living" }, ["bath", "living"])).toBe(false);
    expect(namesOnly({ rule: "door", a: "#wet", b: "living" }, ["bath", "living"])).toBe(false);
  });
});

describe("edgeActions", () => {
  it("adds a rule where none decides, and builds a wall for a door on open floor", () => {
    const actions = edgeActions(edge({}), null);
    expect(actions.map((a) => a.label)).toEqual(["Add an opening", "Add a door", "Build a wall"]);
    const door = actions[1];
    expect("add" in door && door.add).toEqual([
      'plate.wall "living" "dining"',
      'plate.door "living" "dining"',
    ]);
  });

  it("adds just the door where the two are walled anyway", () => {
    const actions = edgeActions(edge({ treatment: "wall", default: "wall", names: ["bath", "hall"] }), null);
    expect(actions.map((a) => ("add" in a ? a.add : null))).toEqual([
      ['plate.open "bath" "hall"'],
      ['plate.opening "bath" "hall"'],
      ['plate.door "bath" "hall"'],
    ]);
  });

  it("changes or removes a rule that names only these two", () => {
    const e = edge({ treatment: "opening", rule: "/rules/5", default: "wall", names: ["nook", "living"] });
    const actions = edgeActions(e, { rule: "door", a: "nook", b: "living" });
    expect(actions.map((a) => a.label)).toEqual([
      "Make open",
      "Add a door",
      "Build a wall",
      "Remove the rule",
    ]);
    expect(actions[0]).toMatchObject({ replace: "/rules/5", text: 'plate.open "nook" "living"' });
    expect(actions[3]).toMatchObject({ remove: "/rules/5" });
  });

  it("doesn't offer a door in place of a rule where the two would have no wall", () => {
    const e = edge({ rule: "/rules/3" });
    const labels = edgeActions(e, { rule: "door", a: "dining", b: "living" }).map((a) => a.label);
    expect(labels).not.toContain("Add a door");
  });

  it("leaves a rule that covers more spaces alone", () => {
    const e = edge({ treatment: "door", rule: "/rules/6", default: "wall", names: ["bath", "living"] });
    expect(edgeActions(e, { rule: "door", a: ["bedroom", "bath"], b: "living" })).toEqual([]);
  });
});
