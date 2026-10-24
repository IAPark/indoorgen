"""opcheck.py OP --in case.json [--params '{...}'] [--out res.json]   (run with the infinigen venv python, bpy 4.2)

Apply ONE Blender operation / modifier to a small mesh and dump the result, so the Rust port of that op can be compared on the same
input (`target/fast/opcheck OP --in case.json --params ... [--blender res.json]`, see src/bin/opcheck.rs and tools/opsuite.sh).

  ~/work/infinigen-ref/.venv/bin/python tools/opcheck.py bevel --in tools/opcases/cube.json --params '{"offset":0.1,"segments":3}' --out /tmp/r.json
  ... opcheck.py --list            list ops and their default parameters

case.json (all keys optional except verts/polys):
  verts [[x,y,z]], polys [[i..]], edges [[a,b]] (extra / loose edges)
  sel_edges   "all" | "sharp:DEG" | [[a,b],...] (vertex pairs) | [edge index,...]
  sel_faces   "all" | [face index,...]            sel_verts "all" | [vert index,...]
  loops       [[v..],[v..]]  vertex loops (bridge_edge_loops / fill_grid select the edges between consecutive loop verts)
  crease_edges [[a,b,w]]   bevel_weight_edges [[a,b,w]]   bevel_weight_verts [[i,w]]
  other       {"verts","polys","loc":[x,y,z]}   second object for BOOLEAN
  params      default op parameters (the --params JSON overrides key by key; sel_*/loops/other keys inside params override the case's)
Result json: {"op","call","params","verts","polys","edges","in_edges"}  ("call" = exact bpy call(s) used).
Selection defaults: edge ops select all edges, face ops all faces.  Params are passed straight to the bpy operator / modifier
properties, so any property of the Blender API can be given.  Add an op: write `def op_name(c, p)` and register it in OPS.
`modifier` is the generic op: params {"type":"WELD","merge_threshold":0.1,...}; modifier properties are set with setattr.
"""
import sys, json, math, argparse, contextlib

CALLS = []
IN_EDGES = []   # edge list (Blender's own order) of the input mesh, saved as "in_edges" so the Rust side can use the same edge order

def rec(s):
    CALLS.append(s)

def kw(d):
    return ", ".join("%s=%r" % (k, v) for k, v in d.items())

def setup():
    import bpy
    bpy.ops.wm.read_factory_settings(use_empty=True)
    return bpy

def make_object(bpy, verts, polys, edges=None, name="o", loc=None):
    me = bpy.data.meshes.new(name)
    me.from_pydata([tuple(v) for v in verts], [tuple(e) for e in (edges or [])], [tuple(p) for p in polys])
    me.update()
    ob = bpy.data.objects.new(name, me)
    bpy.context.scene.collection.objects.link(ob)
    if loc:
        ob.location = tuple(loc)
    return ob

def activate(bpy, ob):
    for o in bpy.context.view_layer.objects:
        o.select_set(False)
    bpy.context.view_layer.objects.active = ob
    ob.select_set(True)

def edge_index(me):
    return {tuple(sorted(e.vertices)): e.index for e in me.edges}

def set_edge_attr(bpy, me, name, items):
    a = me.attributes.new(name, "FLOAT", "EDGE")
    idx = edge_index(me)
    vals = [0.0] * len(me.edges)
    for x, y, w in items:
        vals[idx[tuple(sorted((x, y)))]] = w
    a.data.foreach_set("value", vals)

def load_case(bpy, c):
    ob = make_object(bpy, c["verts"], c["polys"], c.get("edges"))
    me = ob.data
    if c.get("crease_edges"):
        set_edge_attr(bpy, me, "crease_edge", c["crease_edges"])
    if c.get("bevel_weight_edges"):
        set_edge_attr(bpy, me, "bevel_weight_edge", c["bevel_weight_edges"])
    if c.get("bevel_weight_verts"):
        a = me.attributes.new("bevel_weight_vert", "FLOAT", "POINT")
        vals = [0.0] * len(me.vertices)
        for i, w in c["bevel_weight_verts"]:
            vals[i] = w
        a.data.foreach_set("value", vals)
    activate(bpy, ob)
    IN_EDGES[:] = [list(e.vertices) for e in me.edges]
    return ob

@contextlib.contextmanager
def edit_mode(bpy, ob):
    activate(bpy, ob)
    bpy.ops.object.mode_set(mode="EDIT")
    try:
        yield
    finally:
        bpy.ops.object.mode_set(mode="OBJECT")

