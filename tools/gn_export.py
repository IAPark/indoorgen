"""gn_export.py : export Blender geometry node groups to JSON for the Rust interpreter (src/gn).

Run with the infinigen venv:  ~/work/infinigen-ref/.venv/bin/python tools/gn_export.py ...

  gn_export.py group  MODULE.FUNC OUT.json [ARGS_JSON]
      import FUNC (an @node_utils.to_nodegroup function, e.g.
      infinigen.assets.objects.tables.table_utils.nodegroup_n_gon_profile), call it (FUNC(*ARGS)),
      export the returned group + every group it references.
  gn_export.py asset  MODULE.CLASS SEED OUT.json [INST]
      spawn the factory asset (materials stubbed like ~/work/ref/scripts/geomcmp.py) and record every
      geometry-nodes modifier that is applied during construction ("steps"): group (+ nested), modifier
      input values and the input mesh the modifier was applied to.

JSON format (version 1)
  {"version":1,
   "groups": {NAME: {"inputs":[IFACE..], "outputs":[IFACE..], "nodes":[NODE..], "links":[LINK..]}},
   "main": NAME | null,
   "steps": [{"group":NAME,"inputs":{IDENT:value},"mesh":MESH,"object":NAME}]   (asset mode only)
   "objects": {NAME:{"mesh":MESH,"matrix":[16]}}}
  IFACE = {"name","id","type":"FLOAT|INT|BOOLEAN|VECTOR|RGBA|ROTATION|STRING|GEOMETRY|OBJECT|MATERIAL|COLLECTION|IMAGE|MENU",
           "default","min","max","subtype"}
  NODE  = {"name","type":bl_idname,"mute","p":{properties},"in":[SOCK..],"out":[SOCK..]}
  SOCK  = {"id":identifier,"name","t":type,"v":default_value (only inputs, only if enabled)}  (disabled sockets are omitted)
  LINK  = {"fn","fs","tn","ts","sort"}   from node/socket identifier -> to node/socket identifier; links are listed
          in ng.links order; `sort` = multi_input_sort_id (join order for multi-input sockets).
  Frame nodes are dropped, reroutes are kept (type NodeReroute), muted/invalid links are dropped.
  MESH = {"v":[[x,y,z]..],"e":[[a,b]..],"f":[[v..]..]}
  Special props:  FunctionNodeInputVector.vector, ShaderNodeValue "value" (output default), ShaderNodeFloatCurve "curve"
  ({"pts":[[x,y,handle]..],"ext":..,"clip":..}), ShaderNodeValToRGB "ramp", GeometryNodeGroup "group" (name).
"""
import sys, os, json, importlib
import numpy as np

import bpy

SKIP = {"rna_type", "type", "location", "width", "height", "dimensions", "name", "label", "inputs", "outputs",
        "internal_links", "parent", "use_custom_color", "color", "select", "show_options", "show_preview", "hide",
        "mute", "show_texture", "bl_idname", "bl_label", "bl_description", "bl_icon", "bl_static_type",
        "bl_width_default", "bl_width_min", "bl_width_max", "bl_height_default", "bl_height_min", "bl_height_max",
        "is_active_output", "node_tree", "mapping", "color_ramp", "texture_mapping", "color_mapping", "image_user",
        "image", "script", "text", "filepath", "interface", "location_absolute"}

TYPEMAP = {"VALUE": "FLOAT", "INT": "INT", "BOOLEAN": "BOOLEAN", "VECTOR": "VECTOR", "RGBA": "RGBA",
           "ROTATION": "ROTATION", "STRING": "STRING", "GEOMETRY": "GEOMETRY", "OBJECT": "OBJECT",
           "MATERIAL": "MATERIAL", "COLLECTION": "COLLECTION", "IMAGE": "IMAGE", "SHADER": "SHADER",
           "MENU": "MENU", "CUSTOM": "CUSTOM", "TEXTURE": "TEXTURE", "MATRIX": "MATRIX", "BUNDLE": "BUNDLE", "CLOSURE": "CLOSURE"}


def jv(v):
    """json-able value"""
    if v is None:
        return None
    if isinstance(v, (bool, int, float, str)):
        return v
    if isinstance(v, bpy.types.ID):
        return {"id": v.name}
    try:
        return [jv(x) for x in v]
    except TypeError:
        return str(v)


def sock_default(s):
    if not hasattr(s, "default_value"):
        return None
    try:
        return jv(s.default_value)
    except Exception:
        return None


