import sys, json, numpy as np, bpy
from infinigen.assets.objects.tables.table_top import geometry_generate_table_top_wrapper
from infinigen.core import surface
a = sys.argv[sys.argv.index("--")+1:] if "--" in sys.argv else sys.argv[1:]
n, w, asp, fil, th, vf = int(a[0]), *[float(x) for x in a[1:6]]; out = a[6]
bpy.ops.wm.read_factory_settings(use_empty=True)
bpy.ops.mesh.primitive_plane_add(size=2)
obj = bpy.context.active_object
surface.add_geomod(obj, geometry_generate_table_top_wrapper, apply=True, input_kwargs={"Profile N-gon": n, "Profile Width": w, "Profile Aspect Ratio": asp, "Profile Fillet Ratio": fil, "Thickness": th, "Vertical Fillet Ratio": vf})
m = obj.data
V = [[float(x) for x in v.co] for v in m.vertices]; P = [[int(i) for i in p.vertices] for p in m.polygons]
json.dump({"verts": V, "polys": P}, open(out, "w")); print("PYDUMP", len(V), len(P))
