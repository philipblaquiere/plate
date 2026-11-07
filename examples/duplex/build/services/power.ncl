# Power, one panel per dwelling: receptacles along every wall, counter receptacles over the
# kitchen counter, a receptacle behind each appliance, all on circuits back to the panel. The
# lighting service's switches are on the general circuits too, and the heaters on their own.
#
# Written in terms of a code's numbers (`params`) and its cable (`media`), the Canadian code's
# by default. For the US, merge in the other code's:
#   services.power = (import "build/services/power.ncl") & (import "build/codes/nec.ncl"),
#
# The service names spaces by tag, so it fits any plan that tags them: 'dwelling (its scope: one
# panel and one set of circuits each), 'utility (where the panel goes), 'counter, 'appliance,
# 'cooking and 'laundry (fixtures).
#
# Each circuit is a tree of cables: in the walls at outlet height, up and over the doors, across
# the ceilings to reach the lights' switches. Each cable is the thinnest of `media` that meets
# its circuit's breaker: 14/2 on 15 A, 12/2 on 20 A, and what a range or a dryer asks for
# (`load` on the fixture, in the plan). Press A in the Power view for the schematic.

let walls_and_ceilings = { cavity = "89 mm", ceilings = ["#room", "#circulation"] } in
{
  label = "Power",
  scope = "#dwelling",
  params | default = (import "../codes/cec.ncl").params,
  media | default = (import "../media/nmd90.ncl").media,
  parts = {
    panel = { width = "400 mm", depth = "160 mm", above = "1500 mm", symbol = 'panel },
    outlet = { label = "receptacle", above = "300 mm", symbol = 'receptacle },
    counter = { label = "counter receptacle", above = "1100 mm", symbol = 'receptacle_gfci },
    dedicated = { label = "dedicated receptacle", above = "600 mm", symbol = 'receptacle },
    # Takes its fixture's load (a range's 40 A, a dryer's 30 A), so its circuit is sized for it.
    outlet_240 = { label = "240 V receptacle", above = "300 mm", symbol = 'receptacle_240 },
  },
  # What the layout must make room for: a stretch of wall in the utility room for the panel.
  claims.rules = [plate.wall "#utility" 'wall & { cavity = "89 mm", length = "1 m" }],
  place = [
    plate.one "panel" "#utility" & { near = 'door, wall.cavity = "89 mm" },
    plate.on "dedicated" "#appliance",
    plate.on "outlet_240" ["#cooking", "#laundry"],
    plate.every "counter" "#counter" params.counter_spacing,
    plate.every "outlet" ["#room", "#circulation"] params.wall_spacing & { min_run = params.usable_wall },
  ],
  # Appliances get 20 A, whatever circuit they're on.
  feeds = [plate.feed "#appliance" & { rating = "20 A" }],
  connect = [
    { label = "General", from = "panel", to = ["outlet", "lighting.switch"], rating = "15 A", max = params.per_circuit, via = walls_and_ceilings },
    { label = "Counter", from = "panel", to = "counter", rating = "20 A", medium = "12/2", max = 2, via = walls_and_ceilings },
    { label = "Appliances", from = "panel", to = ["dedicated", "outlet_240"], dedicated = true, via = walls_and_ceilings },
    { label = "Heat", from = "panel", to = "heat.baseboard", rating = "20 A", via = walls_and_ceilings },
  ],
}
