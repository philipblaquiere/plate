# Architecture

```
                      schema/plate.schema.json  (JSON Schema: the configuration format)
                          │ compiled to a Nickel contract           │ read by the editor
                          ▼                                         ▼ (hover docs, completions)
 BROWSER   .ncl files ──► Nickel + contract (WebAssembly, in a worker) ──► config data (JSON)
                          │ errors with exact positions → squiggles      │
 ─────────────────────────┼──────────────────────────────────────────────┼─────────────────────
 SERVER                                                                  ▼
                                    validate again (serde) ──► Config ──► Solver ──► Layout ──► DXF / SVG / JSON
```

One linear pipeline. Each arrow is a plain function call with plain data in between, so any stage
can be replaced without touching the others.

The `.ncl` files come from the **workspace** folder, which the server lists, reads and writes
(`/api/files`). The browser holds all of them, so a plate is evaluated with the files it imports,
including unsaved edits in other tabs.

The browser does all language work: it evaluates Nickel, applies the schema, fills defaults and
reports errors, without a round trip. Only valid configuration data is sent to the solver. The
server still validates it (it never trusts a client), and still accepts Nickel source for the CLI
and other tools.

## Crates

| Crate          | Job                                                                                                                          | Knows about      |
| -------------- | ---------------------------------------------------------------------------------------------------------------------------- | ---------------- |
| `plate-core`   | Typed `Config` and plans (serde), units, geometry, the solver (host + `grid` and `spine_stack` engines), `Layout`            | nothing external |
| `plate-config` | Compiles the JSON Schema to a Nickel contract, evaluates Nickel with it, fills defaults, maps errors to line/column, formats | Nickel           |
| `plate-wasm`   | `plate-config` as a WebAssembly module for the browser (WASI with stubs, tiny ABI)                                           | WebAssembly      |
| `plate-export` | `Layout` → DXF (via the `dxf` crate), SVG, JSON                                                                              | file formats     |
| `plate-server` | Axum API used by the editor, plus the `plate` CLI                                                                            | HTTP, the CLI    |

`plate-core` has no I/O and no Nickel, so it can also compile to WebAssembly if the solver ever
needs to run in the browser.

## Web app (`web/`)

| File                          | Job                                                                                |
| ----------------------------- | ---------------------------------------------------------------------------------- |
| `src/main.ts`                 | Wiring: start screen and drawing flow, debounced solve, title block, files, export |
| `src/workspace.ts`            | `WorkspaceStore`: the open project's files as documents, dirty state, drafts       |
| `src/explorer.ts`             | The file tree: pinned plates, filter, keyboard, uses and used by                   |
| `src/palette.ts`              | The search box over a list: project switcher, quick open, asking for a name        |
| `src/projects.ts`             | Routes, recents, names, what copying to a project takes (pure)                     |
| `src/workbench.ts`            | Tabs, up to two editor groups, drag and drop, layout per project                   |
| `src/sidebar.ts`              | The side bar: activity bar, one view at a time (files or reference), widths        |
| `src/reference/model.ts`      | The reference's topics, generated from the schema, `plate.ncl` and the guides      |
| `src/reference/locate.ts`     | What the cursor is on: source map inverted (or the text scanned), schema walked    |
| `src/reference/insert.ts`     | Where an Insert from the reference goes (the right record, the right list)         |
| `src/reference/panel.ts`      | The reference view: follows the cursor, search, pages, Insert and Copy             |
| `src/plates.ts`               | Plate or plan? What a file imports, who imports it; rewriting imports on moves     |
| `src/symbols.ts`              | The symbol catalog: thumbnails for the reference, symbols that suit a space        |
| `src/services.ts`             | Which service an element belongs to, the views bar's chips and counts              |
| `src/library.ts`              | The card shown for a service or assemblies file opened on its own                  |
| `src/preview.ts`              | Plan previews: cases in a file, a plate's slot as sides, a case written as Nickel  |
| `src/menu.ts`                 | Right-click menus                                                                  |
| `src/api.ts`                  | Typed client for `/api/*`                                                          |
| `src/types.ts`                | TypeScript mirror of the Rust JSON                                                 |
| `src/editor/editor.ts`        | Monaco setup, theme, format provider; `TextDoc`: one file's model and edits        |
| `src/editor/nickel.ts`        | Nickel syntax highlighting                                                         |
| `src/editor/schema.ts`        | Hover docs and completions, read from the JSON Schema                              |
| `src/editor/templates.ts`     | The file written for a newly drawn boundary, spine or free origin                  |
| `src/editor/geometryEdits.ts` | Drawing → text: rewrites dragged coordinates, inserts and removes points           |
| `src/engine/`                 | Runs `plate-wasm` in a Web Worker: `ConfigEngine.evaluate({ main, files })`        |
| `src/viewer/viewer.ts`        | The drawing: pan/zoom, grid, dimensions, spaces, walls, parts, views, edits        |
| `src/editor/sources.ts`       | Pointer → text lookups (for solver warnings and "show in file")                    |
| `src/viewer/format.ts`        | `Lengths`: metric decimals or feet-and-inches, areas, grid and scale-bar steps     |
| `src/editor/system.ts`        | Source edit behind the Metric/Imperial switch                                      |
| `src/files.ts`                | Picking a file from the computer, downloads                                        |

