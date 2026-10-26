"""cloth_only.py SCENE.glb OUT.cloth.glb : run ONLY the Blender cloth jobs of a Rust scene (sidecar SCENE.json "cloth") and write the
simulated / moved nodes to a small overlay glb (same world Z-up convention as the scene glb). No materials, no render, no scene import:
the needed meshes are read straight from the glb with numpy (memory-mapped), so it costs the sim time + ~10 s of Blender/infinigen startup.
Preview it with `tools/preview.sh SCENE.glb --cloth OUT.cloth.glb --person 0 out.png` (the overlay replaces the nodes of the same name).
Also prints a body-vs-cloth penetration report (cloth vertices inside person meshes, depth from the nearest body surface).
Run with the Blender-5.2 venv: ~/work/infinigen-ref/.venv52/bin/python tools/cloth_only.py SCENE.glb OUT.cloth.glb
(tools/cloth_only.sh wraps this). Env: INFINIGEN_RS_TOOLS = dir with cloth_jobs.py (default: this file's dir)."""
import os, sys, json, struct, time
import numpy as np

sys.path.insert(0, os.environ.get("INFINIGEN_RS_TOOLS") or os.path.dirname(os.path.abspath(__file__)))

_CT = {5121: np.uint8, 5123: np.uint16, 5125: np.uint32, 5126: np.float32}
_NC = {"SCALAR": 1, "VEC2": 2, "VEC3": 3, "VEC4": 4}


class Glb:
    def __init__(self, path):
        f = open(path, "rb")
        h = f.read(20)
        jl = struct.unpack("<I", h[12:16])[0]
        f.seek(20)
        self.j = json.loads(f.read(jl))
        self.bin_off = 20 + jl + 8
        self.mm = np.memmap(path, dtype=np.uint8, mode="r")
        self.by_name = {}
        for n in self.j["nodes"]:
            if n.get("mesh") is not None:
                self.by_name.setdefault(n.get("name", ""), []).append(n)

    def acc(self, ai):
        a = self.j["accessors"][ai]
        bv = self.j["bufferViews"][a["bufferView"]]
        dt = np.dtype(_CT[a["componentType"]])
        nc = _NC[a["type"]]
        off = self.bin_off + bv.get("byteOffset", 0) + a.get("byteOffset", 0)
        stride = bv.get("byteStride") or dt.itemsize * nc
        n = a["count"]
        if stride == dt.itemsize * nc:
            return np.frombuffer(self.mm[off:off + n * stride], dtype=dt).reshape(n, nc).copy()
        raw = np.frombuffer(self.mm[off:off + n * stride], dtype=np.uint8).reshape(n, stride)[:, :dt.itemsize * nc]
        return np.ascontiguousarray(raw).view(dt).reshape(n, nc)

    def prims(self, name):
        """list of dicts (pos, idx, nrm, uv) of every primitive of the nodes called `name`"""
        out = []
        for n in self.by_name.get(name, []):
            for p in self.j["meshes"][n["mesh"]]["primitives"]:
                at = p["attributes"]
                out.append({"pos": self.acc(at["POSITION"]).astype(np.float64), "idx": self.acc(p["indices"]).reshape(-1, 3).astype(np.int64),
                            "nrm": self.acc(at["NORMAL"]) if "NORMAL" in at else None, "uv": self.acc(at["TEXCOORD_0"]) if "TEXCOORD_0" in at else None})
        return out

    def mesh(self, name):
        ps = self.prims(name)
        if not ps:
            raise KeyError(name)
        v, f, o = [], [], 0
        for p in ps:
            v.append(p["pos"]); f.append(p["idx"] + o); o += len(p["pos"])
        return np.concatenate(v), np.concatenate(f)


def vertex_normals(v, f):
    n = np.zeros_like(v)
    fn = np.cross(v[f[:, 1]] - v[f[:, 0]], v[f[:, 2]] - v[f[:, 0]])
    for k in range(3):
        np.add.at(n, f[:, k], fn)
    l = np.linalg.norm(n, axis=1, keepdims=True)
    return (n / np.maximum(l, 1e-12)).astype(np.float32)


