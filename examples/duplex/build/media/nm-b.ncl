# NM-B cable, as wired in US houses: what each size may carry. Merge it into a service, or let a
# code bring it (build/codes/nec.ncl does):
#   power = (import "build/services/power.ncl") & (import "build/media/nm-b.ncl"),
#
# `rating` is the largest breaker each may be on: NEC 334.80 limits NM cable to its 60 °C
# ampacity (Table 310.16), and 240.4(D) caps 14, 12 and 10 AWG at 15, 20 and 30 A. Check the
# edition your jurisdiction enforces. Jacket colours are the industry's: white 14, yellow 12,
# orange 10, black 8 and 6.

{
  media = {
    "14/2" = { label = "NM-B 14/2", gauge = 14, conductors = 2, rating = "15 A", colour = 'white },
    "14/3" = { label = "NM-B 14/3", gauge = 14, conductors = 3, rating = "15 A", colour = 'white },
    "12/2" = { label = "NM-B 12/2", gauge = 12, conductors = 2, rating = "20 A", colour = 'yellow },
    "12/3" = { label = "NM-B 12/3", gauge = 12, conductors = 3, rating = "20 A", colour = 'yellow },
    "10/2" = { label = "NM-B 10/2", gauge = 10, conductors = 2, rating = "30 A", colour = 'orange },
    "10/3" = { label = "NM-B 10/3", gauge = 10, conductors = 3, rating = "30 A", colour = 'orange }, # a dryer
    "8/3" = { label = "NM-B 8/3", gauge = 8, conductors = 3, rating = "40 A", colour = 'black }, # a 40 A range
    "6/3" = { label = "NM-B 6/3", gauge = 6, conductors = 3, rating = "55 A", colour = 'black }, # a 50 A range
  },
}
