"""holediff.py py.json(rs stage name) rs.json : vertices of each mesh without a partner (<1e-5) in the other"""
import json,sys,numpy as np
from scipy.spatial import cKDTree
py=json.load(open(sys.argv[1]))[sys.argv[2]]; rs=json.load(open(sys.argv[3]))
pv=np.array(py['v']);rv=np.array(rs['v'])
pv=pv[pv[:,0]>1.0];rv=rv[rv[:,0]>1.0]
d1,_=cKDTree(rv).query(pv);d2,_=cKDTree(pv).query(rv)
print("py unmatched",(d1>1e-5).sum(),"rs unmatched",(d2>1e-5).sum())
for v in pv[d1>1e-5][:15]: print("py",np.round(v,5))
for v in rv[d2>1e-5][:15]: print("rs",np.round(v,5))
