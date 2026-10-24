"""facediff.py py.json stage rs.json : faces (as vertex-position tuples) present in only one mesh, restricted to non-quad-or-changed faces"""
import json,sys,numpy as np
from scipy.spatial import cKDTree
py=json.load(open(sys.argv[1]))[sys.argv[2]]; rs=json.load(open(sys.argv[3]))
pv=np.array(py['v']);rv=np.array(rs['v'])
# canonical ids: snap to python vertices where within 1e-5 else own
t=cKDTree(pv)
d,i=t.query(rv)
ids_r=[int(i[k]) if d[k]<1e-5 else -(k+1) for k in range(len(rv))]
def canon(f):
    f=list(f); k=f.index(min(f)); return tuple(f[k:]+f[:k])
A={canon(f) for f in py['p']}
B={canon([ids_r[v] for v in f]) for f in rs['p']}
print("py faces",len(A),"rs faces",len(B),"py-only",len(A-B),"rs-only",len(B-A))
def show(nm,S,V,mapid):
    for f in sorted(S,key=len)[:int(sys.argv[4]) if len(sys.argv)>4 else 40]:
        print(nm,len(f),[tuple(np.round(mapid(v),4)) for v in f])
show("py",A-B,pv,lambda v:pv[v])
show("rs",B-A,rv,lambda v: pv[v] if v>=0 else rv[-v-1])
