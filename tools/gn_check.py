"""gn_check.py : automatic checker of the Rust geometry-nodes interpreter against Blender's own evaluation.

Run with the infinigen venv python (ONE bpy process at a time):
  gn_check.py unit [FILTER ...]            run the built-in per-node unit cases (tools/gn_units.py)
  gn_check.py group MODULE.FUNC [--n N]    export an @to_nodegroup function and check it with N random input sets
  gn_check.py asset MODULE.CLASS SEED      export an asset factory (recorded modifier steps) and check each step
  gn_check.py json FILE.json GROUP [--n N] check an exported file's group with random inputs

Options: --keep (keep the case file), --tol T, --cases PATH (case file path; default /tmp/gncases_<pid>.json), -v
The case file contains the exported groups plus, per case, the inputs and Blender's evaluated mesh (vertices, edges,
polygons, attributes). The Rust side (target/release/gncheck) evaluates the same groups and reports per-case
PASS (max error) or FAIL (first differing vertex/face/attribute element).
"""
import sys, os, json, subprocess, random, math, importlib

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import bpy
import numpy as np
import gn_export as GX
from gn_export import extract_mesh, ATTR_FIELDS

GNCHECK = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "target", "release", "gncheck")

def clean(o):
    """replace nan/inf (not valid JSON for serde) by null / +-3.4e38"""
    if isinstance(o, float):
        if o != o:
            return None
        if o in (float("inf"), float("-inf")):
            return 3.4e38 if o > 0 else -3.4e38
        return o
    if isinstance(o, list):
        return [clean(x) for x in o]
    if isinstance(o, dict):
        return {k: clean(v) for k, v in o.items()}
    return o


def dump_cases(out, path):
    json.dump(clean(out), open(path, "w"), separators=(",", ":"))


def mesh_from_json(mj):
    me = bpy.data.meshes.new("in")
    if mj:
        me.from_pydata([tuple(v) for v in mj["v"]], [tuple(e) for e in mj.get("e", [])], [tuple(f) for f in mj["f"]])
        me.update()
        for name, a in (mj.get("attrs") or {}).items():
            try:
                at = me.attributes.new(name, a["ty"], a["dom"]) if name not in me.attributes else me.attributes[name]
                fld, dim = ATTR_FIELDS[a["ty"]]
                arr = np.array(a["data"], dtype=np.float32 if a["ty"] not in ("INT", "INT8", "BOOLEAN") else (bool if a["ty"] == "BOOLEAN" else np.int32)).reshape(-1)
                if len(arr) == len(at.data) * dim:
                    at.data.foreach_set(fld, arr)
            except Exception as ex:
                print("input attribute", name, "not set:", ex)
        for mname in (mj.get("mats") or []):
            me.materials.append(bpy.data.materials.get(mname) if mname else None)
    return me


def eval_blender(ng, inputs, in_mesh=None, objects=None):
    """evaluate node group `ng` as a modifier on a mesh object; returns the case 'expect' dict"""
    me = mesh_from_json(in_mesh)
    ob = bpy.data.objects.new("o", me)
    bpy.context.scene.collection.objects.link(ob)
    mod = ob.modifiers.new("g", "NODES")
    mod.node_group = ng
    for ident, val in inputs.items():
        if isinstance(val, dict) and "object" in val:
            val = bpy.data.objects[val["object"]]
        elif isinstance(val, dict) and "id" in val:
            nm = val["id"]
            val = bpy.data.materials.get(nm) or bpy.data.objects.get(nm) or bpy.data.collections.get(nm) or bpy.data.images.get(nm)
            if val is None:
                continue
        mod[ident] = val
    dg = bpy.context.evaluated_depsgraph_get()
    dg.update()
    try:
        ev = ob.evaluated_get(dg)
        em = ev.to_mesh()
        out = extract_mesh(em)
        ev.to_mesh_clear()
    except Exception as e:
        out = {"error": repr(e)}
    bpy.data.objects.remove(ob)
    bpy.data.meshes.remove(me)
    return out


