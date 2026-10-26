import type { Diagnostic, ExportFormat, PreviewView, SolveResponse } from "./types";

export class ApiUnavailable extends Error {}

/** Configuration data already evaluated from Nickel in the browser. */
export type ConfigData = Record<string, unknown>;

async function post(path: string, body: unknown, signal?: AbortSignal): Promise<Response> {
  try {
    return await fetch(`/api${path}`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(body),
      signal,
    });
  } catch (err) {
    if ((err as Error).name === "AbortError") throw err;
    throw new ApiUnavailable("Waiting for the plate engine. Start it with `npm run dev`.");
  }
}

async function json<T>(res: Response): Promise<T> {
  // The Vite proxy answers with a non-JSON error page while the engine is down or compiling.
  if (!res.headers.get("content-type")?.includes("application/json")) {
    throw new ApiUnavailable("Waiting for the plate engine. The first build takes a few minutes.");
  }
  return (await res.json()) as T;
}

/**
 * Solve evaluated configuration data. The server validates it again before solving. `view` is
 * what a plan file is shown in (plates ignore it).
 */
export async function solve(
  config: ConfigData,
  signal?: AbortSignal,
  view: PreviewView = "default",
): Promise<SolveResponse> {
  return json<SolveResponse>(await post("/solve", { config, view }, signal));
}

export async function format(
  source: string,
): Promise<{ ok: true; source: string } | { ok: false; diagnostics: Diagnostic[] }> {
  return json(await post("/format", { source }));
}

/** Returns the exported file, or the diagnostics explaining why it couldn't be produced. */
export async function exportLayout(
  config: ConfigData,
  fmt: ExportFormat,
  view: PreviewView = "default",
): Promise<{ ok: true; blob: Blob; filename: string } | { ok: false; diagnostics: Diagnostic[] }> {
  const res = await post(`/export/${fmt}`, { config, view });
  if (!res.ok) return json(res);
  const disposition = res.headers.get("content-disposition") ?? "";
  const filename = /filename="([^"]+)"/.exec(disposition)?.[1] ?? `plate.${fmt}`;
  return { ok: true, blob: await res.blob(), filename };
}

// ------------------------------------------------------------------ workspace files

export interface WorkspaceListing {
  root: string;
  files: { path: string; size: number }[];
  /** Every folder, empty ones included (older engines leave it out). */
  folders?: string[];
}

const encodePath = (path: string) => path.split("/").map(encodeURIComponent).join("/");

async function files<T>(res: Response): Promise<T> {
  const body = (await res.json().catch(() => null)) as ({ ok: boolean; error?: string } & T) | null;
  if (!body) throw new ApiUnavailable("Waiting for the plate engine.");
  if (!res.ok || body.ok === false) throw new Error(body.error ?? `request failed (${res.status})`);
  return body;
}

async function call(path: string, init?: RequestInit): Promise<Response> {
  try {
    return await fetch(`/api${path}`, init);
  } catch {
    throw new ApiUnavailable("Waiting for the plate engine. Start it with `npm run dev`.");
  }
}

/** Every .ncl file in the workspace folder. */
export async function listFiles(): Promise<WorkspaceListing> {
  return files<WorkspaceListing>(await call("/files"));
}

export async function readFile(path: string): Promise<string> {
  const res = await call(`/files/${encodePath(path)}`);
  if (!res.ok) throw new Error((await res.json().catch(() => ({ error: res.statusText }))).error);
  return res.text();
}

export async function writeFile(path: string, text: string): Promise<void> {
  await files(
    await call(`/files/${encodePath(path)}`, {
      method: "PUT",
      headers: { "content-type": "text/plain; charset=utf-8" },
      body: text,
    }),
  );
}

export async function deleteFile(path: string): Promise<void> {
  await files(await call(`/files/${encodePath(path)}`, { method: "DELETE" }));
}

export async function renameFile(from: string, to: string): Promise<void> {
  await files(
    await call("/rename", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ from, to }),
    }),
  );
}

/** Make a folder in the workspace (empty to start with). */
export async function makeFolder(path: string): Promise<void> {
  await files(
    await call("/folders", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ path }),
    }),
  );
}

/** Remove a folder with no files in it. */
export async function removeFolder(path: string): Promise<void> {
  await files(await call(`/folders/${encodePath(path)}`, { method: "DELETE" }));
}
