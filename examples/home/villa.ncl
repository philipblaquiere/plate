# A beach villa on its plot: the house, a terrace and pool towards the sea, parking off the
# street, and a garden that takes the rest of the ground (the fill, drawn as lawn).
#
# What the outdoor spaces' walls are matters to the house: `walls = 'outdoor` on the terrace
# gives the house an 'outdoor edge there, which its plan (plans/house.ncl) opens the living
# room onto; the garden's `walls = 'facade` makes every other side an outside wall for windows.
#
# New here, compared with apartment.ncl: a whole plot, with fixtures (pool, cars) and zones outside.

let villa = import "plans/house.ncl" in
{
  name = "Beach villa",
  system = 'metric,
  units = 'm,
  seed = 46,

  start = {
    mode = 'boundary,
    points = [[0, 0], [34, 0], [34, 42], [0, 42]],
    # The street at the bottom, the beach at the top, neighbours either side.
    ports = ['street, 'blank, 'beach, 'blank],
  },

  spaces = {
    # No size given: a space with a plan is sized by what's inside it (the rooms' areas added
    # up, hall included).
    house = { aspect = 2, side.min = 10, plan = villa },
    terrace = {
      kind = 'zone,
      width = { min = 14, max = 22 },
      depth = { min = 4, max = 6 },
      walls = 'outdoor,
      symbol = 'deck,
    },
    pool = { kind = 'fixture, width = 4, depth = 12, clearance = 1.2, symbol = 'pool },
    parking = {
      kind = 'fixture,
      count = 2,
      row = true, # side by side, off the street
      width = 2.6,
      depth = 5.5,
      against = 'street,
      symbol = 'car,
    },
    garden = { kind = 'zone, fill = true, walls = 'facade, symbol = 'lawn },
  },

  rules = [
    plate.touches "terrace" "house" & { length = 12 },
    plate.touches "pool" "terrace", # its deck (the clearance) runs onto the terrace
    plate.apart "house" 'beach & { distance = 14 }, # well back from the beach
    plate.apart "house" 'street & { distance = 6 }, # and from the street
    plate.door "house" "garden" & { width = 1.2 }, # the front door
    plate.near "parking" "house" 8,
  ],
}