def export_sock(s, is_in):
    d = {"id": s.identifier, "name": s.name, "t": TYPEMAP.get(s.type, s.type)}
    if is_in:
        v = sock_default(s)
        if v is not None:
            d["v"] = v
        if getattr(s, "is_multi_input", False):
            d["multi"] = True
    return d


def export_node(n):
    d = {"name": n.name, "type": n.bl_idname}
    if n.mute:
        d["mute"] = True
    p = {}
    for prop in n.bl_rna.properties:
        k = prop.identifier
        if k in SKIP:
            continue
        try:
            v = getattr(n, k)
        except Exception:
            continue
        if prop.type == "POINTER":
            if v is not None and isinstance(v, bpy.types.ID):
                p[k] = v.name
            continue
        if prop.type == "COLLECTION":
            continue
        p[k] = jv(v)
    if n.bl_idname == "ShaderNodeValue":
        p["value"] = jv(n.outputs[0].default_value)
    if n.bl_idname == "GeometryNodeGroup":
        p["group"] = n.node_tree.name if n.node_tree else None
    if n.bl_idname == "ShaderNodeFloatCurve":
        c = n.mapping.curves[0]
        p["curve"] = {"pts": [[pt.location[0], pt.location[1], pt.handle_type] for pt in c.points],
                      "ext": n.mapping.extend, "clip": [n.mapping.use_clip, n.mapping.clip_min_x,
                                                         n.mapping.clip_min_y, n.mapping.clip_max_x, n.mapping.clip_max_y]}
    if n.bl_idname == "ShaderNodeVectorCurve":
        m = n.mapping
        p["curves3"] = {"pts": [[[pt.location[0], pt.location[1], pt.handle_type] for pt in c.points] for c in m.curves[:3]],
                        "clip": [m.use_clip, m.clip_min_x, m.clip_min_y, m.clip_max_x, m.clip_max_y]}
    if n.bl_idname == "ShaderNodeValToRGB":
        r = n.color_ramp
        p["ramp"] = {"interp": r.interpolation, "mode": r.color_mode, "hue": r.hue_interpolation,
                     "els": [[e.position, list(e.color)] for e in r.elements]}
    if n.bl_idname == "GeometryNodeInputActiveCamera":
        pass
    if n.bl_idname == "NodeGroupOutput":
        p["active"] = bool(n.is_active_output)
    d["p"] = p
    d["in"] = [export_sock(s, True) for s in n.inputs if s.enabled]
    d["out"] = [export_sock(s, False) for s in n.outputs if s.enabled]
    return d


def export_iface(item):
    d = {"name": item.name, "id": item.identifier, "type": TYPEMAP.get(item.socket_type, item.socket_type)}
    d["type"] = {"NodeSocketFloat": "FLOAT", "NodeSocketInt": "INT", "NodeSocketBool": "BOOLEAN",
                 "NodeSocketVector": "VECTOR", "NodeSocketColor": "RGBA", "NodeSocketRotation": "ROTATION",
                 "NodeSocketString": "STRING", "NodeSocketGeometry": "GEOMETRY", "NodeSocketObject": "OBJECT",
                 "NodeSocketMaterial": "MATERIAL", "NodeSocketCollection": "COLLECTION", "NodeSocketImage": "IMAGE",
                 "NodeSocketMenu": "MENU", "NodeSocketMatrix": "MATRIX", "NodeSocketShader": "SHADER"}.get(item.socket_type, item.socket_type)
    if hasattr(item, "default_value"):
        d["default"] = jv(item.default_value)
    for k in ("min_value", "max_value"):
        if hasattr(item, k):
            d[k[:3]] = jv(getattr(item, k))
    if hasattr(item, "subtype"):
        d["subtype"] = item.subtype
    return d


