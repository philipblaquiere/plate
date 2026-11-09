// Look at the app without a person: open a plate in a real browser, wait for its solve, take a
// screenshot of each view (Plan, then each service), and report the verdict and any console
// errors. Starts its own API and Vite on spare ports, on a throwaway copy of the project, so it
// never touches ./workspace or a dev server you have running.
//
//   npm run shot -- fixtures/pinned-flat.ncl
//   npm run shot -- examples/duplex/duplex.ncl --views Plan,Power --width 1600
//   npm run shot -- examples/home/plans/kitchen.ncl --project examples/home
//   npm run shot -- --svg out.svg                     # an exported SVG as a PNG
//
// Options:
//   --project <dir>   the folder copied as the project (default: examples/<name> or the file's folder)
//   --views <list>    view chips to shoot, by label, comma-separated (default: all)
//   --out <dir>       where the PNGs go (default: shots/)
//   --width/--height  the window (default 1440 × 900)
//   --strict          exit 1 on console errors or a verdict that isn't perfect
//
// Needs the wasm engine (`npm run build:wasm`) and a Chrome or Chromium: PLATE_CHROME, else
// one found on the system, else Playwright's own (`npx playwright install chromium`).
import { spawn, execFileSync } from "node:child_process";
import { cpSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { basename, dirname, join, relative, resolve } from "node:path";
import { chromium } from "playwright-core";

const root = resolve(new URL("..", import.meta.url).pathname);
const args = process.argv.slice(2);
const opt = (name, fallback) => {
  const i = args.indexOf(`--${name}`);
  return i >= 0 ? args[i + 1] : fallback;
};
const flag = (name) => args.includes(`--${name}`);
const width = Number(opt("width", 1440));
const height = Number(opt("height", 900));
const out = resolve(opt("out", join(root, "shots")));
mkdirSync(out, { recursive: true });

function chrome() {
  const found = [
    process.env.PLATE_CHROME,
    "/usr/bin/google-chrome",
    "/usr/bin/chromium",
    "/usr/bin/chromium-browser",
    "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
    "C:/Program Files/Google/Chrome/Application/chrome.exe",
  ].find((p) => p && existsSync(p));
  return found ? { executablePath: found } : {};
}

const browser = await chromium.launch({ ...chrome(), args: ["--no-sandbox"] });
const page = await browser.newPage({ viewport: { width, height }, deviceScaleFactor: 1 });

// An SVG on its own: render it and stop.
if (opt("svg")) {
  const file = resolve(opt("svg"));
  await page.setContent(`<body style="margin:0;background:#fff">${readFileSync(file, "utf8")}</body>`);
  const png = join(out, basename(file).replace(/\.svg$/, "") + ".png");
  await page.locator("svg").first().screenshot({ path: png });
  console.log(`wrote ${png}`);
  await browser.close();
  process.exit(0);
}

const target = args.find((a, i) => !a.startsWith("--") && !(i > 0 && args[i - 1].startsWith("--")));
if (!target) {
  console.error(
    "usage: npm run shot -- <file.ncl> [--project dir] [--views Plan,Power] [--out dir] [--strict]",
  );
  process.exit(2);
}
const file = resolve(target);
const inExamples = relative(join(root, "examples"), file).split("/");
const projectDir = resolve(
  opt(
    "project",
    inExamples.length > 1 && inExamples[0] !== ".." ? join(root, "examples", inExamples[0]) : dirname(file),
  ),
);
const project = basename(projectDir);
const path = relative(projectDir, file);
if (path.startsWith("..")) throw new Error(`${target} isn't inside ${projectDir}`);

// A throwaway workspace with just this project.
const workspace = mkdtempSync(join(tmpdir(), "plate-shot-"));
cpSync(projectDir, join(workspace, project), { recursive: true, filter: (p) => !p.includes("node_modules") });

if (!existsSync(join(root, "web/src/engine/generated/plate_engine.wasm"))) {
  console.error("No wasm engine yet: building it (npm run build:wasm, ~2 min the first time)…");
  execFileSync("node", ["scripts/build-wasm.mjs"], { cwd: root, stdio: "inherit" });
}
execFileSync("cargo", ["build", "-q", "-p", "plate-server", "--bin", "plate"], {
  cwd: root,
  stdio: "inherit",
});

const apiPort = 7900 + Math.floor(Math.random() * 90);
const webPort = apiPort + 100;
const children = [];
const start = (cmd, argv, env = {}) => {
  const child = spawn(cmd, argv, {
    cwd: root,
    env: { ...process.env, ...env },
    stdio: ["ignore", "pipe", "pipe"],
  });
  let log = "";
  child.stdout.on("data", (d) => (log += d));
  child.stderr.on("data", (d) => (log += d));
  children.push({ child, log: () => log });
  return child;
};
const stop = () => children.forEach(({ child }) => child.kill());
process.on("exit", () => {
  stop();
  rmSync(workspace, { recursive: true, force: true });
});

start(join(root, "target/debug/plate"), [
  "serve",
  "--port",
  String(apiPort),
  "--web",
  "none",
  "--workspace",
  workspace,
]);
start(join(root, "node_modules/.bin/vite"), ["--port", String(webPort), "--strictPort"], {
  PLATE_API: `http://127.0.0.1:${apiPort}`,
});

async function up(url, seconds = 60) {
  for (let i = 0; i < seconds * 4; i++) {
    try {
      const r = await fetch(url);
      if (r.ok) return;
    } catch {}
    await new Promise((r) => setTimeout(r, 250));
  }
  for (const c of children) console.error(c.log());
  throw new Error(`${url} didn't come up`);
}
await up(`http://127.0.0.1:${apiPort}/api/projects`);
await up(`http://127.0.0.1:${webPort}/`);

const problems = [];
page.on("console", (m) => m.type() === "error" && problems.push(`console: ${m.text()}`));
page.on("pageerror", (e) => problems.push(`page: ${e.message}`));

const route = "#/" + [project, ...path.split("/")].map(encodeURIComponent).join("/");
await page.goto(`http://127.0.0.1:${webPort}/${route}`);
// The start screen opens on every launch: continue with the plate the address names.
const cont = page.locator("#start-continue");
await cont.waitFor({ state: "visible", timeout: 30_000 }).catch(() => {});
if (await cont.isVisible().catch(() => false)) await cont.click();
else await page.keyboard.press("Escape");
// Solved: the verdict is shown and the drawing has spaces (or the status says why not).
try {
  await page.waitForFunction(
    () => {
      const v = document.getElementById("verdict");
      const s = document.getElementById("status")?.dataset.state;
      return (v && !v.hidden && document.querySelector("svg.sheet path.space")) || s === "error";
    },
    null,
    { timeout: 120_000 },
  );
} catch {
  problems.push("timed out waiting for a solve");
}
await page.waitForTimeout(400); // the fit animation

const verdict =
  (await page
    .locator("#verdict .verdict-text")
    .textContent()
    .catch(() => "")) ?? "";
const verdictStatus = await page
  .locator("#verdict")
  .getAttribute("data-status")
  .catch(() => null);
const status = await page
  .locator("#status")
  .getAttribute("data-state")
  .catch(() => null);
const chips = await page.locator("#views-bar .view-chip").allTextContents();
const labels = chips.length ? chips.map((c) => c.replace(/\d+$/, "").trim()) : ["Plan"];
const wanted =
  opt("views", "all") === "all"
    ? labels
    : opt("views")
        .split(",")
        .map((s) => s.trim());
const stem = `${project}-${path.replace(/\.ncl$/, "").replaceAll("/", "-")}`;
const written = [];
for (const label of wanted) {
  const i = labels.findIndex((l) => l.toLowerCase() === label.toLowerCase());
  if (i < 0) {
    problems.push(`no view "${label}" (there are: ${labels.join(", ")})`);
    continue;
  }
  // A plate with no services has a hidden views bar: its plan is all there is to see.
  const chip = page.locator("#views-bar .view-chip").nth(i);
  if (chips.length && (await chip.isVisible())) {
    await chip.click();
    await page.waitForTimeout(150);
  }
  const png = join(out, `${stem}-${labels[i].toLowerCase().replace(/\W+/g, "-")}.png`);
  await page.screenshot({ path: png });
  written.push(relative(process.cwd(), png));
}
await browser.close();

console.log(`${project}/${path}: ${verdict.trim() || "(no verdict)"}${status ? ` [status: ${status}]` : ""}`);
for (const w of written) console.log(`wrote ${w}`);
for (const p of problems) console.log(`! ${p}`);
const perfect = verdictStatus === "perfect";
process.exitCode = flag("strict") && (problems.length || !perfect) ? 1 : 0;
stop();
