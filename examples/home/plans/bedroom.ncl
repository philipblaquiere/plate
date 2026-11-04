# A double bedroom: bed, wardrobe and a desk. Any plate can use it for a room with a door and
# a window; the bed keeps off the window wall and out of the door's swing.

{
  needs.area.min = 9,
  spaces = {
    bed = {
      kind = 'fixture,
      width = 1.6, # along the foot
      depth = 2.05,
      against = 'wall,
      clearance = { front = 0.6, left = 0.6, right = 0.6 },
      symbol = 'bed_queen,
    },
    wardrobe = {
      kind = 'fixture,
      width = { min = 1, max = 2.4 }, # grows while there's room in front of it
      depth = 0.6,
      against = 'wall,
      clearance.front = 0.8,
      symbol = 'wardrobe,
    },
    desk = {
      kind = 'fixture,
      count = { min = 0, max = 1 }, # if it fits
      width = 1.2,
      depth = 0.6,
      against = 'wall,
      clearance.front = 0.8,
      symbol = 'desk,
    },
  },
  rules = [
    plate.apart "bed" 'window & { wall = true },
    plate.prefer (plate.near "desk" "bed" 1),
  ],
  preview = { width = 3.5, depth = 4 },
}
