// Builds crates/plate-wasm for the browser and copies it where the web app loads it from.
// Run by `npm run dev` and `npm run build`; also `npm run build:wasm` on its own.
import { execFileSync, spawnSync } from "node:child_process";
import { copyFileSync, mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
// WASI, not wasm32-unknown-unknown: Nickel needs absolute paths to resolve imports, and only
// WASI has them. The browser provides a stub WASI (web/src/engine/wasi.ts).
const target = "wasm32-wasip1";
const run = (cmd, args) => execFileSync(cmd, args, { cwd: root, stdio: "inherit", shell: process.platform === "win32" });

// Make sure the WebAssembly target is installed (no-op when it already is).
const rustup = spawnSync("rustup", ["target", "list", "--installed"], { encoding: "utf8", shell: process.platform === "win32" });
if (rustup.status === 0 && !rustup.stdout.includes(target)) {
  console.log(`Installing the ${target} Rust target…`);
  run("rustup", ["target", "add", target]);
}

run("cargo", ["build", "-p", "plate-wasm", "--target", target, "--profile", "wasm"]);

const out = join(root, "web", "src", "engine", "generated");
mkdirSync(out, { recursive: true });
copyFileSync(join(root, "target", target, "wasm", "plate_wasm.wasm"), join(out, "plate_engine.wasm"));
console.log("Built web/src/engine/generated/plate_engine.wasm");
