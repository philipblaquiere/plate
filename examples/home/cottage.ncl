# A cottage drawn on its site plan, where the house doesn't run north-south: its walls are at
# 36.87° to the drawing's axes (a 3-4-5 triangle: 4 along, 3 up). One corner is cut off by a
# long wall at 45° to the others, facing the garden: the longest wall of the house.
#
# New here, compared with condo.ncl:
# - `align`: the solver lays rooms out square to the walls most of the outline runs along,
#   whatever angle they run at, not along the longest wall. 'auto (the default) finds that
#   angle; a number gives it in degrees;
# - the long slanted wall cuts the rooms against it, as in condo.ncl, but along the turned grid;
# - each room's plan is solved square to its room, so the beds line up with the walls.

let bedroom_plan = import "plans/bedroom.ncl" in
let bathroom_plan = import "plans/bathroom.ncl" in
{
  name = "Cottage",
  system = 'metric,
  units = 'm,
  seed = 1,
  align = 'auto, # the default, written out: square to the walls most of the outline runs along

  start = {
    mode = 'boundary,
    # 15 × 15 m with a corner cut from 3 m up one side to 3 m along the other, turned: along
    # the walls (4, 3) per 5 m, across (-3, 4).
    points = [[9, 0], [21, 9], [19.2, 11.4], [2.4, 13.8], [0, 12]],
    # The lane is along the wall at the bottom; a garden all round.
    ports = ['entrance, 'facade, 'facade, 'facade, 'facade],
  },

  spaces = {
    hall = { kind = 'circulation, fill = true, area.min = 8, side.min = 1.2 },
    living = { label = "Living / kitchen", area = { min = 40, max = 60 }, side.min = 4.5 },
    bedroom = { count = 3, area = { min = 12, max = 18 }, side.min = 3, plan = bedroom_plan },
    bath = { area = { min = 5, max = 8 }, side.min = 2, walls = 'wet, plan = bathroom_plan },
    store = { area = { min = 2, max = 4 }, side.min = 1.2, shape = 'rect },
  },

  rules = [
    plate.door "hall" 'entrance,
    plate.door ["living", "bedroom", "bath", "store"] "hall",
    plate.window ["living", "bedroom"],
    plate.apart ["bath", "store"] 'facade,
  ],
}
