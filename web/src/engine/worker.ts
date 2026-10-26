/// <reference lib="webworker" />
// Runs the configuration evaluator (Nickel + the plate schema, compiled to WebAssembly) off the
// main thread, so typing stays smooth while files are evaluated.

import wasmUrl from "./generated/plate_engine.wasm?url";
import type { EvalRequest, EvalResponse } from "./protocol";
import { wasiStubs } from "./wasi";

interface Exports {
  memory: WebAssembly.Memory;
  plate_alloc(len: number): number;
  plate_free(ptr: number, len: number): void;
  plate_evaluate(ptr: number, len: number): number;
  plate_schema_nickel(): number;
}

const encoder = new TextEncoder();
const decoder = new TextDecoder();
let module: Promise<WebAssembly.Module> | null = null;
let exports: Exports | null = null;

async function instance(): Promise<Exports> {
  if (exports) return exports;
  module ??= WebAssembly.compileStreaming(fetch(wasmUrl));
  let memory: WebAssembly.Memory | null = null;
  const inst = await WebAssembly.instantiate(
    await module,
    wasiStubs(() => memory!),
  );
  exports = inst.exports as unknown as Exports;
  memory = exports.memory;
  return exports;
}

/** Read a result buffer (u32 little-endian length, then UTF-8) and free it. */
function take(x: Exports, ptr: number): string {
  const len = new DataView(x.memory.buffer).getUint32(ptr, true);
  const text = decoder.decode(new Uint8Array(x.memory.buffer, ptr + 4, len));
  x.plate_free(ptr, len + 4);
  return text;
}

function evaluate(x: Exports, source: string): string {
  const bytes = encoder.encode(source);
  const ptr = x.plate_alloc(bytes.length);
  new Uint8Array(x.memory.buffer, ptr, bytes.length).set(bytes);
  try {
    return take(x, x.plate_evaluate(ptr, bytes.length));
  } finally {
    x.plate_free(ptr, bytes.length);
  }
}

self.onmessage = async (event: MessageEvent<EvalRequest>) => {
  const request = event.data;
  let started = performance.now();
  let response: EvalResponse;
  try {
    const x = await instance();
    started = performance.now(); // time the evaluation, not the one-off module download
    if (request.type === "schema") {
      response = { id: request.id, type: "schema", nickel: take(x, x.plate_schema_nickel()) };
    } else {
      const result = JSON.parse(evaluate(x, JSON.stringify(request.input)));
      response = { id: request.id, type: "evaluate", ms: performance.now() - started, ...result };
    }
  } catch (err) {
    // A trap (e.g. a panic) leaves the instance unusable: start a fresh one next time.
    exports = null;
    response = {
      id: request.id,
      type: "evaluate",
      ms: performance.now() - started,
      ok: false,
      diagnostics: [
        { severity: "error", message: `The configuration engine crashed: ${(err as Error).message}` },
      ],
    };
  }
  self.postMessage(response);
};
