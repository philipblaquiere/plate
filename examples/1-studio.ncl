# The smallest useful plate: an outline, three spaces and the rules between them.
# The solver lays it out as you type. Change a number and watch it re-solve; drag a corner in
# the drawing and the points below follow. 2-apartment.ncl and 3-hotel.ncl go further.

{
  name = "Studio",
  system = 'metric, # how lengths are shown: 'metric or 'imperial
  units = 'm, # the unit of plain numbers

  start = {
    mode = 'boundary, # everything is placed inside this outline
    points = [[0, 0], [8, 0], [8, 6], [0, 6]], # corners, in order, as [x, y]
  },

  # The program, by name. Sizes are ranges; the solver picks the rest.
  spaces = {
    living = { area.min = 30 }, # area.min = 30 is short for area = { min = 30 }
    bath = { area = { min = 4, max = 6 }, side.min = 1.8 },
    entry = { kind = 'circulation, area = { min = 3, max = 6 }, side.min = 1.2 },
  },

  # How spaces relate. Type `plate.` for the full list, with docs.
  rules = [
    plate.door "entry" 'facade, # the front door, on an outside wall
    plate.door ["living", "bath"] "entry",
    plate.window "living",
  ],
}