def export_group(ng, out):
    """add ng (and nested groups) to out['groups']"""
    if ng.name in out["groups"]:
        return
    out["groups"][ng.name] = None  # recursion guard
    nodes = []
    names = set()
    for n in ng.nodes:
        if n.bl_idname == "NodeFrame":
            continue
        names.add(n.name)
        nodes.append(export_node(n))
        if n.bl_idname == "GeometryNodeGroup" and n.node_tree is not None:
            export_group(n.node_tree, out)
    links = []
    for l in ng.links:
        if l.is_muted or not l.is_valid:
            continue
        if l.from_node.name not in names or l.to_node.name not in names:
            continue
        if not l.from_socket.enabled or not l.to_socket.enabled:
            continue
        links.append({"fn": l.from_node.name, "fs": l.from_socket.identifier, "tn": l.to_node.name,
                      "ts": l.to_socket.identifier, "sort": l.multi_input_sort_id})
    for n in ng.nodes:
        for sk in n.inputs:
            v = getattr(sk, "default_value", None) if sk.type == "OBJECT" else None
            if isinstance(v, bpy.types.Object) and v.name not in out["objects"]:
                out["objects"][v.name] = {"mesh": export_mesh(v.data) if v.type == "MESH" else None,
                                          "matrix": [x for r in v.matrix_world for x in r]}
    items = [i for i in ng.interface.items_tree if i.item_type == "SOCKET"]
    out["groups"][ng.name] = {
        "inputs": [export_iface(i) for i in items if i.in_out == "INPUT"],
        "outputs": [export_iface(i) for i in items if i.in_out == "OUTPUT"],
        "nodes": nodes, "links": links}


def export_mesh(me):
    """vertices / edges / polygons plus the (non-internal) attributes and material slot names"""
    m = extract_mesh(me)
    return {"v": m["v"], "e": m["e"], "f": m["f"], "attrs": m["attrs"], "mats": m["mats"]}


ATTR_FIELDS = {"FLOAT": ("value", 1), "INT": ("value", 1), "BOOLEAN": ("value", 1), "INT8": ("value", 1),
               "FLOAT_VECTOR": ("vector", 3), "FLOAT_COLOR": ("color", 4), "BYTE_COLOR": ("color", 4),
               "FLOAT2": ("vector", 2), "QUATERNION": ("value", 4)}


def extract_mesh(me):
    nv, ne, nf = len(me.vertices), len(me.edges), len(me.polygons)
    co = np.zeros(nv * 3, dtype=np.float32)
    me.vertices.foreach_get("co", co)
    ed = np.zeros(ne * 2, dtype=np.int32)
    me.edges.foreach_get("vertices", ed)
    polys = [list(p.vertices) for p in me.polygons]
    attrs = {}
    for a in me.attributes:
        if a.name.startswith(".") or a.name == "position" or a.data_type not in ATTR_FIELDS:
            continue
        if a.domain not in ("POINT", "EDGE", "FACE", "CORNER") or a.data_type == "FLOAT2":
            continue
        fld, dim = ATTR_FIELDS[a.data_type]
        n = len(a.data)
        buf = np.zeros(n * dim, dtype=np.float32 if a.data_type not in ("INT", "INT8", "BOOLEAN") else np.int32)
        if a.data_type == "BOOLEAN":
            buf = np.zeros(n * dim, dtype=bool)
        a.data.foreach_get(fld, buf)
        arr = buf.reshape(n, dim) if dim > 1 else buf
        attrs[a.name] = {"dom": a.domain, "ty": a.data_type, "data": arr.tolist()}
    return {"v": co.reshape(-1, 3).tolist(), "e": ed.reshape(-1, 2).tolist(), "f": polys, "attrs": attrs,
            "mats": [m.name if m else None for m in me.materials]}



def new_out():
    return {"version": 1, "groups": {}, "main": None, "steps": [], "objects": {}}


def mod_inputs(mod, out):
    d = {}
    for i in mod.node_group.interface.items_tree:
        if i.item_type != "SOCKET" or i.in_out != "INPUT":
            continue
        if i.identifier in mod.keys():
            v = mod[i.identifier]
            if isinstance(v, bpy.types.Object):
                out["objects"][v.name] = {"mesh": export_mesh(v.data) if v.type == "MESH" else None,
                                          "matrix": [x for r in v.matrix_world for x in r]}
                v = {"object": v.name}
            elif isinstance(v, bpy.types.ID):
                v = {"id": v.name}
            elif hasattr(v, "to_list"):
                v = v.to_list()
            elif hasattr(v, "__len__") and not isinstance(v, str):
                v = [x for x in v]
            d[i.identifier] = v
    return d


def mod_out_attrs(mod):
    """non-geometry group outputs written to named attributes by the modifier"""
    res = []
    for i in mod.node_group.interface.items_tree:
        if i.item_type != "SOCKET" or i.in_out != "OUTPUT" or i.socket_type == "NodeSocketGeometry":
            continue
        key = i.identifier + "_attribute_name"
        if key in mod.keys() and mod[key]:
            res.append({"id": i.identifier, "name": str(mod[key]), "domain": getattr(i, "attribute_domain", "POINT")})
    return res


