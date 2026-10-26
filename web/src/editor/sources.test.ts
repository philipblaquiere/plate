import { describe, expect, it } from "vitest";
import { rulesOf, spanFor, valueAt } from "./sources";
import type { SourceSpan } from "../types";

const span = (startLine: number, startCol: number, endCol: number) => ({
  startLine,
  startCol,
  endLine: startLine,
  endCol,
});
const src = (pointer: string, s: ReturnType<typeof span>, file = "main.ncl"): SourceSpan => ({
  pointer,
  file,
  span: s,
  kind: "other",
  literal: true,
});

describe("source lookups", () => {
  const sources = [
    src("/rules", span(3, 11, 40)),
    src("/rules/0/a", span(4, 17, 25)),
    src("/rules/0/b", span(4, 26, 32)),
    src("/spaces/kitchen", span(2, 13, 30)),
    src("/spaces/room/plan/spaces/bed", span(5, 3, 20), "room.ncl"),
    src("/spaces/room/plan/rules/0/a", span(9, 3, 9), "room.ncl"),
    src("/spaces/room/area", span(7, 9, 12)),
  ];

  it("prefers the value, then what's inside it, then what encloses it", () => {
    const at = (s: ReturnType<typeof span>, file = "main.ncl") => ({ file, span: s });
    expect(spanFor("/spaces/kitchen", sources)).toEqual(at(span(2, 13, 30)));
    expect(spanFor("/rules/0", sources)).toEqual(at(span(4, 17, 32)));
    expect(spanFor("/rules/5", sources)).toEqual(at(span(3, 11, 40)));
  });

  it("says which file a value is written in, and doesn't mix files", () => {
    expect(spanFor("/spaces/room/plan/rules/0", sources)).toEqual({
      file: "room.ncl",
      span: span(9, 3, 9),
    });
    // Inside /spaces/room: values from room.ncl (written first) only.
    expect(spanFor("/spaces/room", sources)?.file).toBe("room.ncl");
  });

  it("reads values and rules by pointer", () => {
    const config = {
      spaces: { room: { plan: { rules: [{ rule: "unique", a: "x" }, [{ rule: "touch" }]] } } },
    };
    const plan = valueAt(config, "/spaces/room/plan");
    expect(rulesOf(plan).map((r) => r.pointer)).toEqual(["/rules/0", "/rules/1/0"]);
  });
});
