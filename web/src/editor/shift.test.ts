import { describe, expect, it } from "vitest";
import { lineStarts, offsetAt, positionAt, shiftOffset, shiftSources, type TextChange } from "./shift";
import type { SourceSpan } from "../types";

/** Apply `changes` in order, as the editor does. */
function apply(text: string, changes: TextChange[]): string {
  for (const c of changes) text = text.slice(0, c.start) + c.text + text.slice(c.end);
  return text;
}

/** The source map entry for the `n`th occurrence of `needle` in `text` (one line only). */
function at(text: string, needle: string, pointer: string, n = 0): SourceSpan {
  let i = -1;
  for (let k = 0; k <= n; k++) i = text.indexOf(needle, i + 1);
  const starts = lineStarts(text);
  const a = positionAt(starts, i);
  const b = positionAt(starts, i + needle.length);
  return {
    pointer,
    file: "plate.ncl",
    literal: true,
    kind: "number",
    span: { startLine: a.line, startCol: a.col, endLine: b.line, endCol: b.col },
  };
}

function textOf(text: string, s: SourceSpan): string {
  const starts = lineStarts(text);
  return text.slice(
    offsetAt(starts, s.span.startLine, s.span.startCol),
    offsetAt(starts, s.span.endLine, s.span.endCol),
  );
}

/** A change that replaces the source of `s` in `text` by `by`. */
function replace(text: string, s: SourceSpan, by: string): TextChange {
  const starts = lineStarts(text);
  return {
    start: offsetAt(starts, s.span.startLine, s.span.startCol),
    end: offsetAt(starts, s.span.endLine, s.span.endCol),
    text: by,
  };
}

describe("shifting a source map through edits", () => {
  const before = "{\n  points = [[0, 0], [60, 0], [60, 36], [0, 36]],\n}\n";
  const sources = [
    at(before, "60", "/start/points/1/0", 0),
    at(before, "0", "/start/points/1/1", 3),
    at(before, "60", "/start/points/2/0", 1),
    at(before, "36", "/start/points/2/1", 0),
    at(before, "36", "/start/points/3/1", 1),
  ];

  it("follows an edge moved on a line where every point is written", () => {
    // Move the right edge from x = 60 to x = 64.25: both 60s are rewritten, the line gets longer.
    const changes = [replace(before, sources[2], "64.25"), replace(before, sources[0], "64.25")];
    const after = apply(before, changes);
    expect(after).toContain("[[0, 0], [64.25, 0], [64.25, 36], [0, 36]]");
    const shifted = shiftSources(sources, before, after, changes);
    expect(shifted.map((s) => textOf(after, s))).toEqual(["64.25", "0", "64.25", "36", "36"]);
  });

  it("follows a series of moves made before any evaluation", () => {
    let text = before;
    let current = sources;
    const all: TextChange[] = [];
    for (const [k, value] of [
      [2, "64"],
      [3, "38.5"],
      [0, "7"],
      [4, "1234.5678"],
    ] as const) {
      const c = replace(text, current[k], value);
      all.push(c);
      const next = apply(text, [c]);
      current = shiftSources(current, text, next, [c]);
      text = next;
    }
    expect(text).toContain("[[0, 0], [7, 0], [64, 38.5], [0, 1234.5678]]");
    // All at once, from the original map, gives the same as step by step.
    const once = shiftSources(sources, before, text, all);
    expect(once).toEqual(current);
    expect(once.map((s) => textOf(text, s))).toEqual(["7", "0", "64", "38.5", "1234.5678"]);
  });

  it("keeps a span out of text inserted at its ends", () => {
    const s = at(before, "36", "/start/points/2/1", 0);
    const starts = lineStarts(before);
    const start = offsetAt(starts, s.span.startLine, s.span.startCol);
    const end = offsetAt(starts, s.span.endLine, s.span.endCol);
    expect(shiftOffset(start, [{ start, end: start, text: "x" }], "start")).toBe(start + 1);
    expect(shiftOffset(end, [{ start: end, end, text: "x" }], "end")).toBe(end);
  });

  it("follows lines added above and removed", () => {
    const add: TextChange = { start: 2, end: 2, text: "  # a comment\n" };
    const after = apply(before, [add]);
    const shifted = shiftSources(sources, before, after, [add]);
    expect(shifted.map((s) => s.span.startLine)).toEqual([3, 3, 3, 3, 3]);
    expect(shifted.map((s) => textOf(after, s))).toEqual(["60", "0", "60", "36", "36"]);
    const back = shiftSources(shifted, after, before, [{ start: 2, end: 16, text: "" }]);
    expect(back).toEqual(sources);
  });
});
