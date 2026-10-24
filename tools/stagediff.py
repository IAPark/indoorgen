"""stagediff.py py_stages.json key rust_dump.json : list python vertices farthest from the rust mesh (vertex-to-vertex distance), and vice versa"""
import json,sys,numpy as np
a=json.load(open(sys.argv[1]))[sys.argv[2]]; b=json.load(open(sys.argv[3]))
A=np.array(a['v']); B=np.array(b['v'])
from scipy.spatial import cKDTree
def nn(X,Y):
    return cKDTree(Y).query(X)[0]
da=nn(A,B); db=nn(B,A)
print('py->rs vertex dist: max %.4g count>1e-4: %d / %d'%(da.max(),(da>1e-4).sum(),len(A)))
print('rs->py vertex dist: max %.4g count>1e-4: %d / %d'%(db.max(),(db>1e-4).sum(),len(B)))
for nm,X,d in (('py',A,da),('rs',B,db)):
    idx=np.argsort(-d)[:8]
    for i in idx: print(nm,i,X[i].round(5),'%.4g'%d[i])
