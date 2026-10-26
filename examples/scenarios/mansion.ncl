# A large house in three wings around a courtyard. The wings are fixed where they stand (`at`:
# exact corners and sizes), so this file only says how they connect; each wing is a plan of its
# own in mansion/, with a gallery that takes the floor its rooms leave.

let west_wing = import "mansion/west.ncl" in
let centre_block = import "mansion/centre.ncl" in
let east_wing = import "mansion/east.ncl" in
{
  name = "Courtyard house",
  system = 'metric,
  units = 'm,
  seed = 1,

  start = {
    mode = 'boundary,
    # A U: two wings and the centre between them, open to the north.
    points = [[0, 0], [36, 0], [36, 24], [25, 24], [25, 14], [11, 14], [11, 24], [0, 24]],
    ports = ['"facade.front", 'facade, 'facade, '"facade.court", '"facade.court", '"facade.court", 'facade, 'facade],
  },

  spaces = {
    west = { label = "Private wing", width = 11, depth = 24, at = [0, 0], plan = west_wing },
    centre = { label = "Reception", width = 14, depth = 14, at = [11, 0], plan = centre_block },
    east = { label = "Service wing", width = 11, depth = 24, at = [25, 0], plan = east_wing },
  },

  rules = [
    plate.door ["west", "east"] "centre" & { width = 1.8 },
  ],
}
