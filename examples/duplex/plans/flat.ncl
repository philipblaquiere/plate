# A two-bedroom flat, as a plan. Its spaces are tagged, so services find what they need without
# knowing their names: 'wet rooms get a plumbing wall, 'water fixtures stand on it, 'drain
# fixtures are drained, 'utility is where the panel goes, 'counter and 'appliance get power, and
# 'cooking and 'laundry get a 240 V receptacle sized by their `load`.
# duplex.ncl puts two of these side by side. Plain numbers are feet, the plate's units.

let kitchen_plan = {
  spaces = {
    counter = {
      kind = 'fixture,
      width = { min = 8, max = 12 },
      depth = "26 in",
      against = 'wall,
      clearance.front = "3 ft 8 in",
      symbol = 'counter,
      tags = ['counter, 'water, 'drain],
    },
    fridge = {
      kind = 'fixture,
      width = "30 in",
      depth = "28 in",
      against = 'wall,
      clearance.front = 3,
      symbol = 'fridge,
      tags = ['appliance],
    },
    range = {
      kind = 'fixture,
      width = "30 in",
      depth = "26 in",
      against = 'wall,
      clearance.front = 3,
      symbol = 'stove,
      tags = ['cooking],
      # From its spec sheet: a 50 A, 240 V circuit with a neutral. Power sizes the cable from it.
      load.power = { current = "50 A", voltage = 240, conductors = 3 },
    },
  },
  rules = [plate.touches "fridge" "counter", plate.touches "range" "counter"],
} in
let bath_plan = {
  spaces = {
    shower = { kind = 'fixture, width = 3, depth = 3, against = 'wall, symbol = 'shower, tags = ['water, 'drain] },
    toilet = {
      kind = 'fixture,
      width = "16 in",
      depth = "28 in",
      against = 'wall,
      clearance.front = 2,
      symbol = 'toilet,
      tags = ['water, 'drain],
    },
    basin = {
      kind = 'fixture,
      width = 2,
      depth = "18 in",
      against = 'wall,
      clearance.front = "28 in",
      symbol = 'basin,
      tags = ['water, 'drain],
    },
  },
  rules = [plate.apart "shower" 'door & { distance = 1 }],
} in
{
  needs = { width.min = 24, ports.facade = 'required },
  spaces = {
    hall = { kind = 'circulation, fill = true, area.min = 40, side.min = "3 ft 8 in" },
    living = { area = { min = 260, max = 370 }, side.min = 13 },
    kitchen = { area = { min = 90, max = 130 }, side.min = "8 ft 6 in", tags = ['wet], plan = kitchen_plan },
    bedroom = { count = 2, area = { min = 110, max = 140 }, side.min = 10, tags = ['sleep] },
    bath = { area = { min = 50, max = 70 }, side.min = "6 ft 3 in", walls = 'wet, tags = ['wet], plan = bath_plan },
    utility = {
      label = "Utility",
      area = { min = 22, max = 38 },
      side.min = 4,
      tags = ['utility],
      plan.spaces.dryer = {
        kind = 'fixture,
        width = "27 in",
        depth = "28 in",
        against = 'wall,
        clearance.front = 3,
        symbol = 'dryer,
        tags = ['laundry],
        load.power = { current = "30 A", voltage = 240, conductors = 3 },
      },
    },
  },
  rules = [
    plate.touches "hall" 'door, # meets the door the building gives the flat
    plate.door ["living", "bedroom", "bath", "utility"] "hall",
    plate.door "kitchen" "living",
    plate.window ["living", "bedroom"],
    plate.prefer (plate.window "kitchen"),
    plate.wall "#sleep" "#sleep" & { acoustic = 45 }, # quiet between the bedrooms
  ],
  # Opened on its own: the clear space a flat of duplex.ncl gets inside its walls, its front
  # door in the middle of the bottom side.
  preview = {
    system = 'imperial,
    width = 28.7,
    depth = 34.8,
    ports = { bottom = ['facade, 'door], right = 'party, top = 'facade, left = 'party },
  },
}
