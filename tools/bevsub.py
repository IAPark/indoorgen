"""bevsub.py in.json out.json [width segments]: apply Blender BEVEL modifier (defaults: angle 30deg, clamp) to a {"v","p"} mesh. python side of bevsub (rust)."""
import sys, json, bpy
d = json.load(open(sys.argv[1]))
w = float(sys.argv[3]) if len(sys.argv) > 3 else 0.1
seg = int(sys.argv[4]) if len(sys.argv) > 4 else 2
bpy.ops.wm.read_factory_settings(use_empty=True)
me = bpy.data.meshes.new("m")
me.from_pydata([tuple(v) for v in d["v"]], [], [tuple(p) for p in d["p"]])
me.update()
ob = bpy.data.objects.new("o", me)
bpy.context.scene.collection.objects.link(ob)
bpy.context.view_layer.objects.active = ob
m = ob.modifiers.new("b", "BEVEL"); m.width = w; m.segments = seg
bpy.ops.object.modifier_apply(modifier="b")
me = ob.data
json.dump({"v": [list(v.co) for v in me.vertices], "p": [list(p.vertices) for p in me.polygons]}, open(sys.argv[2], "w"))
print(len(me.vertices), len(me.polygons))
