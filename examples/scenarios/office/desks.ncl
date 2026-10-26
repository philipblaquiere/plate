# A cluster of six desks: two benches of three, back to back, with the chairs on the outside.
# The unit open-plan offices are made of. `row = true` places each bench as one straight run.
# It's a zone's plan: a zone has no walls of its own, so nothing here backs onto its edges.

let bench = {
  kind = 'fixture,
  count = 3,
  row = true, # three desks side by side
  width = 1.6,
  depth = 0.8,
  clearance.front = 1, # room for the chairs
  symbol = 'desk,
} in
{
  needs.area.min = 18,
  spaces = { north = bench, south = bench },
  rules = [
    plate.touches "north" "south" & { length = 4.8 }, # the benches meet all along their backs
  ],
  preview = { seed = 3, width = 6.4, depth = 3.8, ports = { bottom = 'open, top = 'facade, left = 'open, right = 'open } },
}
