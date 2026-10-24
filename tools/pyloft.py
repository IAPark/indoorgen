import sys, json, numpy as np, bpy, importlib, types
src = open("/Users/lume/work/infinigen-ref/src/infinigen/assets/objects/seating/chairs/seats/curvy_seats.py").read()
assert 'input_kwargs={"Geometry": warparoundcurvealt},\n        attrs={"is_active_output": True}' in src
src = src.replace('input_kwargs={"Geometry": warparoundcurvealt},\n        attrs={"is_active_output": True}', 'input_kwargs={"Geometry": lofting_001.outputs["Geometry"]},\n        attrs={"is_active_output": True}')
import os
if os.environ.get("STAGE") == "prof":
    src = src.replace('input_kwargs={"Geometry": lofting_001.outputs["Geometry"]},\n        attrs={"is_active_output": True}', 'input_kwargs={"Geometry": nw.new_node(Nodes.CurveToMesh, input_kwargs={"Curve": join_geometry_1})},\n        attrs={"is_active_output": True}')
mod = types.ModuleType("tmp_curvy"); exec(compile(src, "tmp_curvy", "exec"), mod.__dict__)
from infinigen.core import surface
from infinigen.core.nodes.node_wrangler import Nodes
a = sys.argv[sys.argv.index("--")+1:] if "--" in sys.argv else sys.argv[1:]
v = [float(x) for x in a[:13]]; out = a[13]
names = ["Width","Thickness","Front Relative Width","Front Bent","Seat Bent","Mid Relative Width","Mid Bent","Back Relative Width","Back Bent","Top Relative Width","Top Bent","Seat Height","Mid Pos"]
bpy.ops.wm.read_factory_settings(use_empty=True)
bpy.ops.mesh.primitive_plane_add(size=2); obj = bpy.context.active_object
def fn(nw, **kw):
    g = nw.new_node(mod.generate_curvy_seats().name, input_kwargs=dict(zip(names, v)))
    nw.new_node(Nodes.GroupOutput, input_kwargs={"Geometry": g})
surface.add_geomod(obj, fn, apply=True)
m = obj.data
json.dump({"verts": [[float(x) for x in p.co] for p in m.vertices], "polys": [[int(i) for i in p.vertices] for p in m.polygons]}, open(out, "w")); print("PYDUMP", len(m.vertices))
