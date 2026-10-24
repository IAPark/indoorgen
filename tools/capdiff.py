import json,numpy as np,sys
P=json.load(open('tmp/pan0.json')); n=int(sys.argv[1]); r=np.pi/n
def rot(v,a):
    c,s=np.cos(a),np.sin(a); return np.array([c*v[0]-s*v[1],s*v[0]+c*v[1],v[2]])
py=P['filled']; rs=json.load(open('tmp/pd/rs_filled.json'))
pv=np.array([rot(v,r) for v in py['v']]); rv=np.array(rs['v'])
def edges(v,faces):
    E=set()
    for f in faces:
        if all(abs(v[i][2])<1e-6 for i in f):
            for i in range(len(f)):
                a,b=f[i],f[(i+1)%len(f)]
                ka=tuple(np.round(v[a],4));kb=tuple(np.round(v[b],4))
                E.add((min(ka,kb),max(ka,kb)))
    return E
a=edges(pv,py['p']);b=edges(rv,rs['p'])
print(len(a),len(b),len(a-b),len(b-a))
for e in sorted(a-b): print("py only",e)
for e in sorted(b-a): print("rs only",e)
