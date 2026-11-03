# plate

Constraint-based floor plate layout solver. Write a floor as a [Nickel](https://nickel-lang.org/) config (an outline, the spaces you need and the rules between them), and plate lays it out as you type. Export to DXF, SVG or JSON.

The editor runs side by side: config on the left, live drawing on the right. Drag a corner in the drawing and the file is rewritten; edit the file and the layout re-solves.

## Quick start

Prerequisites:

- **Rust** 1.89+ via [rustup](https://rustup.rs). On Windows, also the Visual Studio C++ Build Tools (rustup offers them).
- **Node.js** 20+

```sh
git clone https://github.com/philipblaquiere/plate
cd plate
npm run dev
```

Open <http://localhost:5173>.

`npm run dev` installs npm dependencies, adds the `wasm32-wasip1` Rust target if missing, builds the browser engine, then starts the solver API (`:7878`) and Vite (`:5173`). The first run compiles a lot of Rust and takes a few minutes; the page says "Waiting for engine" until the API is up.

Your files live in `workspace/` (gitignored), seeded with the examples on first run.

## How it works

```
schema/plate.schema.json ──► Nickel contract
                                   │
BROWSER   .ncl files ──► Nickel + contract (WASM worker) ──► config JSON
──────────────────────────────────────────────────────────────┼──────────
SERVER                    validate (serde) ──► Solver ──► Layout ──► DXF / SVG / JSON
```

- **Config**: Nickel files that merge, override defaults, import each other and reference their own values.
- **Schema**: `schema/plate.schema.json` (JSON Schema 2020-12) is the single definition of the format. It drives validation, defaults, editor hover/completions and the in-app reference.
- **Solver**: recursive. A plan places spaces in a slot; a space with its own `plan` is solved inside it. Identical slots are solved once (typicals) and cached.
- **Engines**: `grid` (simulated annealing over cells, with a fill space for leftovers) and `spine_stack` (corridor along a spine, bays on each side).
- **Walls and services**: assemblies turn edges into walls with a thickness (the thinnest type that meets what each wall's roles and rules ask), and services (power, lighting, heat, plumbing) place their parts on the solved plan and connect them, each with its own view.
- **Report**: every solve is `perfect`, `solved`, `incomplete` or `empty`, with diagnostics that point back to the line to change.
- **Deterministic**: same file and seed, same layout.

## A minimal plate

```nickel
{
  name = "Studio",
  system = 'metric,
  units = 'm,
  start = {
    mode = 'boundary,
    points = [[0, 0], [8, 0], [8, 6], [0, 6]],
  },
  spaces = {
    living = { area.min = 30 },
    bath = { area = { min = 4, max = 6 }, side.min = 1.8 },
    entry = { kind = 'circulation, area = { min = 3, max = 6 }, side.min = 1.2 },
  },
  rules = [
    plate.door "entry" 'facade,
    plate.door ["living", "bath"] "entry",
    plate.window "living",
  ],
}
```

More in `examples/`: an apartment, a hotel, scenarios from a beach villa to an office tower floor, and a duplex with walls, power, heat, plumbing and lighting (`5-duplex.ncl`, built from `examples/build/`).

## Repo layout

| Path                  | What                                                               |
| --------------------- | ------------------------------------------------------------------ |
| `crates/plate-core`   | Typed config, units, geometry, solver, `Layout`. No I/O, no Nickel |
| `crates/plate-config` | JSON Schema → Nickel contract, evaluation, defaults, source maps   |
| `crates/plate-wasm`   | `plate-config` compiled to WebAssembly (`wasm32-wasip1`)           |
| `crates/plate-export` | `Layout` → DXF, SVG, JSON                                          |
| `crates/plate-server` | Axum API for the editor, plus the `plate` CLI                      |
| `web/`                | Editor: Vite, TypeScript, Monaco, SVG viewer                       |
| `schema/`             | `plate.schema.json`, `plate.ncl` helpers, `symbols.json` catalog   |
| `examples/`           | Plates seeded into a new workspace                                 |
| `docs/`               | User guide, architecture, reference guides shown in the editor     |

## Commands

```sh
npm run dev          # everything, with live reload
npm start            # one optimised binary serving the app on :7878
npm test             # cargo test + wasm build + tsc + vitest
npm run build:wasm   # rebuild the browser engine after changing plate-config or the schema
npm run format       # prettier + rustfmt

cargo run -p plate-server --bin plate -- check examples/3-hotel.ncl -v       # solve from the CLI
cargo run -p plate-server --bin plate -- export examples/1-studio.ncl dxf -o studio.dxf
```

## Docs

- [docs/GUIDE.md](docs/GUIDE.md): using the app and writing plates
- [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md): how the pieces fit, and where to add things
- [AGENTS.md](AGENTS.md): commands, rules and known traps for contributors

## Roadmap

- User-supplied solvers as sandboxed WebAssembly components behind a WIT interface
- IFC export for BIM tools
