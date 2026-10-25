"""stagedump.py module.Class SEED OUTDIR [--inst N] [--filter SUBSTR] [--max-verts 300000]   (infinigen venv python, bpy 4.2)

Builds ONE python asset (same conventions as tools/refcache.py: np.random.seed(SEED), then Class(SEED).spawn_asset(INST), materials
stubbed) and dumps the mesh after EVERY geometry-changing call made while it is built:
  bpy.ops.mesh.*  (edit-mode ops, read from the edit bmesh),  bpy.ops.object.modifier_apply / join / convert / voxel_remesh
  (every modifier incl. geometry-node group modifiers), and labels them with the butil / surface helper that issued them
  (butil.modify_mesh, apply_modifiers, join_objects, apply_transform, surface.add_geomod).
Selection ops (select_*, hide, ...) are not stages.  Meshes over --max-verts are skipped (listed in the index as skipped).
Files in OUTDIR (small json, local object coordinates, matrix_world included):
  NNN_<stage>.json       mesh AFTER the stage: {n, name, op, via, kwargs, object, matrix_world, verts, polys, edges, in_edges, params,
                         opcheck (op name of tools/opcheck.py, or null)}   -- also usable as the `--blender` file of src/bin/opcheck.rs
  NNN_<stage>.case.json  mesh BEFORE the stage + selection (sel_verts/sel_edges/sel_faces as indices) = an opcheck.py case
  final.json             the finished asset (all mesh children merged, world space)       index.txt  one line per stage
Re-run a single stage in Blender / Rust (when its `opcheck` field is set; params are the exact operator / modifier settings):
  python tools/opcheck.py bevel --in OUT/003_mesh_bevel.case.json --params '{}' --out /tmp/r.json   # Blender again
  target/fast/opcheck bevel --in OUT/003_mesh_bevel.case.json --blender OUT/003_mesh_bevel.json      # Rust vs the recorded Blender stage
Compare a Rust-side stage dump (src/assets/stagecmp.rs / stagefile.rs) with `tools/stagediff.py`.  Supersedes the ad-hoc
pyseatstages/pybackstages/toistages/bathstages; tools/pytrace.py (binary RCv1 stage files for src/bin/stagetest.rs) still works.
"""
import sys, os, json, importlib, time
import numpy as np

class _D:
    def __init__(self, *a, **k): pass
    def __call__(self, *a, **k):
        import bpy
        return bpy.data.materials.new("stub")
    def __getattr__(self, n):
        if n.startswith("__"): raise AttributeError(n)
        return _D()

SKIP_PREFIX = ("select", "hide", "reveal", "mark_", "dissolve_mode", "edit_mode_toggle", "customdata", "attribute", "shade", "set_normals",
               "flip_normals_none", "duplicate_move_none")
OBJ_OPS = {"modifier_apply", "join", "convert", "voxel_remesh", "modifier_remove_none"}
MOD_OPCHECK = {"BEVEL": "bevel_mod", "SOLIDIFY": "solidify", "SUBSURF": "subsurf", "TRIANGULATE": "triangulate", "DISPLACE": "displace",
               "REMESH": "remesh", "BOOLEAN": "boolean"}
OP_OPCHECK = {"bevel": "bevel", "fill_grid": "fill_grid", "subdivide": "subdivide", "remove_doubles": "remove_doubles",
              "convex_hull": "convex_hull", "quads_convert_to_tris": "triangulate_op", "beautify_fill": "beautify_fill",
              "bridge_edge_loops": "bridge"}

