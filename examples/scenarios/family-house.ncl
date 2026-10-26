# A single-storey family house on an L-shaped footprint: three bedrooms, two bathrooms, an
# open kitchen and living room, and a garage. The hall is the fill: it takes whatever floor the
# rooms leave and reaches all of them, so nothing is left over.

let bedroom_plan = import "../plans/rooms/bedroom.ncl" in
let kitchen_plan = import "../plans/rooms/kitchen.ncl" in
let living_plan = import "../plans/rooms/living.ncl" in
let bathroom_plan = import "../plans/bathroom.ncl" in

{
  name = "Family house",
  system = 'metric,
  units = 'm,
  seed = 1,

  start = {
    mode = 'boundary,
    points = [[0, 0], [16, 0], [16, 11], [6, 11], [6, 14], [0, 14]],
    # The street is along the bottom.
    ports = ['"facade.street", 'facade, 'facade, 'facade, 'facade, 'facade],
  },

  spaces = {
    hall = { kind = 'circulation, fill = true, area.min = 12, side.min = 1.2 },
    living = { area = { min = 28, max = 44 }, side.min = 4.2, plan = living_plan },
    kitchen = { label = "Kitchen / dining", area = { min = 20, max = 30 }, side.min = 3.4, plan = kitchen_plan },
    master = { label = "Main bedroom", area = { min = 15, max = 20 }, side.min = 3.4, plan = bedroom_plan },
    ensuite = { area = { min = 4.5, max = 7 }, side.min = 1.8, plan = bathroom_plan, walls = 'wet },
    bedroom = { count = 2, area = { min = 11, max = 14 }, side.min = 3, plan = bedroom_plan },
    bath = { area = { min = 5, max = 8 }, side.min = 1.8, plan = bathroom_plan, walls = 'wet },
    laundry = { area = { min = 4, max = 7 }, side.min = 1.8, walls = 'wet },
    garage = {
      width = { min = 3.6, max = 4.2 },
      depth = { min = 6, max = 6.5 },
      plan = { spaces.car = { kind = 'fixture, width = 1.9, depth = 4.7, clearance = { left = 0.7, right = 0.7 }, symbol = 'car } },
    },
  },

  rules = [
    plate.door "hall" '"facade.street" & { width = 1 }, # the front door
    plate.door ["living", "kitchen", "master", "bedroom", "bath", "laundry"] "hall",
    plate.door "ensuite" "master",
    plate.door "garage" "laundry",
    plate.touches "garage" '"facade.street" & { length = 3.5 }, # the garage door side
    plate.touches "kitchen" "living",
    plate.window ["living", "kitchen", "master", "bedroom"],
    plate.prefer (plate.touches "kitchen" "laundry"),
    plate.prefer (plate.window "bath"),
    plate.prefer (plate.apart ["master", "bedroom"] "garage"),
  ],

  goals = { utilization = 1, compactness = 0.3 },
}