The viewer projects world coordinates to screen space in JavaScript and draws plain SVG. Line
weights, hatching and text stay crisp at every zoom, and hit-testing is simple maths on screen
coordinates.

## Workspace and imports

`plate-server/src/workspace.rs` serves one folder: `.ncl` files only, paths relative to the
folder, nothing outside it reachable (no `..`, no absolute paths, no hidden folders).

`plate-server/src/projects.rs` makes each folder at the top of the workspace a project: a
`Workspace` of its own. There's no manifest. The browser only ever gives the evaluator the open
project's files, so a plate is closed under imports: editing one project can't change a layout in
another (a shared library would break "same file and seed, same layout"), and each solve sends only
what it can read. A hidden `.plate-projects` marks a workspace as laid out this way; without it,
`.ncl` files and folders holding them move into `my-files`, once, with relative imports intact.

The examples are compiled in (`EXAMPLES`), and copied into the workspace when first opened, so they
can improve with the app without touching what someone changed.

Opening a project is one request (`GET /api/projects/{p}` gives every file's text). In the browser,
`WorkspaceStore` keeps the last few projects loaded, so going back is instant and undo survives;
a background request then picks up anything changed on disk. Model URIs include the project, since
two projects can hold the same path. Tabs, main plate and closed folders are per project in
`localStorage` (`plate:project:<name>`), as are drafts (`plate:drafts:<name>`).

Thumbnails in the switcher come from `thumbnail.rs`: the plate solved at its own seed and drawn as
outlines. They're solved at most once per content: kept in memory by a hash of the files, with that
hash as the `ETag`, so the browser's repeat asks cost a hash and no solve.

Evaluation takes a set of files and the one to evaluate (`plate_config::Workspace`). Each file is
added to Nickel's cache in memory under `/workspace/<path>` (`C:\workspace\<path>` on native
Windows, where a path needs a drive to be absolute), so `import "hotel-room.ncl"`
resolves like on disk (relative to the importing file) without touching the file system. Every
file is wrapped to see `plate` (the helper library); the main file also gets the schema contract,
which checks a value with a `start` as a plate (the schema's root) and anything else as a plan
(`$defs/Plan`), so a plan file can be evaluated and drawn on its own.
Each wrapper's offset is remembered, so errors and the source map point at the right line of the
right file (`Diagnostic.file`, `SourceSpan.file`).

The browser build targets `wasm32-wasip1`, not `wasm32-unknown-unknown`: Nickel insists on
absolute import paths, and on `wasm32-unknown-unknown` no path is absolute. The page gives the
module a stub WASI with no files (`web/src/engine/wasi.ts`).

The CLI (`plate check hotel.ncl`) evaluates a file with the other `.ncl` files of its folder.

## Why Nickel

Nickel gives us, for free:

- **Cascading**: records merge with `&`, and `| default` / `| force` set priorities. A building file
  can set defaults that a floor file overrides.
- **Self-reference**: records are recursive, so one value can be computed from another in the
  same file (`points = [[0, 0], [width, 0], ...]` with `width` defined alongside).
- **Reuse**: `let`, functions and `import` for shared fragments.
- **Validation**: contracts, generated from the JSON Schema (below).

Values reach the solver as plain JSON, so it never depends on Nickel internals.

## The schema

`schema/plate.schema.json` is standard JSON Schema (2020-12), and it is the single definition of
the configuration format: fields, types, allowed values, defaults, docs, the length syntax and
error messages. It drives:

- **Validation in Nickel.** `plate-config/src/schema.rs` compiles it to a Nickel contract when the
  engine starts (at runtime, so a user-supplied solver can later bring its own schema). The
  user's file is evaluated with the contract applied, so Nickel reports every problem at its exact
  position: `[0, "30 yd"]` underlines `"30 yd"`, a typo like `pionts` underlines `pionts`, and
  `[0, 0, 0]` underlines the point.
- **Defaults.** Filled in after evaluation, like JSON Schema validators' "use defaults" mode.
  (Not as Nickel `| default`: two defaults for one field are a merge conflict in Nickel, which
  would break files that cascade with their own defaults.)
- **The editor.** Hover docs, value completions and snippets come straight from it.
- **The reference panel.** One page per definition (records, each variant of a union, values),
  with fields, allowed values, defaults, examples and where each is used. Small wrapper
  definitions fold into the page that explains them (`Spaces` into `Space`, `Rule` into
  `Rules`). Helpers come from `schema/plate.ncl` (their `doc` and the rule kinds they write);
  guides for ideas that span fields (imports, typicals, ports…) from `docs/reference/*.md`.

The supported subset is listed at the top of `schema.rs`; anything else is rejected rather than
silently ignored. Non-standard keywords: `errorMessage` (the message shown when a value is
rejected), `x-enumDescriptions` (docs for enum values), `x-examples` (Nickel source shown, and
insertable, in the reference) and `x-docs` (slugs of guides in `docs/reference/` to read).

Tests keep the pieces in line: every example must pass the Nickel contract, a standard JSON
Schema validator and the Rust types; and the schema's length `pattern` must accept exactly the
strings the Rust length parser accepts.

To see the generated Nickel: `cargo run -p plate-server --bin plate -- schema`.

## Starting geometry

The solver understands two modes, chosen by `start.mode`:

- `'boundary`: a closed polyline (≥ 3 points). Everything the solver places stays inside it.
- `'spine`: an open polyline the solver works around. A spine with a single point is the
  **free** mode the UI offers: just an origin.

Both are `{ mode, points }` with points written `[x, y]`, straight segments only. In the schema,
`Start` is a `oneOf` of `BoundaryStart` and `SpineStart`, each with `mode` fixed by `const`; the
compiler turns that into one contract that dispatches on `mode`, so an error is reported against
the right variant. Geometry rules JSON Schema can't express (repeated consecutive points,
self-crossing, zero area) are in `Config::validate` and report the path of the offending point.

