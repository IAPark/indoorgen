"""cloth_jobs.py : re-run Rust-exported cloth jobs (sidecar `cloth`, see Asset::cloth_json) with Blender's own cloth solver, using exactly
python's `infinigen.assets.scatters.clothes.cloth_sim` / `subsurf`. Used by render_rust.py and by tools/clothcheck_bed.py.

run_jobs(jobs, world_mesh, set_mesh):
  world_mesh(name) -> (verts Nx3 world Z-up, faces)       current state of a node (earlier jobs' results included)
  set_mesh(name, verts, faces)                            store a result (verts world Z-up)
Job kinds: "cloth" (pre mesh + colliders + python kwargs, optional z_from shift) and "follow" (re-drop an object onto the new surfaces)."""
import numpy as np, bpy


def _make(name, verts, faces):
    me = bpy.data.meshes.new(name)
    me.from_pydata([tuple(v) for v in verts], [], [list(map(int, f)) for f in faces])
    me.update()
    o = bpy.data.objects.new(name, me)
    bpy.context.scene.collection.objects.link(o)
    return o


def _read(o):
    dg = bpy.context.evaluated_depsgraph_get()
    e = o.evaluated_get(dg)
    me = e.to_mesh()
    v = np.array([tuple(x.co) for x in me.vertices]).reshape(-1, 3)
    f = [list(p.vertices) for p in me.polygons]
    e.to_mesh_clear()
    return v, f


def _drop(bases_mesh, x, y):
    """highest surface z at (x, y) over the given (verts, faces) list, None when missed"""
    from mathutils import Vector
    from mathutils.bvhtree import BVHTree
    best = None
    for v, f in bases_mesh:
        t = BVHTree.FromPolygons([tuple(p) for p in v], [tuple(p) for p in f])
        hit = t.ray_cast(Vector((x, y, 10.0)), Vector((0, 0, -1)))
        if hit[0] is not None:
            best = hit[0].z if best is None else max(best, hit[0].z)
    return best


def _build_blanket(b, z, pose=None):
    """python's own Blanket / Comforter / BoxComforter create_asset (+ fold) with explicit parameters, placed like BedFactory.make_sheet"""
    import math
    from infinigen.assets.objects.clothes import blanket as BL
    from infinigen.core import surface as S
    from infinigen.core.util import blender as butil
    from infinigen.assets.utils.decorate import read_co, write_co
    S.assign_material = lambda *a, **k: None
    BL.surface.assign_material = lambda *a, **k: None
    cls = {"blanket": BL.BlanketFactory, "comforter": BL.ComforterFactory, "box": BL.BoxComforterFactory}[b["type"]]
    f = cls.__new__(cls)
    f.width, f.size, f.thickness = b["width"], b["size"], b["thickness"]
    if b["type"] == "box":
        f.margin = b["margin"]
    f.surface = None
    obj = f.create_asset()
    if b.get("fold"):
        theta, ym = b["fold"]["theta"], b["fold"]["y_margin"]
        # BlanketFactory.fold with the drawn values
        import bpy as _b
        obj.rotation_euler[-1] = theta
        obj.location[1] -= ym
        butil.apply_transform(obj, True)
        with butil.ViewportMode(obj, "EDIT"):
            _b.ops.mesh.select_all(action="SELECT")
            _b.ops.mesh.bisect(plane_co=(0, 0, 0), plane_no=(0, 1, 0))
        x, y, zz = read_co(obj).T
        write_co(obj, np.stack([x, np.where(y > 0, -y, y), np.where(y > 0, 0.05 - zz, zz)], -1))
        obj.location[1] += ym
        butil.apply_transform(obj, True)
        obj.rotation_euler[-1] = -theta
        butil.apply_transform(obj)
    obj.location = b["x"], 0, z - (pose[2][3] if pose else 0.0)
    obj.rotation_euler[-1] = math.pi / 2
    butil.apply_transform(obj, True)
    if pose:
        import mathutils
        obj.matrix_world = mathutils.Matrix([list(r) for r in pose])
        butil.apply_transform(obj, True)
    return obj


def run_jobs(jobs, world_mesh, set_mesh, verbose=True):
    from infinigen.assets.scatters.clothes import cloth_sim
    from infinigen.assets.utils.decorate import subsurf
    from infinigen.core.util import blender as butil
    for j in jobs:
        if j["kind"] == "cloth":
            if j.get("build"):
                pv, _ = world_mesh(j["z_from"]["part"])
                obj = _build_blanket(j["build"], float(pv[:, 2].max()), j.get("pose"))
                verts = np.array([tuple(v.co) for v in obj.data.vertices])
            else:
                verts = np.array(j["verts"], dtype=float).reshape(-1, 3)
                if j.get("z_from"):
                    pv, _ = world_mesh(j["z_from"]["part"])
                    verts[:, 2] += pv[:, 2].max() - j["z_from"]["z_used"]
                obj = _make("sim", verts, j["faces"])
            if j.get("pin"):
                vg = obj.vertex_groups.new(name="pin")
                vg.add([int(i) for i in j["pin"]], 1, "REPLACE")
            cols = [_make("col", *world_mesh(n)) for n in j["collide"]]
            butil.select_none()
            cloth_sim(obj, cols, int(j["end_frame"]), **j["kwargs"])
            if j.get("subsurf"):
                subsurf(obj, int(j["subsurf"]))
            v, f = _read(obj)
            if verbose:
                print("CLOTHJOB", j["target"], len(verts), "->", len(v), "z", v[:, 2].min(), v[:, 2].max())
            set_mesh(j["target"], v, f)
            for o in [obj] + cols:
                bpy.data.objects.remove(o, do_unlink=True)
        elif j["kind"] == "follow":
            bases = [world_mesh(n) for n in j["bases"]]
            z = _drop(bases, j["xy"][0], j["xy"][1])
            if z is None:
                continue
            v, f = world_mesh(j["part"])
            set_mesh(j["part"], v + np.array([0, 0, z - j["z_used"]]), f)
