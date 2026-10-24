"""pycr.py pts.json mode out.json ; mode: eval (catmull-rom evaluated polyline) | res128 (resampled to 128)"""
import sys, json, numpy as np, bpy
pts = json.load(open(sys.argv[1])); mode = sys.argv[2]; out = sys.argv[3]
bpy.ops.wm.read_factory_settings(use_empty=True)
me = bpy.data.meshes.new("m"); n = len(pts)
me.from_pydata([tuple(p) for p in pts], [(i, i+1) for i in range(n-1)], [])
obj = bpy.data.objects.new("o", me); bpy.context.scene.collection.objects.link(obj); bpy.context.view_layer.objects.active = obj
from infinigen.core import surface
from infinigen.core.nodes.node_wrangler import Nodes
def fn(nw):
    inp = nw.new_node(Nodes.GroupInput, expose_input=[("NodeSocketGeometry", "Geometry", None)])
    c = nw.new_node(Nodes.MeshToCurve, input_kwargs={"Mesh": inp})
    c = nw.new_node(Nodes.SplineType, input_kwargs={"Curve": c}, attrs={"spline_type": "CATMULL_ROM"})
    if mode == "res128": c = nw.new_node(Nodes.ResampleCurve, input_kwargs={"Curve": c, "Count": 128})
    g = nw.new_node(Nodes.CurveToMesh, input_kwargs={"Curve": c})
    nw.new_node(Nodes.GroupOutput, input_kwargs={"Geometry": g})
surface.add_geomod(obj, fn, apply=True)
m = obj.data
json.dump({"verts": [[float(x) for x in p.co] for p in m.vertices]}, open(out, "w")); print("PYDUMP", len(m.vertices))
