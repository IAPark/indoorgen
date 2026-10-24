"""pygrp.py out.json 'python code defining fn(nw)': run node function on a plane, apply, dump mesh"""
import sys, json, numpy as np, bpy
from infinigen.core import surface
from infinigen.core.nodes.node_wrangler import Nodes, NodeWrangler
out = sys.argv[1]; code = sys.argv[2]
bpy.ops.wm.read_factory_settings(use_empty=True)
bpy.ops.mesh.primitive_plane_add(size=2); obj = bpy.context.active_object
ns = {"Nodes": Nodes, "np": np}
exec(code, ns)
surface.add_geomod(obj, ns["fn"], apply=True)
m = obj.data
json.dump({"verts": [[float(x) for x in p.co] for p in m.vertices], "polys": [[int(i) for i in p.vertices] for p in m.polygons], "edges": [[int(i) for i in e.vertices] for e in m.edges]}, open(out, "w")); print("PYDUMP", len(m.vertices), len(m.polygons), len(m.edges))
