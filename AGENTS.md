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
npm run test:web       # vitest only
npm run typecheck      # tsc only
npm run build:wasm     # rebuild the browser engine after changing plate-config or the schema
npm run format         # prettier + rustfmt
cargo clippy --workspace --all-targets
cargo run -p plate-server --bin plate -- check examples/commercial/hotel.ncl -v [--seed N]   # PLATE_DEBUG=1 for the grid engine's runs
```

The wasm build takes about 2 minutes from cold; give it a long timeout. The dev server writes to
`./workspace/` (gitignored), one folder per project.

## Before you finish

- `cargo fmt --all`, `npx prettier --write web README.md docs` (or `npm run format`).
- `cargo clippy --workspace --all-targets` with no warnings.
- `cargo test --workspace`, `npm run typecheck`, `npm run test:web`.
- If you touched `plate-config`, `plate-wasm` or the schema: `npm run build:wasm` and check the
  page, not just the native tests. The browser runs a different build target.
- UI changes: look at the page in a browser (Playwright/Chromium works) instead of trusting tsc.
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
  only affects where parts go belongs in the first group.
- **Claims and pointers.** Rules and spaces a service claims are merged into plans in
  `solve/context.rs`; their pointers are absolute (`@/services/power/claims/rules/0`) and must
  stay so through `under()`. Strip the `@` with `resolved()` before anything leaves the solver.
- **Library files are told by shape.** No `start` and no plan fields: a file with any of
  `parts`/`place`/`connect`/`claims`/`params` is a service; a record whose every entry has
  `thickness` or `layers` is a set of assemblies (`FileKind::of`, and `by_shape` in the schema
  prelude for the contract). A service file can't be a Nickel function (it couldn't be exported
  to JSON): parameterise with `params` and merge (`(import "power.ncl") & (import "codes/nec.ncl")`).
- **The schema isn't prettier-managed.** `schema/plate.schema.json` and `schema/symbols.json` keep
  their own formatting (`json.dump(..., indent=2)` style); don't run prettier on them.
- **Annealing tests.** The grid engine is stochastic per seed; tests that assert quality try a
  few seeds rather than one.
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
modules like `plates.ts` and `editor/sources.ts`.

The reference has no hand-written content besides `docs/reference/*.md` (numbered for order;
`03-plans-and-ports.md` is the guide `plans-and-ports`). Give new schema fields a description,
and `x-examples` / `x-docs` where an example or a guide helps; the panel picks them up.
