"""subsel.py stages.json key out.json cx cy cz r : faces of stage `key` whose centre is within r of c (vertices compacted)"""
import sys, json, numpy as np
s = json.load(open(sys.argv[1]))[sys.argv[2]]
V = np.array(s["v"]); c = np.array([float(x) for x in sys.argv[4:7]]); r = float(sys.argv[7])
keep = [p for p in s["p"] if np.linalg.norm(V[p].mean(0) - c) < r]
used = sorted({i for p in keep for i in p}); mp = {o: n for n, o in enumerate(used)}
json.dump({"v": [list(map(float, V[i])) for i in used], "p": [[mp[i] for i in p] for p in keep]}, open(sys.argv[3], "w"))
print(len(used), len(keep))
