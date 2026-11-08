# Electric heat: a baseboard heater under each window of a room, where the cold comes in, and a
# thermostat on an inside wall, near the door and away from the windows.

{
  label = "Heat",
  parts = {
    # As wide as the window; 1000 W at 240 V, on the power service's heat circuits.
    baseboard = { above = "150 mm", depth = "90 mm", symbol = 'baseboard, load = { watts = 1000, voltage = 240 } },
    thermostat = { above = "1500 mm", symbol = 'thermostat },
  },
  place = [
    plate.beside "baseboard" 'window & { spaces = ["#room", "#zone"] },
    # One per room, and one per region (spaces with no wall between them): an open living area
    # is heated as one.
    plate.one "thermostat" ["#room", "#zone"] & { per = 'region, near = 'door, apart = ['window, 'facade] },
  ],
}
