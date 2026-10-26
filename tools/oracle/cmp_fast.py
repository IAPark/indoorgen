"""One pass over both traces: first divergence in iteration RNG / chosen move / object states / POSTVALID / EVAL terms."""
import re, sys
def parse(path, rs):
    its=[]; cur=None; stage=-1
    for l in open(path, errors="ignore"):
        if rs:
            if not l.startswith("RT"): continue
            l=l[2:]
        l=l.strip()
        if l.startswith("STAGE"): stage+=1; continue
        m=re.match(r"ITER (\d+) choose peek (\d+)", l)
        if m: cur={"stage":stage,"it":int(m[1]),"obj":{},"pv":[],"ev":[],"chose":None,"end":None}; its.append(cur); continue
        if cur is None: continue
        m=re.match(r"OBJ (\S+) (.*)", l)
        if m: cur["obj"][m[1]]=[float(x) for x in m[2].split()]; continue
        m=re.match(r"POSTVALID (\S+) (True|False|true|false) \[([-\d.e, ]+)\]", l)
        if m: cur["pv"].append((m[1], m[2].lower()=="true", tuple(round(float(x),4) for x in m[3].split(",")))); continue
        m=re.match(r"EVAL (.*)", l)
        if m:
            d={}
            for tok in m[1].split():
                k,v=tok.rsplit(":",1); d[k]=float(v)
            cur["ev"].append(d); continue
        m=re.match(r"chose (\w+)", l)
        if m: cur["chose"]=m[1].replace("propose_","").replace("_","").lower().replace("relationplanechange","planechange"); continue
        m=re.match(r"end peek (\d+)", l)
        if m: cur["end"]=int(m[1])
    return its
py=parse(sys.argv[1],False); rs=parse(sys.argv[2],True)
n=0
for a,b in zip(py,rs):
    tag=f"stage {a['stage']} it {a['it']}"
    if a["chose"]!=b["chose"]: print(tag,"CHOSE differs",a["chose"],b["chose"]); break
    if set(a["obj"])!=set(b["obj"]): print(tag,"object set differs py-only",sorted(set(a["obj"])-set(b["obj"])),"rs-only",sorted(set(b["obj"])-set(a["obj"]))); break
    bad=[(k,a["obj"][k],b["obj"][k]) for k in a["obj"] if any(abs(x-y)>(1e-5 if i<6 else 2e-8) for i,(x,y) in enumerate(zip(a["obj"][k],b["obj"][k])))]
    if bad:
        print(tag,"object state differs:")
        for k,x,y in bad[:4]: print("  ",k,"\n    py",x,"\n    rs",y)
        break
    if a["pv"]!=b["pv"]:
        print(tag,"POSTVALID differs"); 
        for x in a["pv"][:8]: print("   py",x)
        for x in b["pv"][:8]: print("   rs",x)
        break
    dd=None
    for i,(ea,eb) in enumerate(zip(a["ev"],b["ev"])):
        diffs=[(k,ea[k],eb.get(k)) for k in ea if eb.get(k) is None or abs(ea[k]-eb[k])>2e-4]
        if diffs: dd=(i,diffs); break
    if dd: print(tag,"EVAL term differs (eval #%d):"%dd[0], dd[1]); break
    if a["end"]!=b["end"]: print(tag,"RNG end differs",a["end"],b["end"]); break
    n+=1
else:
    print("identical through all", n, "iterations")
print("iterations matched:", n, "(py %d, rs %d)"%(len(py),len(rs)))
