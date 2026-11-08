# The two-bedroom flat of flat.ncl, opened up: living, dining and kitchen are zones, so no wall
# is built between them, and the hall runs into the living room. The bedrooms, bath and utility
# are the same rooms behind the same doors.
#
# Nothing is copied: the flat is imported and changed by merging (`&`). New spaces and fields are
# added; `| force` replaces what flat.ncl says (its living room's size, its rules).
#
# The services see the open part as one region (spaces with no wall between them): one
# thermostat for it, and one switch box with a switch for each zone. Opened on its own, it's
# solved in flat.ncl's preview: the clear space a flat of duplex.ncl gets.

(import "flat.ncl")
& {
  spaces = {
    living = { kind = 'zone, area | force = { min = 180, max = 260 } },
    dining = { kind = 'zone, area = { min = 80, max = 120 }, side.min = 9 },
    # With open sides it has fewer walls for its fixtures: a shorter counter fits one wall with
    # the range and fridge.
    kitchen = {
      kind = 'zone,
      side.min | force = 11,
      area | force = { min = 110, max = 150 },
      plan.spaces.counter.width | force = { min = 6, max = 10 },
    },
  },
  rules
    | force
    = [
      plate.touches "hall" 'door,
      plate.door ["bedroom", "bath", "utility"] "hall",
      plate.open "living" "hall" & { length = 6 }, # you walk into the living room
      plate.open "dining" "living",
      plate.open "kitchen" ["dining", "living"],
      plate.touches "kitchen" ["bath", "utility"] & { length = 5 }, # a wet wall for its sink
      plate.reach "bedroom" "living" "hall", # the bedrooms are off the hall
      plate.window ["living", "bedroom"],
      plate.wall "#sleep" "#sleep" & { acoustic = 45 },
    ],
}
