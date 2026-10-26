# Rules and goals

Spaces are a program; **rules** say how they relate, and **goals** say what makes one layout better than another.

## Writing rules

Rules are written with the `plate` helpers:

```nickel
rules = [
  plate.touches "kitchen" "living",
  plate.door "bedroom" "hall",
  plate.between "bedroom" "hall",
  plate.window "bedroom",
  plate.prefer (plate.window "kitchen"),
],
```

Names can be a space (`"kitchen"`), a type (`"bedroom"` is every bedroom), a pattern (`"bed*"`) or a list (`["bath", "wc"]`). Where a rule talks to the parent space, give a port role instead: `plate.door "entry" 'entrance`.

## Keeping things apart

`plate.apart` is the opposite of `plate.touches`: no `a` shares a wall with any `b`. Give it a port instead of a space to keep clear of part of the parent space, and a `distance` to keep a gap:

```nickel
rules = [
  plate.apart "bedroom" "kitchen",                       # no shared wall
  plate.apart "bed" 'window,                             # not against a window
  plate.apart "bed" 'window & { wall = true },           # not against any wall with a window
  plate.apart "desk" "bed" & { distance = 0.6 },         # at least 0.6 between them
  plate.prefer (plate.apart "sofa" 'door & { distance = 1 }),
],
```

`wall = true` widens a port to the whole wall it's on: a 1.2 m window becomes the 4 m wall it sits in. Spaces that only meet at a corner don't touch; with a `distance` they must be that far apart, corner to corner.

## Required or preferred

A rule is **required** unless it has a weight. `plate.prefer` gives it weight 1; `plate.prefer_by 3` makes it matter three times as much. A required rule the solver can't meet shows up as a warning, underlined where the rule is written.

## Goals

Once every required rule holds, goals weigh the solutions. Each has a weight; 0 turns it off.

```nickel
goals = { utilization = 1, compactness = 0.3 },
```

The same file and `seed` always give the same layout. Change `seed` to see another solution.
