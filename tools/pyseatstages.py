"""pyseatstages.py seed out.json : ChairFactory(seed).make_seat stages: curve, mirror, grid, solid, subsurf, bevel"""
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
from infinigen.assets.objects.seating.chairs.chair import ChairFactory, bezier_curve, subsurf
from infinigen.core.util import blender as butil
np.random.seed(seed)
self = ChairFactory(seed)
def snap(o):
    dg = bpy.context.evaluated_depsgraph_get(); e = o.evaluated_get(dg).to_mesh()
    return {"verts": [[float(x) for x in v.co] for v in e.vertices], "polys": [[int(i) for i in p.vertices] for p in e.polygons], "edges": [[int(i) for i in ed.vertices] for ed in e.edges]}
st = {}
x_anchors = np.array([0, 0.1, 1, self.seat_mid_x, self.seat_back, 0]) * self.width / 2
y_anchors = np.array([-self.seat_front, -self.seat_front, -1, -self.seat_mid, 0, 0]) * self.size
z_anchors = np.array([0, 0, 0, self.seat_mid_z, 0, 0]) * self.thickness
vector_locations = [4] if self.is_seat_round else [2, 4]
obj = bezier_curve((x_anchors, y_anchors, z_anchors), vector_locations)
st["curve"] = snap(obj)
butil.modify_mesh(obj, "MIRROR", apply=True)
st["mirror"] = snap(obj)
with butil.ViewportMode(obj, "EDIT"):
    bpy.ops.mesh.select_all(action="SELECT")
    bpy.ops.mesh.fill_grid(use_interp_simple=True)
st["grid"] = snap(obj)
butil.modify_mesh(obj, "SOLIDIFY", thickness=self.thickness, offset=0, apply=True)
st["solid"] = snap(obj)
subsurf(obj, 1, not self.is_seat_subsurf)
bpy.context.view_layer.update()
st["subsurf"] = snap(obj)
butil.modify_mesh(obj, "BEVEL", width=self.bevel_width, segments=8, apply=True)
st["bevel"] = snap(obj)
st["params"] = {k: (float(v) if isinstance(v, (float, np.floating)) else (bool(v) if isinstance(v, (bool, np.bool_)) else None)) for k, v in vars(self).items() if k in ("width","size","thickness","bevel_width","seat_back","seat_mid","seat_mid_x","seat_mid_z","seat_front","is_seat_round","is_seat_subsurf")}
json.dump(st, open(out, "w")); print("PYSEAT", {k: len(v["verts"]) for k, v in st.items() if k != "params"}, st["params"]["is_seat_round"], st["params"]["is_seat_subsurf"])
