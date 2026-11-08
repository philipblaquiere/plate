# Open plans

In an open plan the question is what's built between two neighbours: a wall, a wall with a door, a wall with an opening, or nothing at all. Spaces stay rectangles placed by the solver; what changes is the edge between them.

| Between two spaces | Write                            | What's built                         |
| ------------------ | -------------------------------- | ------------------------------------ |
| a wall             | nothing (rooms), or `plate.wall` | a wall the length of the shared edge |
| a door             | `plate.door "a" "b"`             | a wall, cut by a door that swings    |
| an opening         | `plate.opening "a" "b"`          | a wall, cut by a gap with no door    |
| nothing            | zones, or `plate.open "a" "b"`   | no wall: the floor runs on           |

## Zones

A zone (`kind = 'zone`) is a space with no walls of its own: the dining end of a living room, the kitchen of a studio, the desks of an open office. Where a zone meets another zone, circulation or the fill, no wall is built and the edge is drawn dashed. Where it meets a room, the room's wall is there as usual.

```nickel
spaces = {
  living = { kind = 'zone, fill = true, area.min = 20, side.min = 1.2 },
  dining = { kind = 'zone, area = { min = 8, max = 12 }, side.min = 2.8 },
  kitchen = { kind = 'zone, area = { min = 8, max = 12 }, side.min = 2.4 },
  bedroom = { area = { min = 11, max = 14 }, side.min = 3 },
},
```

- Fixtures can't stand with their back to an open edge; their clear floor may run across it.
- Open floor is a way through: `plate.reach` follows it like a door, where two spaces share at least 0.9 m of it.
- A zone with no open edge left (walled all round by rooms and `plate.wall` rules) is reported: it's a room.

## `plate.open`: no wall

`plate.open "dining" "living"` makes the two share an edge, at least 1.2 m (4 ft) of it, with no wall on it. Between zones it's the same as `plate.touches`, and it says what you mean. Between two rooms it takes the wall away.

```nickel
plate.open "dining" "living",
plate.open "entry" "living" & { length = 2 },   # at least 2 m of open edge
```

## `plate.opening`: a gap with no door

`plate.opening` cuts a gap in the wall between two spaces, with no door in it: a cased opening, an archway, a wide way into a nook. It's 1.2 m (4 ft) wide unless `width` says.

```nickel
plate.opening "dining" "hall" & { width = 1.8 },
# As wide as the wall allows, at least 1.5 m, with 0.6 m of wall left at each end:
plate.opening "nook" "living" & { width = { min = 1.5 }, margin = 0.6 },
```

- `width` is a length, a range `{ min, max }` (the opening takes as much of the shared wall as it can within it), or `'full` (that's `plate.open`).
- `margin` is the wall kept at each end: short wall sections either side of the opening.
- Between two zones an opening builds the wall it's cut into.
- The spaces' plans see it as an `'opening` port, the width of the gap.

## A wall where the floor would be open

`plate.wall "nook" "living"` builds a wall between two zones, or between a zone and circulation, that would otherwise be open. With an opening rule on the same pair you don't need it: the opening builds its wall.

## Regions

Spaces joined by open edges, at least 0.9 m (3 ft) of them, form a **region**: the floor you'd call one room. The solver finds them after a level is placed; they never change the layout. Circulation never joins a region, so a hall stays a hall even when it's open to the living room. Rooms joined by `plate.open` join, like zones.

- Hover a space of a region to see the region outlined, with its spaces and their floor added up. It's exported as one closed polyline on `PLATE-REGION`.
- Services can count by region: `plate.one … & { per = 'region }` (see _Services_).

## Inspecting and changing an edge

Hover where two spaces meet to see what's built there and why: "open: living is a zone and dining is a zone", "a rule: plate.opening". A wall's card says the same. Right-click it to change it:

- With no rule deciding it: **Make open**, **Add an opening**, **Add a door** or **Build a wall** adds a rule to the plan that placed the two spaces. A door where the floor would be open adds the wall for it too.
- With a rule that names just these two: the same entries change that rule, and **Remove the rule** goes back to what the two get by default.
- With a rule that covers more spaces (a list, a tag), nothing is changed from here: **Show the rule** opens it.

## What isn't allowed

- A door (`plate.door`) where there's no wall: two zones, say. Use `plate.opening` for a gap with no door, or add `plate.wall` to build a wall for the door.
- `plate.open` and `plate.wall` on the same two spaces.

Both are reported on the rule, with the fix.

## Example

`examples/duplex/open-plan.ncl`: an entry with a coat closet, open to the living room, then the dining area, then the kitchen, and a reading nook off the living room through a wide opening with wall either side.

```nickel
rules = [
  plate.door "entry" 'facade,
  plate.door "closet" "entry",
  plate.open "entry" "living",
  plate.open "dining" "living",
  plate.open "kitchen" "dining",
  plate.opening "nook" "living" & { width = { min = 1.5 }, margin = 0.6 },
  plate.door ["bedroom", "bath"] "living",
  plate.reach "entry" "bedroom" "living",
],
```
