# A hotel's ground floor, across five files. This one is the building; the plans it uses are in
# plans/, each solved inside the space it's given, five levels deep:
#
#   3-hotel.ncl              the plot: a lobby block and a guest wing, side by side
#   plans/public.ncl         lobby, café, back of house, lifts, toilets
#   plans/cafe.ncl           the café's counter and tables
#   plans/wing.ncl           a corridor with guest rooms stacked on both sides
#   plans/guest-room.ncl     entry, bath and bedroom, furnished
#   plans/bathroom.ncl       shower, toilet and basin
#
# Open a plan next to this file (Ctrl+\) and edit it: every room using it re-solves. Right-click
# a room in the drawing to open its plan in that room's exact slot.

let guest_wing = import "plans/wing.ncl" in
let public_side = import "plans/public.ncl" in

# Names for numbers used more than once. Drag a corner in the drawing and the binding is
# rewritten, so the boundary and the spaces stay in step.
let lot = 58 in
let wing_depth = 17 in
let block = 22 in

{
  name = "Hotel ground floor",
  system = 'metric,
  units = 'm,
  seed = 6, # same file, same seed: same layout. Change it for another solution.

  start = {
    mode = 'boundary,
    points = [
      [0, 0],
      [block, 0],
      [lot, 0],
      [lot, wing_depth],
      [block, wing_depth],
      [block, 30],
      [0, 30],
    ],
    # A role per edge, from point 1 to 2, 2 to 3, …. The street edge of the lobby block is a
    # shopfront; the far end of the wing is on the lot line, where nothing may open.
    ports = ['"facade.storefront", 'facade, 'blank, 'facade, 'facade, 'facade, 'facade],
  },

  spaces = {
    public = { label = "Lobby and café", width = block, depth = 30, plan = public_side },
    # The binding is called guest_wing, not wing: inside `spaces`, `wing` is this space itself.
    wing = { label = "Guest wing", width = lot - block, depth = wing_depth, plan = guest_wing },
  },

  rules = [
    plate.touches "wing" 'blank, # the wing runs to the lot line
    # A door between two spaces gives each a 'link port where it is: public.ncl puts its
    # lobby there.
    plate.door "wing" "public" & { width = 1.8 },
  ],

  goals = { utilization = 1, compactness = 0.3 },
}
