import sys, os, numpy as np, bpy, struct, json
out = sys.argv[-1]; os.makedirs(out, exist_ok=True)
def dump(path, ob):
    m = ob.data; n = len(m.vertices)
    co = np.empty(n * 3, dtype=np.float32); m.vertices.foreach_get("co", co)
    ls = np.empty(len(m.polygons), dtype=np.uint32); m.polygons.foreach_get("loop_total", ls)
    lv = np.empty(len(m.loops), dtype=np.uint32); m.loops.foreach_get("vertex_index", lv)
    hb = json.dumps(dict(nv=int(n), nf=int(len(ls)), too_big=False)).encode()
    with open(path, "wb") as fh:
        fh.write(b"RCv1"); fh.write(struct.pack("<I", len(hb))); fh.write(hb)
        fh.write(co.tobytes()); fh.write(ls.astype("<u4").tobytes()); fh.write(lv.astype("<u4").tobytes())
bpy.ops.wm.read_factory_settings(use_empty=True)
bpy.ops.mesh.primitive_torus_add(major_radius=0.3, minor_radius=0.12, major_segments=48, minor_segments=24)
t = bpy.context.active_object
bpy.ops.mesh.primitive_ico_sphere_add(radius=0.2, subdivisions=3, location=(0.75, 0.1, 0.05))
s = bpy.context.active_object
s.scale = (1.0, 0.7, 1.3); 
bpy.ops.object.select_all(action="SELECT")
bpy.ops.object.join()
o = bpy.context.active_object
bpy.ops.object.transform_apply(location=True, rotation=True, scale=True)
dump(out + "/001_in.bin", o)
import infinigen.core.util.blender as butil
from infinigen.core.placement.detail import remesh_with_attrs
o2 = butil.deep_clone_obj(o)
m = o.modifiers.new("r", "REMESH"); m.mode = "VOXEL"; m.voxel_size = 0.02
bpy.ops.object.modifier_apply(modifier=m.name)
dump(out + "/001_out.bin", o)
dump(out + "/002_in.bin", o2)
remesh_with_attrs(o2, 0.02)
dump(out + "/002_out.bin", o2)
print("ok", len(o.data.vertices))
