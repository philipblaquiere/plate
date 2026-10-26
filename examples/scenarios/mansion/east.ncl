# The service wing: kitchen and family room towards the courtyard, then pantry, laundry, gym and
# staff room, and a garage for three cars at the end.

let kitchen_plan = import "../../plans/rooms/kitchen.ncl" in
let living_plan = import "../../plans/rooms/living.ncl" in
{
  needs.ports.link = 'required,
  spaces = {
    gallery = { label = "Service gallery", kind = 'circulation, fill = true, area = { min = 24, max = 60 }, side.min = 1.5 },
    kitchen = { area = { min = 26, max = 36 }, side.min = 4.5, plan = kitchen_plan },
    family = { label = "Family room", area = { min = 32, max = 44 }, side.min = 4.5, plan = living_plan },
    pantry = { area = { min = 5, max = 8 }, side.min = 2 },
    laundry = { area = { min = 6, max = 10 }, side.min = 2.2, walls = 'wet },
    gym = { area = { min = 22, max = 30 }, side.min = 4 },
    staff = { label = "Staff room", area = { min = 12, max = 16 }, side.min = 3.2 },
    garage = {
      width = { min = 9.5, max = 11 },
      depth = { min = 6, max = 6.5 },
      plan = {
        # Three bays side by side: a row is placed as one run and split into car_1..car_3.
        spaces.car = { kind = 'fixture, count = 3, row = true, width = 2.9, depth = 5, against = 'facade, symbol = 'car },
      },
    },
  },
  rules = [
    plate.touches "gallery" 'door & { length = 1.8 }, # where the door from the centre is
    plate.door ["kitchen", "family", "laundry", "gym", "staff", "garage"] "gallery",
    plate.door "pantry" "kitchen",
    plate.touches "kitchen" "family" & { length = 3 },
    plate.touches ["kitchen", "family"] '"facade.court" & { length = 3 },
    plate.touches "garage" 'facade & { length = 9 }, # the garage doors
    plate.window ["kitchen", "family", "gym", "staff"],
  ],
  preview = { width = 11, depth = 24, ports = { bottom = 'facade, right = '"facade.court", top = 'facade, left = ['link, 'door] } },
}