## Two-way editing

Every value Nickel produces keeps the position of the text it came from. After evaluation,
`plate-config/src/sources.rs` walks the result and records a **source map**: JSON pointer →
span, plus whether that text is a plain literal (`30`, `"98'-6"`, `[x, y]`) or an expression.
The browser gets it with every evaluation.

The viewer reports edits in drawing units (point dragged, edge moved, point inserted or removed), and
`web/src/editor/geometryEdits.ts` turns them into text edits:

- **Moving a point** looks up `/start/points/<i>/0` and `/1`. If both are literals, their spans
  are highlighted and rewritten on every mouse move, in their own notation (a number in `units`,
  or a length string in its original unit), as one undo step. Because positions follow `let`
  bindings, a coordinate defined once at the top of the file is rewritten there. Expressions are
  refused with a pointer to their line.
- **Moving an edge** is previewed in the viewer only (a ghost; the geometry is in
  `web/src/viewer/edgeMove.ts`, pure and tested). On release both ends' four coordinates are
  rewritten as one undo step. A span shared by two coordinates (a `let` used by both ends) is
  written once, and refused if the move needs it to take two values; an expression only stops
  the move if the move would change it.
- **Inserting / removing a point** edits the `points` array literal in place, keeping its layout
  (one point per line, or inline).

