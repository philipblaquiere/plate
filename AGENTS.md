# AGENTS.md

Notes for coding agents working in this repo. Read [README.md](README.md) for an overview and [docs/GUIDE.md](docs/GUIDE.md) for what the app does and
[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for how it fits together; this file is the short list
of commands, rules and traps.

## What this is

plate: floor plates written as Nickel (`.ncl`) configuration, solved into 2D layouts, exported to
DXF/SVG/JSON. A Monaco editor with tabs and a side bar on the left (files, or a reference that
follows the cursor), a live SVG drawing on the right.

- `crates/plate-core`: config types, units, geometry, the solver (host + `grid` / `spine_stack`
  engines), `Layout`. No I/O, no Nickel.
- `crates/plate-config`: JSON Schema → Nickel contract, multi-file Nickel evaluation, source map,
  error positions, formatting.
- `crates/plate-wasm`: `plate-config` for the browser (`wasm32-wasip1`, tiny hand-written ABI).
- `crates/plate-export`: DXF, SVG, JSON writers.
- `crates/plate-server`: Axum API, workspace folder API, and the `plate` CLI.
- `schema/plate.schema.json`: the configuration format. `schema/plate.ncl`: the `plate.*`
  helper library every file can use.
- `web/`: Vite + TypeScript + Monaco. No framework.
- `examples/`: three example projects at most, one folder each, closed under imports. `EXAMPLES`
  in `plate-server/src/projects.rs` lists every file (a test checks it); they're copied into the
  workspace when first opened. Every plate and preview must solve with nothing unmet and no floor
  left over at the seed it ships with, and between them they use every schema field and `plate.*`
  helper (`the_examples_use_every_feature`): a new feature goes in an example, or on its exempt
  list with a reason.

## Commands

```sh
npm install
npm run dev            # builds the wasm engine, then API on :7878 + Vite on :5173
npm test               # cargo test + wasm build + tsc + vitest (all of it)
npm run test:unit      # plate-core without annealing: ~0.5 s warm
npm run test:core      # all of plate-core: ~15 s
npm run test:examples  # plate-config: every example file, snapshots, fixtures: ~12 s
npm run test:rust      # cargo test --workspace
npm run test:web       # vitest only
npm run test:wasm      # the browser engine builds (npm run build:wasm)
npm run typecheck      # tsc only
npm run fixtures       # re-solve fixtures/*.ncl into web/src/fixtures/*.json
npm run shot -- <file.ncl> [--views Plan,Power] [--strict]   # screenshots of the app, see below
npm run build:wasm     # rebuild the browser engine after changing plate-config or the schema
npm run format         # prettier + rustfmt
cargo clippy --workspace --all-targets
```

The `plate` CLI (`cargo run -p plate-server --bin plate -- …`, or `target/debug/plate` once built):

```sh
plate check f.ncl [-v] [--seed N]       # solve; prints status, counts, timings and moves
plate check f.ncl --json                # the same as JSON: report, counts, spaces, diagnostics (file, line, pointer, code)
plate check f.ncl --seeds 1..10         # one line per seed and a tally (also 1,4,9); add --json for JSON
plate check f.ncl --strict              # exit 1 unless every requirement is met (with --seeds: at every seed)
plate check plan.ncl --case N           # a plan file in its preview case N
plate check plan.ncl --in plate.ncl:flat_2      # a plan file in the slot that plate gives flat_2
plate slot plate.ncl flat_2 > slot.json         # save that slot once...
plate check plan.ncl --slot slot.json           # ...and iterate on the plan without re-solving the plate
plate export f.ncl json|svg|dxf [-o out] [same --seed/--case/--slot/--in flags]
PLATE_DEBUG=1 plate check f.ncl                 # the grid engine's runs
```

The wasm build takes about 2 minutes from cold; give it a long timeout. A cold `cargo test`
build is several minutes on a small machine; after that, single crates rebuild in seconds. The
dev server writes to `./workspace/` (gitignored), one folder per project.

## Testing: run the smallest thing that proves the change

Start from the row that matches what you changed, get it green, then run the wider tiers before
you finish. Filters are substrings of test paths: one before `--`, or several after it
(`cargo test -p plate-core -- fit:: spread::`).

| You changed                                       | Fastest check                                                                                     | Start from                                                     |
| ------------------------------------------------- | ------------------------------------------------------------------------------------------------- | -------------------------------------------------------------- |
| Placements (`fit.rs`, `spread.rs`)                | `cargo test -p plate-core -- fit:: spread::`                                                      | `testkit::pinned_flat_with` + `power()` / `lighting()`         |
| Networks, cables (`connect.rs`, `route.rs`)       | `cargo test -p plate-core -- connect:: route::`                                                   | `testkit::pinned_flat_with` + `power()` / `plumbing()`         |
| Walls, assemblies (`build.rs`)                    | `cargo test -p plate-core -- testkit fit::tests::rooms`, then `built_tests`                       | `pinned_flat` (exact clear rectangles in `fit.rs`'s `CLEAR`)   |
| Grid engine cost terms, moves (`grid.rs`)         | `cargo test -p plate-core grid::tests`                                                            | `running_cost_never_drifts` with a plan that uses the term     |
| Where the engine puts things, the host (`mod.rs`) | `cargo test -p plate-core solve::tests` (anneals: seconds)                                        | an unpinned plate; check a few seeds                           |
| Geometry, units, ports, outlines                  | `npm run test:unit`                                                                               | the module's own `mod tests`                                   |
| An example, or anything it exercises              | `cargo test -p plate-config examples::duplex` (one project) or `examples::home_studio` (one file) | `plate check examples/… --seeds 1..6`                          |
| Schema, `plate.ncl`, Nickel evaluation            | `cargo test -p plate-config`, then `npm run test:wasm` and `npm run test:web`                     | `plate eval f.ncl`                                             |
| A plan inside a plate                             | `plate check plan.ncl --in plate.ncl:space` (or `--slot`)                                         | `plate slot`                                                   |
| CLI, server                                       | `cargo test -p plate-server`, then run the command                                                |                                                                |
| Viewer drawing (`viewer/`)                        | `npx vitest run web/src/viewer`                                                                   | `web/src/test/viewer.ts` `mount()` + `web/src/fixtures/*.json` |
| Pure web logic                                    | `npx vitest run web/src/<module>`                                                                 | the module's `.test.ts`                                        |
| Anything you can see in the app                   | `npm run shot -- <file.ncl>`, then open the PNGs in `shots/`                                      | `fixtures/pinned-flat.ncl` (fast, never moves) or an example   |

**Pinned plates** are the main tool for everything after the engine. Every space has `at` plus an
exact `width` and `depth`, so the engine has nothing to move: `Solution::moves` is 0, the solve
takes milliseconds, and positions are exact (assert numbers, not "somewhere near"). In Rust,
`solve/testkit.rs` has `pinned_flat(extra)` and small `power()`, `lighting()` and `plumbing()`
services, plus `run`, `assert_met`, `element`, `parts`, `in_space`, `runs`, `walls_between`.
On disk, `fixtures/pinned-flat.ncl` is the same flat for the CLI, the screenshots and the web
fixtures. Add a fixture when a test needs a layout nothing else has; keep it pinned (a test
checks).

**Example snapshots.** Each example plate and preview has a short summary in
`crates/plate-config/tests/snapshots/<project>/`: `*.layout.txt` (report, every space's area and
bounds, openings, walls by assembly) and `*.services.txt` (parts per room, circuits). A diff in
`services` with none in `layout` is what a service change should look like. When a change is
meant to move things, `PLATE_SNAPSHOTS=update cargo test -p plate-config examples::`, read the
diff, and commit it with the change. Don't accept snapshots you haven't read.

**Screenshots.** `npm run shot -- examples/duplex/duplex.ncl` starts its own API and Vite on
spare ports, on a throwaway copy of the project (never `./workspace`), opens the plate in Chrome,
waits for the solve, and writes one PNG per view (Plan, then each service) to `shots/`
(gitignored). It prints the verdict and any console errors; `--strict` makes either fail.
`--svg out.svg` turns an exported SVG into a PNG. Needs the wasm engine; finds Chrome itself
(`PLATE_CHROME` to choose one).

## Before you finish

- `cargo fmt --all`, `npx prettier --write web README.md docs` (or `npm run format`).
- `cargo clippy --workspace --all-targets` with no warnings.
- `cargo test --workspace`, `npm run typecheck`, `npm run test:web`. The small checks in the
  table above are for while you work; this is the gate.
- Snapshot changes (`crates/plate-config/tests/snapshots/`) read and explained. If the Layout
  format or anything on the pinned fixture changed: `npm run fixtures` and commit the JSON.
- If you touched `plate-config`, `plate-wasm` or the schema: `npm run build:wasm` and check the
  page, not just the native tests. The browser runs a different build target.
- UI changes: look at the page (`npm run shot -- <file.ncl>`, then read the PNGs) instead of
  trusting tsc.
- Update README / ARCHITECTURE when behaviour or structure changes.

## Rules

- **The schema is the single definition of the format.** New config fields go in
  `schema/plate.schema.json` first (type, description, default, `errorMessage`); Nickel
  validation, defaults, hover docs, completions and the reference panel follow. Then the Rust
  type in `plate-core`.
- **The file is the source of truth.** The drawing never holds state the text doesn't. Edits made
  in the drawing go through `web/src/editor/geometryEdits.ts` as text edits, to the file the value
  is written in (looked up via the source map).
- **Plain data between stages.** Config → solver → layout → export are plain function calls on
  serde types. Keep `plate-core` free of I/O and Nickel.
- **Determinism.** Same file and `seed` → same layout. Use the seeded RNG, never time or
  `thread_rng`.
- **Diagnostics point somewhere.** Errors carry a span (and `file` when not the main file);
  solver warnings carry a JSON pointer that the editor resolves with the source map.
- Rust: edition 2021, MSRV 1.89, `max_width = 110`. TypeScript/CSS/HTML: prettier,
  `printWidth` 110. 2-space indent except Rust (4).
- Write user-facing text plainly: short sentences, no jargon the user didn't introduce.

## Traps (learned the hard way)

- **Nickel recursive records.** Inside `spaces = { room = { plan = room } }`, `room` is the space
  itself, not an outer `let room`. Name imported plans differently (`let hotel_room = import …`).
  The symptom is a contract error like "plan: extra fields area, aspect…".
- **Imports need absolute paths.** `plate-config` puts every file in Nickel's cache in memory
  under `/workspace/<path>`. Nickel normalises import paths, and on `wasm32-unknown-unknown` no
  path is absolute (it calls `current_dir()`, which fails). That's why the browser build targets
  `wasm32-wasip1` with stub imports (`web/src/engine/wasi.ts`). Don't switch the target back.
- **Nickel API order.** Use `program.compile()` before `custom_transform`; `program.parse()`
  alone panics because terms aren't populated.
- **Wrapped sources.** Each file is wrapped (`let plate = STDLIB in …`, plus the contract for the
  main file). Positions are mapped back through per-file offsets in `plate-config/src/lib.rs`;
  if you change a wrapper, the offsets and the position tests must still agree.
- **Prettier rewrites what you just wrote.** After formatting, re-read a file before doing exact
  string replacements on it.
- **The source map may be older than the text.** A drag writes the file before its evaluation
  comes back. Read spans through `GeometryEdits`' `sources` getter (shifted through the edits since,
  `editor/shift.ts`), never from a stored copy of an evaluation, or a quick second drag writes at
  stale columns (`[603438]`).
- **Text splitting for rules.** Rules written with helpers (`plate.door "a" "b"`) have no span of
  their own; `arrayItems` in `geometryEdits.ts` splits the array literal by text. Keep it aware
  of strings and comments.
- **Workspace paths.** `plate-server/src/workspace.rs` only accepts relative `.ncl` paths with no
  `..`, no backslashes and no dot-folders. Keep every file route going through `resolve()`, and
  every project name through `check_name` in `projects.rs`.
- **Projects are closed under imports.** The evaluator only ever gets the open project's files. An
  import reaching out of a project fails, on purpose: don't widen `store.files()` to other projects.
- **Paths in the web app are project-relative.** `store.project` says which project. Anything keyed
  by path that outlives a switch must be per project (or cleared in `showProject`, like `planViews`).
- **Switching order.** `workbench.leaveProject()` first (it saves cursors and empties the groups),
  then `store.open(name, { announce: false })`, then `showProject`, which calls `enterProject`. While
  `switching` is set, the workbench neither persists nor closes tabs of files it can't find.
- **`web/dist` goes stale.** It's only rebuilt by `npm run build` or `npm start`, so `npm run dev` doesn't
  serve it on `:7878` (`--web none`): an old page there polls routes that no longer exist. In
  development the app is on `:5173`.
- **API answers aren't cached.** The server sends `no-store` and `api.ts` fetches with `cache: "no-store"`.
  A browser once kept the old server's `index.html` as the answer to `/api/projects`, and no refresh cleared it.
- **Windows paths.** `canonicalize` gives `\\?\C:\…` paths, where `/` and `..` mean nothing: walk
  import specs part by part (`import_target` in `main.rs`). Nickel's in-memory root needs a drive on
  native Windows (`ROOT` in `plate-config`).
- **Monaco models can't be renamed.** A renamed file gets a new `TextDoc`; the store emits
  `{ list, renamed }` and the workbench moves tabs. Don't rename by mutating paths in place.
- **Symbols are view-only.** `Space.symbol` is `skip_serializing` (out of the fingerprint, like
  `preview`), so the solve cache key adds `view_key` instead. Don't fold symbols into the
  fingerprint: choosing a drawing would re-seed the solve and move everything.
- **The symbol catalog is two lists.** `schema/symbols.json` and the schema's `Symbol` enum must
  name the same symbols; tests in `plate-config` and `web/src/symbols.test.ts` check it.
- **Plan fingerprints.** A plan's fingerprint (its serialised form) picks the seed of its
  solve. `Plan.preview` is `skip_serializing` so editing a preview never changes a solution;
  keep any other view-only field out of the fingerprint the same way.
- **Services never move a wall.** A service's `place`, `connect`, `parts` and `params` are
  `skip_serializing` (out of the fingerprint) and go into `view_key`; only `scope` and `claims`
  change a solve. `Service::full()` serialises everything when you need it all. A new field that
  only affects where parts go belongs in the first group. The same goes for `media`, `feeds`, a
  space's `load` (it's in `view_key` instead), an assembly's `cross` and the plate's `height`.
