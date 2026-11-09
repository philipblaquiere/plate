# A corner unit high up in a condo tower: the outline is the building's, with three walls at an
# angle and a step where the neighbour's unit comes in.
#
# New here, compared with apartment.ncl:
# - slanted edges: rooms are placed as rectangles and cut by the outline, so a room on an angled
#   facade takes the floor right up to it (`shape = 'fit`, the default). Its area is the floor it
#   gets; its width, depth and side are its rectangle's;
# - edges off the grid (the step is at 14.6 m on a 0.5 m grid): the rooms beside them reach them;
# - `shape = 'rect` keeps a space square to the grid whatever the outline does;
# - windows on the slanted facades, and plans furnished inside rooms the outline cuts.

let bedroom_plan = import "plans/bedroom.ncl" in
let bathroom_plan = import "plans/bathroom.ncl" in
let kitchen_plan = import "plans/kitchen.ncl" in
let living_plan = import "plans/living.ncl" in
{
  name = "Corner condo",
  system = 'metric,
  units = 'm,
  seed = 3,

  start = {
    mode = 'boundary,
    points = [
      [5.5, 0], [24.1, 0], [24.1, 5.95], [19.35, 9.75], [14.6, 9.75],
      [14.6, 15.25], [5.5, 19.55], [0, 19.55], [0, 7.95], [5.5, 3.65],
    ],
    # The step is the neighbour's unit, and the lift lobby is along its long side.
    ports = ['facade, 'facade, 'facade, 'entrance, 'party, 'facade, 'facade, 'facade, 'facade, 'facade],
  },

  spaces = {
    hall = { kind = 'circulation, fill = true, area.min = 14, side.min = 1.4 },
    living = { label = "Living / dining", kind = 'zone, area = { min = 80, max = 125 }, side.min = 6, plan = living_plan },
    kitchen = { kind = 'zone, area = { min = 16, max = 26 }, side.min = 3.2, plan = kitchen_plan },
    primary = { label = "Primary bedroom", area = { min = 22, max = 34 }, side.min = 3.8, plan = bedroom_plan },
    bedroom = { count = 2, area = { min = 14, max = 22 }, side.min = 3.2, plan = bedroom_plan },
    bath = { count = 2, area = { min = 6, max = 9 }, side.min = 2.2, walls = 'wet, plan = bathroom_plan },
    den = { area = { min = 12, max = 22 }, side.min = 3 },
    # A closet that stays square: the outline never cuts it.
    store = { area = { min = 2, max = 4 }, side.min = 1.2, shape = 'rect },
  },

  rules = [
    plate.door "hall" 'entrance,
    plate.door ["primary", "bedroom", "bath", "den", "store"] "hall",
    plate.touches "kitchen" "living" & { length = 3 },
    plate.window ["living", "primary", "bedroom", "den"],
    plate.apart ["bath", "store"] 'facade,
  ],
}
