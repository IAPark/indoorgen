"""Compare room wall/floor/ceiling material families Rust vs python.
usage: roommatcmp.py pymats.json dir_with_glb_sidecars   (pymats.json from tmp/dump_all.py style dump: {case:{order,objs:{name:{mats,faces}}}})"""
import json, sys, os
py = json.load(open(sys.argv[1])); d = sys.argv[2]
def pfam(n):
    if n is None: return "?"
    if n.startswith("shader_plaster"): return "Plaster"
    if "_tile_tile" in n: return "Tile"
    if n.startswith("shader_hardwood"): return "Hardwood"
    if "concrete" in n: return "Concrete"
    if "brick" in n: return "Brick"
    if "rug" in n: return "Rug"
    return n.split(".")[0]
def rfam(c):
    if c in ("ceramic.Plaster",): return "Plaster"
    if c == "wood.HardwoodFloor": return "Hardwood"
    if c == "ceramic.Tile" or c.startswith("wood.") : return "Tile"
    return {"ceramic.Concrete": "Concrete", "ceramic.Brick": "Brick", "fabric.Rug": "Rug"}.get(c, c)
tot = {k: [0, 0] for k in ("wall", "floor", "ceiling")}
alt = [0, 0, 0, 0]  # rooms: py with alt, rust with alt, both, total
for case, v in sorted(py.items()):
    f = os.path.join(d, case + ".json")
    if not os.path.exists(f): continue
    nodes = {n["name"]: n for n in json.load(open(f))["nodes"]}
    line = []
    for r in v["order"]:
        for part in ("wall", "floor", "ceiling"):
            nm = f"{r}.{part}"
            if nm not in v["objs"] or nm not in nodes: continue
            pm = pfam(v["objs"][nm]["mats"][0]); rm = rfam(nodes[nm]["spec"]["cls"])
            tot[part][1] += 1; tot[part][0] += pm == rm
            if pm != rm: line.append(f"{nm}: py {pm} rust {rm}")
        pa = len(v["objs"][f"{r}.wall"]["mats"]) > 1; ra = f"{r}.wall.alt" in nodes
        alt[0] += pa; alt[1] += ra; alt[2] += pa and ra; alt[3] += 1
    print(case, "; ".join(line) or "all classes match")
print({k: f"{a}/{b}" for k, (a, b) in tot.items()}, f"rooms with alt wall: python {alt[0]} rust {alt[1]} both {alt[2]} of {alt[3]}")
