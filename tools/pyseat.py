import sys, json, numpy as np, bpy
from infinigen.assets.objects.seating.chairs.seats.curvy_seats import generate_curvy_seats
from infinigen.core import surface
a = sys.argv[sys.argv.index("--")+1:] if "--" in sys.argv else sys.argv[1:]
v = [float(x) for x in a[:13]]; out = a[13]
names = ["Width","Thickness","Front Relative Width","Front Bent","Seat Bent","Mid Relative Width","Mid Bent","Back Relative Width","Back Bent","Top Relative Width","Top Bent","Seat Height","Mid Pos"]
bpy.ops.wm.read_factory_settings(use_empty=True)
bpy.ops.mesh.primitive_plane_add(size=2); obj = bpy.context.active_object
from infinigen.core.nodes.node_wrangler import Nodes
def fn(nw, **kw):
    g = nw.new_node(generate_curvy_seats().name, input_kwargs=dict(zip(names, v)))
    nw.new_node(Nodes.GroupOutput, input_kwargs={"Geometry": g})
surface.add_geomod(obj, fn, apply=True)
m = obj.data
json.dump({"verts": [[float(x) for x in p.co] for p in m.vertices], "polys": [[int(i) for i in p.vertices] for p in m.polygons]}, open(out, "w")); print("PYDUMP", len(m.vertices))
