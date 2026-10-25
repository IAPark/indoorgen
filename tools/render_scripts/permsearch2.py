"""usage: permsearch2.py ref_dump.json scene_seed stage -- per room, which set orders reproduce its ref material exactly"""
import sys, json, os, itertools, re
import bpy
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import roommats
ref = json.load(open(sys.argv[1])); seed = int(sys.argv[2]); stage = sys.argv[3]; nsc = os.path.basename(sys.argv[1])
ORD = ["living-room_0/0", "bathroom_0/0", "bedroom_0/0", "dining-room_0/0", "kitchen_0/0"]
rooms = [r for r in ORD if r + ".wall" in ref]
parts = {"walls": ("wall", "wall.alt"), "floors": ("floor",), "ceilings": ("ceiling",)}[stage]
def mk(name):
    me = bpy.data.meshes.new(name); me.from_pydata([(0,0,0),(1,0,0),(1,1,0),(0,1,0)], [], [(0,1,2,3)])
    o = bpy.data.objects.new(name, me); bpy.context.collection.objects.link(o)
best = {}
joint = []
for perm in itertools.permutations(range(len(rooms))):
    for o in list(bpy.data.objects): bpy.data.objects.remove(o)
    for m in list(bpy.data.materials): bpy.data.materials.remove(m)
    bpy.data.orphans_purge(do_recursive=True)
    for r in rooms:
        for p in parts:
            if p == "wall.alt" and f"{r}.wall.alt" not in ref: continue
            mk(f"{r}.{p}")
    roommats.ORDER["perm"] = {stage: perm}
    log = roommats.apply_room_materials(seed, [f"{r}.{p}" for r in rooms for p in parts], stages=(stage,))
    d = roommats.dump_room_materials()
    nex = 0; tot = 0
    for r in rooms:
        for p in parts:
            k = f"{r}.{p}"
            if k not in ref: continue
            have = d[k][0] if k in d and d[k][0] else None
            tot += 1; nex += bool(have and have['sig'] == ref[k][0]['sig'])
            sc = len(set(have["lines"]) & set(ref[k][0]["lines"])) if have else -1
            if sc > best.get(k, (-1,))[0]: best[k] = (sc, list(perm), len(ref[k][0]["lines"]), have["sig"] == ref[k][0]["sig"] if have else False, have["name"] if have else None, ref[k][0]["name"])
    if stage == 'walls':
        for r in rooms: nex += (log.get(f'{r}.wall', 'none') != 'none') == (f'{r}.wall.alt' in ref)
    joint.append((nex, list(perm)))
m = max(j[0] for j in joint)
tot = sum(1 for r in rooms for p in parts if f'{r}.{p}' in ref) + (len(rooms) if stage == 'walls' else 0)
print(json.dumps({'scene': nsc, 'stage': stage, 'rooms': len(rooms), 'tot': tot, 'max_exact': m, 'perms': [j[1] for j in joint if j[0] == m][:8], 'n_perms_max': sum(j[0] == m for j in joint), 'log_example': None}), flush=True)
for k, v in best.items(): print(nsc, stage, k, v, flush=True)
