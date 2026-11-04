# A one-bedroom flat, built: real walls, and a service.
#
# Start here, then see duplex.ncl:
# - `assemblies`: what the edges between spaces are built as. Outside walls take their thickness
#   from inside the outline; partitions straddle the line between two rooms; every room is drawn
#   to its walls' faces. Hover a wall to see what it is and why.
# - `services`: lighting puts a light in each room and a switch by each door, on the solved plan.
#   Pick Lighting above the drawing to see it on its own.
#
# Both come from files in build/, so any plate in this project can use them.

{
  name = "One-bedroom flat",
  system = 'metric,
  units = 'm,
  seed = 1,
  start = { mode = 'boundary, points = [[0, 0], [10, 0], [10, 7], [0, 7]] },

  assemblies = import "build/assemblies/basic.ncl",
  services.lighting = import "build/services/lighting.ncl",

  spaces = {
    hall = { kind = 'circulation, fill = true, area.min = 4, side.min = 1.2 },
    living = { area = { min = 28, max = 40 }, side.min = 4 },
    bedroom = { area = { min = 11, max = 15 }, side.min = 3 },
    bath = { area = { min = 4.5, max = 6 }, side.min = 1.8 },
  },
  rules = [
    plate.door "hall" 'facade, # the front door
    plate.door ["living", "bedroom", "bath"] "hall",
    plate.window ["living", "bedroom"],
  ],
}
