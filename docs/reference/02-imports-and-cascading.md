# Imports and cascading

## Imports

Share a plan between files with `import`. The path is relative to the importing file.

```nickel
let hotel_room = import "hotel-room.ncl" in
{
  start = { mode = 'spine, points = [[0, 0], [60, 0]] },
  spaces = {
    corridor = { kind = 'circulation, width = 1.8 },
    room = { count.min = 1, width = { min = 3.6, max = 4.5 }, depth = 7.5, plan = hotel_room },
  },
  rules = [plate.along "room" "corridor"],
}
```

Name the binding differently from the space that uses it. Inside `spaces`, `room` would refer to the space itself, not to the imported plan.

Right-click a file in the explorer to copy an import line for it. Moving or renaming a file updates the imports that point at it.

## Cascading with `&`

`&` merges records, deeply: fields from both sides are kept. Use it to build a variant of a plan without copying it.

```nickel
let hotel_room = import "hotel-room.ncl" in
let accessible_room = hotel_room & {
  spaces.bath.area.min | force = 6,
  spaces.bath.area.max | force = 8,
  spaces.sleep.area.min | force = 16,
} in
{
  start = { mode = 'spine, points = [[0, 0], [60, 0]] },
  spaces = {
    corridor = { kind = 'circulation, width = 1.8 },
    room = { count.min = 1, width = { min = 3.6, max = 4.5 }, depth = 7.5, plan = hotel_room },
  },
  rules = [
    plate.along "room" "corridor",
    plate.instance "room_1" accessible_room,
  ],
}
```

When both sides set the same field to different values, the merge fails. Say which one wins:

- `| force` on the new value overrides the old one, as above.
- `| default` on the original makes it a fallback that any merge can replace: `area.min | default = 4`.

Merges work anywhere a record does: `plate.door "bath" "hall" & { width = 0.8 }` gives one door its own width.

Imports stay inside a project: a plate can only read its own project's files. To reuse a file
elsewhere, right-click it in the file tree and choose **Copy to project…**.
