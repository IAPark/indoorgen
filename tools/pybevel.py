"""pybevel.py kind width segs out.json [angle_deg] : build test mesh `kind`, dump {"in":..., "out":...} (BEVEL modifier applied, limit ANGLE)"""
import sys, json, numpy as np, bpy, bmesh, math
kind, width, segs, out = sys.argv[1], float(sys.argv[2]), int(sys.argv[3]), sys.argv[4]
ang = float(sys.argv[5]) if len(sys.argv) > 5 else 30.0
bpy.ops.wm.read_factory_settings(use_empty=True)
def mk(V, F):
    me = bpy.data.meshes.new("m"); me.from_pydata(V, [], F); me.update()
    ob = bpy.data.objects.new("o", me); bpy.context.scene.collection.objects.link(ob); bpy.context.view_layer.objects.active = ob; ob.select_set(True)
    return ob
def box(dx, dy, dz, shear=(0, 0)):
    V = []
    for z in (0, 1):
        for y in (0, 1):
            for x in (0, 1):
                V.append([x * dx + z * shear[0], y * dy + z * shear[1], z * dz])
    F = [[0, 2, 3, 1], [4, 5, 7, 6], [0, 1, 5, 4], [2, 6, 7, 3], [0, 4, 6, 2], [1, 3, 7, 5]]
    return V, F
if kind == "cube": V, F = box(1, 1, 1)
elif kind == "box": V, F = box(0.4, 0.2, 1.0)
elif kind == "shear": V, F = box(0.4, 0.3, 1.0, (0.25, 0.1))
elif kind == "thin": V, F = box(1.0, 0.8, 0.02)
elif kind == "tube":
    # swept square tube along a bent path
    path = [[0, 0, 0], [0.05, 0, 0.3], [0.12, 0.0, 0.6], [0.2, 0.0, 0.9]]
    V = []; F = []
    h = 0.04
    for p in path:
        for dx, dy in ((-h, -h), (h, -h), (h, h), (-h, h)):
            V.append([p[0] + dx, p[1] + dy, p[2]])
    n = len(path)
    for i in range(n - 1):
        for k in range(4):
            a, b = i * 4 + k, i * 4 + (k + 1) % 4
            F.append([a, b, b + 4, a + 4])
    F.append([3, 2, 1, 0]); F.append([(n - 1) * 4 + k for k in range(4)])
elif kind.startswith("file:"):
    d = json.load(open(kind[5:])); V = d["verts"]; F = d["polys"]
elif kind == "ring":
    # rounded rim: octagon prism
    N = 24; V = []; F = []
    for z in (0, 0.05):
        for i in range(N):
            a = 2 * math.pi * i / N; V.append([0.3 * math.cos(a), 0.2 * math.sin(a), z])
    for i in range(N):
        j = (i + 1) % N; F.append([i, j, N + j, N + i])
    F.append(list(range(N - 1, -1, -1))); F.append([N + i for i in range(N)])
ob = mk(V, F)
# make normals outward
bm = bmesh.new(); bm.from_mesh(ob.data); (kind.startswith("file:") or bmesh.ops.recalc_face_normals(bm, faces=bm.faces)); bm.to_mesh(ob.data); bm.free()
inm = {"verts": [list(v.co) for v in ob.data.vertices], "polys": [list(p.vertices) for p in ob.data.polygons]}
md = ob.modifiers.new("b", "BEVEL"); md.width = width; md.segments = segs; md.angle_limit = math.radians(ang); md.limit_method = "ANGLE"
bpy.context.view_layer.update()
e = ob.evaluated_get(bpy.context.evaluated_depsgraph_get()).to_mesh()
res = {"verts": [list(v.co) for v in e.vertices], "polys": [list(p.vertices) for p in e.polygons]}
json.dump({"in": inm, "py": res}, open(out, "w")); print("PYBEVEL", len(res["verts"]), len(res["polys"]))
