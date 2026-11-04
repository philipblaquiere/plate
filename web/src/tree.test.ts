import { describe, expect, it } from "vitest";

import { parentsOf, treeRows } from "./tree";

const paths = ["hotel.ncl", "plans/rooms/bed.ncl", "plans/wing.ncl", "annex.ncl"];
const show = (rows: ReturnType<typeof treeRows>) =>
  rows.map((r) => `${"  ".repeat(r.depth)}${r.path}${r.kind === "folder" ? "/" : ""}`);

describe("treeRows", () => {
  it("lists folders before files, at each level", () => {
    expect(show(treeRows(paths, [], new Set()))).toEqual([
      "plans/",
      "  plans/rooms/",
      "    plans/rooms/bed.ncl",
      "  plans/wing.ncl",
      "annex.ncl",
      "hotel.ncl",
    ]);
  });

  it("hides what's inside a collapsed folder, but not the folder", () => {
    const rows = treeRows(paths, [], new Set(["plans/rooms"]));
    expect(show(rows)).toEqual(["plans/", "  plans/rooms/", "  plans/wing.ncl", "annex.ncl", "hotel.ncl"]);
    expect(rows[1]).toMatchObject({ kind: "folder", collapsed: true });
  });

  it("hides nested folders of a collapsed folder too", () => {
    expect(show(treeRows(paths, ["plans/rooms"], new Set(["plans"])))).toEqual([
      "plans/",
      "annex.ncl",
      "hotel.ncl",
    ]);
  });

  it("shows empty folders", () => {
    expect(show(treeRows(["a.ncl"], ["studies", "studies/early"], new Set()))).toEqual([
      "studies/",
      "  studies/early/",
      "a.ncl",
    ]);
  });

  it("leaves pinned files out, and a folder they empty", () => {
    const rows = treeRows(["hotel.ncl", "plates/a.ncl", "plans/b.ncl"], [], new Set(), {
      pinned: new Set(["hotel.ncl", "plates/a.ncl"]),
    });
    expect(show(rows)).toEqual(["plans/", "  plans/b.ncl"]);
  });

  it("filters with every folder open, and no empty folders", () => {
    const rows = treeRows(paths, ["studies"], new Set(["plans", "plans/rooms"]), {
      filter: (p) => p.includes("bed"),
    });
    expect(show(rows)).toEqual(["plans/", "  plans/rooms/", "    plans/rooms/bed.ncl"]);
    expect(rows[0]).toMatchObject({ collapsed: false });
  });
});

describe("parentsOf", () => {
  it("gives the folders a path is in", () => {
    expect(parentsOf("a/b/c.ncl")).toEqual(["a", "a/b"]);
    expect(parentsOf("c.ncl")).toEqual([]);
  });
});
