"""bitcmp.py a.json b.json : fraction of vertices whose float32 coordinates are bit-identical (verts lists, same order)"""
import json,sys,struct
a=json.load(open(sys.argv[1]))['verts']; b=json.load(open(sys.argv[2]))['verts']
import array
def f32(x): return struct.unpack('f',struct.pack('f',x))[0]
n=len(a); ex=sum(1 for p,q in zip(a,b) if all(f32(x)==f32(y) for x,y in zip(p,q)))
print(len(a),len(b),"bit-identical %d (%.2f%%)"%(ex,100*ex/max(1,n)))
