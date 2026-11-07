# Lighting: a ceiling light in every room and hall, and a switch beside each door of a room, on
# the side the door opens from.
#
# Use it with `services.lighting = import "build/services/lighting.ncl"`. A service never moves a
# wall: it places its parts on what the solver laid out. power.ncl goes further.

{
  label = "Lighting",
  parts = {
    light = { mount = 'ceiling, symbol = 'light },
    switch = { above = "1100 mm", symbol = 'switch },
  },
  place = [
    plate.over "light" ["#room", "#circulation"],
    plate.beside "switch" 'door & { spaces = "#room" },
  ],
}
