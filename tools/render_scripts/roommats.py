"""Room wall/floor/ceiling materials for Rust room shells, drawn by Infinigen's own sampling code.

Faithful copies of the material part of infinigen.core.constraints.example_solver.room.decorate.{room_walls,room_floors,room_ceilings}
(same draw order, same FixedSeed(int_hash((scene_seed, stage))) as generate_indoors' p.run_stage), applied to the Rust objects
"<room>.wall", "<room>.wall.alt", "<room>.floor", "<room>.ceiling".  Geometry stays Rust's (the alt split is a separate node).
Also: dump_room_materials() = canonical node-tree dump of those objects, for comparing Rust renders with python scene.blend files."""
import re, json, hashlib
import bpy, numpy as np
from numpy.random import uniform
from infinigen.core import surface
from infinigen.core.util.math import FixedSeed, int_hash
from infinigen.core.util.random import log_uniform, weighted_sample, random_general as rg
from infinigen.core.constraints.example_solver.room import decorate as room_dec
from infinigen.core.constraints.example_solver.room.base import room_type


# python iterates set() of fresh instances = memory-address order, different in every process (measured: tools/roommat notes in
# BACKLOG).  ROOMMATS_ORDER=insertion (default, deterministic) | reverse | memory (python's own set order)
ORDER = {"mode": __import__("os").environ.get("ROOMMATS_ORDER", "insertion"), "perm": {}}


def _iter_set(items, stage):
    uniq = list(dict.fromkeys(items))  # all instances are distinct objects
    p = ORDER["perm"].get(stage)
    if p is not None: return [uniq[i] for i in p]
    m = ORDER["mode"]
    if m == "memory": return list(set(items))
    return uniq[::-1] if m == "reverse" else uniq


def _set_mat(o, m):
    o.data.materials.clear(); o.data.materials.append(m)


def _unwrap(o):
    try:
        from infinigen.assets.utils.uv import unwrap_normal
        unwrap_normal(o, selection=None)
    except Exception as e:
        print("unwrap_normal FAIL", o.name, repr(e))


def _walls(walls, alts, n_walls=3):
    """walls: Rust wall objects in room order; alts: {wall name: alt object or None}. Returns per-wall decision log."""
    wall_fns = list(weighted_sample(room_dec.room_wall_fns[room_type(r.name)])() for r in walls)
    kwargs = dict(vertical=True, is_ceramic=True, alternating=False, shape="square")
    for wall_fn in _iter_set(wall_fns, 'walls'):
        shape = np.random.choice(["square", "rectangle", "hexagon"])
        kwargs = dict(vertical=True, alternating=False, shape=shape)
        rooms_ = [o for o, w in zip(walls, wall_fns) if w == wall_fn]
        indices = np.random.randint(0, n_walls, len(rooms_))
        for i in range(n_walls):
            rooms__ = [r for r, j in zip(rooms_, indices) if j == i]
            if wall_fn.__class__.__name__ == "Plaster":
                for r in rooms__: _unwrap(r)
            if wall_fn.__class__.__name__ == "Brick":
                kwargs = {}
            mat = wall_fn(**kwargs)
            if rooms__: _set_mat(rooms__[0], mat)
    log = {}
    for w in walls:
        a = alts.get(w.name)
        fn = rg(room_dec.room_wall_alternative_fns[room_type(w.name)])
        match fn:
            case "none":
                log[w.name] = "none"
                if a is not None:  # Rust split a wall python leaves uniform: same material on both halves
                    _set_mat(a, w.data.materials[0])
                continue
            case "half":
                uniform(0.3, 0.6)  # z_thresh draw
                mat = room_dec.plaster.Plaster()(**kwargs)
                log[w.name] = "half"
                if a is not None: _unwrap(a); _set_mat(a, mat)
            case _:
                mat_gen = fn()
                mat = mat_gen(scale=log_uniform(0.5, 2.0), **kwargs)
                log[w.name] = "aligned:" + mat_gen.__class__.__name__
                if a is not None:
                    if mat_gen.__class__.__name__ == "Plaster": _unwrap(a)
                    _set_mat(a, mat)
    return log


def _ceilings(ceilings):
    fns = list(weighted_sample(room_dec.room_ceiling_fns[room_type(r.name)])() for r in ceilings)
    for fn in _iter_set(fns, 'ceilings'):
        rooms_ = [o for o, f in zip(ceilings, fns) if f == fn]
        if fn.__class__.__name__ == "Plaster":
            for r in rooms_: _unwrap(r)
        mat = fn()
        if rooms_: _set_mat(rooms_[0], mat)


