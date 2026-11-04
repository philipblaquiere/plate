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

// ------------------------------------------------------------------ projects

export interface ProjectInfo {
  name: string;
  files: number;
  plates: number;
  /** The plate to show when nothing says otherwise. */
  main: string | null;
  /** When a file in it last changed (milliseconds since the epoch). */
  modified: number;
}

export interface ExampleInfo {
  name: string;
  title: string;
  blurb: string;
  main: string;
  files: number;
}

export interface ProjectListing {
  root: string;
  projects: ProjectInfo[];
  examples: ExampleInfo[];
}

/** A project's files, all at once. */
export interface ProjectContents {
  root: string;
  files: { path: string; text: string }[];
  folders: string[];
}

const encodePath = (path: string) => path.split("/").map(encodeURIComponent).join("/");
const projectUrl = (name: string) => `/projects/${encodeURIComponent(name)}`;

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

const postJson = (path: string, body: unknown) =>
  call(path, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(body),
  });

export async function listProjects(): Promise<ProjectListing> {
  return files<ProjectListing>(await call("/projects"));
}

export async function readProject(name: string): Promise<ProjectContents> {
  return files<ProjectContents>(await call(projectUrl(name)));
}

export async function createProject(name: string): Promise<{ name: string }> {
  return files(await postJson("/projects", { name }));
}

/** Copy an example into the workspace, or find the copy made before. */
export async function openExample(example: string): Promise<{ name: string; main: string }> {
  return files(await postJson("/projects", { example }));
}

export async function moveProject(name: string, to: string): Promise<void> {
  await files(await postJson(`${projectUrl(name)}/move`, { to }));
}

export async function duplicateProject(name: string, to: string): Promise<{ name: string }> {
  return files(await postJson(`${projectUrl(name)}/duplicate`, { to }));
}

/** To the trash: what to restore it by. */
export async function trashProject(name: string): Promise<{ trash: string }> {
  return files(await call(projectUrl(name), { method: "DELETE" }));
}

export async function restoreProject(id: string): Promise<{ name: string }> {
  return files(await postJson(`/trash/${encodeURIComponent(id)}/restore`, {}));
}

export async function importProject(name: string, zip: Blob): Promise<{ name: string; main: string | null }> {
  return files(
    await call(`/projects/import?name=${encodeURIComponent(name)}`, {
      method: "POST",
      headers: { "content-type": "application/zip" },
      body: zip,
    }),
  );
}

/** Addresses the browser can load directly. */
export const projectExportUrl = (name: string) => `/api${projectUrl(name)}/export`;
export const projectThumbnailUrl = (name: string, main?: string | null) =>
  `/api${projectUrl(name)}/thumbnail${main ? `?main=${encodeURIComponent(main)}` : ""}`;
export const exampleThumbnailUrl = (name: string) => `/api/examples/${encodeURIComponent(name)}/thumbnail`;

// ------------------------------------------------------------------ files in a project

export async function writeFile(project: string, path: string, text: string): Promise<void> {
  await files(
    await call(`${projectUrl(project)}/files/${encodePath(path)}`, {
      method: "PUT",
      headers: { "content-type": "text/plain; charset=utf-8" },
      body: text,
    }),
  );
}

export async function deleteFile(project: string, path: string): Promise<void> {
  await files(await call(`${projectUrl(project)}/files/${encodePath(path)}`, { method: "DELETE" }));
}

export async function renameFile(project: string, from: string, to: string): Promise<void> {
  await files(await postJson(`${projectUrl(project)}/rename`, { from, to }));
}

/** Make a folder in a project (empty to start with). */
export async function makeFolder(project: string, path: string): Promise<void> {
  await files(await postJson(`${projectUrl(project)}/folders`, { path }));
}

/** Remove a folder with no files in it. */
export async function removeFolder(project: string, path: string): Promise<void> {
  await files(await call(`${projectUrl(project)}/folders/${encodePath(path)}`, { method: "DELETE" }));
}
