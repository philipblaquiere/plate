# plate

Write a floor plate as a configuration file: an outline, the spaces you need and the rules
between them. A solver lays it out as you type. Export it to CAD.

```
 ┌──────────────────────────────┬─────────────────────────────────────┐
 │ plate.ncl                    │   ┌──── 24.00 ────┐                 │
 │                              │   │///////////////│                 │
 │ {                            │   │///////////////└───── 18.00 ───┐ │
 │   name = "Typical floor",    │ 30.00 ////////////////////////////│ │
 │   system = 'metric,          │   │/////////////////////////////// │ │
 │   start = {                  │   └──────────── 42.00 ────────────┘ │
 │     mode = 'boundary,        │                ┌──────────────────┐ │
 │     points = [               │                │ Typical floor    │ │
 │       [0, 0], [42, 0], ...   │                │ Area 1,044.00 m² │ │
 │     ],                       │                └──────────────────┘ │
 │   },                         │                                     │
 │ }                            │                                     │
 └──────────────────────────────┴─────────────────────────────────────┘
```

## Run it

You need:

- **Rust** (stable, 1.89 or newer) from <https://rustup.rs>. On Windows, rustup also asks for the
  Visual Studio C++ Build Tools. Install them; the Nickel formatter compiles a bit of C.
- **Node.js 20+** from <https://nodejs.org>.

```powershell
npm install
npm run dev
```

Then open <http://localhost:5173>.

`npm run dev` first builds the browser half of the engine: Nickel and the configuration schema,
compiled to WebAssembly (it adds the `wasm32-wasip1` Rust target if it's missing; the
first build takes a few minutes). Then it starts two processes side by side:

- `api`: the solver on `127.0.0.1:7878`. The first build takes a few minutes; the page shows
  "Waiting for engine" until it's ready, then connects by itself. Errors in your file show up
  even before that, because they're checked in the browser.
- `web`: Vite on `:5173`, which proxies `/api` to the solver.

After changing Rust code in `plate-config` or the schema, restart `npm run dev` (or run
`npm run build:wasm`) so the browser picks it up.

Your files live in the **workspace**, a folder on disk the server reads and writes: `workspace/`
next to where you run it (created with the examples on first run), or any folder with
`plate serve --workspace <dir>`. Edit those files with any other tool too; reload the page to see
outside changes.

For a single optimised binary that serves everything on <http://127.0.0.1:7878>:

```powershell
npm start
```

## Starting a plate

When the app opens, pick how to start. Each choice asks you to draw on the sheet, then writes the
configuration for you.

| Mode         | You draw               | The solver                  | Written as                     |
| ------------ | ---------------------- | --------------------------- | ------------------------------ |
| **Boundary** | A closed outline (≥ 3) | Places everything inside it | `mode = 'boundary`, `points`   |
| **Spine**    | A line of points (≥ 2) | Works around it             | `mode = 'spine`, `points`      |
| **Free**     | One point: the origin  | Starts from that point      | `mode = 'spine` with one point |

While drawing: click to place points; the cursor snaps to the grid (`Alt` turns it off) and
`Shift` snaps to 45°. Close a boundary by clicking its first point; finish a spine with `Enter`
or by clicking its last point again. `Backspace` removes the last point, `Esc` cancels. Lines
are straight for now.

**New** brings the start screen back. `1`, `2`, `3` pick a mode there; `Esc` returns to the
file you were on.

## Shaping it afterwards

The drawing and the file stay in step both ways:

- **Drag a point** in the drawing. Its two coordinates are rewritten in the file as you move,
  highlighted, and the whole drag is one undo step (`Ctrl+Z` in the editor). `Esc` cancels.
  If a coordinate comes from a `let` binding, the binding is rewritten. If it's computed
  (`w - 12`), the drag is refused and you're pointed at the line.
- **Drag an edge** to move the whole wall. While the button is down a ghost shows the outline as
  it would be, with its dimensions, how far the edge moved and the new area; the file is only
  written when you let go, as one undo step. The edge stays parallel and its ends slide along the
  edges beside it, so on a rectangular plate it's a straight push or pull. `Shift` moves it
  freely, `Alt` turns off snapping, `Esc` cancels. A drop that would make the outline cross
  itself or turn inside out is refused.
