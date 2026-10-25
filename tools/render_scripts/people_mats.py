"""Blender materials for the Rocketbox people (cls people.skin / people.cloth / people.hair). Imported by render_rust.py (and tools/people/preview.py).
spec kwargs: color, normal, spec (absolute image paths or null), avatar (name)."""
import bpy, os


def _img(nt, path, colorspace):
    n = nt.nodes.new("ShaderNodeTexImage")
    n.image = bpy.data.images.load(path, check_existing=True)
    n.image.colorspace_settings.name = colorspace
    n.interpolation = "Cubic"
    n.extension = "EXTEND"
    return n


def _sock(bsdf, *names):
    for nm in names:
        if nm in bsdf.inputs:
            return bsdf.inputs[nm]
    raise KeyError(names)


def make(spec):
    kw = spec.get("kwargs", {})
    kind = spec["cls"].split(".", 1)[1]
    mat = bpy.data.materials.new("%s_%s" % (kw.get("avatar", "p"), kind))
    mat.use_nodes = True
    nt = mat.node_tree
    nt.nodes.clear()
    out = nt.nodes.new("ShaderNodeOutputMaterial")
    bsdf = nt.nodes.new("ShaderNodeBsdfPrincipled")
    nt.links.new(bsdf.outputs["BSDF"], out.inputs["Surface"])
    uv = nt.nodes.new("ShaderNodeUVMap")
    col = kw.get("color")
    if col and os.path.exists(col):
        t = _img(nt, col, "sRGB")
        nt.links.new(uv.outputs["UV"], t.inputs["Vector"])
        nt.links.new(t.outputs["Color"], _sock(bsdf, "Base Color"))
        if kind == "hair":
            nt.links.new(t.outputs["Alpha"], _sock(bsdf, "Alpha"))
    nrm = kw.get("normal")
    if nrm and os.path.exists(nrm):
        t = _img(nt, nrm, "Non-Color")
        nt.links.new(uv.outputs["UV"], t.inputs["Vector"])
        nm = nt.nodes.new("ShaderNodeNormalMap")
        nm.inputs["Strength"].default_value = 0.6 if kind == "skin" else 0.8
        nt.links.new(t.outputs["Color"], nm.inputs["Color"])
        nt.links.new(nm.outputs["Normal"], _sock(bsdf, "Normal"))
    sp = kw.get("spec")
    rough_lo, rough_hi = {"skin": (0.62, 0.34), "cloth": (0.9, 0.5), "hair": (0.55, 0.35)}[kind]
    if sp and os.path.exists(sp):
        t = _img(nt, sp, "Non-Color")
        nt.links.new(uv.outputs["UV"], t.inputs["Vector"])
        mr = nt.nodes.new("ShaderNodeMapRange")
        mr.inputs["To Min"].default_value = rough_lo
        mr.inputs["To Max"].default_value = rough_hi
        nt.links.new(t.outputs["Color"], mr.inputs["Value"])
        nt.links.new(mr.outputs["Result"], _sock(bsdf, "Roughness"))
    else:
        _sock(bsdf, "Roughness").default_value = 0.5 * (rough_lo + rough_hi)
    _sock(bsdf, "IOR").default_value = 1.4
    if kind in ("skin", "cloth"):
        # skin: red-shifted scattering radius (metres * scale), Burley-style random walk
        w = 0.55 if kind == "skin" else 0.18
        _sock(bsdf, "Subsurface Weight").default_value = w
        _sock(bsdf, "Subsurface Radius").default_value = (1.0, 0.35, 0.18)
        _sock(bsdf, "Subsurface Scale").default_value = 0.012
        try:
            bsdf.subsurface_method = "RANDOM_WALK_SKIN"
        except Exception:
            pass
        if kind == "skin":
            _sock(bsdf, "Specular IOR Level").default_value = 0.45
            _sock(bsdf, "Coat Weight").default_value = 0.0
        else:
            _sock(bsdf, "Specular IOR Level").default_value = 0.3
    if kind == "hair":
        _sock(bsdf, "Specular IOR Level").default_value = 0.6
        try:
            mat.surface_render_method = "DITHERED"
        except Exception:
            pass
        try:
            mat.blend_method = "HASHED"
        except Exception:
            pass
    return mat
