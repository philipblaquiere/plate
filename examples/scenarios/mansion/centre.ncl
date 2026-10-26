# The centre of the house: the front door, a gallery through to both wings, and the reception
# rooms, which look onto the courtyard behind.

let living_plan = import "../../plans/rooms/living.ncl" in
{
  needs.ports = { link = 'required, "facade.front" = 'required },
  spaces = {
    gallery = { label = "Entrance gallery", kind = 'circulation, fill = true, area = { min = 30, max = 76 }, side.min = 2.4 },
    living = { area = { min = 44, max = 64 }, side.min = 5, plan = living_plan },
    dining = {
      area = { min = 24, max = 36 },
      side.min = 4.5,
      plan = {
        spaces.table = { kind = 'fixture, width = 3, depth = 1.6, clearance = 0.9, symbol = 'dining_table_8 },
      },
    },
    library = { area = { min = 18, max = 28 }, side.min = 3.5 },
    cloak = { label = "Cloakroom", area = { min = 4, max = 6 }, side.min = 1.6, walls = 'wet },
  },
  rules = [
    plate.door "gallery" '"facade.front" & { width = 1.8 },
    # Through to both wings: the doors the plate put in this block's walls, each of them.
    plate.touches "gallery" 'door & { length = 1.8, each = true },
    plate.door ["living", "dining", "library", "cloak"] "gallery",
    plate.prefer (plate.door "library" "living"), # and, if it can, off the living room too
    plate.touches "living" "dining" & { length = 3 },
    plate.window ["living", "dining"] & { port = '"facade.court", width = 3 },
    plate.window "library",
  ],
  preview = {
    width = 14,
    depth = 14,
    ports = { bottom = '"facade.front", top = '"facade.court", left = ['link, 'door], right = ['link, 'door] },
  },
}
