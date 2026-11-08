# The one-bedroom flat, opened up: no wall between the entry, the living room, the dining area
# and the kitchen, and a reading nook off the living room through a wide opening.
#
# New here, compared with one-bedroom.ncl:
# - zones (`kind = 'zone`): spaces with no walls of their own. Where a zone meets another zone,
#   circulation or the fill, the floor runs on: no wall is built.
# - `plate.open`: two spaces share an edge with no wall on it, at least 1.2 m of it.
# - `plate.opening`: a gap in a wall with no door. As wide as the wall allows within `width`,
#   with `margin` of wall left at each end: the short walls either side of the nook's opening.
# - a door rule into a zone (the closet's, the bedroom's) works as usual: the room has walls.

let kitchen_plan = {
  spaces = {
    counter = {
      kind = 'fixture,
      width = { min = 2.4, max = 3.6 },
      depth = 0.65,
      against = 'wall,
      clearance.front = 1.1,
      symbol = 'counter,
    },
    fridge = { kind = 'fixture, width = 0.75, depth = 0.7, against = 'wall, clearance.front = 0.9, symbol = 'fridge },
  },
  rules = [plate.touches "fridge" "counter"],
} in
{
  name = "Open-plan flat",
  system = 'metric,
  units = 'm,
  seed = 5,
  start = { mode = 'boundary, points = [[0, 0], [11, 0], [11, 8], [0, 8]] },

  assemblies = import "build/assemblies/basic.ncl",
  services.lighting = import "build/services/lighting.ncl",

  spaces = {
    # The fill: the living room takes the floor the others leave.
    living = { kind = 'zone, fill = true, area.min = 20, side.min = 1.2 },
    entry = { kind = 'zone, area = { min = 3, max = 5 }, side.min = 1.5 },
    closet = { label = "Coats", area = { min = 0.75, max = 1.5 }, side.min = 0.75 },
    dining = { kind = 'zone, area = { min = 8, max = 12 }, side.min = 2.8 },
    kitchen = { kind = 'zone, area = { min = 8, max = 12 }, side.min = 2.4, plan = kitchen_plan },
    nook = { label = "Reading nook", area = { min = 4, max = 6.5 }, side.min = 2 },
    bedroom = { area = { min = 11, max = 14 }, side.min = 3 },
    bath = { area = { min = 4.5, max = 6 }, side.min = 2 },
  },

  rules = [
    plate.door "entry" 'facade, # the front door
    plate.door "closet" "entry",
    plate.open "entry" "living",
    plate.open "dining" "living",
    plate.open "kitchen" "dining",
    # The nook is a room, walled all round, with a wide opening onto the living room and at
    # least 0.6 m of wall left on each side of it.
    plate.opening "nook" "living" & { width = { min = 1.5 }, margin = 0.6 },
    plate.door ["bedroom", "bath"] "living",
    plate.window ["living", "dining", "nook", "bedroom"],
    plate.reach "entry" "bedroom" "living", # through the living room
  ],
}
