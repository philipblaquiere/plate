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
- `plate.over part spaces`: in the middle, or a grid (`per` area each, or `spacing`).
- `plate.on part fixtures`: at the back of each.
- `plate.one part spaces`: the best place on its walls, `near` and `apart` from port roles, `'door`, `'window`, `'entrance` or tags, on a wall that is at least `wall`.

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
  { label = "Circuits", from = "panel", to = ["outlet", "heat.baseboard"], max = 12, via.cavity = "89 mm" },
  { label = "Drains", from = "stack", to = "#drain", via.cavity = "140 mm", limit = "4 m", draw = 'routed },
],
```

A network joins parts (of this service, of another as `"service.part"`, or tagged fixtures) back to its source, room by room, in groups of at most `max` (circuits). `'arcs` draws the electrical convention: an arc from part to part and a numbered home run. `'routed` runs in the walls whose cavity is at least `via.cavity`, and across the ceilings of `via.ceilings`, by the shortest way, each to its nearest source; `limit` caps a run. What stands against a wall joins that wall, or crosses its room's ceiling if `via.ceilings` allows it, never the floor to another wall. What's on the floor or ceiling joins the nearest wall of its room.

## Scope

`scope = "#dwelling"` gives each such space its own parts and networks (a panel per flat), and its claims apply inside them. Without it, the service covers the plan it's declared in. Plans inside inherit services; a plan that names one the same replaces it for itself.

## Parameters

`params` holds the numbers a service is written in terms of (a code's spacings). Give them `| default` and a merge replaces them: `(import "build/services/power.ncl") & (import "build/codes/nec.ncl")`.

## In the drawing

Above the drawing, **Plan** shows the plan on its own and each service its own view (S cycles them): the plan fades, the service's parts and networks are drawn over it, with a legend of what it placed. A badge counts its problems. Hover a part to see why it's there; right-click it to find its placement in the file.

Opened on its own, a service file says what it places, claims and connects, and which plates use it.