- **Double-click an edge** to add a point there; **right-click a point** (or hover it and press
  `Delete`) to remove it. The `points` list is edited in place.
- **Edit the coordinates** in the file and the drawing follows.

The checks that JSON Schema can't express (a repeated point, a boundary that crosses itself or
has no area) underline the offending point.

## Spaces, rules and goals

A file describes a program; the solver finds the layout. Each starting mode writes a starter
program sized to what you drew, so there is something to change straight away.

```nickel
{
  start = { mode = 'boundary, points = [[0, 0], [14, 0], [14, 9], [0, 9]] },

  spaces = {
    living = { area.min = 24 },
    kitchen = { area = { min = 9, max = 14 } },
    hall = { kind = 'circulation, area = { min = 6, max = 12 }, side.min = 1.2 },
    bedroom = { count = 2, area = { min = 11, max = 16 }, side.min = 2.8 },   # bedroom_1, bedroom_2
    bath = { area = { min = 4.5, max = 7 }, side.min = 1.8 },
  },

  rules = [
    plate.touches "kitchen" "living",
    plate.door "bedroom" "hall",            # each bedroom gets its own door to the hall
    plate.between "bedroom" "hall",         # the hall keeps the bedrooms apart
    plate.window "bedroom",                 # on a 'facade edge of the boundary
    plate.prefer (plate.window "kitchen"),  # nice to have, not required
  ],

  goals = { utilization = 1, compactness = 0.3 },
}
```

