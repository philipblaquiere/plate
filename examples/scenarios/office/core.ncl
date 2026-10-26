# A building core: stairs, lifts and toilets, behind the walls the floor plate wraps around.
# The core's own walls give the floor 'core edges (`walls` on the core in the plate).

{
  spaces = {
    lobby = { label = "Lift lobby", kind = 'circulation, fill = true, area.min = 8, side.min = 2 },
    stair = { kind = 'vertical, width = 2.75, depth = 5.5 },
    lift = { kind = 'vertical, count = 2, width = 2.25, depth = 2.25, symbol = 'lift },
    wc = { label = "WC", count = 2, area = { min = 6, max = 10 }, side.min = 2, walls = 'wet },
    shaft = { label = "Riser", kind = 'void, width = 1.25, depth = 2 },
  },
  rules = [
    plate.door "lobby" 'entrance & { width = 1.8 },
    plate.door ["stair", "wc"] "lobby",
    plate.touches "lift" "lobby" & { length = 1.5 },
    plate.touches "shaft" "wc",
  ],
  preview = { width = 9, depth = 8, ports = { bottom = 'entrance, left = 'partition, right = 'partition, top = 'facade } },
}
