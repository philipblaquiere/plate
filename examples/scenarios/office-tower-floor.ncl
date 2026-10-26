# A typical floor of an office tower: a square plate around a central core that's already
# there (`at`). Team areas take the window walls, meeting and focus rooms sit between them,
# and the open floor (the fill) runs round the core and reaches everything.

let workplace = import "office/workplace.ncl" in
let tower_core = import "office/tower-core.ncl" in
let meeting_room = import "office/meeting.ncl" in
let kitchen_plan = import "../plans/rooms/kitchen.ncl" in
{
  name = "Tower floor",
  system = 'metric,
  units = 'm,
  seed = 1,

  start = {
    mode = 'boundary,
    points = [[0, 0], [45.2, 0], [45.2, 45.2], [0, 45.2]],
  },

  spaces = {
    core = {
      kind = 'vertical,
      width = 17.6,
      depth = 14.4,
      at = [13.6, 15.2], # in the middle
      walls = ['core, 'wet],
      plan = tower_core,
    },
    floor = { label = "Open floor", kind = 'zone, fill = true, area = { min = 400, max = 900 }, side.min = 1.8 },
    team = { label = "Team area", kind = 'zone, count = 8, width = 12.8, depth = 7.6, plan = workplace },
    meeting = { label = "Meeting", count = 4, area = { min = 20, max = 30 }, side.min = 3.6, plan = meeting_room },
    focus = { label = "Focus room", count = 4, area = { min = 5, max = 8 }, side.min = 2 },
    pantry = { label = "Pantry", area = { min = 30, max = 45 }, side.min = 4.4, plan = kitchen_plan },
    print = { label = "Print", area = { min = 6, max = 10 }, side.min = 2.2 },
    comms = { label = "Comms", area = { min = 6, max = 9 }, side.min = 2.2 },
  },

  rules = [
    plate.door "core" "floor" & { width = 2 },
    plate.touches "team" 'facade & { length = 12 }, # the team areas get the windows
    plate.touches "team" "floor" & { length = 6 },
    plate.door ["meeting", "focus", "pantry", "print", "comms"] "floor",
    plate.prefer (plate.window "meeting"),
    plate.apart ["focus", "print", "comms"] 'facade, # inner rooms
    plate.touches "pantry" "core", # by the risers
  ],
}
