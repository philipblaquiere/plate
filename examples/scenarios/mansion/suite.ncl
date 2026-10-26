# A bedroom suite: a bathroom and a walk-in closet off the bedroom, which takes the rest of the
# suite (the fill) and is furnished by the shared bedroom plan, whatever its shape.

let bedroom_plan = import "../../plans/rooms/bedroom.ncl" in
let bathroom_plan = import "../../plans/bathroom.ncl" in
{
  needs = { area.min = 24, ports = { entrance = 'required, facade = 'required } },
  spaces = {
    # For a fill, side.min is its narrowest passage: the bedroom may wrap round the closet.
    bedroom = { fill = true, area.min = 14, side.min = 0.9, plan = bedroom_plan },
    bath = { area = { min = 5, max = 8 }, side.min = 2, plan = bathroom_plan, walls = 'wet },
    closet = { label = "Walk-in", area = { min = 3, max = 6 }, side.min = 1.6 },
  },
  rules = [
    plate.door "bedroom" 'entrance,
    plate.door ["bath", "closet"] "bedroom",
    plate.window "bedroom",
    plate.apart "closet" 'facade, # the bedroom gets the daylight
  ],
  preview = { width = 6, depth = 6.5 },
}
