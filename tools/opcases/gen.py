"""gen.py : regenerates the small regression meshes tools/opcases/*.json (plain python, no bpy).  Case format: header of tools/opcheck.py."""
import json, math, os, random
here = os.path.dirname(os.path.abspath(__file__))
cube = [[-1,-1,-1],[1,-1,-1],[-1,1,-1],[1,1,-1],[-1,-1,1],[1,-1,1],[-1,1,1],[1,1,1]]
P = [[0,2,3,1],[4,5,7,6],[0,1,5,4],[2,6,7,3],[0,4,6,2],[1,3,7,5]]
def sc(V, s): return [[a*s[0], b*s[1], c*s[2]] for a, b, c in V]
cases = {}
cases['cube'] = {"verts": sc(cube, [.5]*3), "polys": P}
cases['slab'] = {"verts": sc(cube, [1, 1, .05]), "polys": P}
cases['sheared'] = {"verts": [[x+0.4*z, y, z] for x, y, z in sc(cube, [.5, .4, .3])], "polys": P}
L = [[0,0],[2,0],[2,1],[1,1],[1,2],[0,2]]; n = 6
cases['lshape'] = {"verts": [[x,y,0] for x,y in L] + [[x,y,0.5] for x,y in L],
                   "polys": [list(reversed(range(n))), [n+i for i in range(n)]] + [[i,(i+1)%n,n+(i+1)%n,n+i] for i in range(n)]}
m = 8
V = [[r*math.cos(2*math.pi*i/m), r*math.sin(2*math.pi*i/m), z] for z in (0, 0.3) for r in (1.0, 0.6) for i in range(m)]
vi = lambda z, r, i: (z*2+r)*m + i % m
ring_faces = {"outer": [], "inner": [], "top": [], "bottom": []}
for i in range(m):
    ring_faces["outer"].append([vi(0,0,i+1), vi(0,0,i), vi(1,0,i), vi(1,0,i+1)])
    ring_faces["inner"].append([vi(0,1,i), vi(0,1,i+1), vi(1,1,i+1), vi(1,1,i)])
    ring_faces["top"].append([vi(1,0,i), vi(1,1,i), vi(1,1,i+1), vi(1,0,i+1)])
    ring_faces["bottom"].append([vi(0,0,i), vi(0,0,i+1), vi(0,1,i+1), vi(0,1,i)])
cases['ring'] = {"verts": V, "polys": sum(ring_faces.values(), [])}
cases['ring_open'] = {"verts": V, "polys": ring_faces["outer"] + ring_faces["inner"] + ring_faces["bottom"],
                      "loops": [[vi(1,0,i) for i in range(m)], [vi(1,1,i) for i in range(m)]]}
cases['fold'] = {"verts": [[x,0,0] for x in range(4)] + [[x,1,0] for x in range(4)] + [[x,1.5,0.8] for x in range(4)],
                 "polys": [[i,i+1,5+i,4+i] for i in range(3)] + [[4+i,5+i,9+i,8+i] for i in range(3)]}
sq = lambda s, z, a=0.0: [[s*math.cos(a+math.pi/4+k*math.pi/2)*1.414, s*math.sin(a+math.pi/4+k*math.pi/2)*1.414, z] for k in range(4)]
cases['bridge_sq'] = {"verts": sq(.5, 0) + sq(.8, 1, 0.3), "polys": [], "edges": [[i,(i+1)%4] for i in range(4)] + [[4+i,4+(i+1)%4] for i in range(4)],
                      "loops": [[0,1,2,3],[4,5,6,7]]}
o8 = lambda r, z, a: [[r*math.cos(a+2*math.pi*k/8), r*math.sin(a+2*math.pi*k/8), z] for k in range(8)]
cases['bridge_oct'] = {"verts": o8(.6, 0, 0) + o8(1.0, .8, 0.2), "polys": [], "edges": [[i,(i+1)%8] for i in range(8)] + [[8+i,8+(i+1)%8] for i in range(8)],
                       "loops": [list(range(8)), list(range(8, 16))]}
cases['bridge_cone'] = {"verts": o8(1.0, 0.3, 0) + o8(0.6, 0, 0), "polys": [], "edges": [[i,(i+1)%8] for i in range(8)] + [[8+i,8+(i+1)%8] for i in range(8)], "loops": [list(range(8)), list(range(8, 16))]}
rect = [[x,0,0] for x in range(4)] + [[4,y,0] for y in range(0,2)] + [[x,2,0] for x in range(4,0,-1)] + [[0,y,0] for y in range(2,0,-1)]
cases['rect12'] = {"verts": rect, "polys": [], "edges": [[i,(i+1)%12] for i in range(12)], "loops": [list(range(12))]}
cases['oct8'] = {"verts": o8(1.0, 0, 0.1), "polys": [], "edges": [[i,(i+1)%8] for i in range(8)], "loops": [list(range(8))]}
# per-face duplicated cube (remove_doubles must merge to 8 verts)
dv, dp = [], []
for f in P:
    dp.append([len(dv)+k for k in range(4)]); dv += [sc(cube, [.5]*3)[i] for i in f]
cases['dup_cube'] = {"verts": dv, "polys": dp}
cases['cube_box'] = dict(cases['cube'], other={"verts": sc(cube, [.25]*3), "polys": P, "loc": [0.35, 0.3, 0.2]})
cases['cube_hole'] = dict(cases['cube'], other={"verts": sc(cube, [.15, .15, 1.0]), "polys": P, "loc": [0.1, 0.05, 0.0]})
g = 8
cases['grid'] = {"verts": [[(i/(g-1)-.5)*2, (j/(g-1)-.5)*2, 0.0] for j in range(g) for i in range(g)],
                 "polys": [[j*g+i, j*g+i+1, (j+1)*g+i+1, (j+1)*g+i] for j in range(g-1) for i in range(g-1)]}
random.seed(3)
cases['pts'] = {"verts": [[random.uniform(-1,1), random.uniform(-1,1), random.uniform(-1,1)] for _ in range(24)], "polys": []}
for k, v in cases.items():
    v["name"] = k
    json.dump(v, open(os.path.join(here, k + ".json"), "w"))
print(len(cases), "cases")
