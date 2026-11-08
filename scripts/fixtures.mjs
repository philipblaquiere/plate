// Solve every plate in fixtures/ and write its layout to web/src/fixtures/<name>.json, for the
// web tests (viewer.test.ts and friends). The fixtures are pinned, so the layouts only change
// when what's built on them changes (walls, parts, networks, the Layout format).
//
//   npm run fixtures            # all of them
//   npm run fixtures -- flat    # only fixtures whose name contains "flat"
import { execFileSync } from "node:child_process";
import { mkdirSync, readdirSync } from "node:fs";
import { join } from "node:path";

const root = new URL("..", import.meta.url).pathname;
const only = process.argv[2] ?? "";
const out = join(root, "web/src/fixtures");
mkdirSync(out, { recursive: true });

execFileSync("cargo", ["build", "-q", "-p", "plate-server", "--bin", "plate"], {
  cwd: root,
  stdio: "inherit",
});
const plate = join(root, "target/debug/plate");

const files = readdirSync(join(root, "fixtures")).filter((f) => f.endsWith(".ncl") && f.includes(only));
for (const file of files) {
  const name = file.replace(/\.ncl$/, "");
  const target = join(out, `${name}.json`);
  execFileSync(plate, ["export", join("fixtures", file), "json", "-o", target], {
    cwd: root,
    stdio: "inherit",
  });
}
if (files.length === 0) console.error(`no fixture matches "${only}"`);