def select(bpy, ob, c, mode, key, default="all"):
    """edit mode: set select mode and select elements per case[key] (mode EDGE/VERT/FACE)"""
    import bmesh
    bpy.ops.mesh.select_mode(type=mode)
    bpy.ops.mesh.select_all(action="DESELECT")
    bm = bmesh.from_edit_mesh(ob.data)
    for t in (bm.verts, bm.edges, bm.faces):
        t.ensure_lookup_table()
    spec = c.get(key, default)
    if spec is None:
        return bm
    if mode == "EDGE":
        if spec == "all":
            es = list(bm.edges)
        elif isinstance(spec, str) and spec.startswith("sharp:"):
            lim = math.radians(float(spec[6:]))
            es = [e for e in bm.edges if len(e.link_faces) == 2 and e.calc_face_angle(0.0) > lim]
        else:
            es = []
            for s in spec:
                if isinstance(s, int):
                    es.append(bm.edges[s])
                else:
                    es.append(next(e for e in bm.edges if sorted((e.verts[0].index, e.verts[1].index)) == sorted(s)))
        for e in es:
            e.select_set(True)
    elif mode == "VERT":
        for i in (range(len(bm.verts)) if spec == "all" else spec):
            bm.verts[i].select_set(True)
    else:
        for i in (range(len(bm.faces)) if spec == "all" else spec):
            bm.faces[i].select_set(True)
    bm.select_flush_mode()
    bmesh.update_edit_mesh(ob.data)
    return bm

def select_loops(bpy, ob, c):
    edges = []
    for lp in c["loops"]:
        n = len(lp)
        edges += [[lp[i], lp[(i + 1) % n]] for i in range(n)]
    c = dict(c, sel_edges=edges)
    select(bpy, ob, c, "EDGE", "sel_edges")

def run_ops(bpy, name, **k):
    """call bpy.ops.<name>(**k) and record it"""
    rec("bpy.ops.%s(%s)" % (name, kw(k)))
    r = bpy.ops
    for part in name.split("."):
        r = getattr(r, part)
    res = r(**k)
    return res

def apply_modifier(bpy, ob, mtype, props, name="m"):
    props = dict(props)
    tex = props.pop("texture", None)
    m = ob.modifiers.new(name, mtype)
    rec("m = ob.modifiers.new(%r, %r)" % (name, mtype))
    if tex is not None:
        t = dict(tex)
        tt = bpy.data.textures.new("t", type=t.pop("type"))
        rec("tex = bpy.data.textures.new('t', type=%r)" % tt.type)
        for k, v in t.items():
            setattr(tt, k, v)
            rec("tex.%s = %r" % (k, v))
        m.texture = tt
        rec("m.texture = tex")
    for k, v in props.items():
        if k == "object":
            continue
        setattr(m, k, v)
        rec("m.%s = %r" % (k, v))
    return m

def commit(bpy, ob, m):
    rec("bpy.ops.object.modifier_apply(modifier=%r)" % m.name)
    activate(bpy, ob)
    bpy.ops.object.modifier_apply(modifier=m.name)

def mod_op(mtype, defaults=None):
    def f(c, p):
        bpy = setup()
        ob = load_case(bpy, c)
        props = dict(defaults or {}); props.update(p)
        m = apply_modifier(bpy, ob, mtype, props)
        commit(bpy, ob, m)
        return bpy, ob, props
    return f

def op_modifier(c, p):
    p = dict(p)
    mtype = p.pop("type")
    return mod_op(mtype)(c, p)

def op_bevel(c, p):
    bpy = setup(); ob = load_case(bpy, c)
    d = dict(offset_type="OFFSET", offset=0.1, segments=1, profile=0.5, affect="EDGES", clamp_overlap=False, loop_slide=True)
    d.update(p)
    with edit_mode(bpy, ob):
        if d["affect"] == "VERTICES":
            select(bpy, ob, c, "VERT", "sel_verts")
        else:
            select(bpy, ob, c, "EDGE", "sel_edges")
        run_ops(bpy, "mesh.bevel", **d)
    return bpy, ob, d

def op_bridge(c, p):
    bpy = setup(); ob = load_case(bpy, c)
    d = dict(number_cuts=0, interpolation="PATH", smoothness=1.0, profile_shape_factor=0.0)
    d.update(p)
    with edit_mode(bpy, ob):
        select_loops(bpy, ob, c)
        run_ops(bpy, "mesh.bridge_edge_loops", **d)
    return bpy, ob, d

def op_fill_grid(c, p):
    bpy = setup(); ob = load_case(bpy, c)
    d = dict(use_interp_simple=True)
    d.update(p)
    with edit_mode(bpy, ob):
        if "loops" in c:
            select_loops(bpy, ob, c)
        else:
            select(bpy, ob, c, "EDGE", "sel_edges")
        run_ops(bpy, "mesh.fill_grid", **d)
    return bpy, ob, d

def op_subdivide(c, p):
    bpy = setup(); ob = load_case(bpy, c)
    d = dict(number_cuts=1, smoothness=0.0)
    d.update(p)
    with edit_mode(bpy, ob):
        select(bpy, ob, c, "EDGE", "sel_edges")
        run_ops(bpy, "mesh.subdivide", **d)
    return bpy, ob, d