def random_inputs(ng, rnd):
    """random modifier input values for the non-geometry interface sockets"""
    d = {}
    for i in ng.interface.items_tree:
        if i.item_type != "SOCKET" or i.in_out != "INPUT":
            continue
        t = i.socket_type
        if t == "NodeSocketFloat":
            lo, hi = max(i.min_value, -5.0), min(i.max_value, 5.0)
            base = i.default_value
            d[i.identifier] = float(np.float32(rnd.choice([base, rnd.uniform(lo, hi), rnd.uniform(lo, hi)])))
        elif t == "NodeSocketInt":
            lo, hi = max(i.min_value, -3), min(i.max_value, 8)
            d[i.identifier] = int(rnd.choice([i.default_value, rnd.randint(lo, hi)]))
        elif t == "NodeSocketBool":
            d[i.identifier] = bool(rnd.choice([i.default_value, rnd.random() < 0.5]))
        elif t == "NodeSocketVector":
            lo, hi = max(i.min_value, -3.0), min(i.max_value, 3.0)
            base = list(i.default_value)
            d[i.identifier] = [float(np.float32(rnd.choice([b, rnd.uniform(lo, hi)]))) for b in base]
    return d


def run_rust(path, extra=()):
    exe = os.path.abspath(GNCHECK)
    if not os.path.exists(exe):
        print("gncheck binary missing: cargo build --release --bin gncheck")
        sys.exit(2)
    r = subprocess.run([exe, path] + list(extra), capture_output=True, text=True)
    return r.returncode, r.stdout, r.stderr


def report(out, verbose=False):
    n_pass = n_fail = 0
    n_edge = sum(1 for l in out.splitlines() if "EDGE-ORDER-DIFFERS" in l)
    if n_edge:
        print(f"NOTE {n_edge} passing cases have a different edge order than Blender (use -v to list)")
    for line in out.splitlines():
        if line.startswith("CASE"):
            if line.startswith("CASE PASS"):
                n_pass += 1
                if verbose or "SURFACE-EQUAL" in line or ("EDGE-ORDER" in line and verbose):
                    print(line)
            else:
                n_fail += 1
                print(line)
        elif line.startswith("SUMMARY"):
            pass
    print(f"RESULT pass={n_pass} fail={n_fail}")
    return n_pass, n_fail


SKIP_PROBE = ("NodeGroupInput", "NodeGroupOutput", "NodeReroute", "NodeFrame")


def _geo_in(go):
    return [i for i in go.inputs if i.type == "GEOMETRY"][0]


def build_probe_group(ng, path, sock_id):
    """deep copy of ng (and of the nested groups along `path`) whose first geometry output is node `path[-1]`.sock_id"""
    ng2 = ng.copy()
    go = [x for x in ng2.nodes if x.bl_idname == "NodeGroupOutput"][-1]
    gsock = _geo_in(go)
    for l in list(ng2.links):
        if l.to_socket == gsock:
            ng2.links.remove(l)
    node = ng2.nodes[path[0]]
    if len(path) == 1:
        ng2.links.new(node.outputs[sock_id], gsock)
    else:
        sub = build_probe_group(node.node_tree, path[1:], sock_id)
        node.node_tree = sub
        outs = [o for o in node.outputs if o.type == "GEOMETRY" and o.enabled]
        ng2.links.new(outs[0], gsock)
    return ng2


def add_tail_and_eval(ng2, case):
    """append the probe tail (Realize -> mesh | CurveToMesh | PointsToVertices) to ng2's geometry output and evaluate"""
    go = [x for x in ng2.nodes if x.bl_idname == "NodeGroupOutput"][-1]
    gsock = _geo_in(go)
    src = [l.from_socket for l in ng2.links if l.to_socket == gsock][0]
    L = ng2.links
    real = ng2.nodes.new("GeometryNodeRealizeInstances")
    L.new(src, real.inputs["Geometry"])
    L.new(real.outputs["Geometry"], gsock)
    exp = eval_blender(ng2, case["inputs"], case.get("mesh"))
    if len(exp.get("v", [])) == 0 and "error" not in exp:
        for l in list(L):
            if l.to_socket == gsock:
                L.remove(l)
        c2m = ng2.nodes.new("GeometryNodeCurveToMesh")
        L.new(real.outputs["Geometry"], c2m.inputs["Curve"])
        L.new(c2m.outputs["Mesh"], gsock)
        exp = eval_blender(ng2, case["inputs"], case.get("mesh"))
        if len(exp.get("v", [])) == 0:
            for l in list(L):
                if l.to_socket == gsock:
                    L.remove(l)
            p2v = ng2.nodes.new("GeometryNodePointsToVertices")
            L.new(real.outputs["Geometry"], p2v.inputs["Points"])
            L.new(p2v.outputs["Mesh"], gsock)
            exp = eval_blender(ng2, case["inputs"], case.get("mesh"))
    return exp


