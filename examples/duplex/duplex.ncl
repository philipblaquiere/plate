# Two flats side by side, built and serviced.
#
# New here, compared with one-bedroom.ncl:
# - wood-frame assemblies with their build-ups and ratings. The wall between the flats is a party
#   wall (each flat is a 'party to the other); the bedrooms' shared wall is asked for STC 45 (in
#   plans/flat.ncl), so it's built as the thinnest assembly that is.
# - services that serve each flat on its own (`scope = "#dwelling"`): a panel and circuits each.
# - power is written in terms of a code's numbers: this is a US building, in feet and inches, so
#   it merges in build/codes/nec.ncl. A length can carry its own unit ("140 mm", "4 ft").
# - plumbing claims what it needs, and the solver makes room: a plumbing wall in every wet room,
#   water fixtures against it, drains no further than 4 m from a stack.
#
# - power's circuits are sized from what's on them: the range and the dryer (plans/flat.ncl) say
#   what they need (`load`), and each cable is the thinnest of the code's cable that carries it.
#
# Pick a service above the drawing to see it on its own: its parts, and where its cables run (in
# the walls, over the doors, across the ceilings). Press A there for the schematic.

let flat_plan = import "plans/flat.ncl" in
let open_flat = import "plans/open-flat.ncl" in
{
  name = "Duplex",
  system = 'imperial, # lengths shown in feet and inches
  units = 'ft, # plain numbers are feet, in this file and the plan
  seed = 4,
  height = "8 ft", # floor to ceiling: how far a cable climbs to reach the ceiling
  start = {
    mode = 'boundary,
    points = [[0, 0], [60, 0], [60, 36], [0, 36]],
    ports = ['facade, 'blank, 'facade, 'blank], # the neighbours either side
  },

  assemblies = import "build/assemblies/wood-frame.ncl",
  services = {
    power = (import "build/services/power.ncl") & (import "build/codes/nec.ncl"),
    heat = import "build/services/heat.ncl",
    lighting = import "build/services/lighting.ncl",
    plumbing = import "build/services/plumbing.ncl",
  },

  spaces = {
    flat = { count = 2, width = 30, depth = 36, tags = ['dwelling], plan = flat_plan },
  },
  rules = [
    plate.door "flat" 'facade,
    # The second flat is the same program opened up (plans/open-flat.ncl): living, dining and
    # kitchen with no wall between them, under the same services.
    plate.instance "flat_2" open_flat,
  ],
}
