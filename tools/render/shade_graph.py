"""shade_graph.py: build a Blender material from a graph Rust wrote (crates/ig_shade, sidecar "graphs"). The other backend is ig_shade/src/wgsl.rs."""
import bpy


def _set(nt, sock, v, nodes):
    """number, [r, g, b] or {"n": id}: a default value, or a link from that node's output"""
    if isinstance(v, dict):
        nt.links.new(nodes[v["n"]], sock)
    elif isinstance(v, list):
        sock.default_value = tuple(v) + (1.0,) if len(sock.default_value) == 4 else tuple(v)
    else:
        sock.default_value = v


def _noise(nt, i, nodes, fbm, out):
    n = nt.nodes.new("ShaderNodeTexNoise")
    has_w = not (isinstance(i.get("w"), (int, float)) and i["w"] == 0)
    n.noise_dimensions = "4D" if has_w else "3D"
    n.noise_type = "FBM"
    n.normalize = not fbm
    _set(nt, n.inputs["Vector"], i["v"], nodes)
    if has_w: _set(nt, n.inputs["W"], i["w"], nodes)
    for name, k in (("Scale", "scale"), ("Detail", "detail"), ("Roughness", "rough"), ("Lacunarity", "lac"), ("Distortion", "dist")):
        _set(nt, n.inputs[name], i[k], nodes)
    return n.outputs[out]


def _ramp(nt, i, nodes):
    n = nt.nodes.new("ShaderNodeValToRGB")
    _set(nt, n.inputs["Fac"], i["fac"], nodes)
    els = n.color_ramp.elements
    while len(els) < len(i["stops"]): els.new(0.5)
    for e, (pos, c) in zip(els, i["stops"]):
        e.position = pos
        e.color = tuple(c) + (1.0,)
    return n.outputs["Color"]


def _mix(nt, i, nodes, blend):
    n = nt.nodes.new("ShaderNodeMix")
    n.data_type = "RGBA"
    n.blend_type = blend
    _set(nt, n.inputs[0], i["fac"], nodes)
    _set(nt, n.inputs[6], i["a"], nodes)
    _set(nt, n.inputs[7], i["b"], nodes)
    return n.outputs[2]


def _node(nt, nd, nodes):
    op, i = nd["op"], nd["in"]
    if op == "coord":
        return nt.nodes.new("ShaderNodeTexCoord").outputs["Object"]
    if op == "mapping":
        n = nt.nodes.new("ShaderNodeMapping")
        _set(nt, n.inputs["Vector"], i["v"], nodes)
        n.inputs["Scale"].default_value = i["scale"]
        n.inputs["Rotation"].default_value = i["rot"]
        return n.outputs[0]
    if op in ("noise", "noise_color"):
        return _noise(nt, i, nodes, False, "Fac" if op == "noise" else "Color")
    if op == "musgrave":
        return _noise(nt, i, nodes, True, "Fac")
    if op == "voronoi_color":
        n = nt.nodes.new("ShaderNodeTexVoronoi")
        _set(nt, n.inputs["Vector"], i["v"], nodes)
        _set(nt, n.inputs["Scale"], i["scale"], nodes)
        return n.outputs["Color"]
    if op == "ramp":
        return _ramp(nt, i, nodes)
    if op in ("brick_color", "brick_fac"):
        n = nt.nodes.new("ShaderNodeTexBrick")
        n.offset = i["offset"]
        n.offset_frequency = 2
        _set(nt, n.inputs["Vector"], i["v"], nodes)
        n.inputs["Color1"].default_value = (0, 0, 0, 1)
        n.inputs["Color2"].default_value = (1, 1, 1, 1)
        n.inputs["Mortar"].default_value = (0.5, 0.5, 0.5, 1)
        for name, k in (("Scale", "scale"), ("Mortar Size", "mortar"), ("Brick Width", "width"), ("Row Height", "row")):
            _set(nt, n.inputs[name], i[k], nodes)
        return n.outputs["Color" if op == "brick_color" else "Fac"]
    if op == "checker":
        n = nt.nodes.new("ShaderNodeTexChecker")
        _set(nt, n.inputs["Vector"], i["v"], nodes)
        n.inputs["Color1"].default_value = (0, 0, 0, 1)
        n.inputs["Color2"].default_value = (1, 1, 1, 1)
        _set(nt, n.inputs["Scale"], i["scale"], nodes)
        return n.outputs["Fac"]
    if op == "wall_uv":
        geo = nt.nodes.new("ShaderNodeNewGeometry")
        cr = nt.nodes.new("ShaderNodeVectorMath")
        cr.operation = "CROSS_PRODUCT"
        _set(nt, cr.inputs[0], i["v"], nodes)
        nt.links.new(geo.outputs["Normal"], cr.inputs[1])
        a = nt.nodes.new("ShaderNodeSeparateXYZ")
        nt.links.new(cr.outputs[0], a.inputs[0])
        b = nt.nodes.new("ShaderNodeSeparateXYZ")
        _set(nt, b.inputs[0], i["v"], nodes)
        c = nt.nodes.new("ShaderNodeCombineXYZ")
        nt.links.new(a.outputs["Z"], c.inputs["X"])
        nt.links.new(b.outputs["Z"], c.inputs["Y"])
        return c.outputs[0]
    if op == "add":
        n = nt.nodes.new("ShaderNodeVectorMath")
        n.operation = "ADD"
        _set(nt, n.inputs[0], i["a"], nodes)
        _set(nt, n.inputs[1], i["b"], nodes)
        return n.outputs[0]
    if op == "map_range":
        n = nt.nodes.new("ShaderNodeMapRange")
        for idx, k in ((0, "v"), (1, "f0"), (2, "f1"), (3, "t0"), (4, "t1")):
            _set(nt, n.inputs[idx], i[k], nodes)
        return n.outputs[0]
    if op == "mul":
        n = nt.nodes.new("ShaderNodeMath")
        n.operation = "MULTIPLY"
        _set(nt, n.inputs[0], i["a"], nodes)
        _set(nt, n.inputs[1], i["b"], nodes)
        return n.outputs[0]
    if op.startswith("mix_"):
        return _mix(nt, i, nodes, {"mix_mix": "MIX", "mix_multiply": "MULTIPLY", "mix_difference": "DIFFERENCE"}[op])
    if op == "hsv_adjust":
        n = nt.nodes.new("ShaderNodeHueSaturation")
        _set(nt, n.inputs["Color"], i["color"], nodes)
        _set(nt, n.inputs["Saturation"], i["sat"], nodes)
        _set(nt, n.inputs["Value"], i["val"], nodes)
        return n.outputs[0]
    raise ValueError("unknown graph op " + op)


