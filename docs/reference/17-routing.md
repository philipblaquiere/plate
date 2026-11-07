# Routing

Runs live in wall cavities and across ceilings, never across a floor. A wall a network can run in has a cavity at least `via.cavity` deep and an assembly that lets runs through.

With `via.ceilings`, a wall has two bands:

- **low**, at outlet height: broken at every door, since nothing runs through a doorway;
- **high**, just under the ceiling: it passes over door and window headers.

A run climbs between them anywhere along a wall, and from the high band it reaches the ceilings of the spaces `via.ceilings` names, square to the room (along the joists, then across). Ceiling parts (lights) are reached from above. Climbing costs a little, so runs look as an electrician wires: receptacles stay low in the walls, lights are reached through the ceiling, and a run goes up and over a door rather than round the room.

Without `via.ceilings`, a network runs in the walls only, in one band that passes doors (as a drain does, below the floor).

`height` on the plate is floor to ceiling (2.4 m or 8 ft by default): it's how long a climb is, for cable lengths.

## Crossing walls

An assembly's `cross` says whether runs may use it:

```nickel
party = { …, fire = 1, cross = 'firestop },
shaft = { …, cross = 'no },
```

- `'yes` (the default): drilled studs and plates.
- `'firestop` (the default for a wall with a `fire` rating): allowed, through a sealed penetration, marked with a red diamond.
- `'no`: nothing runs in it or through it. A part on such a wall is a problem.

## In the drawing

In a service view, a cable is drawn in its jacket's colour: solid low in a wall, dashed where it's up in a wall over a door, long dashes across a ceiling, thin dashes for a switch leg. A ring marks where it climbs a wall, a tick where it passes over a wall from one ceiling to the next. Cables side by side in one cavity are drawn side by side. Press A for the schematic.
