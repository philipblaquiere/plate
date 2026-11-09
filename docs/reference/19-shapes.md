# Shapes: rooms cut by the outline

An outline isn't always square. A tower's corner is cut at an angle, a facade steps back, a dimension like 15'-6 5/16" doesn't fall on the grid. Rooms there take the floor right up to the wall, like they would on a drawing: each is placed as a rectangle, and the outline cuts it.

```nickel
start = {
  mode = 'boundary,
  points = [[0, 0], [24, 0], [24, 6], [19.4, 9.8], [0, 9.8]], # the top right corner is cut at an angle
},
spaces = {
  living = { area = { min = 40, max = 60 }, side.min = 5 },  # reaches the angled wall if it's there
  store = { area = { min = 2, max = 4 }, shape = 'rect },     # stays square
},
```

## What a cut room is

- **Placed as a rectangle.** The solver moves rooms around as rectangles on its grid, as always. A room's rectangle may reach across a slanted edge of the outline; the part beyond it is cut away.
- **Its floor is what's left.** The room is drawn, walled and furnished as the floor it has: a rectangle with a corner cut off, a trapezoid. Its `area` is that floor.
- **Its sizes are its rectangle's.** `width`, `depth`, `side` and `aspect` are measured on the rectangle, so a bedroom with `side.min = 3` is at least 3 m across both ways, even where the angled wall makes one corner narrower.
- **It keeps at least half its rectangle.** No room is a sliver in a sharp corner.
- **It doesn't reach across a step.** Only slanted edges are cut across. Where the outline steps (a notch, a neighbour's unit), a room stays on its own side, so it's never cut in two.

## Edges off the grid

The solver's grid starts at the outline's lower-left corner, so an edge at 14.6 m on a 0.5 m grid is between two grid lines. The rooms along it reach it all the same, and the fill (`fill = true`) takes the floor up to it too: no thin strip is left over.

## `shape`

| `shape`          | What it does                                                                      |
| ---------------- | --------------------------------------------------------------------------------- |
| `'fit` (default) | A rectangle, cut by the outline where it's slanted or off the grid.               |
| `'rect`          | Always a rectangle. Where the outline is slanted or off the grid, it stops short. |

Fixtures are always rectangles: a bed doesn't get a corner cut off. Inside a cut room they stand on its floor, with their backs to its straight walls.

## Rules on slanted walls

Everything that works on a wall of the outline works on a slanted one: `plate.window` puts a window on it, `plate.touches "living" 'facade` counts it, `plate.apart "bath" 'facade` keeps away from it, `plate.door "hall" 'entrance` opens onto it.

## Which way the grid runs: `align`

The solver lays rooms out on a grid, and the grid runs square to the walls most of the outline runs along. A house drawn on its site plan at 36.87° to the drawing gets rooms at 36.87°; a house with one long wall at 45° to the others gets rooms square to the others, and the long wall cuts the ones against it.

```nickel
align = 'auto, # the default: square to most of the walls
align = 30,    # the grid turned 30° counter-clockwise from the x axis
```

- **Each plan decides for itself.** A room's plan is solved square to the room, so a bed lines up with its walls whichever way the house is turned.
- **Pinned spaces.** `at` is written in the plate's coordinates, so a plan with pinned spaces keeps its grid square to the x axis under `'auto`. With an angle written in `align`, `at` is measured along the turned grid instead, from the plan's origin.
- A number is for when most of the walls aren't the ones that matter: an octagon, or a plan where the interior should follow one facade.

## Plans inside a cut room

A room the outline cuts that has a `plan` (a bedroom's furniture, a kitchen's counter) is solved in its real shape, like the fill's plan. It isn't shared as a typical with other rooms of its type, since its shape is its own.

## Reading the result

- A room cut by the outline shows its floor: its area is the floor it has, not its rectangle's. Hover it to see the rectangle it was placed as and how much of it it kept.
- "keeps only 9 m² of its 4 × 5" means the outline cuts off more than half of it. Loosen its sizes so it can sit where the outline is squarer, or give it `shape = 'rect`.
- "cut in two by the outline": a rectangle across an inward corner. The solver avoids it; if you see it, loosen sizes nearby.
