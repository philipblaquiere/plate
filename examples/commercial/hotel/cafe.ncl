# The café: a counter and ten tables. It asks for a shopfront, and keeps the counter off it.
# '"facade.storefront" is a refined role: a kind of 'facade (so windows go on it) that the plate
# marks on its street edge. Dotted roles are written in quotes.

{
  needs = {
    area.min = 60,
    ports = { "facade.storefront" = 'required },
  },
  spaces = {
    counter = {
      kind = 'fixture,
      width = { min = 4, max = 7 },
      depth = 0.7,
      against = 'wall,
      clearance.front = 1.2, # room for the queue
      symbol = 'counter,
    },
    table = {
      kind = 'fixture,
      count = 10, # table_1 … table_10
      width = 1.2,
      depth = 1.4,
      clearance = 0.5, # all round; neighbours' clearances may overlap, like an aisle
      symbol = 'dining_table_4,
    },
  },
  rules = [
    plate.apart "counter" '"facade.storefront" & { wall = true },
  ],
  preview = {
    width = 10,
    depth = 12,
    ports = { bottom = 'entrance, top = '"facade.storefront", left = 'partition, right = 'partition },
  },
}
