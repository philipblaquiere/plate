# A furnished two-bedroom apartment between two neighbours, with a garden at the back.
#
# New here, compared with studio.ncl:
# - edge roles (`start.ports`): which walls are outside, shared, or open onto the garden;
# - a fill space: the hall takes whatever floor the rooms leave, in any shape, so nothing is
#   left over and it reaches every room;
# - a plan inside a space, imported from plans/: rooms furnish themselves with fixtures;
# - `count` and typicals: both bedrooms share one solved plan, drawn as "Type A";
# - zones: the living room and its kitchen are one room, with no wall between them;
# - preferences (`plate.prefer`) and goals.

# Named bedroom_plan rather than bedroom: inside `spaces`, `bedroom` means the space itself.
let bedroom_plan = import "plans/bedroom.ncl" in
let bathroom_plan = import "plans/bathroom.ncl" in
let kitchen_plan = import "plans/kitchen.ncl" in
let living_plan = import "plans/living.ncl" in
{
  name = "Garden apartment",
  system = 'metric,
  units = 'm,
  seed = 3, # same file, same seed: same layout. Change it for another solution.

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
    # count = 2 makes bedroom_1 and bedroom_2; rules can name both as "bedroom". `mirror = false`
    # keeps the shared solution from being flipped to fit: it may only turn.
    bedroom = { count = 2, area = { min = 11, max = 16 }, side.min = 3, mirror = false, plan = bedroom_plan },
    # `walls = 'wet`: the bathroom's neighbours see a plumbing wall, which the kitchen likes.
    bath = { area = { min = 4.5, max = 7 }, side.min = 1.8, walls = 'wet, plan = bathroom_plan },
    store = { area = { min = 1, max = 3 }, side.min = 0.9 },
    open_plan = {
      label = "Living / kitchen",
      area = { min = 40, max = 60 },
      side.min = 4,
      plan = {
        spaces = {
          # Zones have no walls of their own: where they meet, the floor is open.
          kitchen = { kind = 'zone, area = { min = 9, max = 14 }, side.min = 2.4, plan = kitchen_plan },
          lounge = { kind = 'zone, fill = true, plan = living_plan },
        },
        # Its counter against the bathroom's plumbing wall, if it can.
        rules = [plate.prefer (plate.touches "kitchen" 'wet & { length = 2.4 })],
      },
    },
  },

  rules = [
    plate.door "hall" 'facade, # the front door
    plate.door ["open_plan", "bedroom", "bath", "store"] "hall",
    plate.between "bedroom" "hall", # the hall keeps the bedrooms apart
    plate.window "bedroom", # on a 'facade edge unless a port is given
    plate.window "open_plan" & { port = 'outdoor, width = 2.4 }, # `&` merges options into a rule
    plate.apart "bath" 'outdoor, # the garden is for the rooms you live in
    plate.prefer (plate.touches "open_plan" "bath"), # shared plumbing: nice to have, not required
  ],

  # Once every required rule holds, goals weigh one layout against another (0 turns one off).
  goals = { utilization = 1, compactness = 0.3 },
}
