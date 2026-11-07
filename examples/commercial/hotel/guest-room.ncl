# A guest room, as a plan of its own. It never names the corridor or the building: it talks about
# ports, the labelled edges of whatever space it's placed in ('entrance, 'facade, 'wet). That's
# what makes it portable: wing.ncl stacks it along a corridor, annex.ncl along a spine, and any
# plate that gives a space those ports can use it.
#
# Three levels in one file: the room, then its bath (imported) and its sleeping area (inline),
# each solved inside the space it belongs to.

let bathroom = import "bathroom.ncl" in
{
  # What the room asks of the space it's given. The wing sizes rooms to fit; a space that
  # doesn't fit is left empty, with a warning on the need it misses.
  needs = {
    width = { min = 3.6, max = 4.5 },
    depth.min = 7,
    ports = { entrance = 'required, facade = 'required },
  },

  spaces = {
    entry = { kind = 'circulation, area = { min = 3, max = 6 }, side.min = 1.2 },
    # `| default` makes these sizes a fallback: wing.ncl's accessible room overrides them by
    # merging its own, without needing `| force`.
    bath = { area | default = { min = 4, max = 6 }, side.min | default = 1.6, plan = bathroom },
    sleep = {
      area.min = 14,
      side.min = 3.2,
      plan = {
        spaces = {
          bed = {
            kind = 'fixture,
            width = "1930 mm", # a king, measured along the foot
            depth = "2030 mm",
            against = 'wall,
            clearance = { front = 0.7, left = 0.6, right = 0.6 },
            symbol = 'bed_king,
          },
          desk = {
            kind = 'fixture,
            width = { min = 1, max = 1.4 }, # grows while there's room
            depth = 0.6,
            against = 'wall,
            clearance.front = 0.8,
            symbol = 'desk,
          },
          armchair = { kind = 'fixture, width = 0.85, depth = 0.85, symbol = 'armchair },
        },
        rules = [
          # 'window is the window the room put in this space; `wall = true` widens it to the
          # whole wall it's on.
          plate.apart "bed" 'window & { wall = true },
          plate.prefer (plate.near "armchair" "desk" 1),
        ],
      },
    },
  },

  rules = [
    plate.door "entry" 'entrance, # a door onto whatever this room's entrance is
    plate.door ["bath", "sleep"] "entry",
    plate.window "sleep", # on a 'facade edge
    plate.prefer_by 3 (plate.touches "bath" 'wet), # baths back to back, three times as keen
  ],

  # Slots to draw this plan in when its file is opened on its own (switch them above the
  # drawing). Sides left out are a room on a corridor: 'entrance below, 'facade above, 'party
  # either side, the left one 'wet too.
  preview = [
    { name = "typical", width = 4, depth = 7.5 },
    { name = "corner", width = 4, depth = 7.5, ports = { left = 'facade, right = ['party, 'wet] } },
  ],
}