def main():
    a = [x for x in sys.argv[sys.argv.index("--") + 1:]] if "--" in sys.argv else sys.argv[1:]
    def opt(k, d=None):
        return a[a.index(k) + 1] if k in a else d
    pos = [x for i, x in enumerate(a) if not x.startswith("--") and (i == 0 or not a[i - 1].startswith("--"))]
    modcls, seed, outdir = pos[0], int(pos[1]), pos[2]
    inst = int(opt("--inst", 0)); filt = opt("--filter"); maxv = int(opt("--max-verts", 300000))
    os.makedirs(outdir, exist_ok=True)
    for f in os.listdir(outdir):
        if f.endswith(".json") or f == "index.txt":
            os.remove(os.path.join(outdir, f))
    import bpy, bmesh
    import bpy.ops as bops
    import infinigen.core.util.random as R
    import infinigen.core.surface as S
    import infinigen.core.util.blender as butil
    _orig_ws = R.weighted_sample
    def _ws(*a, **k):
        # same as tools/refcache.py: consume the real weighted_sample's draws (Rust/refcache convention), return a stub class
        try:
            _orig_ws(*a, **k)
        except Exception:
            pass
        return _D
    R.weighted_sample = _ws
    S.assign_material = lambda *a, **k: None

    VIA = []
    state = {"n": 0, "depth": 0}
    index = []

    def mesh_dict(ob):
        """(verts, polys, edges, sel) of the object's mesh as it is NOW (edit mesh when in edit mode)"""
        if ob.mode == "EDIT":
            bm = bmesh.from_edit_mesh(ob.data)
            for t in (bm.verts, bm.edges, bm.faces):
                t.ensure_lookup_table()
            d = {"verts": [list(v.co) for v in bm.verts], "polys": [[v.index for v in f.verts] for f in bm.faces],
                 "edges": [[v.index for v in e.verts] for e in bm.edges]}
            sel = {"sel_verts": [v.index for v in bm.verts if v.select], "sel_edges": [e.index for e in bm.edges if e.select],
                   "sel_faces": [f.index for f in bm.faces if f.select]}
            return d, sel
        me = ob.data
        return {"verts": [list(v.co) for v in me.vertices], "polys": [list(p.vertices) for p in me.polygons],
                "edges": [list(e.vertices) for e in me.edges]}, {}

    def nverts(ob):
        if ob.mode == "EDIT":
            return len(bmesh.from_edit_mesh(ob.data).verts)
        return len(ob.data.vertices)

    def jsonable(v):
        try:
            if hasattr(v, "__len__") and not isinstance(v, str):
                return [jsonable(x) for x in v]
            json.dumps(v); return v
        except Exception:
            return repr(v)

    def rna_props(idb, skip=()):
        out = {}
        for p in idb.bl_rna.properties:
            if p.identifier in ("rna_type", "name", "type") or p.identifier in skip or p.is_readonly:
                continue
            if p.type in ("BOOLEAN", "INT", "FLOAT", "ENUM", "STRING"):
                try:
                    out[p.identifier] = jsonable(getattr(idb, p.identifier))
                except Exception:
                    pass
        return out

    def modifier_info(ob, kw):
        name = kw.get("modifier") or (ob.modifiers.active.name if ob.modifiers.active else None)
        m = ob.modifiers.get(name) if name else None
        if m is None:
            return None, {}, {}
        params = rna_props(m)
        extra = {}
        tex = getattr(m, "texture", None)
        if tex is not None:
            t = rna_props(tex); t["type"] = tex.type
            params["texture"] = t
        if m.type == "BOOLEAN" and getattr(m, "object", None) is not None:
            o2 = m.object; mw = ob.matrix_world.inverted() @ o2.matrix_world
            extra["other"] = {"verts": [list(mw @ v.co) for v in o2.data.vertices], "polys": [list(p.vertices) for p in o2.data.polygons]}
        if m.type == "NODES":
            params["node_group"] = m.node_group.name if m.node_group else None
        return m.type, params, extra

    def loops_of(edges, sel_edges):
        """closed vertex loops formed by the selected edges (for bridge_edge_loops cases)"""
        adj = {}
        for ei in sel_edges:
            u, v = edges[ei]
            adj.setdefault(u, []).append(v); adj.setdefault(v, []).append(u)
        seen, loops = set(), []
        for s in adj:
            if s in seen or any(len(adj[x]) != 2 for x in [s]):
                continue
            lp, prev, cur = [s], None, s
            while True:
                seen.add(cur)
                nxt = [x for x in adj[cur] if x != prev] if prev is not None else adj[cur][:1]
                if not nxt or nxt[0] == s or nxt[0] in seen:
                    break
                prev, cur = cur, nxt[0]; lp.append(cur)
            loops.append(lp)
        return loops

    def record(name, ob, before, sel, kw, via):
        i = state["n"]; state["n"] += 1
        tag = "%03d_%s" % (i, name.replace(".", "_"))
        nvb = len(before["verts"]) if before else -1
        after, _ = mesh_dict(ob) if ob is not None and ob.type == "MESH" else (None, None)
        nva = len(after["verts"]) if after else -1
        line = "%03d %-34s via=%-34s verts %d -> %d" % (i, name, "/".join(via) or "-", nvb, nva)
        if after is None or max(nva, nvb) > maxv:
            index.append(line + "  SKIPPED(size/type)"); print(line, "SKIPPED"); return
        modtype, params, extra = (None, {}, {})
        opc = None
        short = name.split(".", 1)[1]
        if name == "object.modifier_apply":
            modtype, params, extra = MODIFIER_PRE.pop(i, (None, {}, {}))
            opc = MOD_OPCHECK.get(modtype)
            tag += "_" + str(modtype)
        else:
            params = {k: jsonable(v) for k, v in kw.items()}
            opc = OP_OPCHECK.get(short) if name.startswith("mesh.") else None
        case = dict(before, name=tag, params=params, **sel)
        case.update(extra)
        if opc == "bridge":
            case["loops"] = loops_of(before["edges"], sel.get("sel_edges", []))
        res = dict(after, n=i, name=tag, op=opc, opcheck=opc, via=list(via), kwargs={k: jsonable(v) for k, v in kw.items()},
                   object=ob.name, matrix_world=[list(r) for r in ob.matrix_world], params=params, in_edges=before["edges"], call=[name])
        json.dump(res, open(os.path.join(outdir, tag + ".json"), "w"), separators=(",", ":"))
        json.dump(case, open(os.path.join(outdir, tag + ".case.json"), "w"), separators=(",", ":"))
        index.append(line + ("  opcheck=" + opc if opc else ""))
        print(line, ("opcheck=" + opc) if opc else "")

    MODIFIER_PRE = {}
    orig_call = bops._BPyOpsSubModOp.__call__

    def tracked(mod, fn):
        if mod == "mesh":
            return not fn.startswith(SKIP_PREFIX)
        return mod == "object" and fn in OBJ_OPS

    def call(self, *args, **kw):
        mod, fn = self._module, self._func
        ob = bpy.context.active_object
        if state["depth"] or not tracked(mod, fn) or ob is None or ob.type != "MESH":
            return orig_call(self, *args, **kw)
        name = "%s.%s" % (mod, fn)
        if filt and filt not in name and not any(filt in v for v in VIA):
            return orig_call(self, *args, **kw)
        try:
            if nverts(ob) > maxv:
                return orig_call(self, *args, **kw)
            before, sel = mesh_dict(ob)
        except Exception:
            return orig_call(self, *args, **kw)
        idx = state["n"]
        if name == "object.modifier_apply":
            MODIFIER_PRE[idx] = modifier_info(ob, kw)
        state["depth"] += 1
        try:
            r = orig_call(self, *args, **kw)
        finally:
            state["depth"] -= 1
        try:
            record(name, ob if (bpy.context.active_object is None or bpy.context.active_object.type != "MESH" or fn != "join") else bpy.context.active_object,
                   before, sel, kw, VIA)
        except Exception as e:
            print("STAGEDUMP record failed", name, repr(e))
        return r
    bops._BPyOpsSubModOp.__call__ = call

    def label_wrap(mod, attr, labeler):
        orig = getattr(mod, attr, None)
        if orig is None:
            return
        def w(*a, **k):
            try:
                VIA.append(labeler(*a, **k))
            except Exception:
                VIA.append(attr)
            try:
                return orig(*a, **k)
            finally:
                VIA.pop()
        w.__name__ = getattr(orig, "__name__", attr)
        setattr(mod, attr, w)
    label_wrap(butil, "modify_mesh", lambda obj, type, *a, **k: "modify_mesh:" + str(type))
    label_wrap(butil, "apply_modifiers", lambda *a, **k: "apply_modifiers")
    label_wrap(butil, "apply_transform", lambda *a, **k: "apply_transform")
    label_wrap(butil, "join_objects", lambda *a, **k: "join_objects")
    label_wrap(S, "add_geomod", lambda obj, fn, *a, **k: "add_geomod:" + getattr(fn, "__name__", "fn"))
    try:
        import infinigen.assets.utils.object as AO
        label_wrap(AO, "join_objects", lambda *a, **k: "join_objects")
    except Exception:
        pass

    mod, cls = modcls.rsplit(".", 1)
    C = getattr(importlib.import_module("infinigen.assets.objects." + mod), cls)
    bpy.ops.wm.read_factory_settings(use_empty=True)
    np.random.seed(seed)
    t0 = time.time()
    f = C(seed)
    o = f.spawn_asset(inst)
    bpy.context.view_layer.update()
    # final asset: all mesh objects of the asset, evaluated, world space
    V, P = [], []
    for ob in [o] + list(o.children_recursive):
        if ob.type != "MESH":
            continue
        e = ob.evaluated_get(bpy.context.evaluated_depsgraph_get()).to_mesh()
        mw = ob.matrix_world; b = len(V)
        V += [list(mw @ v.co) for v in e.vertices]
        P += [[b + i for i in p.vertices] for p in e.polygons]
    json.dump({"verts": V, "polys": P}, open(os.path.join(outdir, "final.json"), "w"), separators=(",", ":"))
    open(os.path.join(outdir, "index.txt"), "w").write("\n".join(index) + "\n")
    print("STAGEDUMP %s seed %d: %d stages, final %d verts, %.1fs -> %s" % (modcls, seed, state["n"], len(V), time.time() - t0, outdir))

main()
