# Assemblies

An assembly is what the edges between spaces are built as: a wall type. It takes floor. Every space is drawn to its walls' faces, and the plans inside a space are solved in what its walls leave it. Without assemblies, edges stay lines, as before.

```nickel
assemblies = {
  exterior = { on = 'facade, thickness = "250 mm" },
  partition = { on = ['partition, 'party, 'wet], thickness = "100 mm" },
},
```

Or from a file: `assemblies = import "build/assemblies/wood-frame.ncl"`.

## Which wall gets which

Walls come from ports. Each edge two spaces share is one wall, and it has the port roles both sides see: a wall between a bath (`walls = 'wet`) and a hall is `'partition` to one and `'wet` to the other. Each assembly whose `on` names one of those roles is a requirement, and so is each `wall` rule about that wall. The wall is built as **the thinnest assembly that meets them all**: at least as thick, with a cavity at least as deep, ratings at least as high. A wall that's both `'party` and `'wet` is built as an assembly that is both, or the problems list says none is and suggests one.

An assembly without `on` is never asked for by a role, only by a rule: by name, or because it's the thinnest that meets what a rule asks.

## Build-ups and ratings

`layers` lists the build-up from one face to the other; the thickness and the cavity (layers marked `cavity`) add up from it. `fire` (hours), `acoustic` (STC) and `bearing` are what rules can ask for.

```nickel
partition = {
  on = 'partition,
  layers = [
    { material = "gypsum", thickness = "13 mm" },
    { material = "stud", thickness = "89 mm", cavity = true },
    { material = "gypsum", thickness = "13 mm" },
  ],
},
```

## Asking for more: the `wall` rule

```nickel
plate.wall "bath" "bedroom" & { acoustic = 45 },          # between these
plate.wall "#corridor" 'wall & { fire = 1 },               # every wall of a corridor
plate.wall "#room" 'wall & { cavity = "140 mm", length = 1.2 }, # one stretch is enough
plate.wall "bedroom_1" "bath" & { assembly = 'quiet },     # this one, by name
```

With `length`, the space's longest wall clear of doors is the one built so; the solver tries another layout if no wall is that long.

## On the boundary

A wall on the plate's outline grows inward: the outline is its outside face, as gross floor area is measured. `line = 'centre` straddles the outline instead (a party wall on a lot line).

## Inside a space

Each port inside a space also carries the name of the assembly its wall is built as. `against = 'wet` puts a fixture on the wall built as `wet`, whatever the roles around it.

## In the drawing

Walls are drawn by their `pattern`: `'solid`, `'hatch`, `'cross` or `'light`. Hover a wall to see what it is and why; right-click it to find its assembly in the file. They're exported on a layer each (`A-WALL-PARTITION`, or the assembly's `layer`).

Opened on its own, a file of assemblies shows each one, its build-up and which plates use it.
