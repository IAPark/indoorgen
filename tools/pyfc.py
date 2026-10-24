"""pyfc.py out.json pts... : evaluate blender FloatCurve (points x:y,...) at 2001 xs in [-0.1,1.1]"""
import sys, json, bpy, numpy as np
out = sys.argv[sys.argv.index("--")+1] if "--" in sys.argv else sys.argv[1]
res = {}
cases = json.loads(sys.argv[-1])
tree = bpy.data.node_groups.new("t", "GeometryNodeTree")
for ci, pts in enumerate(cases):
    n = tree.nodes.new("ShaderNodeFloatCurve")
    c = n.mapping.curves[0]
    for i, p in enumerate(pts):
        if i < 2: c.points[i].location = p
        else: c.points.new(*p)
    n.mapping.update()
    xs = np.linspace(-0.1, 1.1, 2001)
    res[ci] = [n.mapping.evaluate(c, float(x)) for x in xs]
json.dump(res, open(out, "w"))
