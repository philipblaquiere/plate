# Symbols

A symbol is a drawing from the catalog that a space is shown with: a bed, a sofa, a car, a pool, a tree. Give any space a `symbol`. It changes only how the plan looks, never how it's solved.

```nickel
spaces = {
  bed = { kind = 'fixture, width = 1.53, depth = 2.03, against = 'wall, symbol = 'bed_queen },
  parking = { kind = 'fixture, width = 3, depth = 6, symbol = 'car },
  pool = { kind = 'fixture, width = 4, depth = 9, clearance = 1.2, symbol = 'pool },
},
```

Open the **Symbol** page in the reference to see the whole catalog, by category: bedroom, living, dining, kitchen, bathroom, laundry, office, vehicles, outdoor and planting. Click one to insert it into the space at the cursor. In the drawing, right-click a space for the symbols that suit its name, or **Choose a symbol…** for the catalog.

## How it's drawn

A symbol is drawn from the space's front: a fixture's front (a bed's foot, a counter's working side), or the side a room is entered from. Symbols for things with a fixed shape (a car, a toilet, a round table) keep their proportions and sit in the middle. The rest (beds, sofas, counters, paving, pools) stretch to fill the space.

Symbols are exported with the drawing: in DXF on the `PLATE-SYMBOL` layer, in SVG as a group per symbol.

The catalog is `schema/symbols.json`: each symbol is a few shapes (rectangles, lines, circles, arcs) in millimetres.
