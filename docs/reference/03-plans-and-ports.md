# Plans and ports

Give a space a `plan` and it is solved inside that space once the space is placed. Plans can hold spaces with plans of their own, as deep as you like: a floor of hotel rooms, each room with its bath and bed.

```nickel
let hotel_room = {
  needs = { width = { min = 3.4, max = 4.5 }, ports = { entrance = 'required, facade = 'required } },
  spaces = {
    entry = { kind = 'circulation, area.min = 3 },
    bath = { area.min = 4 },
    sleep = { area.min = 14 },
  },
  rules = [
    plate.door "entry" 'entrance,
    plate.window "sleep",
    plate.prefer (plate.touches "bath" 'wet),
  ],
} in
{
  start = { mode = 'spine, points = [[0, 0], [60, 0]] },
  spaces = {
    corridor = { kind = 'circulation, width = 1.8 },
    room = { count.min = 1, depth = 7.5, plan = hotel_room },
  },
  rules = [plate.along "room" "corridor"],
}
```

## Portable plans

A plan never names its parent's spaces. It talks about **ports** instead: the labelled edges of the space it is placed in. `plate.door "entry" 'entrance` means "a door to whatever this room's entrance is", so the same room plan works on any corridor. See [Ports](guide:ports).

## Needs

`needs` is what a plan asks of its space: a width, a depth, an area, and ports that must (or must not) be there. The parent sizes the space to fit, and the plan reports a warning when a space doesn't.

## On its own

Open a plan's file and it is drawn on its own, in a slot sized from its `needs`, or in the slots its `preview` lists. Right-click a room in a plate and choose **Open its plan here** to see the plan in that room's exact slot. See [Previews](guide:previews).

## Engines

Each plan is placed by an engine: `'grid` packs rectangles inside the space, `'spine_stack` lines spaces up along a corridor. A plate that starts from a spine uses `'spine_stack` unless it says otherwise.
