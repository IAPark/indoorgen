"""meshsurf.py a.json b.json [n] : two-way surface distance between two {v,p} meshes (verts + edge midpoints of one -> triangles of the other), worst points listed.
run with the venv python (needs bpy/mathutils): $PY_REF tools/meshsurf.py"""
import json,sys,numpy as np, bpy, mathutils
from mathutils.bvhtree import BVHTree
A=json.load(open(sys.argv[1])); B=json.load(open(sys.argv[2])); N=int(sys.argv[3]) if len(sys.argv)>3 else 8
def tris(P): return [(p[0],p[k],p[k+1]) for p in P for k in range(1,len(p)-1)]
def probes(V,P):
    E=set()
    for p in P:
        for i in range(len(p)):
            x,y=p[i],p[(i+1)%len(p)]; E.add((min(x,y),max(x,y)))
    E=np.array(sorted(E)).reshape(-1,2)
    return np.concatenate([V,V[E[:,0]]+(V[E[:,1]]-V[E[:,0]])*0.5])
def dist(X,V,P):
    bvh=BVHTree.FromPolygons([tuple(v) for v in V],[tuple(int(i) for i in t) for t in tris(P)])
    out=np.empty(len(X))
    for i,p in enumerate(X):
        loc=bvh.find_nearest(mathutils.Vector(p))[0]; out[i]=(loc-mathutils.Vector(p)).length if loc is not None else 9
    return out
va=np.array(A['v']); vb=np.array(B['v'])
for nm,(V1,P1),(V2,P2) in (("a->b",(va,A['p']),(vb,B['p'])),("b->a",(vb,B['p']),(va,A['p']))):
    pr=probes(V1,P1); d=dist(pr,V2,P2)
    print(nm,"n",len(pr),"mean %.3g max %.3g n>1e-3: %d"%(d.mean(),d.max(),(d>1e-3).sum()))
    for i in np.argsort(-d)[:N]: print("   ",pr[i].round(4),"%.3g"%d[i])
