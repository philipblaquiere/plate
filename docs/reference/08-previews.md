# Previews

A plan is a file of its own, and you can open it on its own. The drawing shows it solved in a **slot**: a rectangle with its ports, like the space a plate would give it. The plan is solved there exactly as a plate would solve it in a space of that size and ports, so what you see is what you'll get.

The slot comes from one of three places, shown above the drawing:

- **A space in a plate.** Right-click a room in a plate's drawing and choose **Open its plan here**. The plan opens in that room's exact slot, with the plate's seed, so it matches the room pixel for pixel. **Save as preview** keeps that slot in the file.
- **`preview` in the file.** One slot, or a list of named ones to switch between.
- **Nothing.** The slot is sized from `needs` (or from the spaces' areas), with the ports of a typical room on a corridor. **Set size** writes it into the file.

```nickel
{
  needs = { width = { min = 3.4, max = 4.5 }, ports = { entrance = 'required, facade = 'required } },
  preview = [
    { name = "typical", width = 4, depth = 7.5 },
    { name = "corner", width = 4, depth = 7.5, ports.left = 'facade },
  ],
  spaces = { … },
}
```

## The slot

The entrance is along the bottom. Sides you don't give a port are those of a room on a corridor: `'entrance` at the bottom, `'facade` at the top, `'party` on the right and `['party, 'wet]` on the left. Drag the slot's corners in the drawing to resize it; the file follows.

A slot that doesn't meet the plan's `needs` is drawn empty, with a warning on the need it misses: that's how you check a plan copes with the narrow end of its range.

## Units and seed

A plate decides the units and seed of the plans it uses. While previewing, `system`, `units` and `seed` in the slot stand in for them. `preview` is ignored when the plan is used in a space, and changing it never changes a plate's solution.