Edits go to the file the value is written in: a rule added to a plan imported from
`hotel-room.ncl` lands in `hotel-room.ncl`.

The file stays the single source of truth: the drawing never holds state the text doesn't.

## Solving

`plate-core/src/solve/` turns a plan into spaces, level by level.

- **Engines** (`grid.rs`, `spine.rs`) solve one level: given a _slot_ (a boundary polygon with
  ports, or open space around a spine) and a plan, they place the plan's spaces and return each
  one's rectangle, its frame, and its ports.
  - `grid` rasterises the slot and runs simulated annealing over rectangles of whole cells
    (move, resize, move a shared wall, line a side up with a neighbour's, reshape at the same
    area, dock against a neighbour, swap, rotate, switch optional instances on and off), then
    polishes each run one cell at a time. Cost = hard violations (outside, overlap, sizes,
    required rules) × a large weight + preferences + goals. Moves are scored incrementally:
    only the terms that touch the moved rectangle are recomputed. Runs go in parallel
    (`std::thread::scope`, one seeded generator per run) in batches, with more batches while a
    requirement is unmet; the best run is the one with the fewest unmet requirements, then the
    lowest cost. Effort is counted in moves (`Ctx.work`, capped by `WORK_LIMIT`), never in time,
    so results don't depend on the machine.
  - **The fill** (`fill = true`) isn't a rectangle: it's every cell the rectangles leave.
    `Occ` keeps per-cell coverage counts; `FillState` caches the fill's terms (area, door and
    window runs, floor cut off from its main piece, narrow strips beside each rectangle) and
    updates them per move. Annealing uses cheap proxies (gap and edge slivers as pair and unary
    terms, cut-off floor only while the free area is small); polishing and choosing between runs
    measure exactly (corners, slivers, corner pinches, the room its plan's biggest fixture
    needs). The fill's outline comes from `outline.rs` (loops traced along cell edges, holes
    included).
  - A level of big rooms only (no fixtures, no pins, every side ≥ 2 m) is solved on a coarser
    grid when its slot is large (`max_cells`).
  - `spine_stack` places a corridor along the spine (mitred at bends) and fills bays on each
    side: fixed counts first, then open-ended ones at grid-snapped widths, ends for stairs.
