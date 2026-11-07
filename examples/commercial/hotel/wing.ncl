# The guest wing: a corridor down the middle with rooms stacked on both sides. `engine =
# 'spine_stack` lines spaces up along a corridor (the slot's long axis here), where the default
# 'grid engine would pack them freely.
#
# The stack gives every room its ports: 'entrance on the corridor, 'facade on the outside,
# 'party between rooms and 'wet on every other party wall, which is what guest-room.ncl asks for.

let guest_room = import "guest-room.ncl" in

# Cascading: the same room with a bigger bath and a wider entry, for wheelchair users.
# `&` merges records deeply. The bath sizes were written `| default` in guest-room.ncl, so plain
# values replace them; the entry's side was not, so it needs `| force`.
let accessible_room = guest_room & {
  spaces.bath.area = { min = 5.5, max = 7 },
  spaces.bath.side.min = 2.2,
  spaces.entry.side.min | force = 1.5,
} in
{
  engine = 'spine_stack,

  # Rooms need daylight: the wing must have an outside wall.
  needs.ports.facade = 'required,

  spaces = {
    corridor = { kind = 'circulation, width = 2 },
    # `count.min = 1`: as many rooms as fit. They're numbered along the corridor, so a name
    # stays on the same room as the layout changes.
    room = { count.min = 1, width = { min = 3.6, max = 4.2 }, depth = 7.5, plan = guest_room },
    # Spaces that aren't lined up along the corridor take its ends.
    stair = { kind = 'vertical, width = 3.6, depth = 7.5, symbol = 'stair },
  },

  rules = [
    plate.along "room" "corridor", # both sides; `& { sides = 'left }` for one
    # Rooms whose slots match (same size and ports, turned or mirrored) are solved once and
    # share the solution: "Type A", "Type B" in the drawing. These rules take rooms out of that.
    plate.instance "room_1" accessible_room, # another plan for this one
    plate.unique "room_2", # its own solution (right-click a room → Make unique)
  ],

  # A slot like the one the plate gives the wing: facades front and back, the lobby's door on
  # the left, the lot line on the right.
  preview = { width = 36, depth = 17, ports = { bottom = 'facade, top = 'facade, left = 'link, right = 'blank } },
}