- **Spaces** are named by their key. `kind` is `'room`, `'circulation`, `'zone` (floor without
  walls), `'void`, `'vertical` or `'fixture` (see [Fixtures and symbols](#fixtures-and-symbols));
  sizes are ranges (`area`, `side`, `width`, `depth`, `aspect`). `count = 3` makes `room_1` to
  `room_3`; `count.min = 1` lets the solver place as many as fit. Instances are numbered by
  position (left to right, or along the spine), so names stay put as the layout changes.
- **Rules** are written with the `plate` helpers, which every file can use: `touches`, `door`,
  `window`, `apart`, `between`, `through`, `reach`, `near`, `along`, `unique`, `instance`,
  `prefer`. Names can be a space (`"kitchen"`), a type (`"bedroom"` is every bedroom), a pattern
  (`"bed*"`), or a list. A rule is required unless wrapped in `plate.prefer`. Type `plate.` in the
  editor for the list, with docs.
- **Goals** weigh solutions once every required rule holds.

A rule the solver can't meet shows up as a warning, underlined where the rule is written, with
what to change right under it in the problems list.

### Filling the floor, and the verdict

A finished plan leaves no floor to nobody. `fill = true` makes a space take whatever the others
leave, in any shape: the hall that reaches every room, the open office around the meeting
rooms, the garden around a house. Its `side.min` is its narrowest passage, and it can have a plan
of its own, solved in its real shape.

```nickel
hall = { kind = 'circulation, fill = true, area = { min = 8, max = 20 }, side.min = 1.2 },
```

- `kind = 'zone` is floor without walls (the desks of an open office, the kitchen end of a
  living room): where zones meet, the edge is open.
- `walls = 'wet` (or `'outdoor`, `'facade`…) says what a space's walls are to its neighbours.
- `at = [x, y]` fixes a space where it stands (a core, a wing), with an exact width and depth.
- `row = true` places fixtures side by side as one run (desks, lifts, cars).
- `plate.touches "gallery" 'door & { each = true }` reaches every door the parent put in the
  space's walls, not just one; `plate.faces "sofa" "tv"` turns one fixture to face another.

Every solve ends with a **verdict**, on the drawing and in the status bar: **Perfect** (every
rule and preference holds, no floor left over), **Solved** (every requirement holds; a preference
is missed or some floor is left over, hatched in red) or **Incomplete** (something isn't met).
Each finding says what to change. The search is stochastic: **Try other seeds** on the verdict
solves with the next few seeds and offers the best. See the reference guides _Filling the floor_
and _Reading the result_.

### Examples

A new workspace starts with these (`examples/`):

| File                               | What it shows                                                                  |
| ---------------------------------- | ------------------------------------------------------------------------------ |
| `1-studio.ncl`                     | The basics: an outline, three spaces, a door and a window                      |
| `2-apartment.ncl`                  | Edge roles, a fill hall, `count`, a furnished bedroom plan, preferences, goals |
| `3-hotel.ncl`                      | A whole ground floor across files: wings of typical rooms, a café, a lobby     |
| `scenarios/small-apartment.ncl`    | A furnished one-bedroom flat with an open-plan living room and kitchen (zones) |
| `scenarios/family-house.ncl`       | An L-shaped house with a garage                                                |
| `scenarios/beach-villa.ncl`        | A villa on its plot: terrace, pool, parking and a garden that takes the rest   |
| `scenarios/mansion.ncl`            | Three pinned wings round a courtyard, suites, galleries meeting at their doors |
| `scenarios/office-building.ncl`    | A small office floor: core, meeting rooms, benches of desks                    |
| `scenarios/office-tower-floor.ncl` | A tower floor round a pinned core, team zones in an open floor                 |
| `4-walls.ncl`                      | Walls with thickness: two assemblies, rooms drawn to their faces, lighting     |
| `5-duplex.ncl`                     | Two flats on wood-frame assemblies, with power, heat, plumbing and lighting    |

The plans they share are in `plans/` and `plans/rooms/` (bedroom, kitchen, living room).
`build/` has what plates are built with: wall types in `build/assemblies/` (`basic.ncl` and a
layered `wood-frame.ncl`), services in `build/services/` (`lighting.ncl`, the simplest; `power.ncl`,
written in terms of a code's numbers), and the codes' numbers in `build/codes/`.

## Files, tabs and imports

The editor works like VS Code on the workspace folder:

- **Side bar** on the left, with two views: the files (`Ctrl+Shift+E`) and the reference
  (`Ctrl+Shift+H`). `Ctrl+B` hides it; drag its edge to resize.
- **Explorer**. Right-click a file to open it to the side, rename or move it (type a path such as
  `plans/room.ncl`), duplicate, delete, or copy an import line for it. The `+` makes a new file;
  folders are created as needed.
- **Reference**: everything you can write, generated from the schema, the `plate` helpers and the
  guides in `docs/reference/`. It follows the cursor: click into `area` and it shows the space's
  fields with `area` highlighted; click a `'facade` and it explains that port role. The pin stops
  it following. Search finds fields, values, rules and helpers by name. **Insert** puts an
  example or a field where it belongs near the cursor: a field into the record the page is about,
  a rule into the list you're in. Hovers have an **Open in reference** link.
- **Tabs**, with a dot for unsaved changes. `Ctrl+S` saves the file in front of you, shift-click
  **Save** saves all. Unsaved text survives a reload (it's kept in the browser).
- **Side by side**: the split button on a tab bar or `Ctrl+\` opens the file in a second editor
  group. Drag tabs between groups, or onto the right side of a group to split.

A file with a `start` is a **plate**: something to draw (marked with a diagonal). A file with
`spaces` but no `start` is a **plan**, meant to be imported, and it can be drawn on its own too.
Opening a plate shows it. Opening a plan the drawn plate uses keeps the plate, so you can edit a
room plan on the left and watch every room of the hotel re-solve on the right; opening any other
plan shows it on its own (see [Plans on their own](#plans-on-their-own)). The chip at the top
right of the sheet says which file is drawn, and switches to another.

Share a plan between plates with Nickel's `import`, relative to the importing file:

```nickel
let hotel_room = import "plans/guest-room.ncl" in   # see examples/3-hotel.ncl
{
  spaces = { room = { count.min = 1, plan = hotel_room } },
  ...
}
```

Name the binding differently from the space that uses it: inside `spaces`, `room` would refer to
the space itself. Every file can use `plate.*`. Errors in an imported file are reported in that
file, and moving or renaming a file updates the imports that point at it.

### Plans inside spaces

Give a space a `plan` and it is solved inside that space once placed, as deep as you like. A plan
is portable: it never names its parent's spaces, only **ports**, the labelled edges of the space
it is placed in.

```nickel
let hotel_room = {
  needs = { width = { min = 3.4, max = 4.5 }, ports = { entrance = 'required, facade = 'required } },
  spaces = { entry = { kind = 'circulation, area.min = 3 }, bath = { area.min = 4 }, sleep = { area.min = 14 } },
  rules = [
    plate.door "entry" 'entrance,           # a port of the room, not the corridor
    plate.window "sleep",
    plate.prefer (plate.touches "bath" 'wet),
  ],
} in
{
  start = { mode = 'spine, points = [[0, 0], [60, 0]] },
  spaces = {
    corridor = { kind = 'circulation, width = 1.8 },
    room = { count.min = 1, depth = 7.5, plan = hotel_room },
  },
  rules = [plate.along "room" "corridor"],
}
```

- **Ports.** The parent gives each space ports from geometry: `'entrance` on the corridor,
  `'facade` on the outside, `'party` between neighbours of the same type, `'wet` on every other
  party wall (so bathrooms land back to back), `'partition` elsewhere. The boundary's own edges
  are `'facade` unless `start.ports` says otherwise; right-click an edge in the drawing to set it.
  Roles are dotted paths: `'facade.storefront` is a kind of `'facade`.
- **Needs** are what a plan asks of its space. The parent sizes spaces to fit them, and a plan
  rejects a space that doesn't (with a warning).
- **Typicals.** Rooms whose slots match (same size and ports, after turning and mirroring) are
  solved once and share the solution: "Type A". It's fast, and it's how buildings are drawn.
  `typical = false` on a space solves every instance on its own; `plate.unique "room_7"` does it
  for one (right-click a room → Make unique). `plate.instance "room_1" accessible_room` gives
  particular instances another plan. `mirror = false` stops copies from being mirrored.

### Fixtures and symbols

A **fixture** is the last level of a plan: something that takes up floor and holds nothing else.
A bed, a counter, an island, a bath, a pool, a parking pad. It can't have a `plan`.

```nickel
spaces = {
  bed = {
    kind = 'fixture,
    width = 1.53,                    # along its front (the foot of the bed)
    depth = 2.03,                    # front to back: drawn at exactly this size
    against = 'wall,                 # back against an edge of the room ('wet, 'facade… for one kind)
    clearance = { front = 0.9, left = 0.6, right = 0.6 },
    symbol = 'bed_queen,
  },
  island = { kind = 'fixture, width = { min = 1.2, max = 3 }, depth = 1, clearance = 1.05 },
},
```

- **Front.** `width` runs along the front, `depth` from front to back. The solver turns fixtures
  to fit; the drawing marks the front with a heavier line.
- **Clearance** is clear floor around it, per side or all round. Nothing else may stand in it and
  it stays inside the space; clearances of different fixtures may overlap (a shared walkway). A
  fixture with a size range grows until its clearance meets something.
- **Doors and windows.** A space knows where its doors and windows are (`'door` and `'window`
  ports from its parent). Fixtures keep a door's swing clear; keep one off a window with
  `plate.apart "bed" 'window`, or off any wall with a window with `& { wall = true }`.
- **Grid.** A plan with fixtures is solved on the coarsest round grid (250, 200 or 100 mm; 9 to
  4 in) its fixtures' sizes and clearances fit, unless it sets `grid`.

A **symbol** is a drawing from the catalog (`schema/symbols.json`): about 80 of them, indoors and
out: beds, sofas, tables, kitchens, bathrooms, laundry, office, stairs, cars, a boat, pools, hot
tubs, courts, patios, decks, trees and lawns. Any space can have one; it only changes the drawing,
never the solution. It's drawn from the space's front and fitted to it. The **Symbol** page of the
reference shows the catalog (click one to insert it), and right-clicking a space in the drawing
offers the symbols that suit its name. Symbols are exported too (DXF layer `PLATE-SYMBOL`).

`examples/plans/rooms/` has a bedroom, a kitchen and a living room to furnish any plate's rooms;
`examples/scenarios/beach-villa.ncl` lays out a plot.

### Plans on their own

A plan file is drawn in a **slot**: a rectangle with ports, solved exactly as a plate would solve
the plan in a space of that size and ports. The bar under the file chip says which slot:

- **A room of a plate.** Right-click a room in the plate's drawing → **Open its plan here**. The
  plan opens in that room's slot, with the plate's seed, so it matches the room exactly. **Save
  as preview** keeps the slot in the file; **Back to …** returns to the plate.
- **`preview` in the file**, one slot or a list of named ones to switch between:

  ```nickel
  preview = [
    { name = "typical", width = 4, depth = 7.5 },
    { name = "corner", width = 4, depth = 7.5, ports.left = 'facade },
  ],
  ```

  Sides left out are a room on a corridor: `'entrance` below, `'facade` above, `'party` either
  side (the left one `'wet` too). `system`, `units` and `seed` stand in for the plate's.

- **Nothing**: a slot sized from `needs` (or the spaces' areas). **Set size** writes it down.

Drag the slot's corners to resize it; the file follows. A slot that misses the plan's `needs` is
drawn empty, with a warning on the need. `preview` is ignored when the plan is used in a space,
and never changes a plate's solution.

Engines: `'grid` (default) packs rectangles on a grid inside the space; `'spine_stack` (default
in spine mode) lines spaces up along a corridor. Rectangles only, for now. The same file and
`seed` always give the same layout.

## Walls and services

So far edges are lines. Give a plate **assemblies** and they're built: each edge becomes a wall
of a type, with a thickness, and every space is drawn to its walls' faces. Give it **services**
and power, lighting, heat or plumbing is placed on the result and connected.

```nickel
{
  start = { ... },
  assemblies = import "build/assemblies/wood-frame.ncl",
  services = {
    lighting = import "build/services/lighting.ncl",
    power = (import "build/services/power.ncl") & (import "build/codes/nec.ncl"),
  },
  spaces = { bath = { walls = 'wet, tags = ['wet], ... }, ... },
}
```

- **Assemblies** are picked by port role: a wall gets the thinnest assembly that meets what both
  sides' roles ask (`on`) and what any `plate.wall` rule asks (a fire rating, an STC, a cavity
  deep enough for a pipe). Walls take floor, so a plan inside a room is solved in the clear
  floor its walls leave it, and its `needs` are clear sizes: the room is made big enough for
  them and for the walls it gets. Boundary walls grow inward; `line = 'centre` puts a party wall on the
  line between two plates.
- **Services** never move a wall. They place **parts** (receptacles, switches, lights, panels,
  stacks) where the solved plan says: `plate.every` along walls, `plate.beside` doors or windows,
  `plate.over` a room's floor or ceiling, `plate.on` the back of a fixture, `plate.one` in the
  best place. **Networks** connect them: circuits drawn the electrical way (arcs and numbered
  home runs) or runs routed through walls with a deep enough cavity. What a service needs from
  the layout (a stretch of wall for the panel, a drain in each bath) it **claims**, and the
  solver makes room for it like any rule.
- **Tags** let a service fit any plan: it names `"#wet"` or `"#dwelling"`, and each plan tags its
  spaces (`tags = ['wet]`). A space's kind is a tag too: `"#room"`, `"#circulation"`.

In the drawing, walls are drawn with their hatch; hover one for its assembly and why it's that
one. A **views bar** at the top left switches between the plan and each service (`S` cycles):
in a service's view the plan is faded, its parts and runs are drawn with a legend, and the
service's unmet requirements are counted on its chip. Hover a part for what it is and how it got
there; right-click a wall or a part for **Show in file**. Opening an assemblies or service file
on its own shows a card of what's in it, and which plates use it.

See the reference guides _Assemblies_, _Services_ and _Tags_.

## Using it

| Where   | What                                                  | How                                                    |
| ------- | ----------------------------------------------------- | ------------------------------------------------------ |
| Editor  | Live solve                                            | Type. The drawing updates about 100 ms after you stop. |
|         | Errors                                                | Red squiggle and a problems list. Click to jump.       |
|         | Docs and completion                                   | Hover a field; `Ctrl+Space` for suggestions            |
|         | Reference (follows the cursor)                        | `Ctrl+Shift+H`, or the book in the side bar            |
|         | Format                                                | `Shift+Alt+F` or the Format button                     |
|         | Save / save all                                       | `Ctrl+S` / shift-click Save                            |
|         | Add a file from your computer                         | Open (`Ctrl+O`): copies it into the workspace          |
|         | Files / side bar / split                              | `Ctrl+Shift+E` / `Ctrl+B` / `Ctrl+\`, drag tabs        |
| Drawing | Pan / zoom                                            | Drag, scroll, pinch. `F` or double-click fits.         |
|         | Inspect                                               | Hover an edge, corner, space or face                   |
|         | Make unique / port roles                              | Right-click a space / a boundary edge                  |
|         | Plan or a service's view                              | Views bar, top left; `S` cycles                        |
|         | Where a wall or part comes from                       | Hover it; right-click → Show in file                   |
|         | Measure                                               | `M`, click two points (snaps to corners and midpoints) |
|         | Dimensions / grid                                     | `D` / `G`                                              |
| Export  | DXF (AutoCAD 2000), SVG (1:100 or 1/8" = 1'-0"), JSON | Export menu                                            |

Unsaved changes are kept as drafts in the browser, so a refresh never loses them. A new plate
drawn from the start screen becomes `plate.ncl` (then `plate-2.ncl`, …) in the workspace.

## Metric and imperial

Two settings, both in the file:

- `system = 'metric | 'imperial` decides how lengths are shown: `30.00 m`, or `98'-6"`. Areas
  follow (m² or ft²), and so do the export's units and print scale. The **Metric | Imperial**
  switch in the top bar edits this line for you.
- `units` is the unit of plain numbers like the `30` in `[30, 0]`. It defaults to metres in metric and
  feet in imperial.

Any length can also be a string with its own unit. Strings are converted exactly:

```nickel
[0, 0], ["98'-6", 0],       # feet and inches (the closing " is optional)
["98 ft 6 1/2 in", "30 m"],  # units can be mixed
["9150 mm", "30 m"],
```

The switch never changes the size of your plate. If the file doesn't say what unit its plain
numbers are in, the switch writes the current one into `units` before changing `system`.
Switch any example to imperial to see it in feet and inches.

## The configuration format

`schema/plate.schema.json` defines it, as standard JSON Schema: every field, its type, allowed
values, default, documentation and error message. Edit that one file and Nickel validation,
defaults and the editor's hover docs and completions all follow. See
[ARCHITECTURE.md](ARCHITECTURE.md#the-schema).

## Command line

```powershell
cargo run -p plate-server --bin plate -- check examples/2-apartment.ncl -v  # verdict, spaces, what's unmet
cargo run -p plate-server --bin plate -- check examples/3-hotel.ncl --seed 4  # imports resolve; another seed
cargo run -p plate-server --bin plate -- export examples/1-studio.ncl dxf -o studio.dxf
cargo run -p plate-server --bin plate -- eval examples/1-studio.ncl   # the checked data, as JSON
cargo run -p plate-server --bin plate -- schema                       # the generated Nickel contract
cargo run -p plate-server --bin plate -- serve --workspace D:\plans   # serve another folder
```

## Develop

```powershell
npm test             # cargo test + WebAssembly build + TypeScript check + vitest
npm run format       # prettier + rustfmt
cargo clippy --workspace --all-targets
```

How the pieces fit, and where to add things: [ARCHITECTURE.md](ARCHITECTURE.md).
Notes for coding agents (commands, rules, known traps): [AGENTS.md](../AGENTS.md).
