# The public side of the ground floor: lobby, café, back of house, lifts and toilets. Like the
# other plans, it only talks about ports: the shopfront along the street, and a 'link where the
# plate puts the door to the guest wing. The lobby is the fill: whatever floor the rooms leave,
# furnished by its own plan in whatever shape it takes.

let cafe_plan = import "cafe.ncl" in
{
  needs.ports = { "facade.storefront" = 'required, link = 'required },

  spaces = {
    lobby = {
      kind = 'circulation,
      fill = true,
      area = { min = 120, max = 300 },
      side.min = 2.4, # its narrowest passage
      plan = {
        spaces = {
          reception = { kind = 'fixture, width = 3, depth = 0.8, against = 'wall, clearance.front = 1.5, symbol = 'counter },
          sofa = { kind = 'fixture, count = 2, width = 2.1, depth = 0.9, clearance.front = 1, symbol = 'sofa },
          plant = { kind = 'fixture, count = 3, width = 0.6, depth = 0.6, symbol = 'house_plant },
        },
        rules = [plate.apart "reception" '"facade.storefront" & { wall = true }],
      },
    },
    cafe = { label = "Café", area = { min = 120, max = 180 }, side.min = 8, plan = cafe_plan },
    kitchen = { label = "Back of house", area = { min = 60, max = 100 }, side.min = 5, walls = 'wet },
    office = { label = "Hotel office", area = { min = 14, max = 24 }, side.min = 3 },
    luggage = { label = "Luggage", area = { min = 8, max = 14 }, side.min = 2.4 },
    core = {
      kind = 'vertical,
      label = "Stair and lift",
      width = 6,
      depth = 6,
      plan = {
        spaces = {
          stair = { kind = 'fixture, width = 2.4, depth = 5, against = 'wall, symbol = 'stair },
          lift = { kind = 'fixture, width = 2, depth = 2, against = 'wall, clearance.front = 1.5, symbol = 'lift },
        },
      },
    },
    wc = { count = 2, area = { min = 6, max = 9 }, side.min = 2, symbol = 'toilet },
  },

  rules = [
    plate.door "lobby" '"facade.storefront" & { width = 2 }, # the front door, onto the street
    plate.touches "lobby" 'link, # and through to the guest wing
    plate.door ["cafe", "wc", "office", "luggage"] "lobby",
    plate.door "kitchen" "cafe",
    plate.reach "kitchen" "lobby" "cafe", # staff reach the lobby only through the café
    plate.touches "cafe" '"facade.storefront",
    plate.window "cafe" & { port = '"facade.storefront", width = 4 },
    plate.apart ["kitchen", "wc*", "luggage"] '"facade.storefront", # "wc*" is a pattern: wc_1, wc_2
    plate.prefer_by 2 (plate.touches "core" "lobby"),
  ],

  goals = { utilization = 1, compactness = 0.3 },

  preview = { seed = 3,
    width = 22,
    depth = 30,
    ports = { bottom = '"facade.storefront", top = 'facade, left = 'facade, right = 'link },
  },
}
