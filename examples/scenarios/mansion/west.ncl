# The private wing: the main suite, two guest suites (one plan, solved once and shared: a
# typical) and a study, off a gallery that takes the rest of the wing.

let suite = import "suite.ncl" in
{
  needs.ports.link = 'required,
  spaces = {
    gallery = { kind = 'circulation, fill = true, area = { min = 30, max = 76 }, side.min = 1.8 },
    main_suite = { label = "Main suite", area = { min = 56, max = 72 }, side.min = 6, plan = suite },
    guest = { label = "Guest suite", count = 2, area = { min = 36, max = 44 }, side.min = 5, plan = suite },
    study = { area = { min = 16, max = 24 }, side.min = 3.6 },
    linen = { label = "Linen", area = { min = 3, max = 6 }, side.min = 1.2 },
  },
  rules = [
    # Right where the door from the centre of the house is: 'door marks the doors the plate put in
    # this wing's walls ('link is the whole wall they're in).
    plate.touches "gallery" 'door & { length = 1.8 },
    plate.door ["main_suite", "guest", "study", "linen"] "gallery",
    plate.touches ["main_suite", "guest", "study"] 'facade & { length = 3 },
    plate.prefer (plate.touches "main_suite" '"facade.court" & { length = 4 }),
  ],
  preview = { width = 11, depth = 24, ports = { bottom = 'facade, right = ['link, 'door], top = 'facade, left = '"facade.court" } },
}