- **Both drawings of a network.** `connect.rs` emits every network routed and as arcs; `alt`
  marks the one `draw` didn't ask for, and exports skip it (`Element::exported`). Tests that
  count runs should filter by `draw`. Circuit numbers run across a scope's networks.
- **Claims and pointers.** Rules and spaces a service claims are merged into plans in
  `solve/context.rs`; their pointers are absolute (`@/services/power/claims/rules/0`) and must
  stay so through `under()`. Strip the `@` with `resolved()` before anything leaves the solver.
- **Library files are told by shape.** No `start` and no plan fields: a file with any of
  `parts`/`place`/`connect`/`claims`/`params`/`media`/`feeds` is a service (a cable library is
  `{ media = … }`); a record whose every entry has
  `thickness` or `layers` is a set of assemblies (`FileKind::of`, and `by_shape` in the schema
  prelude for the contract). A service file can't be a Nickel function (it couldn't be exported
  to JSON): parameterise with `params` and merge (`(import "power.ncl") & (import "codes/nec.ncl")`).
- **The schema isn't prettier-managed.** `schema/plate.schema.json` and `schema/symbols.json` keep
  their own formatting (`json.dump(..., indent=2)` style); don't run prettier on them.
- **Annealing tests.** The grid engine is stochastic per seed; tests that assert quality try a
  few seeds rather than one. A test about anything after the engine (walls, parts, networks)
  shouldn't anneal at all: pin the plate (`testkit::pinned_flat`).
