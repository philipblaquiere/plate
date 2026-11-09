# Filling the floor

A real plan has no floor that belongs to nobody. The tools for that are the fill space, zones, walls that say what they are, and spaces fixed where they stand.

## The fill: whatever is left

Most plans have one space whose shape is "the rest": the hall that reaches every room, the open-plan office around the meeting rooms, the garden around a house. Write it with `fill = true`. The solver places the other spaces as rectangles and gives the fill every bit of floor they leave, in whatever shape that is.

```nickel
spaces = {
  hall = { kind = 'circulation, fill = true, area = { min = 8, max = 20 }, side.min = 1.2 },
  living = { area = { min = 28, max = 40 }, side.min = 4 },
  bedroom = { count = 2, area = { min = 11, max = 15 }, side.min = 3 },
},
rules = [
  plate.door "hall" 'entrance,
  plate.door ["living", "bedroom"] "hall",
],
```

- At most one fill per plan.
- Where the outline is slanted or off the grid, it takes the floor right up to it (see _Shapes_).
- Its `area` still counts: the others grow (up to their `area.max`) so it doesn't get too big, and stop so it isn't too small.
- `side.min` is its narrowest passage. A hall with `side.min = 1.2` never narrows below 1.2 m, also where two rooms' corners come close.
- Rules name it like any space: `plate.door "bedroom" "hall"` is a door wherever the hall meets the bedroom, `plate.window "hall"` a window where it reaches a facade.
- It can have a `plan` of its own, solved in its real shape: a garden with a pool in it, a bedroom that wraps round a walk-in closet and is still furnished. The solver keeps the fill wide enough somewhere for the biggest fixture of that plan (a bed and the floor it keeps clear), and says so when it can't.

When the spaces can't take the floor the fill shouldn't, the solver says how much and why: "the spaces take at most 372 m² and lobby at most 260: 28 m² of the 660 m² here has nowhere to go". Raise an `area.max`, add a space, or let the fill grow.

## Zones: floor without walls

A zone is part of a floor with no walls of its own: the desks of an open-plan office, the dining end of a living room, the lounge and kitchen of a studio. Write `kind = 'zone`.

- Where a zone meets another zone, circulation or the fill, the edge is `'open`: no wall is drawn, and fixtures can't stand with their back to it. See _Open plans_ for openings, `plate.open` and walls between zones.
- Fixtures' clear floor may run across zones (a walkway is shared).
- A door from a zone (or from circulation) into a room gives the room an `'entrance`; a door from a room into another room gives it a `'link`.

```nickel
open_plan = {
  fill = true,
  plan = {
    spaces = {
      kitchen = { kind = 'zone, area = { min = 7, max = 10 }, plan = kitchen_plan },
      lounge = { kind = 'zone, fill = true, plan = living_plan },
    },
  },
},
```

## What a wall is

A space can say what its walls are to its neighbours with `walls`: `walls = 'wet` on a bathroom makes the kitchen next to it see a plumbing wall (the kitchen plan likes its counter there); `walls = 'outdoor` on a terrace gives the house an outdoor edge to open the living room onto; `walls = 'facade` on a garden makes every wall facing it an outside wall, for windows.

## Fixed in place

`at` pins a space where it is: the core of a tower, the wings of a house drawn by the architect, a stair. Give the corner nearest the origin (its lower left) and an exact `width` and `depth`; with `count`, one corner per instance.

```nickel
core = { kind = 'vertical, width = 13.6, depth = 15.2, at = [15.8, 15], plan = tower_core },
```

## Doors between plans

When two spaces that have plans of their own share a door (two wings of a house, a flat and the stair hall), the door goes mid-wall, and each plan sees it as a `'door` port. To meet it with circulation, touch the door:

```nickel
plate.touches "gallery" 'door & { length = 1.8 },                  # in each wing
plate.touches "gallery" 'door & { length = 1.8, each = true },     # in the centre: every door
```

`each = true` asks for every stretch of a port, not just one: the centre block's gallery reaches the doors to both wings. A touch longer than the port it asks for (a 1.8 m touch on a 0.9 m door) means all of it.

A door a plan puts on its own `'entrance` and the door its parent puts there are the same door: it's drawn once.

## Rows

`row = true` on a fixture with an exact `count` places them side by side as one run, then names each one (`desk_1`, `desk_2`, …): benches of desks, a bank of lifts, cars in a garage.

```nickel
car = { kind = 'fixture, count = 3, row = true, width = 2.9, depth = 5, against = 'facade, symbol = 'car },
```