def candidates(ng):
    c = []
    for n in ng.nodes:
        if n.bl_idname in SKIP_PROBE:
            continue
        for s in n.outputs:
            if s.type == "GEOMETRY" and s.enabled:
                c.append((n, s))
    return c


def probe_cases(case, prefix=(), max_nodes=400):
    """Blender+Rust probe cases for every geometry-valued node output of the group at nesting `prefix`"""
    base = bpy.data.node_groups[case.get("keep") or case["group"]]
    cur = base
    for nm in prefix:
        cur = cur.nodes[nm].node_tree
    out_cases = []
    cands = candidates(cur)[:max_nodes]
    for k, (n, s) in enumerate(cands):
        path = list(prefix) + [n.name]
        ng2 = build_probe_group(base, path, s.identifier)
        exp = add_tail_and_eval(ng2, case)
        for g in [ng2]:
            pass
        bpy.data.node_groups.remove(ng2)
        out_cases.append({"name": f"p{k}", "group": case["group"], "inputs": case["inputs"], "mesh": case.get("mesh"),
                          "objects": case.get("objects"), "probe": path + [s.identifier], "expect": exp,
                          "_node": n.name, "_type": n.bl_idname, "_sock": s.identifier})
    return out_cases


def bisect_case(out, case, casefile):
    """descend through nested groups to the first node whose geometry output differs between Blender and Rust"""
    prefix = []
    while True:
        pcs = probe_cases(case, tuple(prefix))
        if not pcs:
            print("bisect: no geometry-valued nodes at", prefix)
            return
        tmp = dict(out)
        tmp["cases"] = pcs
        pf = casefile + (f".{case['name'].split('.')[-2] if '.' in case['name'] else ''}.{len(prefix)}" if os.environ.get("GN_KEEPB") else "")
        dump_cases(tmp, pf)
        rc, o, err = run_rust(pf, (["--tol", os.environ["GNTOL"]] if os.environ.get("GNTOL") else []))
        res = {}
        for line in o.splitlines():
            if line.startswith("CASE "):
                parts = line.split(" ", 3)
                res[parts[2]] = (parts[1], parts[3] if len(parts) > 3 else "")
        first = None
        npass = 0
        for pc in pcs:
            st = res.get(pc["name"], ("FAIL", "no result"))
            if st[0] == "PASS":
                npass += 1
            elif first is None:
                first = (pc, st)
        loc = " > ".join(prefix) or "<top>"
        if os.environ.get("GNALL"):
            print("bisect: failing probes at", loc, [pc["_node"] for pc in pcs if res.get(pc["name"], ("FAIL",))[0] != "PASS"])
        if first is None:
            print(f"bisect: all {len(pcs)} probes at {loc} pass; mismatch must be in the final output stage")
            return
        pc, st = first
        print(f"bisect: {npass}/{len(pcs)} probes pass at {loc}; first failing: node '{pc['_node']}' ({pc['_type']}) socket {pc['_sock']}: {st[1][:300]}")
        if pc["_type"] == "GeometryNodeGroup":
            prefix.append(pc["_node"])
            continue
        return


def finish(casefile, args, keep):
    extra = []
    if "--tol" in args:
        extra = ["--tol", args[args.index("--tol") + 1]]
    if "--filter" in args:
        extra += ["--filter", args[args.index("--filter") + 1]]
    rc, out, err = run_rust(casefile, extra + (["-v"] if "-v" in args else []))
    if rc != 0:
        print("gncheck exited with rc", rc)
        print(err[-2000:])
    res = report(out, "-v" in args)
    if not keep:
        os.remove(casefile)
    return res


def opt(args, name, default=None):
    return args[args.index(name) + 1] if name in args else default