def build(graph):
    m = bpy.data.materials.new("rs_" + graph["name"])
    m.use_nodes = True
    nt = m.node_tree
    nt.nodes.clear()
    nodes = []
    for nd in graph["nodes"]:
        nodes.append(_node(nt, nd, nodes))
    out = graph["out"]
    mo = nt.nodes.new("ShaderNodeOutputMaterial")
    if out.get("glass"):
        # thin glass: light passes straight through with a faint reflection (a refracting glass shader blocks direct light with caustics off: rooms
        # behind windows went dark, and the denoiser smeared what was left into blotches)
        tr = nt.nodes.new("ShaderNodeBsdfTransparent")
        tr.inputs["Color"].default_value = tuple(out["color"]) + (1.0,)
        gy = nt.nodes.new("ShaderNodeBsdfGlossy")
        gy.inputs["Roughness"].default_value = out.get("roughness", 0.02)
        fr = nt.nodes.new("ShaderNodeFresnel")
        fr.inputs["IOR"].default_value = max(out.get("ior", 1.5), 1.3)
        mx = nt.nodes.new("ShaderNodeMixShader")
        # (only the face the light enters by reflects. Fresnel on the far face of a pane reads as leaving a denser medium, and past its critical angle -- 42 degrees at
        # IOR 1.5, 82 at 1.01 -- went to a full mirror: hard curved edges across windows, and half a shower pane black)
        geo = nt.nodes.new("ShaderNodeNewGeometry")
        keep = nt.nodes.new("ShaderNodeMath"); keep.operation = "SUBTRACT"; keep.inputs[0].default_value = 1.0
        nt.links.new(geo.outputs["Backfacing"], keep.inputs[1])
        only = nt.nodes.new("ShaderNodeMath"); only.operation = "MULTIPLY"
        nt.links.new(fr.outputs[0], only.inputs[0]); nt.links.new(keep.outputs[0], only.inputs[1])
        nt.links.new(only.outputs[0], mx.inputs[0])
        nt.links.new(tr.outputs[0], mx.inputs[1])
        nt.links.new(gy.outputs[0], mx.inputs[2])
        nt.links.new(mx.outputs[0], mo.inputs["Surface"])
        return m
    bs = nt.nodes.new("ShaderNodeBsdfPrincipled")
    for key, name in (("color", "Base Color"), ("roughness", "Roughness"), ("metallic", "Metallic"), ("coat", "Coat Weight"), ("coat_roughness", "Coat Roughness"), ("specular", "Specular IOR Level"), ("transmission", "Transmission Weight")):
        if key in out: _set(nt, bs.inputs[name], out[key], nodes)
    nt.links.new(bs.outputs["BSDF"], mo.inputs["Surface"])
    if "height" in out:
        d = nt.nodes.new("ShaderNodeDisplacement")
        d.inputs["Midlevel"].default_value = 0.0
        d.inputs["Scale"].default_value = 1.0
        _set(nt, d.inputs["Height"], out["height"], nodes)
        nt.links.new(d.outputs[0], mo.inputs["Displacement"])
    return m
