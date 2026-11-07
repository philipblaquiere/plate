# A kitchen: a run of counter with the sink on the plumbing wall, a fridge, and an island when
# there's room. The island is optional (count 0 to 1): the solver adds it if it fits.

{
  needs.area.min = 7,
  spaces = {
    counter = {
      kind = 'fixture,
      width = { min = 2.4, max = 4.8 },
      depth = 0.65,
      against = 'wall,
      clearance.front = 1.1,
      symbol = 'counter,
    },
    fridge = { kind = 'fixture, width = 0.7, depth = 0.7, against = 'wall, clearance.front = 0.9, symbol = 'fridge },
    island = {
      kind = 'fixture,
      count = { min = 0, max = 1 },
      width = { min = 1.4, max = 2.4 },
      depth = 0.9,
      clearance = 1, # walk all round it
      symbol = 'kitchen_island,
    },
  },
  rules = [
    plate.touches "fridge" "counter",
    plate.prefer (plate.touches "counter" 'wet),
  ],
  preview = { width = 4, depth = 3.5 },
}