- **`moves` isn't `work`.** `Ctx::work` counts the moves a solve _budgets_ and decides retries
  and extra runs; changing how it's counted changes solutions. `Ctx::moves` /
  `Solution::moves` count the moves actually made and only report. Keep it that way.
- **Grid tests drive `setup`.** `grid::solve` is `setup` (the problem and its starting
  rectangles) then `anneal` and `report`. The tests in `grid.rs` call `setup` and make moves by
  hand through `apply` / `FillState::update` / `recut` / `restore`, as `anneal_once` does. A new
  cost term must keep `running_cost_never_drifts` passing; add a case that uses it.
- **Every example file has a test.** `example_tests!` in `plate-config/src/lib.rs` lists them,
  and `every_example_file_has_a_test` fails until a new file is added there.
- **Fixtures are generated.** `web/src/fixtures/*.json` come from `fixtures/*.ncl` through
  `npm run fixtures`; don't edit them by hand (prettier skips them too). Viewer tests read them
  typed as `Layout`, so a Rust field `types.ts` doesn't know about breaks `tsc`.
- **Tuning an example.** Sweep seeds before blaming a file: `plate check f.ncl --seed N` for N in
  1..6. A plan that fails at most seeds is over-constrained; the notes say which limit binds. Pair
  proxies (gap slivers) must not penalise gaps that a third space covers: they did once, and a
  flat with a store between two rooms became unsolvable.