def _floors(floors, n_floors=3):
    gens = [weighted_sample(room_dec.room_floor_fns[room_type(r.name)])() for r in floors]
    for fn in _iter_set(gens, 'floors'):
        rooms_ = [o for o, f in zip(floors, gens) if f == fn]
        indices = np.random.randint(0, n_floors, len(rooms_))
        for i in range(n_floors):
            rooms__ = [r for r, j in zip(rooms_, indices) if j == i]
            if fn.__class__.__name__ == "Plaster":
                for r in rooms__: _unwrap(r)
            mat = fn()
            if rooms__: _set_mat(rooms__[0], mat)


def apply_room_materials(scene_seed, order=None, stages=("walls", "floors", "ceilings")):
    """Run python's three room stages on the Rust room objects. order = node names in sidecar (= python room_meshes) order, since
    bpy.data.objects iterates alphabetically. Returns {wall name: alt decision}."""
    objs = [o for o in bpy.data.objects if o.type == "MESH"]
    if order: objs.sort(key=lambda o: order.index(o.name) if o.name in order else len(order))
    by = {o.name: o for o in objs}
    pick = lambda suf: [o for o in objs if o.name.endswith(suf) and not o.name.endswith(".alt" + suf) and re.fullmatch(r"[a-z\-]+_\d+/\d+" + re.escape(suf), o.name)]
    walls, floors, ceilings = pick(".wall"), pick(".floor"), pick(".ceiling")
    # blender uniquifies names (".001"): the glb importer keeps ours
    alts = {w.name: by.get(w.name + ".alt") for w in walls}
    log = {}
    if "walls" in stages:
        with FixedSeed(int_hash((scene_seed, "room_walls"))): log = _walls(walls, alts)
    if "floors" in stages:
        with FixedSeed(int_hash((scene_seed, "room_floors"))): _floors(floors)
    if "ceilings" in stages:
        with FixedSeed(int_hash((scene_seed, "room_ceilings"))): _ceilings(ceilings)
    return log


# ---- canonical dump -------------------------------------------------------------------------------------------
def _r(v):
    try:
        return [_r(x) for x in v]
    except TypeError:
        return float(f"{v:.4g}") if isinstance(v, float) else v


def _dump_tree(nt, out, depth=0, seen=None):
    seen = seen if seen is not None else set()
    for n in nt.nodes:
        line = [n.bl_idname]
        for k in ("operation", "data_type", "blend_type", "noise_dimensions", "voronoi_dimensions", "feature", "distance", "interpolation", "wave_type", "subsurface_method", "distribution", "projection", "clamp", "use_clamp"):
            if hasattr(n, k): line.append(f"{k}={getattr(n, k)}")
        for s in n.inputs:
            if s.is_linked or not hasattr(s, "default_value"): continue
            line.append(f"{s.identifier}={_r(s.default_value)}")
        if n.bl_idname == "ShaderNodeValToRGB":
            line.append("ramp=" + str([(round(e.position, 4), _r(list(e.color))) for e in n.color_ramp.elements]))
        if n.bl_idname == "ShaderNodeValue":
            line.append(f"value={_r(n.outputs[0].default_value)}")
        if n.bl_idname == "ShaderNodeRGB":
            line.append(f"rgb={_r(list(n.outputs[0].default_value))}")
        if n.bl_idname in ("ShaderNodeFloatCurve", "ShaderNodeRGBCurve", "ShaderNodeVectorCurve"):
            line.append("curves=" + str([[(round(p.location[0], 4), round(p.location[1], 4), p.handle_type) for p in c.points] for c in n.mapping.curves]))
        out.append("  " * depth + " ".join(map(str, line)))
        if n.bl_idname == "ShaderNodeGroup" and n.node_tree is not None and n.node_tree.name not in seen:
            seen.add(n.node_tree.name)
            _dump_tree(n.node_tree, out, depth + 1, seen)


def dump_material(m):
    if m is None: return None
    out = []
    if m.use_nodes and m.node_tree: _dump_tree(m.node_tree, out)
    return {"name": m.name, "sig": hashlib.md5("\n".join(out).encode()).hexdigest()[:10], "lines": out}


def dump_room_materials():
    """{object name: [material dump per slot]} for room shells and skirting; python's multi-material wall is split into wall / wall.alt"""
    res = {}
    for o in bpy.data.objects:
        if o.type != "MESH": continue
        if re.fullmatch(r"[a-z\-]+_\d+/\d+\.(wall|floor|ceiling)(\.alt)?", o.name) or o.name.startswith("skirting"):
            mats = [dump_material(s.material) for s in o.material_slots]
            if o.name.endswith(".wall") and len(mats) > 1:
                # python: slot 1 holds the "alternative" faces; only count it if some face uses it
                used = {p.material_index for p in o.data.polygons}
                res[o.name] = mats[:1]
                if 1 in used: res[o.name + ".alt"] = mats[1:2]
            else:
                res[o.name] = mats[:1]
    return res
