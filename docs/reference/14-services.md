# Services

A service runs through the spaces and their walls: power, lighting, heat, plumbing, air. It never builds anything. It **claims** what it needs, and the solver makes room; then it **places** its parts on the solved walls, floors and ceilings, and **connects** them.

```nickel
services.lighting = {
  parts = {
    light = { mount = 'ceiling, symbol = 'light },
    switch = { above = "1100 mm", symbol = 'switch },
  },
  place = [
    plate.over "light" "#room",
    plate.beside "switch" 'door,
  ],
},
```

Or from a file: `services.power = import "build/services/power.ncl"`.

## Parts

A part goes on a wall (`above` the floor), on the ceiling, on the floor, or in a wall's cavity (`mount = 'cavity`: a stack, a riser). It takes no floor: something that does (a furnace) is a fixture, and a service claims it. Its size defaults to its symbol's.

## Where parts go

- `plate.every part spaces spacing`: along each wall run (the perimeter, cut at doorways), at most `spacing` apart and half that from each end. On fixtures, along their backs.
- `plate.beside part 'door` (by the latch, or `side = 'hinge`) or `'window` (under it, as wide as it).
- `plate.over part spaces`: on the ceiling or floor. Alone, one in the middle of the space's biggest part; with `count`, `spacing` or `per`, spread over all of it (see below). A selector that names fixtures puts them over the fixtures: `plate.over "pendant" "#island" & { count = 3 }`.
- `plate.on part fixtures`: at the back of each.
- `plate.one part spaces`: the best place on its walls, `near` and `apart` from port roles, `'door`, `'window`, `'entrance` or tags, on a wall that is at least `wall`.

### Spreading parts over a room

A room that isn't a rectangle is cut into rectangles first, biggest first: an L is its body and its arm, a U three pieces. Pieces narrower than 600 mm, or smaller than a tenth of the room, get nothing of their own. Then:

```nickel
plate.over "light" "#room",                                                   # one, in the middle of the biggest piece
plate.over "downlight" "living" & { count = 6 },                              # exactly 6, shared by area
plate.over "downlight" "kitchen" & { spacing = "1.5 m" },                     # at most 1.5 m apart, about 0.75 m from walls
plate.over "light" "#room" & { per = 16, count.max = 4 },                     # one per 16 m², no more than 4
plate.over "downlight" "#circulation" & { spacing = "2.4 m", layout = 'line }, # a line down each leg of a hall
```

- `spacing`: each piece gets a grid at most that far apart, each part in the middle of an equal share, so about half that from the walls. A piece narrower than `spacing` gets a line.
- `count`: exactly that many, shared between the pieces by area, so each part lights about as much floor. Each piece lays its share out as evenly as it can: 6 in a 6 × 4 room is 3 × 2, 7 in a square is rows of 2, 3 and 2. Fewer than pieces: one each in the biggest. As `{ min, max }`, it bounds what `spacing` or `per` gives.
- `per`: one per that much floor (m²).
- `layout`: `'grid` (the default), `'line` (one row along each piece), `'centre` (one in the middle of each piece: an L gets two).
- `margin`: never closer than this to a wall, where the shares leave room (300 mm by default).

Parts never overlap: if more are asked for than fit, as many as fit are placed, and the problems list says how many.

Parts of every service share the walls: anchored ones go first (`on`, `beside`, `one`), then `every` and `over` flow around them. A switch can go over a baseboard: parts 300 mm or more apart in height share a stretch of wall. A part that can't be placed is a problem, in the room it's missing from.

Placing never changes the layout, so it isn't part of a plan's identity: changing an outlet's spacing redraws the outlets, not the rooms.

## Claims

```nickel
claims = {
  rules = [plate.wall "#wet" 'wall & { assembly = 'wet, length = 1.5 }],
  inside."#utility".spaces.heater = { kind = 'fixture, width = 0.6, depth = 0.6, against = 'wall },
},
```

`rules` are added to every plan the service covers; name spaces by tag, so they fit any plan. `inside` merges a plan piece (spaces, rules, needs) into each space the key selects. Claims are solved like rules written in the plan: a problem with one points at the claim. Needs a claim brings add to the plan's own, so both hold; if they can't, the problems list says so.

## Networks

```nickel
connect = [
  { label = "General", from = "panel", to = ["outlet", "lighting.switch"], rating = "15 A", max = 12, via = { cavity = "89 mm", ceilings = "#room" } },
  { label = "Drains", from = "stack", to = "#drain", via.cavity = "140 mm", limit = "4 m" },
],
```

A network joins parts (of this service, of another as `"service.part"`, or tagged fixtures) back to its source, room by room, in groups (circuits): at most `max` in each, at most 80 % of `rating` in load, and one each when `dedicated`. Each group is a tree of runs, as an electrician wires it, plus one home run to the source; `limit` caps how far a terminal is from the source. `role = 'control` joins switches to the lights they control instead, room by room. See [cables](cables) for loads, breakers and cable sizes, and [routing](routing) for where runs go: in the walls whose cavity is at least `via.cavity`, up and over doors and across the ceilings of `via.ceilings`, never across a floor.

Every network is drawn two ways: where it runs, and the schematic (an arc from part to part and a numbered home run, the electrical convention). Press A in a service view to switch. `draw` says which one exports keep.

## Scope

`scope = "#dwelling"` gives each such space its own parts and networks (a panel per flat), and its claims apply inside them. Without it, the service covers the plan it's declared in. Plans inside inherit services; a plan that names one the same replaces it for itself.

## Parameters

`params` holds the numbers a service is written in terms of (a code's spacings). Give them `| default` and a merge replaces them: `(import "build/services/power.ncl") & (import "build/codes/nec.ncl")`.

## In the drawing

Above the drawing, **Plan** shows the plan on its own and each service its own view (S cycles them): the plan fades, the service's parts and networks are drawn over it, with a legend of what it placed. A badge counts its problems. Hover a part to see why it's there, and a cable to see its circuit, what it's made of and where it runs; right-click either to find it in the file. A service whose circuits feed another's parts (power feeding the lighting's switches) shows them too, and its legend has a panel schedule.

Opened on its own, a service file says what it places, claims and connects, and which plates use it.
