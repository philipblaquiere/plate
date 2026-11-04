# A guest annex, started from a spine instead of an outline: draw the corridor's line, and the
# solver stacks rooms along it. There is no boundary to fill, so the building is as long as the
# rooms need.
#
# New here, compared with hotel.ncl: `start.mode = 'spine`. It uses hotel/guest-room.ncl as is,
# because that plan only talks about ports. Change `count` and the annex grows.

let guest_room = import "hotel/guest-room.ncl" in
{
  name = "Guest annex",
  system = 'metric,
  units = 'm,
  seed = 1,

  start = {
    mode = 'spine, # the plate grows around this line
    points = [[0, 0], [24, 0]],
  },

  spaces = {
    corridor = { kind = 'circulation, width = 2 },
    room = { count = 12, width = 4, depth = 7.5, plan = guest_room },
  },

  rules = [plate.along "room" "corridor"],
}
