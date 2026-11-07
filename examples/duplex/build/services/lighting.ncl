# Lighting: ceiling lights in every room (more in a big one), a line of them down the hall, and a
# switch beside each door of a room, on the side the door opens from.
#
# Use it with `services.lighting = import "build/services/lighting.ncl"`. A service never moves a
# wall: it places its parts on what the solver laid out. power.ncl goes further.
#
# `plate.over` spreads lights over a room. A room that isn't a rectangle (an L-shaped hall) is
# cut into rectangles, and each gets its share: a grid, or a line where it's narrow.

{
  label = "Lighting",
  parts = {
    light = { mount = 'ceiling, symbol = 'light },
    downlight = { mount = 'ceiling, symbol = 'downlight },
    switch = { above = "1100 mm", symbol = 'switch },
  },
  place = [
    # One per 16 m² of floor, at most 4: a bedroom gets one, a living room two or three.
    plate.over "light" "#room" & { per = 16, count.max = 4 },
    # Downlights no more than 2.4 m apart, in a line down each leg of the hall.
    plate.over "downlight" "#circulation" & { spacing = "2.4 m", layout = 'line, margin = "600 mm" },
    plate.beside "switch" 'door & { spaces = "#room" },
  ],
}