def op_face(opname, defaults):
    def f(c, p):
        bpy = setup(); ob = load_case(bpy, c)
        d = dict(defaults); d.update(p)
        with edit_mode(bpy, ob):
            select(bpy, ob, c, "FACE", "sel_faces")
            run_ops(bpy, opname, **d)
        return bpy, ob, d
    return f

def op_remove_doubles(c, p):
    bpy = setup(); ob = load_case(bpy, c)
    d = dict(threshold=1e-4); d.update(p)
    with edit_mode(bpy, ob):
        select(bpy, ob, c, "VERT", "sel_verts")
        run_ops(bpy, "mesh.remove_doubles", **d)
    return bpy, ob, d

def op_convex_hull(c, p):
    bpy = setup(); ob = load_case(bpy, c)
    d = dict(); d.update(p)
    with edit_mode(bpy, ob):
        select(bpy, ob, c, "VERT", "sel_verts")
        run_ops(bpy, "mesh.convex_hull", **d)
    return bpy, ob, d

def op_boolean(c, p):
    bpy = setup(); ob = load_case(bpy, c)
    o = c["other"]
    ob2 = make_object(bpy, o["verts"], o["polys"], name="other", loc=o.get("loc"))
    d = dict(operation="DIFFERENCE", solver="EXACT"); d.update(p)
    m = apply_modifier(bpy, ob, "BOOLEAN", d)
    m.object = ob2
    rec("m.object = other")
    commit(bpy, ob, m)
    return bpy, ob, d

OPS = {
    "bevel": op_bevel,  # bpy.ops.mesh.bevel on selected edges / verts
    "bevel_mod": mod_op("BEVEL", dict(width=0.1, segments=1, limit_method="ANGLE")),
    "solidify": mod_op("SOLIDIFY", dict(thickness=0.1, offset=-1.0, solidify_mode="EXTRUDE")),
    "bridge": op_bridge,
    "fill_grid": op_fill_grid,
    "subsurf": mod_op("SUBSURF", dict(levels=1, render_levels=1, subdivision_type="CATMULL_CLARK")),
    "subdivide": op_subdivide,
    "boolean": op_boolean,
    "triangulate": mod_op("TRIANGULATE", dict(quad_method="BEAUTY", ngon_method="BEAUTY", min_vertices=4)),
    "triangulate_op": op_face("mesh.quads_convert_to_tris", dict(quad_method="BEAUTY", ngon_method="BEAUTY")),
    "beautify_fill": op_face("mesh.beautify_fill", dict(angle_limit=math.radians(180))),
    "remove_doubles": op_remove_doubles,
    "remesh": mod_op("REMESH", dict(mode="VOXEL", voxel_size=0.1)),
    "displace": mod_op("DISPLACE", dict(strength=1.0, mid_level=0.5, direction="NORMAL", texture_coords="LOCAL",
                                         texture={"type": "CLOUDS", "noise_scale": 0.25})),
    "convex_hull": op_convex_hull,
    "poke": op_face("mesh.poke", dict()),
    "tris_to_quads": op_face("mesh.tris_convert_to_quads", dict()),
    "modifier": op_modifier,
}

def result(bpy, ob):
    me = ob.data
    return {"verts": [[v.co.x, v.co.y, v.co.z] for v in me.vertices],
            "polys": [list(p.vertices) for p in me.polygons],
            "edges": [list(e.vertices) for e in me.edges]}

def run(op, case, params):
    del CALLS[:]
    c = dict(case)
    p = dict(c.get("params", {})); p.update(params)
    for k in [k for k in p if k.startswith("_")]:   # harness meta (read by the rust side only)
        p.pop(k)
    for k in [k for k in p if k.startswith("sel_") or k in ("loops", "other")]:   # selections may be given in --params too
        c[k] = p.pop(k)
    bpy, ob, used = OPS[op](c, p)
    r = result(bpy, ob)
    r["in_edges"] = IN_EDGES
    r.update(op=op, call=list(CALLS), params=used)
    return r

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("op", nargs="?")
    ap.add_argument("--in", dest="inp")
    ap.add_argument("--params", default="{}")
    ap.add_argument("--out")
    ap.add_argument("--list", action="store_true")
    a = ap.parse_args()
    if a.list or not a.op:
        print("ops:", " ".join(OPS)); return
    r = run(a.op, json.load(open(a.inp)), json.loads(a.params))
    if a.out:
        json.dump(r, open(a.out, "w"))
    print("OPCHECK %s verts %d polys %d edges %d" % (a.op, len(r["verts"]), len(r["polys"]), len(r["edges"])))
    for s in r["call"]:
        print("  ", s)

if __name__ == "__main__":
    main()
