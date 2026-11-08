# Lighting: ceiling lights in every room and zone (more in a big one), a line of them down the
# hall, a switch beside each door of a room, on the side the door opens from (round the corner
# when the door fills its wall), and one in the hall and in each zone by a door.
#
# Use it with `services.lighting = import "build/services/lighting.ncl"`. A service never moves a
# wall: it places its parts on what the solver laid out.
#
# `plate.over` spreads lights over a room. A room that isn't a rectangle (an L-shaped hall) is
# cut into rectangles, and each gets its share: a grid, or a line where it's narrow.
#
# The switch legs: each room's switches joined to its lights, up the wall and across the ceiling.
# A room with two switches gets 3-conductor cable between them (a 3-way pair). With power.ncl in
# the same plate, the switches are on its circuits and the lights' load with them.

{
  label = "Lighting",
  parts = {
    light = { mount = 'ceiling, symbol = 'light, load.watts = 60 },
    downlight = { mount = 'ceiling, symbol = 'downlight, load.watts = 15 },
    switch = { above = "1100 mm", symbol = 'switch },
  },
  place = [
    # One per 16 m² of floor, at most 4: a bedroom gets one, a living room two or three.
    plate.over "light" ["#room", "#zone"] & { per = 16, count.max = 4 },
    # Downlights no more than 2.4 m apart, in a line down each leg of the hall.
    plate.over "downlight" "#circulation" & { spacing = "2.4 m", layout = 'line, margin = "600 mm" },
    plate.beside "switch" 'door & { spaces = "#room" },
    # A hall or a zone (open floor) gets one switch, on the wall nearest a door.
    plate.one "switch" ["#circulation", "#zone"] & { near = 'door },
  ],
  connect = [
    {
      label = "Switch legs",
      role = 'control,
      from = "switch",
      to = ["light", "downlight"],
      via.ceilings = ["#room", "#zone", "#circulation"],
    },
  ],
}
