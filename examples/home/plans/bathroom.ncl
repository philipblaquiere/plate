# A bathroom: three fixtures, each drawn from the symbol catalog. Sizes are in millimetres
# here because that's how fixtures are specified; plain numbers would be in the plate's `units`.
#
# Every plate here imports it as the plan of a `bath`, so it's solved in each one. Opened on its
# own, it's drawn in its `preview` slot.

{
  needs.area.min = 4,
  spaces = {
    shower = { kind = 'fixture, width = "900 mm", depth = "900 mm", against = 'wall, symbol = 'shower },
    toilet = {
      kind = 'fixture,
      width = "400 mm",
      depth = "700 mm",
      against = 'wall,
      clearance.front = "600 mm",
      symbol = 'toilet,
    },
    basin = {
      kind = 'fixture,
      width = "600 mm",
      depth = "450 mm",
      against = 'wall,
      clearance.front = "700 mm",
      symbol = 'basin,
    },
  },
  rules = [
    # 'door is where the room's door is: keep the shower away from it.
    plate.apart "shower" 'door & { distance = 0.3 },
  ],
  preview = { width = 2, depth = 2.4 },
}
