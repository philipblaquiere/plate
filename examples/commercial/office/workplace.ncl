# A team area on an open office floor: four benches of six desks (back to back in pairs) and
# a storage wall. A zone: no walls of its own, just its edge on the open floor.

let bench = {
  kind = 'fixture,
  count = 3,
  row = true,
  width = 1.6,
  depth = 0.8,
  clearance.front = 1,
  symbol = 'desk,
} in
{
  needs.area.min = 40,
  spaces = {
    a = bench,
    b = bench,
    c = bench,
    d = bench,
    storage = { kind = 'fixture, width = { min = 2.4, max = 4.8 }, depth = 0.5, clearance.front = 1, symbol = 'bookshelf },
  },
  rules = [
    plate.touches "a" "b" & { length = 4.8 }, # two pods, back to back
    plate.touches "c" "d" & { length = 4.8 },
  ],
  preview = { seed = 2, width = 12.8, depth = 7.6, ports = { bottom = 'open, top = 'facade, left = 'open, right = 'open } },
}
