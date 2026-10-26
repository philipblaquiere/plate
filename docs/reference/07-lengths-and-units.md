# Lengths and units

Two settings decide how lengths work, both in the file:

- `system` is how lengths are **shown**: `'metric` (`30.00 m`, m²) or `'imperial` (`98'-6"`, ft²). It also sets the export's print scale. The **Metric | Imperial** switch in the top bar edits this line.
- `units` is the unit of **plain numbers**, like the `30` in `[30, 0]`. It defaults to `'m` for metric and `'ft` for imperial.

## Lengths with their own unit

Any length can be a string with its own unit. Strings are converted exactly, and units can be mixed:

```nickel
points = [
  [0, 0],
  ["98'-6", 0],              # feet and inches (the closing " is optional)
  ["98 ft 6 1/2 in", "30 m"],
  ["9150 mm", "30 m"],
],
```

Areas are in square `units`: `area.min = 24` is 24 m² in a metric plate.

The switch never changes the size of a plate. If the file doesn't say what unit its plain numbers are in, the switch writes the current one into `units` before changing `system`.
