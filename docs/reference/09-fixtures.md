# Fixtures

A fixture is the last level of a plan: something that takes up floor and holds nothing else. A bed, a counter, a kitchen island, a bath, a pool, a parking stall. Write it as a space with `kind = 'fixture`. It can't have a `plan`.

```nickel
spaces = {
  bed = {
    kind = 'fixture,
    width = 1.6,                    # along its front (the foot of the bed)
    depth = 2.1,                    # front to back
    against = 'wall,                # its back goes against an edge of the room
    clearance = { front = 0.9, left = 0.6, right = 0.6 },
  },
  island = {
    kind = 'fixture,
    width = { min = 1.2, max = 3 }, # grows while there's room around it
    depth = { min = 0.9, max = 1.2 },
    clearance = 1.05,               # all round
  },
},
```

## Front, width and depth

A fixture has a front: the side you use it from. `width` runs along the front and `depth` from the front to the back. The solver turns fixtures to fit, and the drawing marks the front with a heavier line.

A fixed size is drawn exactly (a 1.53 m bed is 1.53 m, not rounded to the grid). A range lets the fixture grow while the goals push it to, as far as its clearance allows.

## Clearance

`clearance` is clear floor kept around a fixture: one length on every side, or `{ front, back, left, right }`. Left and right are as seen standing at the front, facing it. Nothing else may stand in a clearance, and it must stay inside the space. Clearances of different fixtures may overlap: they share the walkway, like the aisle between a counter and an island.

## Against an edge

`against = 'wall` puts the fixture's back against any edge of the space it's in. Give a port role instead to ask for a particular edge: `against = 'wet` for a vanity on the plumbing wall, `against = 'facade` for a desk under the window.

## The grid

Fixtures need a finer grid than rooms. A plan with fixtures is solved on the coarsest round grid their sizes and clearances are whole numbers of (250, 200 or 100 mm; 9, 8, 6 or 4 in), unless it sets `grid` itself. A plan of big rooms only (no fixtures, every side 2 m or more) may be solved on a coarser grid when its slot is large.

## Windows and doors

A room knows where its parent put its doors and windows: they're `'door` and `'window` ports. Fixtures keep a door's swing clear on their own. To keep one off a window, or off the whole wall a window is in:

```nickel
rules = [
  plate.apart "bed" 'window,                   # the bed doesn't touch a window
  plate.apart "bed" 'window & { wall = true }, # nor any wall that has one
  plate.apart "desk" 'door & { distance = 0.3 },
],
```

Other rules work as usual: `plate.touches "fridge" "counter"`, `plate.near "sofa" "tv" 3`, `plate.apart "desk" "bed"`. A rule can't ask for what a clearance forbids: a nightstand can't touch the side of a bed that keeps clearance on that side.