- **The host** (`mod.rs`) does the rest:
  - Recursion: a placed space with a `plan` becomes a rectangular slot with the ports the
    parent gave it, and its plan is solved inside.
  - Canonical frames: the slot is turned (and mirrored, unless `mirror = false`) so its
    entrance is at the bottom and its description (size + ports) is smallest. Equal
    descriptions are interchangeable, so they're solved once: a typical. Solutions are cached
    per solve, and across solves (so editing one level doesn't re-solve the others).
  - `needs` accept or reject a slot; `unique` / `instance` rules take instances out of
    typicals; `reach` rules are checked on the door graph.
  - Warnings carry a JSON pointer (`/spaces/room/plan/rules/3`); the editor maps it to text
    with the source map, and repeats across typicals are merged ("room_3 and 23 more like it").
  - Retries: a level whose layout (or a level inside it) breaks a requirement, or cuts floor off
    from its fill, is solved again with derived seeds (`ATTEMPTS`, within the work budget); the
    best attempt is kept.
  - Doors between two spaces with plans go mid-wall; once the plans inside are solved,
    `align_doors` merges a door with the one a plan put on that wall (its `'entrance`), or
    moves it to where circulation meets circulation.
  - **The report** (`Report`): `perfect` (every rule and preference, no floor left over),
    `solved`, `incomplete` or `empty`, with counts. Floor no space covers is found per level
    (`leftover`), drawn as `leftover` elements and explained (`leftover_hint`: which neighbour is
    at its `area.max`, which could only grow into it as an L, whether it's cut off from the
    fill). Diagnostics carry a `code` (`unmet`, `preference`, `leftover`) and notes saying what
    to change; programs that can't fill their floor (or need more than there is) are caught
    before solving, with the numbers.
- **Ports** (`ports.rs`) are labelled segments of a slot's boundary, found by collinear
  overlap: a wall on the parent's facade is `facade`, a wall shared with a sibling is `party`
  or `partition`, a wall with a door to circulation is `entrance`. Each door also gives the
  spaces on both sides a `door` port of exactly its width, so fixtures inside keep its swing
  clear, and each window gives its space a `window` port the same way, so rules inside can keep
  things off it (`apart` with a `port`, optionally the whole `wall` it's on).
- **Fixtures** (`kind = 'fixture`) are the last level: no plan. In `grid` they carry a facing
  (which side of the rectangle is the front), a per-side clearance that other spaces can't stand
  in (unary and pair cost terms, so the incremental scoring still holds) and an `against`
  constraint on their back. Their cells are rounded up to hold their exact size, which is what's
  drawn, back-aligned. A plan with fixtures gets the coarsest round grid (250, 200 or 100 mm)
  its fixtures' sizes and clearances fit (`plan_grid`). `row = true` places an exact count as
  one rectangle, split into pieces when drawn. `spine_stack`
  refuses them with a warning. Placed fixtures come back with their front edge (`SpaceInfo.front`)
  and a `clearance` element.
- **Symbols** (`symbols.rs`, catalog in `schema/symbols.json`) are drawings fitted to a space
  from its front (a fixture's, or the side it's entered from) and flattened to paths in a
  `symbol` element, so the viewer and exporters draw them without knowing the catalog. `symbol`
  is left out of the plan fingerprint like `preview` (choosing one never re-seeds a solve), but
  it is part of the solution cache key, so a cached child still draws the current symbols.
- **Plan files** (no `start`) are solved by `solve_preview` in a rectangular slot, through the
  same child solve the host uses for a placed space (`Host::solve_child`: canonical frame,
  needs, typical key, seed, cache). The slot comes from a `View`: a `preview` case in the file,
  a slot sized from `needs`, or a slot a plate gave a space. Every space with a plan carries its
  slot (`SpaceInfo.slot`: canonical size and ports, units, seed, grid, plan fingerprint); sent
  back as `View::Slot`, the plan file alone gets the same key, seed and cache entry, so the
  same solution. `preview` is skipped when fingerprinting a plan, so it never changes one. The
  slot and its ports come back as `slot` and `port` elements; the web app resizes the slot by
  rewriting `preview.width` / `depth` (`geometryEdits.ts`).
- **Determinism**: a seeded xorshift generator; the seed of each child solve is derived from
  the root `seed` and the typical's description, so the same file always gives the same layout.

### Assemblies and services

Three more passes, around the solve above. Types are in `assembly.rs` and `service.rs`.

- **Context** (`context.rs`), before a level is solved: `prepare` gives a plan what it inherits
  (assemblies, services, from its parent), merges the plan fragments services claim `inside`
  matching spaces, appends their claimed rules, and resolves tags (`"#wet"`) to the space types
  that have them (a space's kind counts). Injected rules and spaces point at the claim that put
  them there through `Plan.origins`, with absolute pointers written `@/services/…` (`under`
  leaves them alone, `resolved` strips the `@`). The prepared plan is what's fingerprinted, so
  a claim changes the solve; a slot carries its `context` so a plan file opened in that slot is
  prepared the same way and gets the same solution.
- **Build** (`build.rs`), after a level is solved and before its children are: each shared edge
  becomes a `BuiltWall` with the roles both sides see (plus the assembly names already built
  around it, so a child's port can be `'wet` because its wall is). Each wall gets the thinnest
  assembly meeting every `Need` (from roles' `on` and from `wall` rules; a rule with `length`
  picks the one wall with the longest run clear of doors). Walls are inset into the rectangles
  they bound (boundary walls inward, unless `line = 'centre`), ends extended to meet at corners,
  and each child's slot is the clear rectangle left, so plans inside are solved in clear floor.
  The engines look ahead to this: a space whose plan's `needs` its walls could decide
  (`SpaceType::clear`) is held to them in the clear. The grid engine scores what the walls around
  it would leave at every move, from `build::Foresight` (the same choice of assembly, by the roles
  the edge would have); spine bays are sized for the walls they'll have. Spaces with no such needs
  pay nothing. Foresight assumes a door rule's door gets built and leaves out `wall` rules with
  `length`, so `check_needs` still has the last word, on the walls really built.
  Openings snap to their wall's centreline and take its depth (`in_wall`).
- **Fit** (`fit.rs`, `connect.rs`), once everything is solved: services' `place` and `connect`
  run on the finished elements. `fit` builds a model of leaf rooms (faces split by the wall
  behind them, openings on faces, fixtures and their backs), resolves each service's `scope`
  into instances (one per `"#dwelling"`), and places parts anchored first (`on`, `beside`,
  `one`, then `every`, `over`), keeping track of what each face holds by height band (a switch
  can sit over a baseboard). `over` cuts a room into rectangles (`geometry::zones`: the
  biggest rectangle of free cells on a grid through its corners, repeatedly) and spreads parts
  over them (`spread.rs`: a grid per zone by spacing, or a count shared by area). `connect`
  (`connect.rs`) takes a part on a fixture's `load` when it has none, then:
  - **control** networks (`role = 'control`) find each room's switches and lights; the lights'
    load goes on the room's first switch.
  - **feed** networks order terminals room by room from the source and cut them into groups
    (circuits) by `max`, voltage and load (≤ 80 % of the breaker); `dedicated` and `feeds`
    (`plate.feed`) give terminals their own group, breaker or medium. Groups are numbered across
    a scope's networks (one panel schedule). Each group's breaker is its `rating`, else the
    smallest standard one its loads need; its medium is the thinnest in `media` that meets the
    breaker and conductors (the same rule as assemblies). Each group is a tree (Prim on
    shortest-path distances) plus one home run, every joint one cable; a `Circuit` row goes to
    `Layout.circuits`.
  - Then the control networks are laid, sized to the circuit that feeds their switches, and
    the switches and lights are marked `fedBy` that service.
    Runs are laid on `route.rs`: a graph per network and scope. Walls the network can use (cavity
    ≥ `via.cavity`, `cross` not `'no`) have a low band (broken at doors) and, with `via.ceilings`,
    a high band over the headers, risers between them, and L-shaped ceiling edges between the
    high band and the ceiling parts of each room. Costs are lengths plus a little per riser. A
    laid path becomes `run` pieces by `along` (wall, header, ceiling, drop) with `marks` (riser,
    cross, firestop), offset sideways where earlier cables share a stretch. Every network is
    also drawn as the schematic (arcs, numbered home-run arrows); `alt` marks the one `draw`
    didn't ask for, which exports skip (`Element::exported`). Issues are merged per placement
    across rooms.
- **The fingerprint** leaves out a service's `place`, `connect`, `parts`, `params`, `media` and
  `feeds` (and `label`), a space's `load`, an assembly's `cross` and the plate's `height`: those never change a layout, so editing them doesn't re-solve or re-seed anything.
  They are part of the cache key (`view_key`, like symbols), so a cached child still gets the
  current parts. A service's `scope` and `claims` are in the fingerprint.
- **Output**: `wall`, `part` and `run` elements (`WallInfo`, `PartInfo`, `RunInfo`), with CAD
  layers (`PLATE-WALL-<ASSEMBLY>`, `PLATE-<SERVICE>`, `PLATE-<SERVICE>-RUN-<MEDIUM>`,
  `PLATE-<SERVICE>-LEG`); DXF and SVG draw all three, routed runs only (or arcs, if `draw`
  says so). `Layout.circuits` carries the panel schedule.
- **Library files.** A file that is a service (`parts`, `place`…) or a record of assemblies is
  told apart by shape (`FileKind`, the `by_shape` contract in the prelude): it's checked against
  its own definition, and the web app shows a card of it instead of solving it.

## Units

Lengths are resolved once, in Rust, when the config is read (`plate-core/src/units.rs`):

- `system` ('metric | 'imperial) is presentation: how the viewer and exports show lengths.
- `units` is the unit of plain numbers and of the output geometry. It defaults from `system`.
- A `Length` field accepts a number (in `units`) or a string with a unit (`"98'-6"`, `"9.5 m"`).
  New length fields should use the `Length` type so they get this for free.

The solver only ever sees plain numbers in `units`. The viewer converts them for display with
`Lengths` (`web/src/viewer/format.ts`).

## Adding things

**A new config concept** (say, a core):

1. Describe it in `schema/plate.schema.json`: type, description, default, error message, and
   `x-examples` / `x-docs` if they help. Nickel validation, the editor's docs and completions,
   and its page in the reference panel follow automatically.
2. Add the matching Rust type to `crates/plate-core/src/config.rs` (lengths use `Length`).
3. Make the solver use it (`solver.rs`), emitting new `Element`s with a new `ElementKind`. Set
   `Element::source` to the JSON pointer of the points it was drawn from, and the viewer's
   point editing works on it with no extra code.
4. Give the kind a CAD layer in `layout.rs`. Exports pick it up automatically.

**A new symbol**: add it to `schema/symbols.json` (shapes in millimetres, front at y = 0) and to
the `Symbol` enum in the schema, with a description starting with its label. Tests check that
the two lists match and that every shape draws.

**A new starting mode**: add a `$defs` entry with `mode` fixed by `const` and list it in
`Start.oneOf`; add the variant to `Start` in Rust and handle it in `solve::solve`. For a mode
the user draws, add a template in `web/src/editor/templates.ts` and a card on the start screen
(`web/index.html`).

**A new export format**: add a module in `plate-export`, a `Format` variant, and a menu item in
`web/index.html`. IFC is the obvious next one for BIM tools.

**A new rule**: add a `Rule…` definition to the schema (with `rule` fixed by `const`) and list it
in `Rule.oneOf`; add the variant to `Rule` in `plan.rs`; give it a cost term in the engines
(`grid.rs`: `C`, `rule_term`, `contribution`); optionally a helper in `schema/plate.ncl` (with
a `doc`; the reference lists it with the rule kind it writes, and its argument names come from
`PARAMS` in `web/src/editor/schema.ts`).

**A new engine**: write a function `(plan, slot, ctx) -> LevelResult` next to `grid.rs`, add an
`Engine` variant and dispatch to it in `Host::solve_plan`. Recursion, typicals, needs and
diagnostics come for free.

## User-supplied solvers (planned)

Third-party solvers will be WebAssembly components behind a WIT interface. They are sandboxed (no
file, network or clock access unless granted), metered, and language-agnostic. The host hands a
solver the resolved config as JSON and gets geometry back. Solvers never see Nickel, files or
exporters.

```wit
package plate:solver@0.1.0;

world solver {
  export describe: func() -> string;
  export solve: func(config-json: string, seed: u64) -> result<string, string>;
}
```

A user-supplied engine will solve one level, like the built-in ones: the host keeps recursion,
typicals and caching, so third-party engines stay small and can be mixed in one building (a
vendor's spine engine around your own room engine). The built-in `PlateSolver` stays as the
reference implementation.