def write_glb(path, nodes):
    """nodes: [(name, verts Nx3, tris Mx3, normals|None, uv|None)] -> minimal glb"""
    bin_, views, accs, meshes, nds = bytearray(), [], [], [], []

    def add(arr, target, typ, ct, mn=None, mx=None):
        while len(bin_) % 4:
            bin_.append(0)
        views.append({"buffer": 0, "byteOffset": len(bin_), "byteLength": arr.nbytes, "target": target})
        bin_.extend(arr.tobytes())
        a = {"bufferView": len(views) - 1, "componentType": ct, "count": len(arr), "type": typ}
        if mn is not None:
            a["min"], a["max"] = mn, mx
        accs.append(a)
        return len(accs) - 1

    for name, v, f, nrm, uv in nodes:
        v = np.asarray(v, dtype=np.float32)
        at = {"POSITION": add(v, 34962, "VEC3", 5126, v.min(0).tolist(), v.max(0).tolist())}
        at["NORMAL"] = add(np.asarray(nrm if nrm is not None else vertex_normals(v.astype(np.float64), f), dtype=np.float32), 34962, "VEC3", 5126)
        if uv is not None:
            at["TEXCOORD_0"] = add(np.asarray(uv, dtype=np.float32), 34962, "VEC2", 5126)
        ia = add(np.asarray(f, dtype=np.uint32).reshape(-1), 34963, "SCALAR", 5125)
        meshes.append({"primitives": [{"attributes": at, "indices": ia}]})
        nds.append({"name": name, "mesh": len(meshes) - 1})
    while len(bin_) % 4:
        bin_.append(0)
    j = {"asset": {"version": "2.0"}, "scene": 0, "scenes": [{"nodes": list(range(len(nds)))}], "nodes": nds, "meshes": meshes, "accessors": accs,
         "bufferViews": views, "buffers": [{"byteLength": len(bin_)}]}
    js = json.dumps(j).encode()
    js += b" " * ((4 - len(js) % 4) % 4)
    with open(path, "wb") as fo:
        fo.write(b"glTF" + struct.pack("<II", 2, 12 + 8 + len(js) + 8 + len(bin_)))
        fo.write(struct.pack("<I", len(js)) + b"JSON" + js)
        fo.write(struct.pack("<I", len(bin_)) + b"BIN\0" + bytes(bin_))


SIGN = 1.0
NEAR = 0.06  # only a vertex within 6 cm of a body surface can be 'inside' it (the cloth never wraps deeper than that)


def penetration(cloth, bodies, thr=(0.005, 0.01, 0.02)):
    """cloth vertices inside the body meshes: depth = distance to the nearest body surface for vertices on the inner side"""
    import bpy
    from mathutils import Vector
    from mathutils.bvhtree import BVHTree
    trees = [BVHTree.FromPolygons([tuple(p) for p in v], [tuple(map(int, t)) for t in f]) for v, f in bodies]
    depth = np.zeros(len(cloth))
    for i, p in enumerate(cloth):
        pv = Vector(p)
        best = 0.0
        for t in trees:
            hit = t.find_nearest(pv)
            if hit[0] is None:
                continue
            d = (pv - hit[0]).length
            if d < NEAR and (pv - hit[0]).dot(hit[1]) * SIGN < 0:
                best = max(best, d)
        depth[i] = best
    return depth


def main():
    global SIGN
    SIGN = float(os.environ.get("PEN_SIGN", "1"))
    glb_path, out = sys.argv[1], sys.argv[2]
    t0 = time.time()
    g = Glb(glb_path)
    side = json.load(open(glb_path.replace(".glb", ".json")))
    jobs = side.get("cloth") or []
    if not jobs:
        print("no cloth jobs")
        return 0
    store = {}

    def world_mesh(name):
        if name not in store:
            store[name] = g.mesh(name)
        v, f = store[name]
        return v, [list(map(int, t)) for t in f]

    changed = {}

    def set_mesh(name, v, f):
        f = np.array([p for p in f], dtype=object)
        tris = []
        for p in f:  # quads (after subsurf) -> triangles
            p = list(p)
            for k in range(1, len(p) - 1):
                tris.append([p[0], p[k], p[k + 1]])
        store[name] = (np.asarray(v, dtype=np.float64), np.array(tris, dtype=np.int64))
        changed[name] = True

    if "--pen-only" in sys.argv:  # re-check an existing overlay without simulating
        og = Glb(out)
        for name in og.by_name:
            store[name] = og.mesh(name)
    else:
        import cloth_jobs
        cloth_jobs.run_jobs(jobs, world_mesh, set_mesh)
        print("cloth jobs done %.1fs" % (time.time() - t0))
    nodes = []
    for name in ([] if "--pen-only" in sys.argv else changed):
        v, f = store[name]
        orig = g.prims(name)
        nrm = uv = None
        if len(orig) == 1 and len(orig[0]["pos"]) == len(v):  # moved only (follow): keep the original shading data
            nrm, uv = orig[0]["nrm"], orig[0]["uv"]
        nodes.append((name, v, f, nrm, uv))
    if nodes:
        write_glb(out, nodes)
        print("wrote", out, len(nodes), "nodes")
    # body-vs-cloth penetration
    bodies = []
    for j in jobs:
        if j["kind"] == "cloth":
            bodies = [world_mesh(n) for n in j["collide"] if n.startswith("person")]
            if not bodies:
                continue
            v, f = store[j["target"]]
            d = penetration(v, bodies)
            print("CLOTHPEN %s verts %d  max %.3f  >5mm %d  >1cm %d  >2cm %d" % (j["target"], len(v), d.max(), (d > 0.005).sum(), (d > 0.01).sum(), (d > 0.02).sum()))
    return 0


if __name__ == "__main__":
    sys.exit(main())
