# Reading the result

Every solve ends with a verdict, shown on the drawing and in the status bar.

- **Perfect**: every rule and preference holds, and every bit of floor belongs to a space.
- **Solved**: every requirement holds, but a preference is missed or some floor is left over.
- **Incomplete**: some requirement isn't met. The drawing still shows the best layout found, with the spaces that break a rule outlined in red.

## What keeps it from perfect

The problems list under the editor has one line per finding, with what to change right under it. Click one to go to the value it's about, in whichever file it's written.

- **Unmet**: a requirement (a rule, a size, fitting inside) the layout breaks. "bath is 6.25 m², more than 5.5: the floor around it has nowhere else to go. Raise its area.max, or let a `fill = true` space take the rest."
- **Preference**: a `plate.prefer` rule that doesn't hold. Preferences never make a solve incomplete.
- **Left over**: floor no space covers, hatched in red on the drawing with its area. The note says why the spaces around it didn't take it: one is at its `area.max`, one could only grow into it as an L, or it's cut off from the fill.

The solver checks a program before it starts: when the spaces can't fill the floor, or need more than there is, it says by how much.

## Seeds

The solver searches: the same file and `seed` always give the same layout, and another seed gives another one. When a solve isn't perfect, try other seeds from the verdict on the drawing: it solves with the next few and offers the best, which writes `seed = …` into the plate. From the command line: `plate check plate.ncl --seed 7`.

A plate that is incomplete at every seed is asking for something that doesn't fit; the notes say what to loosen.

## Floors in exports

Floor left over is exported too, on its own layer (`PLATE-LEFTOVER` in DXF, hatched in SVG), so it's never silently lost.
