# Electric heat: a baseboard heater under each window of a room, where the cold comes in, and a
# thermostat on an inside wall, near the door and away from the windows.

{
  label = "Heat",
  parts = {
    baseboard = { above = "150 mm", depth = "90 mm", symbol = 'baseboard }, # as wide as the window
    thermostat = { above = "1500 mm", symbol = 'thermostat },
  },
  place = [
    plate.beside "baseboard" 'window & { spaces = "#room" },
    plate.one "thermostat" "#room" & { near = 'door, apart = ['window, 'facade] },
  ],
}
