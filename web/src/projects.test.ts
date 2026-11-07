import { describe, expect, it } from "vitest";

import {
  byRecent,
  cleanProjectName,
  formatRoute,
  parseRoute,
  planCopy,
  pushRecent,
  timeAgo,
} from "./projects";

describe("routes", () => {
  it("round-trips a project and a file", () => {
    const route = { project: "my flats", path: "plans/room one.ncl" };
    expect(formatRoute(route)).toBe("#/my%20flats/plans/room%20one.ncl");
    expect(parseRoute(formatRoute(route))).toEqual(route);
  });

  it("reads a project on its own", () => {
    expect(parseRoute("#/home")).toEqual({ project: "home", path: null });
    expect(formatRoute({ project: "home", path: null })).toBe("#/home");
  });

  it("gives nothing for no route or a broken one", () => {
    expect(parseRoute("")).toBeNull();
    expect(parseRoute("#/")).toBeNull();
    expect(parseRoute("#/%E0%A4%A")).toBeNull();
  });
});

describe("recents", () => {
  const list = [{ name: "a" }, { name: "b" }, { name: "c" }];

  it("lists recent projects first, then the rest by name", () => {
    expect(byRecent(list, ["c", "b"]).map((p) => p.name)).toEqual(["c", "b", "a"]);
    expect(byRecent(list, []).map((p) => p.name)).toEqual(["a", "b", "c"]);
  });

  it("moves a project to the front without repeating it", () => {
    expect(pushRecent(["a", "b"], "b")).toEqual(["b", "a"]);
    expect(pushRecent(["a", "b"], "c", 2)).toEqual(["c", "a"]);
  });
});

describe("cleanProjectName", () => {
  it("keeps what a folder name can hold", () => {
    expect(cleanProjectName("  Riverside flats (2) ")).toBe("Riverside flats (2)");
    expect(cleanProjectName("Résidence été")).toBe("Résidence été");
  });

  it("turns what it can't hold into dashes", () => {
    expect(cleanProjectName("a/b\\c:d")).toBe("a-b-c-d");
  });

  it("can't make a hidden or relative name", () => {
    expect(cleanProjectName("../..")).toBe("");
    expect(cleanProjectName(".hidden")).toBe("hidden");
    expect(cleanProjectName("trailing.")).toBe("trailing");
  });
});

describe("timeAgo", () => {
  const now = 1_000_000_000_000;
  it("reads like a person would say it", () => {
    expect(timeAgo(now - 10_000, now)).toBe("just now");
    expect(timeAgo(now - 5 * 60_000, now)).toBe("5 min ago");
    expect(timeAgo(now - 3 * 3_600_000, now)).toBe("3 h ago");
    expect(timeAgo(now - 2 * 86_400_000, now)).toBe("2 d ago");
    expect(timeAgo(0, now)).toBe("");
  });
});

describe("planCopy", () => {
  const files = {
    "hotel.ncl": 'let w = import "plans/wing.ncl" in { start = 1 }',
    "plans/wing.ncl": 'let r = import "room.ncl" in {}',
    "plans/room.ncl": "{ a = 1 }",
    "other.ncl": "{}",
  };

  it("brings what the file imports, directly or not, and nothing else", () => {
    const plan = planCopy("plans/wing.ncl", files, {});
    expect(plan.write.map((f) => f.path)).toEqual(["plans/room.ncl", "plans/wing.ncl"]);
  });

  it("skips what's there already, and leaves different files alone", () => {
    const plan = planCopy("hotel.ncl", files, {
      "plans/room.ncl": "{ a = 1 }",
      "plans/wing.ncl": "{ changed = true }",
    });
    expect(plan.write.map((f) => f.path)).toEqual(["hotel.ncl"]);
    expect(plan.same).toEqual(["plans/room.ncl"]);
    expect(plan.clashes).toEqual(["plans/wing.ncl"]);
  });
});
