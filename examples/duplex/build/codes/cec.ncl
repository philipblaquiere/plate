# The numbers build/services/power.ncl is written in terms of, from the Canadian Electrical Code,
# Part I (2021). power.ncl has them by default; merge this to say so, or nec.ncl for the US:
#   power = (import "build/services/power.ncl") & (import "build/codes/nec.ncl"),
# Check the edition your jurisdiction enforces: a service the solver says is met is met as
# written, not certified.

{
  media = (import "../media/nmd90.ncl").media, # the cable it's wired with
  params = {
    wall_spacing = "3.6 m", # 26-722 a): no point along a wall more than 1.8 m from a receptacle
    usable_wall = "900 mm", # 26-722 c): shorter walls don't count
    counter_spacing = "1.8 m", # 26-722 d): no point along a counter more than 900 mm from one
    per_circuit = 12, # 8-304: a 15 A circuit with a breaker marked for 80 %
  },
}
