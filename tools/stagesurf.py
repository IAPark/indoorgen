"""stagesurf.py py_stages.json key rust_dump.json [xmin] : two-way surface distance (python verts+edge points -> rust triangles) restricted to x>xmin; lists the worst points"""
import json,sys,numpy as np, bpy, mathutils
from mathutils.bvhtree import BVHTree
a=json.load(open(sys.argv[1]))[sys.argv[2]]; b=json.load(open(sys.argv[3])); xmin=float(sys.argv[4]) if len(sys.argv)>4 else -9
def tris(V,P):
    return [(p[0],p[k],p[k+1]) for p in P for k in range(1,len(p)-1)]
def probes(V,P):
    E=set()
    for p in P:
        for i in range(len(p)):
            x,y=p[i],p[(i+1)%len(p)]; E.add((min(x,y),max(x,y)))
    E=np.array(sorted(E)).reshape(-1,2)
    return np.concatenate([V]+[V[E[:,0]]+(V[E[:,1]]-V[E[:,0]])*f for f in (0.5,)])
def dist(A,V,P):
    bvh=BVHTree.FromPolygons([tuple(v) for v in V],[tuple(int(i) for i in t) for t in tris(V,P)])
    out=np.empty(len(A))
    for i,p in enumerate(A):
        loc=bvh.find_nearest(mathutils.Vector(p))[0]; out[i]=(loc-mathutils.Vector(p)).length if loc is not None else 9
    return out
A=(np.array(a['v']),a['p']); B=(np.array(b['v']),b['p'])
for nm,X,Y in (("py->rs",A,B),("rs->py",B,A)):
    pr=probes(*X); pr=pr[pr[:,0]>xmin]
    d=dist(pr,*Y)
    print(nm,"n",len(pr),"mean %.3g p99 %.3g max %.3g n>1e-4: %d"%(d.mean(),np.percentile(d,99),d.max(),(d>1e-4).sum()))
    for i in np.argsort(-d)[:6]: print("   ",pr[i].round(4),"%.3g"%d[i])
