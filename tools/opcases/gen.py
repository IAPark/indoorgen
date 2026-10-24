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
def ng(n, r, z, a, sx=1.0):
    return [[sx*r*math.cos(a+2*math.pi*k/n), r*math.sin(a+2*math.pi*k/n), z] for k in range(n)]
def loops_case(counts, rs, zs, angs, faces=False):
    V = []; L = []
    for n, r, z, a in zip(counts, rs, zs, angs):
        L.append(list(range(len(V), len(V)+n))); V += ng(n, r, z, a)
    E = [[l[i], l[(i+1) % len(l)]] for l in L for i in range(len(l))]
    return {"verts": V, "polys": [], "edges": E, "loops": L}
cases['bridge_uneq48'] = loops_case([4, 8], [0.5, 1.0], [0, 0.7], [0.2, 0.0])
cases['bridge_uneq610'] = loops_case([6, 10], [0.6, 1.0], [0, 0.9], [0.0, 0.3])
cases['bridge_uneq58'] = loops_case([5, 8], [0.7, 0.9], [0, 0.4], [0.5, 0.1])
cases['bridge_uneq3_7'] = loops_case([3, 7], [0.6, 1.0], [0.1, 0.8], [0.0, 0.0])
# two open-ended square tubes facing each other (faces exist on the tube walls)
def tube(z0, z1, a):
    V = [[0.5*math.cos(a+k*math.pi/2)*1.414, 0.5*math.sin(a+k*math.pi/2)*1.414, z] for z in (z0, z1) for k in range(4)]
    P = [[k, (k+1) % 4, 4+(k+1) % 4, 4+k] for k in range(4)]
    return V, P
V1, P1 = tube(0.0, 0.5, 0.0); V2, P2 = tube(1.2, 1.7, 0.4)
cases['bridge_tubes'] = {"verts": V1 + V2, "polys": P1 + [[v+8 for v in f] for f in P2], "loops": [[4,5,6,7], [8,9,10,11]]}
# open polylines (ladder)
cases['bridge_open'] = {"verts": [[x*0.5, 0, 0] for x in range(5)] + [[x*0.5, 1.0, 0.3*x] for x in range(5)], "polys": [],
                        "edges": [[i, i+1] for i in range(4)] + [[5+i, 6+i] for i in range(4)], "loops": [[0,1,2,3,4], [5,6,7,8,9]]}
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
gt = [[v[0], v[1], random.uniform(-0.15, 0.15)] for v in cases['grid']['verts']]
tp = []
for f in cases['grid']['polys']:
    a, b, c, d = f
    tp += [[a, b, c], [a, c, d]] if (a + b) % 2 == 0 else [[a, b, d], [b, c, d]]
cases['grid_tri'] = {"verts": gt, "polys": tp}
cases['fold_tri'] = {"verts": cases['fold']['verts'], "polys": [t for f in cases['fold']['polys'] for t in ([f[0], f[1], f[2]], [f[0], f[2], f[3]])]}
cases['pts'] = {"verts": [[random.uniform(-1,1), random.uniform(-1,1), random.uniform(-1,1)] for _ in range(24)], "polys": []}
for k, v in cases.items():
    v["name"] = k
    json.dump(v, open(os.path.join(here, k + ".json"), "w"))
print(len(cases), "cases")
