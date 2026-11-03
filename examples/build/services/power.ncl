# Power, one panel per dwelling: receptacles along every wall, counter receptacles over the
# kitchen counter, a dedicated one behind each appliance, all on circuits back to the panel.
#
# Written in terms of a code's numbers (`params`), the Canadian code's by default. For the US,
# merge in the other code's:
#   services.power = (import "build/services/power.ncl") & (import "build/codes/nec.ncl"),
#
# The service names spaces by tag, so it fits any plan that tags them: 'dwelling (its scope: one
# panel and one set of circuits each), 'utility (where the panel goes), 'counter and 'appliance
# (fixtures).

{
  label = "Power",
  scope = "#dwelling",
  params | default = (import "../codes/cec.ncl").params,
  parts = {
    panel = { width = "400 mm", depth = "160 mm", above = "1500 mm", symbol = 'panel },
    outlet = { label = "receptacle", above = "300 mm", symbol = 'receptacle },
    counter = { label = "counter receptacle", above = "1100 mm", symbol = 'receptacle_gfci },
    dedicated = { label = "dedicated receptacle", above = "600 mm", symbol = 'receptacle },
  },
  # What the layout must make room for: a stretch of wall in the utility room for the panel.
  claims.rules = [plate.wall "#utility" 'wall & { cavity = "89 mm", length = 1 }],
  place = [
    plate.one "panel" "#utility" & { near = 'door, wall.cavity = "89 mm" },
    plate.on "dedicated" "#appliance",
    plate.every "counter" "#counter" params.counter_spacing,
    plate.every "outlet" ["#room", "#circulation"] params.wall_spacing & { min_run = params.usable_wall },
  ],
  connect = [
    { label = "Circuits", from = "panel", to = ["outlet", "dedicated"], max = params.per_circuit, via.cavity = "89 mm" },
    { label = "Counter circuits", from = "panel", to = "counter", max = 2, via.cavity = "89 mm" },
    { label = "Heat circuits", from = "panel", to = "heat.baseboard", max = 3, via.cavity = "89 mm" },
  ],
}