def stub_materials():
    class _D:
        def __init__(self, *a, **k): pass
        def __call__(self, *a, **k):
            return bpy.data.materials.new("stub")
        def __getattr__(self, n):
            if n.startswith("__"):
                raise AttributeError(n)
            return _D()
    import infinigen.core.util.random as R
    R.weighted_sample = lambda *a, **k: _D
    import infinigen.core.surface as S
    S.assign_material = lambda *a, **k: None
    for modn, fn in (("infinigen.assets.materials.wood.plywood", "get_shelf_material"),):
        try:
            mm = importlib.import_module(modn)
            setattr(mm, fn, lambda *a, **k: bpy.data.materials.new("stub"))
        except Exception:
            pass


_ORIG_ADD = None
_ORIG_APPLY = None


def json_safe(v):
    if isinstance(v, dict):
        return {str(k): json_safe(x) for k, x in v.items()}
    if isinstance(v, (list, tuple)):
        return [json_safe(x) for x in v]
    if isinstance(v, np.generic):
        return v.item()
    if isinstance(v, (bool, int, float, str)) or v is None:
        return v
    return "<%s>" % type(v).__name__


def export_asset(modcls, seed, inst=0):
    import numpy as np
    stub_materials()
    mod, cls = modcls.rsplit(".", 1)
    C = getattr(importlib.import_module("infinigen.assets.objects." + mod), cls)
    out = new_out()
    import infinigen.core.util.blender as butil
    global _ORIG_APPLY
    if _ORIG_APPLY is None:
        _ORIG_APPLY = butil.apply_modifiers
    orig = _ORIG_APPLY

    def hook(obj, mod=None, quiet=True):
        ms = mod
        if ms is None:
            ms = list(obj.modifiers)
        if not isinstance(ms, list):
            ms = [ms]
        ms = [obj.modifiers[m] if isinstance(m, str) else m for m in ms]
        for m in ms:
            step = None
            if m.type == "NODES" and m.node_group is not None:
                export_group(m.node_group, out)
                step = {"group": m.node_group.name, "inputs": mod_inputs(m, out), "out_attrs": mod_out_attrs(m),
                        "mesh": export_mesh(obj.data) if obj.type == "MESH" else None, "object": obj.name}
            if step is not None:
                keep = m.node_group.copy()
                keep.use_fake_user = True
                step["keep"] = keep.name
            res = orig(obj, m, quiet)
            if step is not None:
                try:
                    step["expect"] = extract_mesh(obj.data)
                except Exception as ex:
                    step["expect"] = {"error": repr(ex)}
                out["steps"].append(step)
        return None
    butil.apply_modifiers = hook
    import infinigen.core.surface as S
    S.butil.apply_modifiers = hook
    global _ORIG_ADD
    if _ORIG_ADD is None:
        _ORIG_ADD = S.add_geomod
    _orig_add = _ORIG_ADD

    def add_geomod(objs, geo_func, *a, **k):
        r = _orig_add(objs, geo_func, *a, **k)
        out.setdefault("_kwargs", []).append({"func": getattr(geo_func, "__name__", str(geo_func)), "kwargs": json_safe(k.get("input_kwargs", {})),
                                                "args": json_safe(k.get("input_args", []))})
        return r
    S.add_geomod = add_geomod
    import sys as _sys
    for mn, md in list(_sys.modules.items()):
        if mn.startswith("infinigen.assets") and getattr(md, "add_geomod", None) is _orig_add:
            md.add_geomod = add_geomod
    # modules imported later (the factory module itself) pick up S.add_geomod through `surface.add_geomod` attribute access
    bpy.ops.wm.read_factory_settings(use_empty=True)
    np.random.seed(seed)
    f = C(seed)
    o = f.spawn_asset(inst)
    if o.type == "MESH":
        try:
            out["final"] = extract_mesh(o.data)  # mesh of the finished asset (after add_bevel etc.)
        except Exception as ex:
            out["final"] = {"error": repr(ex)}
    # unapplied modifiers on the final object
    for ob in [o] + list(o.children_recursive):
        for m in ob.modifiers:
            if m.type == "NODES" and m.node_group is not None:
                export_group(m.node_group, out)
                out["steps"].append({"group": m.node_group.name, "inputs": mod_inputs(m, out), "out_attrs": mod_out_attrs(m),
                                     "mesh": export_mesh(ob.data) if ob.type == "MESH" else None, "object": ob.name,
                                     "unapplied": True})
    return out