def check_export(exp, group, n, seed=0):
    """cases for one exported group with n random input sets (+ default)"""
    ng = bpy.data.node_groups[group]
    rnd = random.Random(seed)
    cases = []
    for k in range(n):
        ins = {} if k == 0 else random_inputs(ng, rnd)
        cases.append({"name": f"{group}#{k}", "group": group, "inputs": ins, "expect": eval_blender(ng, ins)})
    return cases


def factory_expect(out, last):
    """expected mesh of the finished asset (geometry only: ExactMesh carries no attributes)"""
    fin = out.get("final")
    if fin and "error" not in fin:
        fin = dict(fin)
        fin["attrs"] = {}
        return fin
    return last["expect"]


def main():
    a = sys.argv[1:]
    if not a:
        print(__doc__)
        return
    mode = a[0]
    keep = "--keep" in a
    casefile = opt(a, "--cases", f"/tmp/gncases_{os.getpid()}.json")
    bpy.ops.wm.read_factory_settings(use_empty=True)
    if mode == "unit":
        import gn_units
        filt = [x for x in a[1:] if not x.startswith("-") and x not in (opt(a, "--tol"), opt(a, "--cases"), opt(a, "--filter"))]
        out = GX.new_out()
        cases = gn_units.build_cases(out, filt)
        out["cases"] = cases
        if "--show" in a:
            for c in cases:
                e = c["expect"]
                print("==", c["name"], "nv", len(e.get("v", [])), "ne", len(e.get("e", [])), "nf", len(e.get("f", [])), "mats", e.get("mats"))
                print("V", [[round(x, 4) for x in v] for v in e.get("v", [])])
                print("F", e.get("f"))
                print("E", e.get("e"))
                for k, v in e.get("attrs", {}).items():
                    print("A", k, v["dom"], v["ty"], v["data"])
            return
        dump_cases(out, casefile)
        finish(casefile, a, keep)
    elif mode == "group":
        stub = GX.stub_materials
        out = GX.export_function(a[1], [])
        out["cases"] = check_export(out, out["main"], int(opt(a, "--n", 3)))
        dump_cases(out, casefile)
        res = finish(casefile, a, keep or "--bisect" in a)
        if "--bisect" in a and res[1] > 0:
            for c in out["cases"]:
                c["keep"] = c["group"]
                bisect_case(out, c, casefile + ".b")
            for f in (casefile, casefile + ".b"):
                if os.path.exists(f) and not keep:
                    os.remove(f)
    elif mode == "json":
        # rebuild is not supported; exported json only has descriptions (use group/asset modes)
        print("json mode: use `group` or `asset`")
    elif mode == "factory":
        # factory MODULE.CLASS SEED [INST]: compare the Rust Factory (make_factory(CLASS)) with Blender's spawn_asset(INST)
        inst = int(a[3]) if len(a) > 3 and not a[3].startswith("-") else 0
        out = GX.export_asset(a[1], int(a[2]), inst)
        cls = a[1].rsplit(".", 1)[1]
        last = out["steps"][-1]
        out["cases"] = [{"name": f"{a[1]}#{a[2]}.{inst}", "group": last["group"], "factory": [cls, int(a[2]), inst], "expect": factory_expect(out, last)}]
        dump_cases(out, casefile)
        finish(casefile, a, keep)
    elif mode == "asset":
        out = GX.export_asset(a[1], int(a[2]))
        cases = []
        for k, st in enumerate(out["steps"]):
            exp = st.pop("expect", None)
            if exp is None:
                exp = eval_blender(bpy.data.node_groups[st["group"]], st["inputs"], st["mesh"])
            cases.append({"name": f"{a[1]}#{a[2]}.step{k}.{st['group']}", "group": st["group"], "keep": st.get("keep"), "inputs": st["inputs"], "out_attrs": st.get("out_attrs"), "mesh": st["mesh"], "expect": exp, "objects": out["objects"]})
        out["cases"] = cases
        dump_cases(out, casefile)
        res = finish(casefile, a, keep or "--bisect" in a)
        if "--bisect" in a and res[1] > 0:
            for c in cases:
                if c["expect"] is not None:
                    bisect_case(out, c, casefile + ".b")
            for f in (casefile, casefile + ".b"):
                if os.path.exists(f) and not keep:
                    os.remove(f)


if __name__ == "__main__":
    main()
