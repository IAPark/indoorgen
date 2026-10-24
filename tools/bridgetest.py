"""bridgetest.py in.json out.json : in = {v, p, loops:[[...],[...]], cuts, shape}: select the edges of the two loops (edge mode, vertex order of the
loops irrelevant) and run bpy.ops.mesh.bridge_edge_loops(number_cuts, profile_shape_factor); write {v,p}."""
import sys, json, bpy, bmesh
d = json.load(open(sys.argv[1]))
bpy.ops.wm.read_factory_settings(use_empty=True)
me = bpy.data.meshes.new("m")
loops = d["loops"]
edges = []
for lp in loops:
    for i in range(len(lp)):
        edges.append((lp[i], lp[(i + 1) % len(lp)]))
me.from_pydata([tuple(v) for v in d["v"]], [], [tuple(p) for p in d["p"]])
me.update()
ob = bpy.data.objects.new("o", me)
bpy.context.scene.collection.objects.link(ob)
bpy.context.view_layer.objects.active = ob
ob.select_set(True)
bpy.ops.object.mode_set(mode="EDIT")
bpy.ops.mesh.select_mode(type="EDGE")
bpy.ops.mesh.select_all(action="DESELECT")
bm = bmesh.from_edit_mesh(me)
bm.edges.ensure_lookup_table()
want = {tuple(sorted(e)) for e in edges}
for e in bm.edges:
    a, b = e.verts[0].index, e.verts[1].index
    if tuple(sorted((a, b))) in want:
        e.select_set(True)
bm.select_flush_mode()
bmesh.update_edit_mesh(me)
bpy.ops.mesh.bridge_edge_loops(number_cuts=d.get("cuts", 0), profile_shape_factor=d.get("shape", 0.0))
bpy.ops.object.mode_set(mode="OBJECT")
json.dump({"v": [list(v.co) for v in me.vertices], "p": [list(p.vertices) for p in me.polygons]}, open(sys.argv[2], "w"))
print(len(me.vertices), len(me.polygons))