def export_function(modfn, args):
    mod, fn = modfn.rsplit(".", 1)
    f = getattr(importlib.import_module(mod), fn)
    out = new_out()
    ng = f(*args)
    export_group(ng, out)
    out["main"] = ng.name
    return out


def _flat_diff(a, b, path, acc):
    """collect (path, va, vb) for leaves that differ"""
    if isinstance(a, dict) and isinstance(b, dict):
        for k in a:
            if k in b:
                _flat_diff(a[k], b[k], path + [k], acc)
    elif isinstance(a, list) and isinstance(b, list) and len(a) == len(b):
        for i, (x, y) in enumerate(zip(a, b)):
            _flat_diff(x, y, path + [i], acc)
    elif a != b:
        acc.append((path, a, b))


def _close(x, y):
    return abs(x - y) <= 1e-6 * max(1.0, abs(x), abs(y))


def _match_param(va, vb, kwa, kwb):
    # scalar parameters first, then elements of sequence parameters
    for name, pa in sorted(kwa.items(), key=lambda kv: isinstance(kv[1], list)):
        pb = kwb.get(name)
        if isinstance(pa, (int, float)) and not isinstance(pa, bool) and isinstance(pb, (int, float)) and isinstance(va, (int, float)):
            if _close(pa, va) and _close(pb, vb):
                return name, None
        elif isinstance(pa, list) and isinstance(pb, list) and isinstance(va, (int, float)):
            for i, (x, y) in enumerate(zip(pa, pb)):
                if isinstance(x, (int, float)) and _close(x, va) and _close(y, vb):
                    return name, i
        elif pa == va and pb == vb:
            return name, None
    return None, None


def make_template(modcls, seed_a, seed_b, inst=0):
    """export the asset for two factory seeds; the differing leaves become parameter patches (matched against the
    add_geomod input_kwargs). Returns the seed-A export plus out['patches'] / out['unmapped']."""
    a = export_asset(modcls, seed_a, inst)
    b = export_asset(modcls, seed_b, inst)
    kwa = {}
    kwb = {}
    for k in a.get("_kwargs", []):
        kwa.update(k["kwargs"])
    for k in b.get("_kwargs", []):
        kwb.update(k["kwargs"])
    patches, unmapped = [], []
    for gname, ga in a["groups"].items():
        gb = b["groups"].get(gname)
        if gb is None or len(ga["nodes"]) != len(gb["nodes"]):
            unmapped.append({"group": gname, "why": "structure differs"})
            continue
        acc = []
        for na, nb in zip(ga["nodes"], gb["nodes"]):
            if na["name"] != nb["name"]:
                unmapped.append({"group": gname, "why": "node order differs"})
                break
            _flat_diff(na, nb, [], [])
            local = []
            _flat_diff(na, nb, [], local)
            for path, va, vb in local:
                name, idx = _match_param(va, vb, kwa, kwb)
                rec = {"g": gname, "n": na["name"], "at": path}
                if name is None:
                    rec["a"], rec["b"] = va, vb
                    unmapped.append(rec)
                else:
                    rec["param"] = name
                    if idx is not None:
                        rec["idx"] = idx
                    patches.append(rec)
    a["patches"] = patches
    a["unmapped"] = unmapped
    a["params_a"] = kwa
    a["params_b"] = kwb
    return a


def dump(out, path):
    with open(path, "w") as fh:
        json.dump(out, fh, separators=(",", ":"))


if __name__ == "__main__":
    a = sys.argv[1:]
    if a and a[0] == "group":
        dump(export_function(a[1], json.loads(a[3]) if len(a) > 3 else []), a[2])
    elif a and a[0] == "template":
        # template MODULE.CLASS SEED_A SEED_B OUT.json
        t = make_template(a[1], int(a[2]), int(a[3]))
        t.pop("_kwargs", None)
        dump(t, a[4])
        print("patches:", len(t["patches"]), "unmapped:", len(t["unmapped"]))
        for u in t["unmapped"][:20]:
            print("  unmapped", u)
    elif a and a[0] == "asset":
        dump(export_asset(a[1], int(a[2]), int(a[4]) if len(a) > 4 else 0), a[3])
    else:
        print(__doc__)