- **Debug builds.** `plate-core` is built with `opt-level = 3` even in dev (Cargo.toml): the
  tests solve every example.

## Web app map

- `main.ts`: wiring (solve loop, projects, start screen and drawing, menus, title block, keys).
- `workspace.ts`: `WorkspaceStore`, the open project's files as documents (the last few projects
  stay loaded), dirty state, drafts in localStorage (`plate:drafts:<project>`).
- `workbench.ts`: tabs, up to two editor groups, drag and drop, layout (`plate:layout` for the side
  bar, `plate:project:<project>` for the rest). `sidebar.ts`: the activity bar and side bar views
  (files, reference). `explorer.ts`: the file tree.
- `palette.ts`: the search box over a list, for the project switcher, quick open and names.
  `projects.ts`: routes, recents, names, copy plans. `fuzzy.ts`, `tree.ts`: matching, and the
  tree's rows. All three pure and tested.
- `reference/`: the reference view. `model.ts` builds every page from the schema, `plate.ncl` and
  `docs/reference/*.md`; `locate.ts` finds what the cursor is on (inverted source map, else a
  text scan) and walks the schema; `insert.ts` places inserts. All pure and tested except
  `panel.ts`.
- `plates.ts`: plate vs plan (`start =`), import graph, import rewriting on moves. Pure, tested.
- `services.ts`: which service an element belongs to, the views bar's chips and their counts.
  `library.ts`: the card for a service or assemblies file opened on its own. Pure, tested.
