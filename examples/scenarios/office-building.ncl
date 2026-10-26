# A typical floor of a small office building: the core is fixed (`at`), meeting rooms, offices
# and support spaces line the walls, desk clusters sit by the windows, and the open floor (the
# fill) takes the rest and connects everything.

let desk_cluster = import "office/desks.ncl" in
let meeting_room = import "office/meeting.ncl" in
let building_core = import "office/core.ncl" in
let kitchen_plan = import "../plans/rooms/kitchen.ncl" in
{
  name = "Office floor",
  system = 'metric,
  units = 'm,
  seed = 4,

  start = {
    mode = 'boundary,
    points = [[0, 0], [32, 0], [32, 17], [0, 17]],
  },

  spaces = {
    # Fixed: the core is already there. Its walls are 'core and plumbing ('wet).
    core = { kind = 'vertical, width = 9, depth = 8, at = [11.6, 9], walls = ['core, 'wet], plan = building_core },
    floor = { label = "Open floor", kind = 'zone, fill = true, area = { min = 120, max = 240 }, side.min = 1.6 },
    desks = { label = "Desks", kind = 'zone, count = 6, width = 6.4, depth = 3.8, plan = desk_cluster },
    meeting = { label = "Meeting", count = 2, area = { min = 20, max = 28 }, side.min = 3.6, plan = meeting_room },
    office = { label = "Office", count = 2, area = { min = 10, max = 14 }, side.min = 2.8 },
    booth = { label = "Phone booth", count = 2, area = { min = 2, max = 3 }, side.min = 1.2 },
    kitchen = { label = "Kitchen", area = { min = 14, max = 22 }, side.min = 3.2, plan = kitchen_plan },
    print = { label = "Print / store", area = { min = 5, max = 8 }, side.min = 2 },
  },

  rules = [
    plate.door "core" "floor" & { width = 1.8 },
    plate.door ["meeting", "office", "booth", "kitchen", "print"] "floor",
    plate.touches "desks" 'facade & { length = 6 }, # desks by the windows
    plate.touches "desks" "floor" & { length = 3 },
    plate.window ["office", "meeting"],
    plate.apart ["booth", "print"] 'facade, # inner rooms: daylight is for desks
    plate.prefer (plate.touches "kitchen" "core"), # near the risers
  ],
}
