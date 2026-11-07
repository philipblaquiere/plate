# The numbers build/services/power.ncl is written in terms of, from the US National Electrical
# Code (NFPA 70). Merge it into the power service:
#   power = (import "build/services/power.ncl") & (import "build/codes/nec.ncl"),
# Check the edition your jurisdiction enforces: a service the solver says is met is met as
# written, not certified.

{
  params = {
    wall_spacing = "12 ft", # 210.52(A)(1): no point along the floor line more than 6 ft from a receptacle
    usable_wall = "2 ft", # 210.52(A)(2): walls 2 ft wide or more count
    counter_spacing = "4 ft", # 210.52(C)(1): no point along a counter more than 24 in from one
    per_circuit = 10, # not in the code: a common design limit for a 15 A circuit
  },
}