- `symbols.ts`: the symbol catalog (`schema/symbols.json`) for the reference gallery and the
  drawing's right-click suggestions. Pure, tested. The engine draws placed symbols itself.
- `preview.ts`: plan previews (a plan file drawn on its own in a slot): its cases, a plate's slot
  read as four sides, a case written as Nickel. Pure, tested. The bar and wiring are in `main.ts`.
- `editor/`: Monaco setup and `TextDoc`, Nickel highlighting, schema hovers, templates, source
  lookups, drawing → text edits.
- `engine/`: the wasm engine in a worker. `viewer/`: the SVG drawing.

Files that import Monaco can't be loaded by vitest (no `window`); keep testable logic in plain
modules like `plates.ts` and `editor/sources.ts`. `viewer/viewer.ts` doesn't import Monaco: test
it on happy-dom with `mount()` from `web/src/test/viewer.ts` (first line of the test file:
`// @vitest-environment happy-dom`), which draws a fixture layout synchronously (`renderNow()`)
in a sheet of a given size.

The reference has no hand-written content besides `docs/reference/*.md` (numbered for order;
`03-plans-and-ports.md` is the guide `plans-and-ports`). Give new schema fields a description,
and `x-examples` / `x-docs` where an example or a guide helps; the panel picks them up.
