"""creaturedump.py FSEED [OUTDIR]: replay NatureShelfTrinketsFactory(FSEED) (a Carnivore/Herbivore seed) the way refcache does and dump
per-stage creature data as JSON: parts (class, params, local mesh, skeleton, extras), world transforms after attach, joined mesh.
OUTDIR default tmp/cd/<fseed>/ ; files: parts.json, joined.json"""
import sys, os, json, importlib
import numpy as np

class _D:
    def __init__(self, *a, **k): pass
    def __call__(self, *a, **k):
        import bpy
        return bpy.data.materials.new("stub")
    def __getattr__(self, n):
        if n.startswith("__"): raise AttributeError(n)
        return _D()

def main():
    a = sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else sys.argv[1:]
    fs = int(a[0]); out = a[1] if len(a) > 1 else f"tmp/cd/{fs}"
    os.makedirs(out, exist_ok=True)
    import bpy
    import infinigen.core.util.random as R
    _orig = R.weighted_sample
    def _ws(*a, **k):
        try: _orig(*a, **k)
        except Exception: pass
        return _D
    R.weighted_sample = _ws
    import infinigen.core.surface as S
    S.assign_material = lambda *a, **k: None
    from infinigen.assets.objects.elements.nature_shelf_trinkets.generate import NatureShelfTrinketsFactory
    from infinigen.assets.objects.creatures.util import creature, joining
    import infinigen.assets.objects.creatures.carnivore as carn
    import infinigen.assets.objects.creatures.herbivore as herb

    def tolist(x):
        if isinstance(x, np.ndarray): return x.tolist()
        if isinstance(x, dict): return {k: tolist(v) for k, v in x.items()}
        if isinstance(x, (list, tuple)): return [tolist(v) for v in x]
        if isinstance(x, (np.floating, np.integer)): return x.item()
        return x

    def meshdump(o):
        dg = bpy.context.evaluated_depsgraph_get()
        e = o.evaluated_get(dg); m = e.to_mesh()
        n = len(m.vertices)
        co = np.empty(n * 3, dtype=np.float32); m.vertices.foreach_get("co", co)
        ls = np.empty(len(m.polygons), dtype=np.int32); m.polygons.foreach_get("loop_total", ls)
        lv = np.empty(len(m.loops), dtype=np.int32); m.loops.foreach_get("vertex_index", lv)
        d = dict(name=o.name, co=co.tolist(), sizes=ls.tolist(), idx=lv.tolist(), mw=np.array(o.matrix_world).tolist(), loc=list(o.location), rot=list(o.rotation_euler), scale=list(o.scale))
        e.to_mesh_clear()
        return d

    partlog = []
    orig_call = creature.PartFactory.__call__
    def call(self, rand=0):
        p = orig_call(self, rand)
        rec = dict(cls=self.__class__.__name__, params=tolist(self.params), skeleton=tolist(p.skeleton), mesh=meshdump(p.obj),
                   extras=[meshdump(c) for c in p.obj.children if c.type == "MESH"], base=meshdump(p.attach_basemesh) if p.attach_basemesh else None)
        partlog.append(rec)
        return p
    creature.PartFactory.__call__ = call
    for k in list(vars(creature.PartFactory.__mro__[0]).keys()): pass
    # subclasses that override __call__? none expected
    gtc = creature.genome_to_creature
    posed = []
    def g2c(genome, name):
        root, parts = gtc(genome, name)
        atts = [dict(coord=tolist(g.att.coord) if g.att else None, rest=tolist(g.att.joint.rest) if g.att else None, side=g.att.side if g.att else None, basis=g.att.rotation_basis if g.att else None) for g in __import__("infinigen.assets.objects.creatures.util.tree", fromlist=["x"]).iter_items(genome.parts)]
        for i, p in enumerate(parts):
            posed.append(dict(i=i, mesh=meshdump(p.obj), skeleton_global=tolist(p.skeleton_global()), side=p.side,
                              extras=[meshdump(c) for c in p.obj.children if c.type == "MESH"], att=atts[i]))
        return root, parts
    creature.genome_to_creature = g2c
    carn.creature.genome_to_creature = g2c
    herb.creature.genome_to_creature = g2c
    joined_log = []
    jr = joining.join_and_rig_parts
    def jrp(*a, **k):
        r = jr(*a, **k)
        joined_log.append(meshdump(r[0]))
        return r
    joining.join_and_rig_parts = jrp
    carn.joining.join_and_rig_parts = jrp
    herb.joining.join_and_rig_parts = jrp

    bpy.ops.wm.read_factory_settings(use_empty=True)
    np.random.seed(fs)
    f = NatureShelfTrinketsFactory(fs)
    print("base", type(f.base_factory).__name__)
    o = f.spawn_asset(0)
    json.dump(dict(parts=partlog, posed=posed), open(out + "/parts.json", "w"))
    json.dump(joined_log[0], open(out + "/joined.json", "w"))
    print("DUMP", fs, len(partlog), "parts", len(joined_log[0]["co"]) // 3, "joined verts")

main()
