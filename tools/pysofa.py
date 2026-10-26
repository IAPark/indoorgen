"""pysofa.py seed varname out.json : sofa geometry node group output replaced by an intermediate node variable (geometry of that node)"""
import sys, json, types, numpy as np, bpy
seed = int(sys.argv[1]); var = sys.argv[2]; out = sys.argv[3]
src = open(__import__("os").path.join(__import__("os").environ["INDOORGEN_REF"], "src")+"/infinigen/assets/objects/seating/sofa.py").read()
i = src.index("    group_output = nw.new_node(\n        Nodes.GroupOutput,\n        input_kwargs={\"Geometry\": switch_1")
j = src.index("\n    )\n", i) + 7
new = '    group_output = nw.new_node(Nodes.GroupOutput, input_kwargs={"Geometry": %s}, attrs={"is_active_output": True})\n' % var
src = src[:i] + new + src[j:]
mod = types.ModuleType("tmp_sofa"); exec(compile(src, "tmp_sofa", "exec"), mod.__dict__)
import infinigen.core.util.random as R
class _D:
    def __init__(self, *a, **k): pass
    def __call__(self, *a, **k): return bpy.data.materials.new("stub")
    def __getattr__(self, n):
        if n.startswith("__"): raise AttributeError(n)
        return _D()
R.weighted_sample = lambda *a, **k: _D
import infinigen.core.surface as S
S.assign_material = lambda *a, **k: None
bpy.ops.wm.read_factory_settings(use_empty=True)
np.random.seed(seed)
f = mod.SofaFactory(seed)
o = f.create_placeholder(); bpy.context.view_layer.update()
m = o.data
json.dump({"verts": [[float(x) for x in p.co] for p in m.vertices], "polys": [[int(i) for i in p.vertices] for p in m.polygons]}, open(out, "w")); print("PYDUMP", len(m.vertices))
