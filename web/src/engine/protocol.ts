import type { Diagnostic, SourceSpan } from "../types";

/** What to evaluate: the plate file and every file it may import, by workspace path. */
export interface EvalInput {
  main: string;
  files: Record<string, string>;
}

export type EvalRequest = { id: number; type: "evaluate"; input: EvalInput } | { id: number; type: "schema" };

/** What a file is: something to draw (a plate, a plan), or a library to import (a service, a
 * set of assemblies). */
export type FileKind = "plate" | "plan" | "service" | "assemblies";

export type EvalResult =
  | { ok: true; config: Record<string, unknown>; sources: SourceSpan[]; kind?: FileKind; ms: number }
  | { ok: false; diagnostics: Diagnostic[]; ms: number };

export type EvalResponse =
  ({ id: number; type: "evaluate" } & EvalResult) | { id: number; type: "schema"; nickel: string };
