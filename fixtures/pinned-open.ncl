# A test fixture, not an example: a 12 × 8 m flat with an open living area, every space pinned,
# so the layout never changes. Living and dining are zones (no wall between them: one region);
# the hall is circulation, open to both; the nook is a room with an opening onto the dining area.
# Lighting puts one switch box in the region, a switch for each zone. It's the same flat as
# `open_flat()` in crates/plate-core/src/solve/open_tests.rs.
#
#   npm run fixtures -- open                          # rewrites web/src/fixtures/pinned-open.json
{
  name = "Pinned open flat",
  seed = 1,
  start = {
    mode = 'boundary,
    points = [[0, 0], [12, 0], [12, 8], [0, 8]],
    ports = ['facade, 'blank, 'facade, 'blank],
  },
  assemblies = {
    exterior = { on = ['facade, 'blank], thickness = 0.3 },
    partition = { on = ['partition, 'link, 'entrance], thickness = 0.1 },
  },
  spaces = {
    hall = { kind = 'circulation, fill = true, area = { min = 8, max = 60 }, side = { min = 1.2 } },
    living = { kind = 'zone, at = [0, 0], width = 6, depth = 4.5 },
    dining = { kind = 'zone, at = [6, 0], width = 3, depth = 4.5 },
    nook = { at = [9, 0], width = 3, depth = 3.5 },
    bath = { at = [0, 6], width = 3.5, depth = 2 },
  },
  rules = [
    plate.door "bath" "hall",
    plate.opening "nook" "dining" & { width = { min = 1.5 }, margin = 0.6 },
  ],
  services.lighting = {
    parts = {
      switch = { above = 1.1, width = 0.08 },
      light = { mount = 'ceiling },
    },
    place = [
      plate.beside "switch" 'door & { spaces = "#room" },
      plate.one "switch" "#zone" & { per = 'region, each = true, near = 'door },
      plate.over "light" ["#room", "#zone"],
    ],
    connect = [
      { role = 'control, from = "switch", to = "light", draw = 'routed, via.ceilings = ["#room", "#zone", "#circulation"] },
    ],
  },
}
