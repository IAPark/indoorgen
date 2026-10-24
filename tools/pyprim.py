import bpy, sys, numpy as np
from infinigen.core import surface
from infinigen.core.nodes.node_wrangler import Nodes, NodeWrangler
bpy.ops.wm.read_factory_settings(use_empty=True)
bpy.ops.mesh.primitive_plane_add(size=2); obj = bpy.context.active_object
def fn(nw):
    c = nw.new_node("GeometryNodeMeshCylinder", input_kwargs={"Vertices": 6, "Side Segments": 2, "Radius": 1.0, "Depth": 2.0})
    nw.new_node(Nodes.GroupOutput, input_kwargs={"Geometry": c.outputs["Mesh"]})
surface.add_geomod(obj, fn, apply=True)
m = obj.data
np.set_printoptions(precision=3, suppress=True)
print("V", [tuple(np.round(v.co,3)) for v in m.vertices])
print("P", [list(p.vertices) for p in m.polygons])
