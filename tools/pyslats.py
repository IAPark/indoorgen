"""pyslats.py seed out.json : vertical-bar `other` stages: wire (after removals), solid (decorate.solidify), bevel"""
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
from infinigen.assets.utils.decorate import read_co, remove_edges, read_edge_direction, remove_vertices, solidify
from infinigen.assets.utils.object import join_objects
from infinigen.core.util.blender import deep_clone_obj
from infinigen.core.util.math import FixedSeed, int_hash
from numpy.random import uniform
np.random.seed(seed)
self = C.ChairFactory(seed)
def snap(o):
    dg = bpy.context.evaluated_depsgraph_get(); e = o.evaluated_get(dg).to_mesh()
    return {"verts": [[float(x) for x in v.co] for v in e.vertices], "polys": [[int(i) for i in p.vertices] for p in e.polygons], "edges": [[int(i) for i in ed.vertices] for ed in e.edges]}
with FixedSeed(int_hash((seed, 0))):
    legs = self.make_legs(); backs = self.make_backs()
    self.make_leg_decors(legs)
    smoothness = uniform(0, 1); psf = uniform(0, 0.4)
    other = join_objects([deep_clone_obj(b) for b in backs])
    st = {"draws": [smoothness, psf]}
    with butil.ViewportMode(other, "EDIT"):
        bpy.ops.mesh.select_mode(type="EDGE")
        bpy.ops.mesh.select_all(action="SELECT")
        bpy.ops.mesh.bridge_edge_loops(number_cuts=self.back_vertical_cuts, interpolation="LINEAR", smoothness=smoothness, profile_shape_factor=psf)
        bpy.ops.mesh.select_all(action="INVERT")
        bpy.ops.mesh.delete()
        bpy.ops.mesh.select_all(action="SELECT")
        bpy.ops.mesh.delete(type="ONLY_FACE")
    st["wire0"] = snap(other)
    remove_edges(other, np.abs(read_edge_direction(other)[:, -1]) < 0.5)
    remove_vertices(other, lambda x, y, z: z < -self.thickness / 2)
    remove_vertices(other, lambda x, y, z: z > (self.back_profile[0][0] + self.back_profile[0][1]) * self.back_height / 2)
    st["wire"] = snap(other)
    st["zcut"] = float((self.back_profile[0][0] + self.back_profile[0][1]) * self.back_height / 2); st["cuts"] = int(self.back_vertical_cuts)
    if self.is_leg_round:
        solidify(other, 2, self.back_thickness)
        st["solid"] = snap(other)
        butil.modify_mesh(other, "BEVEL", width=self.bevel_width, segments=8)
        st["bevel"] = snap(other)
json.dump(st, open(out, "w")); print("PYSLATS", {k: len(v["verts"]) for k, v in st.items() if isinstance(v, dict)})
