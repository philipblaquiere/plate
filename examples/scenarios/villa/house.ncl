# The villa itself: living spaces open onto the terrace ('outdoor, where the plot plan puts the
# terrace against it), bedrooms look out over the garden ('facade). The hall takes what's left.

let bedroom_plan = import "../../plans/rooms/bedroom.ncl" in
let living_plan = import "../../plans/rooms/living.ncl" in
let kitchen_plan = import "../../plans/rooms/kitchen.ncl" in
let bathroom_plan = import "../../plans/bathroom.ncl" in
{
  needs.ports = { outdoor = 'required, facade = 'required },
  spaces = {
    hall = { kind = 'circulation, fill = true, area = { min = 14, max = 30 }, side.min = 1.2 },
    living = { area = { min = 36, max = 52 }, side.min = 5, plan = living_plan },
    kitchen = { area = { min = 16, max = 24 }, side.min = 3.4, plan = kitchen_plan },
    master = { label = "Main bedroom", area = { min = 16, max = 22 }, side.min = 3.6, plan = bedroom_plan },
    bedroom = { count = 2, area = { min = 12, max = 16 }, side.min = 3.2, plan = bedroom_plan },
    bath = { count = 2, area = { min = 5, max = 8 }, side.min = 2, plan = bathroom_plan, walls = 'wet },
  },
  rules = [
    plate.door "hall" 'entrance,
    plate.door ["living", "kitchen", "master", "bedroom", "bath"] "hall",
    plate.touches "kitchen" "living" & { length = 3 },
    plate.touches "living" 'outdoor & { length = 5 }, # a wide opening onto the terrace
    plate.window "living" & { port = 'outdoor, width = 4 },
    plate.window ["master", "bedroom", "kitchen"],
    plate.prefer (plate.window "master" & { port = 'outdoor }),
  ],
  preview = { seed = 4,
    width = 16,
    depth = 10.5,
    ports = { bottom = ['facade, 'entrance], top = 'outdoor, left = 'facade, right = 'facade },
  },
}
