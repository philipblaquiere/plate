import type { EvalInput, EvalRequest, EvalResponse, EvalResult } from "./protocol";

export type { EvalInput, EvalResult } from "./protocol";

/**
 * Evaluates configuration files in the browser: Nickel plus the plate schema, compiled to
 * WebAssembly and run in a worker. Only checked configuration data goes on to the solver.
 */
export class ConfigEngine {
  private readonly worker = new Worker(new URL("./worker.ts", import.meta.url), { type: "module" });
  private next = 0;
  private readonly pending = new Map<number, (r: EvalResponse) => void>();

  constructor() {
    this.worker.onmessage = (e: MessageEvent<EvalResponse>) => {
      this.pending.get(e.data.id)?.(e.data);
      this.pending.delete(e.data.id);
    };
  }

  private request(req: { type: "evaluate"; input: EvalInput } | { type: "schema" }): Promise<EvalResponse> {
    const id = ++this.next;
    return new Promise((resolve) => {
      this.pending.set(id, resolve);
      this.worker.postMessage({ ...req, id } as EvalRequest);
    });
  }

  /** Evaluate a plate file, with the files it may import. */
  async evaluate(input: EvalInput): Promise<EvalResult> {
    const r = await this.request({ type: "evaluate", input });
    if (r.type !== "evaluate") throw new Error("unexpected response");
    const { id: _id, type: _type, ...result } = r;
    return result as EvalResult;
  }

  /** The Nickel contract generated from the JSON Schema. */
  async schemaNickel(): Promise<string> {
    const r = await this.request({ type: "schema" });
    return r.type === "schema" ? r.nickel : "";
  }
}
