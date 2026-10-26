# A two-bedroom apartment between two neighbours, with a garden at the back.
#
# New here, compared with 1-studio.ncl:
# - edge roles (`start.ports`): which walls are outside, shared, or open onto the garden;
# - a fill space: the hall takes whatever floor the rooms leave, in any shape, so nothing is
#   left over and it reaches every room;
# - spaces repeated with `count`, and a plan inside a space: each bedroom is furnished by
#   `bedroom_plan`, solved once and shared by both bedrooms (a "typical");
# - fixtures (a bed, a wardrobe) with clearances, and symbols to draw them;
# - more rules, preferences (`plate.prefer`) and goals.

# A plan, named with `let` so spaces can use it. It's called bedroom_plan rather than bedroom:
# inside `spaces`, `bedroom` would mean the space itself.
let bedroom_plan = {
  spaces = {
    # A fixture takes up floor and holds nothing else. `width` runs along its front (the foot
    # of the bed), `depth` front to back; `clearance` is floor kept clear around it (dashed).
    bed = {
      kind = 'fixture,
      width = 1.53,
      depth = 2.03,
      against = 'wall, # its back against an edge of the room
      clearance = { front = 0.6, left = 0.6, right = 0.6 },
      symbol = 'bed_queen, # a drawing from the catalog: changes the look, never the layout
    },
    wardrobe = {
      kind = 'fixture,
      width = { min = 1, max = 2.4 }, # grows while there's room in front of it
      depth = 0.6,
      against = 'wall,
      clearance.front = 0.8,
      symbol = 'wardrobe,
    },
  },
  rules = [
    # 'window is the window the apartment gives each bedroom (a port of the room).
    # `wall = true` keeps the bed off the whole wall it's on.
    plate.apart "bed" 'window & { wall = true },
  ],
} in
{
  name = "Garden apartment",
  system = 'metric,
  units = 'm,
  seed = 1, # same file, same seed: same layout. Change it for another solution.
  start = {
    mode = 'boundary,
    points = [[0, 0], [15, 0], [15, 9], [0, 9]],
    # One role per edge, from point 1 to 2, 2 to 3, …: the street side, a wall shared with the
    # neighbour, the garden side, the other neighbour. Right-click an edge in the drawing to change it.
    ports = ['facade, 'blank, 'outdoor, 'blank],
  },
  spaces = {
    # The fill: the floor the other spaces leave, whatever its shape. Its side.min is its
    # narrowest passage. The other spaces grow up to their area.max; it takes the rest.
    hall = { kind = 'circulation, fill = true, area.min = 6, side.min = 1.2 },
    living = { area = { min = 30, max = 50 }, side.min = 4 },
    kitchen = { area = { min = 9, max = 14 }, side.min = 2.4 },
    # count = 2 makes bedroom_1 and bedroom_2; rules can name both as "bedroom".
    bedroom = { count = 2, area = { min = 11, max = 16 }, side.min = 3, plan = bedroom_plan },
    bath = { area = { min = 4.5, max = 7 }, side.min = 1.8 },
  },
  rules = [
    plate.door ["living", "bedroom", "bath"] "hall",
    plate.touches "kitchen" "living",
    plate.between "bedroom" "hall", # the hall keeps the bedrooms apart
    plate.window "bedroom", # on a 'facade edge unless a port is given
    plate.window "living" & { port = 'outdoor, width = 2.4 }, # `&` merges options into a rule
    plate.apart "bedroom" "kitchen", # no shared wall
    plate.prefer (plate.window "kitchen" & { port = 'outdoor }), # nice to have, not required
  ],
  # Once every required rule holds, goals weigh one layout against another (0 turns one off).
  goals = { utilization = 1, compactness = 0.3 },
}
