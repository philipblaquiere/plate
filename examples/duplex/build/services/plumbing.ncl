# Plumbing, per dwelling. Its claims shape the layout: every wet room gets a plumbing wall, and
# the fixtures that use water stand against it. Then a stack goes in a plumbing wall near the
# drains, and each drain runs in the walls to it, no further than a drain can fall.
#
# Needs an assembly named `wet` (build/assemblies/wood-frame.ncl has one). Inside a space, a port
# carries the name of the assembly its wall is built as, so 'wet below means that wall.

{
  label = "Plumbing",
  scope = "#dwelling",
  parts = { stack = { mount = 'cavity, width = "110 mm", depth = "110 mm", symbol = 'stack } }, # in the wall
  claims = {
    rules = [plate.wall "#wet" 'wall & { assembly = 'wet, length = "1.5 m" }],
    inside."#wet".rules = [plate.touches "#water" 'wet],
  },
  place = [plate.one "stack" "#wet" & { near = "#drain", wall.cavity = "140 mm" }],
  connect = [
    { label = "Drains", from = "stack", to = "#drain", via.cavity = "140 mm", limit = "4 m", draw = 'routed },
  ],
}
