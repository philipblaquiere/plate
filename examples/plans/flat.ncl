# A two-bedroom flat, as a plan. Its spaces are tagged, so services find what they need without
# knowing their names: 'wet rooms get a plumbing wall, 'water fixtures stand on it, 'drain
# fixtures are drained, 'utility is where the panel goes, 'counter and 'appliance get power.
# 5-duplex.ncl puts two of these side by side.

let kitchen_plan = {
  spaces = {
    counter = {
      kind = 'fixture,
      width = { min = 2.4, max = 3.6 },
      depth = 0.65,
      against = 'wall,
      clearance.front = 1.1,
      symbol = 'counter,
      tags = ['counter, 'water, 'drain],
    },
    fridge = {
      kind = 'fixture,
      width = 0.75,
      depth = 0.7,
      against = 'wall,
      clearance.front = 0.9,
      symbol = 'fridge,
      tags = ['appliance],
    },
  },
  rules = [plate.touches "fridge" "counter"],
} in
let bath_plan = {
  spaces = {
    shower = { kind = 'fixture, width = 0.9, depth = 0.9, against = 'wall, symbol = 'shower, tags = ['water, 'drain] },
    toilet = {
      kind = 'fixture,
      width = 0.4,
      depth = 0.7,
      against = 'wall,
      clearance.front = 0.6,
      symbol = 'toilet,
      tags = ['water, 'drain],
    },
    basin = {
      kind = 'fixture,
      width = 0.6,
      depth = 0.45,
      against = 'wall,
      clearance.front = 0.7,
      symbol = 'basin,
      tags = ['water, 'drain],
    },
  },
  rules = [plate.apart "shower" 'door & { distance = 0.3 }],
} in
{
  needs = { width.min = 7, ports.facade = 'required },
  spaces = {
    hall = { kind = 'circulation, fill = true, area.min = 4, side.min = 1.1 },
    living = { area = { min = 24, max = 34 }, side.min = 4 },
    kitchen = { area = { min = 8, max = 12 }, side.min = 2.6, tags = ['wet], plan = kitchen_plan },
    bedroom = { count = 2, area = { min = 10, max = 13 }, side.min = 3, tags = ['sleep] },
    bath = { area = { min = 4.5, max = 6.5 }, side.min = 1.9, walls = 'wet, tags = ['wet], plan = bath_plan },
    utility = { label = "Utility", area = { min = 2, max = 3.5 }, side.min = 1.2, tags = ['utility] },
  },
  rules = [
    plate.touches "hall" 'door, # meets the door the building gives the flat
    plate.door ["living", "bedroom", "bath", "utility"] "hall",
    plate.door "kitchen" "living",
    plate.window ["living", "bedroom"],
    plate.prefer (plate.window "kitchen"),
    plate.wall "#sleep" "#sleep" & { acoustic = 45 }, # quiet between the bedrooms
  ],
  # Opened on its own: the clear space a flat of 5-duplex.ncl gets inside its walls, its front
  # door in the middle of the bottom side.
  preview = {
    width = 8.75,
    depth = 10.6,
    ports = { bottom = ['facade, 'door], right = 'party, top = 'facade, left = 'party },
  },
}
