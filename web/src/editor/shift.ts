import type { SourceSpan, Span } from "../types";

/**
 * Keeping a source map in step with the text.
 *
 * A source map comes from an evaluation of the text as it was then. The text may have changed
 * since (a drag writes it at once; the next evaluation comes later), so spans are shifted through
 * the edits made in between, the way an editor moves its markers. Pure, so it can be tested.
 */

/** One edit, in offsets of the text just before it. */
export interface TextChange {
  start: number;
  end: number;
  text: string;
}

/**
 * Where offset `p` ends up after `changes` (applied in order). A span's start stays after text
 * inserted where it starts, and its end stays before text inserted where it ends; a span whose
 * text is replaced covers the replacement.
 */
export function shiftOffset(p: number, changes: readonly TextChange[], side: "start" | "end"): number {
  for (const c of changes) {
    const delta = c.text.length - (c.end - c.start);
    if (side === "start") {
      if (p >= c.end) p += delta;
      else if (p > c.start) p = c.start;
    } else {
      if (p > c.start && p >= c.end) p += delta;
      else if (p > c.start) p = c.start + c.text.length;
    }
  }
  return p;
}

/** Offsets where each line starts. */
export function lineStarts(text: string): number[] {
  const starts = [0];
  for (let i = 0; i < text.length; i++) if (text.charCodeAt(i) === 10) starts.push(i + 1);
  return starts;
}

/** Offset of a 1-based line and column. */
export function offsetAt(starts: readonly number[], line: number, col: number): number {
  return (starts[Math.min(line, starts.length) - 1] ?? 0) + col - 1;
}

/** 1-based line and column of an offset. */
export function positionAt(starts: readonly number[], offset: number): { line: number; col: number } {
  let lo = 0;
  let hi = starts.length - 1;
  while (lo < hi) {
    const mid = (lo + hi + 1) >> 1;
    if (starts[mid] <= offset) lo = mid;
    else hi = mid - 1;
  }
  return { line: lo + 1, col: offset - starts[lo] + 1 };
}

/** `span`, written in `before`, as it is in `after` once `changes` turned one into the other. */
export function shiftSpan(
  span: Span,
  before: readonly number[],
  after: readonly number[],
  changes: readonly TextChange[],
): Span {
  const start = shiftOffset(offsetAt(before, span.startLine, span.startCol), changes, "start");
  const end = Math.max(start, shiftOffset(offsetAt(before, span.endLine, span.endCol), changes, "end"));
  const a = positionAt(after, start);
  const b = positionAt(after, end);
  return { startLine: a.line, startCol: a.col, endLine: b.line, endCol: b.col };
}

/** The entries of `sources` written in one file, shifted from `before` to `after`. */
export function shiftSources(
  sources: readonly SourceSpan[],
  before: string,
  after: string,
  changes: readonly TextChange[],
): SourceSpan[] {
  if (!changes.length) return [...sources];
  const from = lineStarts(before);
  const to = lineStarts(after);
  return sources.map((s) => ({ ...s, span: shiftSpan(s.span, from, to, changes) }));
}
