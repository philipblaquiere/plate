# A furnished one-bedroom flat of 51 m². The entry, bathroom, store and bedroom are rooms; the
# living room and its kitchen are one open space that takes the rest of the floor (the fill),
# divided into two zones with no wall between them: the kitchen along a wall, the lounge in
# what's left.
#
# Every room is furnished by a shared plan from plans/: the same bedroom, bathroom, kitchen and
# living-room plans the other scenarios use.

let bedroom_plan = import "../plans/rooms/bedroom.ncl" in
let bathroom_plan = import "../plans/bathroom.ncl" in
let kitchen_plan = import "../plans/rooms/kitchen.ncl" in
let living_plan = import "../plans/rooms/living.ncl" in
{
  name = "One-bedroom flat",
  system = 'metric,
  units = 'm,
  seed = 1,

  start = {
    mode = 'boundary,
    points = [[0, 0], [8.5, 0], [8.5, 6], [0, 6]],
    # The corridor of the building below, the street above, neighbours either side.
    ports = ['entrance, 'blank, 'facade, 'blank],
  },

  spaces = {
    entry = { kind = 'circulation, area = { min = 3, max = 5 }, side.min = 1.2 },
    # `walls = 'wet`: the bathroom's neighbours see a plumbing wall, which the kitchen plan
    # likes its counter against.
    bath = { area = { min = 4, max = 6.5 }, side.min = 1.8, plan = bathroom_plan, walls = 'wet },
    store = { area = { min = 1, max = 3 }, side.min = 0.9 },
    bedroom = { area = { min = 10, max = 13 }, side.min = 3, plan = bedroom_plan },
    open_plan = {
      label = "Living / kitchen",
      fill = true,
      area.min = 24,
      side.min = 2.4, # its narrowest part: no thin corridors of living room
      plan = {
        spaces = {
          # Zones have no walls of their own: where they meet, the floor is open.
          kitchen = { kind = 'zone, area = { min = 7, max = 10 }, side.min = 2.2, plan = kitchen_plan },
          lounge = { kind = 'zone, fill = true, plan = living_plan },
        },
        # Its counter against the bathroom's plumbing wall, if it can.
        rules = [plate.prefer (plate.touches "kitchen" 'wet & { length = 2.4 })],
      },
    },
  },

  rules = [
    plate.door "entry" 'entrance,
    plate.door ["open_plan", "bath", "store"] "entry",
    plate.door "bedroom" "open_plan",
    plate.window ["open_plan", "bedroom"],
    plate.apart "bath" 'facade, # the daylight is for the rooms you live in
  ],
}
