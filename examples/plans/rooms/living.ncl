# A living room: a sofa facing a TV, and an armchair or two beside it. The TV goes against a
# wall (not a window); the sofa stands free, so it can face the TV in a room of any width.

{
  needs.area.min = 14,
  spaces = {
    sofa = {
      kind = 'fixture,
      width = { min = 2, max = 2.6 },
      depth = 0.95,
      clearance = { front = 1.4, back = 0.3 }, # free-standing: it floats to face the TV
      symbol = 'sofa,
    },
    tv = { kind = 'fixture, width = 1.8, depth = 0.45, against = 'wall, clearance.front = 2.2, symbol = 'tv_unit },
    armchair = { kind = 'fixture, count = { min = 1, max = 2 }, width = 0.85, depth = 0.85, symbol = 'armchair },
  },
  rules = [
    plate.faces "sofa" "tv", # the TV across from the sofa,
    plate.faces "tv" "sofa", # and turned towards it
    plate.near "tv" "sofa" 3.2,
    plate.near "armchair" "sofa" 1.2,
    plate.apart "tv" 'window,
  ],
  preview = { seed = 3, width = 5, depth = 4.5 },
}
