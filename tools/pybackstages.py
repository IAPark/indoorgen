"""pybackstages.py seed out.json : ChairFactory(seed).make_back_decors stages (bridge, solid, bevel) + params"""
import sys, json, numpy as np, bpy
seed = int(sys.argv[1]); out = sys.argv[2]
bpy.ops.wm.read_factory_settings(use_empty=True)
import infinigen.core.util.random as R
class _D:
    def __init__(self, *a, **k): pass
    def __call__(self, *a, **k): return bpy.data.materials.new("stub")
    def __getattr__(self, n):
        if n.startswith("__"): raise AttributeError(n)
        return _D()
R.weighted_sample = lambda *a, **k: _D
from infinigen.assets.objects.seating.chairs import chair as C
from infinigen.core.util import blender as butil
from infinigen.assets.utils.decorate import read_co, write_co, read_edge_center, select_edges
from infinigen.assets.utils.object import join_objects
from infinigen.core.util.blender import deep_clone_obj
from numpy.random import uniform
np.random.seed(seed)
self = C.ChairFactory(seed)
def snap(o):
    dg = bpy.context.evaluated_depsgraph_get(); e = o.evaluated_get(dg).to_mesh()
    return {"verts": [[float(x) for x in v.co] for v in e.vertices], "polys": [[int(i) for i in p.vertices] for p in e.polygons], "edges": [[int(i) for i in ed.vertices] for ed in e.edges]}
legs = self.make_legs(); backs = self.make_backs()
st = {}
from infinigen.core.util.math import FixedSeed
with FixedSeed(int_hash := __import__("infinigen.core.util.math", fromlist=["int_hash"]).int_hash((seed, 0))):
    # replicate create_asset draws before make_back_decors: leg decors
    decors = self.make_leg_decors(legs)
    obj = join_objects([deep_clone_obj(b) for b in backs])
    x, y, z = read_co(obj).T
    x += np.where(x > 0, self.back_thickness / 2, -self.back_thickness / 2)
    write_co(obj, np.stack([x, y, z], -1))
    smoothness = uniform(0, 1)
    profile_shape_factor = uniform(0, 0.4)
    st["draws"] = [smoothness, profile_shape_factor]
    import bpy as _b
    with butil.ViewportMode(obj, "EDIT"):
        _b.ops.mesh.select_mode(type="EDGE")
        center = read_edge_center(obj)
        for z_min, z_max in self.back_profile:
            select_edges(obj, (z_min * self.back_height <= center[:, -1]) & (center[:, -1] <= z_max * self.back_height))
            _b.ops.mesh.bridge_edge_loops(number_cuts=64, interpolation="LINEAR", smoothness=smoothness, profile_shape_factor=profile_shape_factor)
        _b.ops.mesh.select_loose()
        _b.ops.mesh.delete()
    st["bridge"] = snap(obj)
    butil.modify_mesh(obj, "SOLIDIFY", thickness=np.minimum(self.thickness, self.back_thickness), offset=0)
    st["solid"] = snap(obj)
    butil.modify_mesh(obj, "BEVEL", width=self.bevel_width, segments=8)
    st["bevel"] = snap(obj)
st["profile"] = [list(map(float, p)) for p in self.back_profile]; st["back_type"] = str(self.back_type); st["bh"] = self.back_height
json.dump(st, open(out, "w")); print("PYBACK", {k: len(v["verts"]) for k, v in st.items() if isinstance(v, dict)}, st["back_type"], st["profile"])
