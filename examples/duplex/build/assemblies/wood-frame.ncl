# Wood-frame walls, as built across much of North America.
#
# Each assembly lists its build-up (`layers`, from one face to the other): its thickness and the
# cavity services can run in add up from it. `on` says which edges call for it, by port role.
# A wall between two spaces gets the thinnest assembly that meets everything asked of it: a wall
# that is both 'party and 'wet must be at least as good as both, and ratings (`fire`,
# `acoustic`) let a rule ask for more: `plate.wall "#sleep" 'partition & { acoustic = 45 }`
# builds those walls as `quiet`. `quiet` and `rated` have no `on`: they're only used when asked.

let gypsum = { material = "gypsum", thickness = "13 mm" } in
let type_x = { material = "type X gypsum", thickness = "16 mm" } in
let stud = fun depth => { material = "stud", thickness = depth, cavity = true } in
{
  exterior = {
    label = "Exterior wall",
    on = ['facade, 'outdoor],
    layers = [
      { material = "siding", thickness = "19 mm" },
      { material = "sheathing", thickness = "11 mm" },
      stud "140 mm", # insulated: services still run in it
      gypsum,
    ],
  },
  party = {
    label = "Party wall",
    on = ['party, 'blank],
    line = 'centre, # on the lot line, half on each side
    bearing = true, # carries the floors above: a rule can ask for it
    layer = "A-WALL-FIRE", # the CAD layer it's exported to
    fire = 1,
    acoustic = 55,
    cross = 'firestop, # cables may pass, through a sealed hole (the default for a fire-rated wall)
    layers = [type_x, stud "89 mm", { material = "air", thickness = "25 mm" }, stud "89 mm", type_x],
    pattern = 'cross,
  },
  wet = {
    label = "Plumbing wall",
    on = 'wet,
    layers = [gypsum, stud "140 mm", gypsum], # deep enough for a 3" drain
    pattern = 'hatch,
  },
  partition = {
    label = "Partition",
    on = ['partition, 'link],
    layers = [gypsum, stud "89 mm", gypsum],
    pattern = 'light,
  },
  quiet = {
    label = "Sound partition",
    acoustic = 50,
    layers = [gypsum, stud "89 mm", { material = "resilient channel", thickness = "13 mm" }, gypsum, gypsum],
    pattern = 'light,
  },
  rated = {
    label = "Fire-rated partition",
    fire = 1,
    layers = [type_x, stud "89 mm", type_x],
    pattern = 'cross,
  },
}
