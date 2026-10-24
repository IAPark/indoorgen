"""pytree.py module:func [group name filter] : print a readable node listing of a nodegroup built by `func()` (to_nodegroup decorated)"""
import sys, importlib, bpy, json
mod, fn = sys.argv[1].split(":")
bpy.ops.wm.read_factory_settings(use_empty=True)
M = importlib.import_module(mod)
ng = getattr(M, fn)()
def fmt(v):
    try:
        v = list(v); return "(" + ",".join("%.6g" % x for x in v) + ")"
    except TypeError:
        return "%.9g" % v if isinstance(v, float) else str(v)
done = set()
def dump(tree, indent=""):
    order = []
    seen = set()
    def visit(n):
        if n.name in seen: return
        seen.add(n.name)
        for s in n.inputs:
            for l in s.links:
                visit(l.from_node)
        order.append(n)
    for n in tree.nodes:
        if n.bl_idname == "NodeGroupOutput": visit(n)
    for n in order:
        attrs = []
        for a in ("operation", "data_type", "domain", "mode", "axis", "pivot_axis", "rotation_type", "spline_type", "fill_type", "interpolation_type", "use_clamp", "clamp", "component", "transform_space"):
            if hasattr(n, a): attrs.append("%s=%s" % (a, getattr(n, a)))
        ins = []
        for s in n.inputs:
            if not s.enabled and not s.links: continue
            if s.links:
                ins.append("%s<-[%s]" % (s.name, ", ".join("%s.%s" % (l.from_node.name, l.from_socket.name if l.from_node.bl_idname != "NodeReroute" else "") for l in s.links)))
            elif hasattr(s, "default_value") and s.type not in ("GEOMETRY", "SHADER"):
                ins.append("%s=%s" % (s.name, fmt(s.default_value)))
        label = n.label or ""
        nm = getattr(n, "node_tree", None)
        print(indent + "%s [%s %s%s] %s" % (n.name, n.bl_idname, nm.name if nm else "", " ".join(attrs), "; ".join(ins)))
        if n.bl_idname == "ShaderNodeFloatCurve" or n.bl_idname == "ShaderNodeMapRange": pass
        if n.bl_idname == "ShaderNodeFloatCurve":
            c = n.mapping.curves[0]; print(indent + "    curve pts:", [tuple(round(x, 6) for x in p.location) for p in c.points])
        if nm is not None and nm.name not in done:
            done.add(nm.name); print(indent + "  --- group", nm.name); dump(nm, indent + "    "); print(indent + "  --- end")
dump(ng)
