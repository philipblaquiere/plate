# NMD90 cable, as wired in Canadian houses: what each size may carry. power.ncl uses it by
# default; build/codes/cec.ncl brings it too.
#
# `rating` is the largest breaker each may be on: CEC Table 2 (copper, 60 °C column, the
# termination limit of 4-006 for most residential equipment), and 14-104 for 14, 12 and 10 AWG.
# A household range of 12 kW or less is counted as 8 kW (8-300), so it's usually on 40 A and 8/3.
# Check the edition your jurisdiction enforces (Quebec: Code de construction, chapitre V).
# Jacket colours are the industry's: white 14, yellow 12, orange 10, white 8 and larger.

{
  media = {
    "14/2" = { label = "NMD90 14/2", gauge = 14, conductors = 2, rating = "15 A", colour = 'white },
    "14/3" = { label = "NMD90 14/3", gauge = 14, conductors = 3, rating = "15 A", colour = 'white },
    "12/2" = { label = "NMD90 12/2", gauge = 12, conductors = 2, rating = "20 A", colour = 'yellow },
    "12/3" = { label = "NMD90 12/3", gauge = 12, conductors = 3, rating = "20 A", colour = 'yellow },
    "10/2" = { label = "NMD90 10/2", gauge = 10, conductors = 2, rating = "30 A", colour = 'orange },
    "10/3" = { label = "NMD90 10/3", gauge = 10, conductors = 3, rating = "30 A", colour = 'orange }, # a dryer
    "8/3" = { label = "NMD90 8/3", gauge = 8, conductors = 3, rating = "40 A", colour = 'white }, # a range
    "6/3" = { label = "NMD90 6/3", gauge = 6, conductors = 3, rating = "55 A", colour = 'white },
  },
}
