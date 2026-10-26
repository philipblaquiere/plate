# Typicals

Rooms whose slots match (same size and same ports, after turning and mirroring) are solved once and share the solution. The drawing labels them "Type A", "Type B", …. It is fast, and it is how buildings are drawn: one typical room, repeated.

## Opting out

- `typical = false` on a space solves every instance on its own.
- `plate.unique "room_7"` gives one instance its own solution. Right-click a room in the drawing and choose **Make unique** to add it.
- `plate.instance "room_1" accessible_room` gives particular instances a different plan altogether.
- `mirror = false` stops copies from being mirrored to fit; they are only rotated.

```nickel
spaces = {
  room = { count.min = 1, depth = 7.5, plan = hotel_room, mirror = false },
},
rules = [
  plate.unique "room_7",
  plate.instance "room_1" accessible_room,
],
```

Instances are numbered by position (left to right, or along the spine), so `room_7` stays the same room as the layout changes.
