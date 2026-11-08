# A test fixture, not an example: a 12 × 8 m flat with every room pinned (`at` plus an exact
# size), so the engine has nothing to move and the layout never changes. Walls, parts and
# networks are still solved on it, in milliseconds. It's the same flat as `pinned_flat()` in
# crates/plate-core/src/solve/testkit.rs.
#
#   plate check fixtures/pinned-flat.ncl              # 0 moves: nothing annealed
#   npm run fixtures                                  # rewrites web/src/fixtures/pinned-flat.json
{
  name = "Pinned flat",
  seed = 1,
  start = {
    mode = 'boundary,
    points = [[0, 0], [12, 0], [12, 8], [0, 8]],
    ports = ['facade, 'blank, 'facade, 'blank],
  },
  assemblies = {
    exterior = { on = 'facade, thickness = 0.3, cavity = 0.14 },
    party = { on = 'blank, thickness = 0.25, fire = 1 },
    partition = { on = ['partition, 'link, 'entrance], thickness = 0.1, cavity = 0.09 },
    wet = { on = 'wet, thickness = 0.15, cavity = 0.14 },
  },
  spaces = {
    hall = { kind = 'circulation, fill = true, area = { min = 8, max = 60 }, side = { min = 1.2 } },
    living = { at = [0, 0], width = 6, depth = 4.5 },
    bedroom = { at = [8, 0], width = 4, depth = 3.5, tags = ['sleep] },
    bath = { at = [0, 6], width = 3.5, depth = 2, walls = 'wet, tags = ['wet] },
  },
  rules = [
    plate.door ["living", "#sleep", "#wet"] "hall",
    plate.window ["living", "#sleep"],
  ],
  services = {
    power = {
      parts = {
        panel = { above = 1.5, width = 0.4 },
        outlet = { above = 0.3 },
      },
      place = [
        { place = 'one, part = "panel", spaces = "hall" },
        { place = 'every, part = "outlet", spaces = ["living", "#sleep"], spacing = 3.6 },
      ],
      connect = [
        { label = "General", from = "panel", to = "outlet", max = 6, draw = 'routed, via = { ceilings = "hall" } },
      ],
    },
    lighting = {
      parts = {
        switch = { above = 1.1 },
        light = { mount = 'ceiling },
      },
      place = [
        { place = 'beside, part = "switch", opening = 'door, spaces = ["living", "#sleep", "#wet"] },
        { place = 'over, part = "light", spaces = ["living", "#sleep", "#wet"] },
      ],
    },
    plumbing = {
      parts = {
        stack = { mount = 'cavity, width = 0.1, depth = 0.1 },
        drain = { mount = 'floor },
      },
      place = [
        { place = 'one, part = "stack", spaces = "#wet", wall = { cavity = 0.12 } },
        { place = 'over, part = "drain", spaces = "#wet" },
      ],
      connect = [{ from = "stack", to = "drain", draw = 'routed, limit = 10 }],
    },
  },
}
