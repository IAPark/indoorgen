#!/usr/bin/env python3
"""layold.py H.glb OUT.old.json: today's layout as boxes for the side-by-side in layview.py. Per object (sidecar H.json): class, room, world box of its nodes."""
import json, struct, sys
glb, out = sys.argv[1], sys.argv[2]
f = open(glb, "rb"); f.read(12); l, _ = struct.unpack("<II", f.read(8)); g = json.loads(f.read(l))
side = json.load(open(glb[:-4] + ".json"))
objs = {o["key"]: o for o in side["objects"]}
boxes = {}
for n in g["nodes"]:
    nm = n.get("name", ""); k = nm.split("#")[0]
    if "mesh" not in n or k not in objs or objs[k]["class"] in ("Door", "Window", "CeilingLightFactory") or objs[k].get("parent"): continue
    for p in g["meshes"][n["mesh"]]["primitives"]:
        a = g["accessors"][p["attributes"]["POSITION"]]
        b = boxes.setdefault(k, [list(a["min"]), list(a["max"])])
        for d in range(3): b[0][d] = min(b[0][d], a["min"][d]); b[1][d] = max(b[1][d], a["max"][d])
json.dump([{"key": k, "class": objs[k]["class"], "room": objs[k]["room"], "lo": b[0], "hi": b[1]} for k, b in boxes.items()], open(out, "w"))
print(out, len(boxes))
