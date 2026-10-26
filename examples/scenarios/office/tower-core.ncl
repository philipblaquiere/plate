# A tower core: two banks of three lifts facing a lift lobby, two stairs, toilets and risers.
# A bank is a `row` of lift fixtures: three side by side, doors in line.

let bank = { kind = 'fixture, count = 3, row = true, width = 2.4, depth = 2.4, clearance.front = 3, symbol = 'lift } in
{
  spaces = {
    lobby = { label = "Lift lobby", kind = 'circulation, fill = true, area = { min = 60, max = 140 }, side.min = 1.8 },
    east = bank,
    west = bank,
    stair = { kind = 'vertical, count = 2, width = 2.8, depth = 6 },
    wc = { label = "WC", count = 2, area = { min = 20, max = 30 }, side.min = 3.2, walls = 'wet },
    store = { label = "Cleaner's store", area = { min = 4, max = 8 }, side.min = 1.6 },
    riser = { kind = 'void, count = 2, area = { min = 3, max = 6 }, side.min = 1.2 },
  },
  rules = [
    plate.door "lobby" 'entrance & { width = 2 },
    plate.faces "east" "west", # the banks face each other across the lobby
    plate.door ["stair", "wc", "store"] "lobby",
    plate.touches "riser" "wc",
  ],
  preview = { seed = 3, width = 17.6, depth = 14.4, ports = { bottom = 'entrance, right = 'entrance, top = 'entrance, left = 'entrance } },
}
