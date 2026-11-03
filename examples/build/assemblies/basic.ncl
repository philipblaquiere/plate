# Two wall types: enough to draw most plans with real walls. Outside walls take their thickness
# from inside the plate's outline; every wall between two spaces is a partition.
#
# Use them with `assemblies = import "build/assemblies/basic.ncl"`. wood-frame.ncl goes further.

{
  exterior = { label = "Exterior wall", on = ['facade, 'blank, 'outdoor], thickness = "250 mm" },
  partition = { label = "Partition", on = ['partition, 'party, 'wet, 'link], thickness = "100 mm" },
}
