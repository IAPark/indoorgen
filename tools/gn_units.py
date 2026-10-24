"""Per-node unit cases for gn_check.py.  Each case builds a tiny node group with raw bpy calls.

Case builder signature: build(b) -> output geometry (socket/node) ; the group has Geometry in/out.
Field nodes are tested by storing the field into a named attribute 'r' on a base mesh (see base_grid)."""
import bpy, math, itertools
import gn_export as GX
import gn_check as GC


class B:
    def __init__(self, name):
        self.ng = bpy.data.node_groups.new(name, "GeometryNodeTree")
        self.ng.interface.new_socket("Geometry", in_out="INPUT", socket_type="NodeSocketGeometry")
        self.ng.interface.new_socket("Geometry", in_out="OUTPUT", socket_type="NodeSocketGeometry")
        self.gi = self.ng.nodes.new("NodeGroupInput")
        self.go = self.ng.nodes.new("NodeGroupOutput")
        self.objects = {}

    def out_sock(self, x):
        if isinstance(x, tuple):
            n, k = x
            for s in n.outputs:
                if s.identifier == k:
                    return s
            return n.outputs[k]
        if hasattr(x, "outputs"):
            for s in x.outputs:
                if s.enabled:
                    return s
        return x

    def n(self, idname, props=None, **ins):
        node = self.ng.nodes.new(idname)
        for k, v in (props or {}).items():
            setattr(node, k, v)
        for k, v in ins.items():
            k = k.replace("__", " ")
            sock = None
            for s in node.inputs:
                if s.identifier == k:
                    sock = s
                    break
            if sock is None:
                for s in node.inputs:
                    if s.name == k and s.enabled:
                        sock = s
                        break
            if sock is None:
                raise KeyError(f"{idname} has no input {k}: {[s.identifier for s in node.inputs]}")
            if isinstance(v, list) and sock.is_multi_input:
                for x in v:
                    self.ng.links.new(self.out_sock(x), sock)
            elif isinstance(v, (tuple,)) and len(v) == 2 and hasattr(v[0], "outputs"):
                self.ng.links.new(self.out_sock(v), sock)
            elif hasattr(v, "outputs") or hasattr(v, "default_value") and hasattr(v, "node"):
                self.ng.links.new(self.out_sock(v), sock)
            else:
                if isinstance(v, bpy.types.Object):
                    self.objects[v.name] = v
                sock.default_value = v
        return node

    def S(self, node, ident, v):
        for s in node.inputs:
            if s.identifier == ident:
                self._set(s, v)
                return node
        raise KeyError(ident)

    def finish(self, geo):
        self.ng.links.new(self.out_sock(geo), self.go.inputs[0])
        return self.ng

    # helpers
    def math(self, op, a=0.0, b=0.0, c=0.0, clamp=False):
        n = self.n("ShaderNodeMath", {"operation": op, "use_clamp": clamp})
        for i, x in enumerate((a, b, c)):
            self._set(n.inputs[i], x)
        return n

    def _set(self, sock, x):
        if isinstance(x, tuple) and len(x) == 2 and hasattr(x[0], "outputs"):
            self.ng.links.new(self.out_sock(x), sock)
        elif hasattr(x, "outputs"):
            self.ng.links.new(self.out_sock(x), sock)
        else:
            sock.default_value = x

    def vmath(self, op, a=(0, 0, 0), b=(0, 0, 0), c=(0, 0, 0), s=None):
        n = self.n("ShaderNodeVectorMath", {"operation": op})
        for i, x in enumerate((a, b, c)):
            self._set(n.inputs[i], x)
        if s is not None:
            self._set(n.inputs[3], s)
        return n

    def store(self, geo, value, name="r", dom="POINT", dt="FLOAT", sel=True):
        n = self.n("GeometryNodeStoreNamedAttribute", {"data_type": dt, "domain": dom})
        self.ng.links.new(self.out_sock(geo), n.inputs["Geometry"])
        n.inputs["Name"].default_value = name
        self._set(n.inputs["Value"], value)
        if sel is not True:
            self._set(n.inputs["Selection"], sel)
        return n

    def sep(self, v):
        n = self.n("ShaderNodeSeparateXYZ")
        self._set(n.inputs[0], v)
        return n

    def pos(self):
        return self.n("GeometryNodeInputPosition")

    def index(self):
        return self.n("GeometryNodeInputIndex")


def base_grid(b, vx=5, vy=4):
    return b.n("GeometryNodeMeshGrid", **{"Size__X": 3.7, "Size__Y": 2.9, "Vertices__X": vx, "Vertices__Y": vy})


def field_case(name, make_field, dt="FLOAT", dom="POINT", base=None):
    """case: grid mesh + store make_field(b) into attribute r"""
    def build(b):
        g = (base or base_grid)(b)
        f = make_field(b)
        return b.store((g, "Mesh"), f, "r", dom, dt)
    return (name, build)


def noisy_xyz(b):
    """test inputs: x,y from position, z from index"""
    p = b.pos()
    s = b.sep((p, "Position"))
    z = b.math("MULTIPLY_ADD", b.index(), 0.37, -3.1)
    return (s, "X"), (s, "Y"), (z, "Value")


MATH1 = ["SQRT", "INVERSE_SQRT", "ABSOLUTE", "EXPONENT", "SIGN", "ROUND", "FLOOR", "CEIL", "TRUNC", "FRACT", "SINE", "COSINE", "TANGENT",
         "ARCSINE", "ARCCOSINE", "ARCTANGENT", "SINH", "COSH", "TANH", "RADIANS", "DEGREES"]
MATH2 = ["ADD", "SUBTRACT", "MULTIPLY", "DIVIDE", "POWER", "LOGARITHM", "MINIMUM", "MAXIMUM", "LESS_THAN", "GREATER_THAN", "MODULO",
         "FLOORED_MODULO", "SNAP", "ARCTAN2", "PINGPONG"]
MATH3 = ["MULTIPLY_ADD", "COMPARE", "SMOOTH_MIN", "SMOOTH_MAX", "WRAP"]
VEC_OPS2 = ["ADD", "SUBTRACT", "MULTIPLY", "DIVIDE", "CROSS_PRODUCT", "PROJECT", "REFLECT", "DOT_PRODUCT", "DISTANCE", "MINIMUM", "MAXIMUM", "MODULO", "SNAP"]
VEC_OPS1 = ["LENGTH", "NORMALIZE", "ABSOLUTE", "FLOOR", "CEIL", "FRACTION", "SINE", "COSINE", "TANGENT"]


def math_cases():
    out = []
    for op in MATH1 + MATH2 + MATH3:
        def mk(b, op=op):
            x, y, z = noisy_xyz(b)
            return b.math(op, x, y, z)
        out.append(field_case(f"Math.{op}", mk))
    for op in ("ADD", "SINE"):
        def mk(b, op=op):
            x, y, z = noisy_xyz(b)
            return b.math(op, x, y, 0.0, clamp=True)
        out.append(field_case(f"Math.{op}.clamp", mk))
    # constant operands (constant folding + broadcast)
    def mk(b):
        x, y, z = noisy_xyz(b)
        return b.math("MULTIPLY", x, 2.5)
    out.append(field_case("Math.MULTIPLY.const", mk))
    return out


def vector_math_cases():
    out = []
    for op in VEC_OPS2 + VEC_OPS1 + ["SCALE", "MULTIPLY_ADD", "WRAP", "FACEFORWARD", "REFRACT"]:
        def mk(b, op=op):
            x, y, z = noisy_xyz(b)
            a = b.n("ShaderNodeCombineXYZ", X=x, Y=y, Z=z)
            c = b.n("ShaderNodeCombineXYZ", X=z, Y=x, Z=y)
            d = b.n("ShaderNodeCombineXYZ", X=y, Y=z, Z=(x))
            v = b.vmath(op, a, c, d, s=1.7 if op in ("SCALE", "REFRACT") else None)
            return v
        dt = "FLOAT" if op in ("DOT_PRODUCT", "DISTANCE", "LENGTH") else "FLOAT_VECTOR"
        if dt == "FLOAT":
            def mk2(b, mk=mk):
                v = mk(b)
                return (v, "Value")
            out.append(field_case(f"VectorMath.{op}", mk2, dt))
        else:
            out.append(field_case(f"VectorMath.{op}", mk, dt))
    return out


def misc_fn_cases():
    out = []

    def mk(b):
        x, y, z = noisy_xyz(b)
        return b.n("ShaderNodeCombineXYZ", X=x, Y=y, Z=z)
    out.append(field_case("CombineXYZ", mk, "FLOAT_VECTOR"))
    for ax in ("X", "Y", "Z"):
        def mk(b, ax=ax):
            p = b.pos()
            s = b.sep((p, "Position"))
            return (s, ax)
        out.append(field_case(f"SeparateXYZ.{ax}", mk))
    for interp in ("LINEAR", "STEPPED", "SMOOTHSTEP", "SMOOTHERSTEP"):
        for clamp in (True, False):
            def mk(b, interp=interp, clamp=clamp):
                x, y, z = noisy_xyz(b)
                n = b.n("ShaderNodeMapRange", {"interpolation_type": interp, "clamp": clamp}, Value=x)
                n.inputs["From Min"].default_value = -1.0
                n.inputs["From Max"].default_value = 1.5
                n.inputs["To Min"].default_value = 2.0
                n.inputs["To Max"].default_value = -3.0
                if interp == "STEPPED":
                    n.inputs["Steps"].default_value = 4.0
                return n
            out.append(field_case(f"MapRange.{interp}.{'clamp' if clamp else 'noclamp'}", mk))
    def mk(b):
        x, y, z = noisy_xyz(b)
        p = b.pos()
        n = b.n("ShaderNodeMapRange", {"data_type": "FLOAT_VECTOR", "clamp": True}, Vector=(p, "Position"))
        b.S(n, "From_Min_FLOAT3", (-1, -1, -1))
        b.S(n, "From_Max_FLOAT3", (1, 2, 1))
        b.S(n, "To_Min_FLOAT3", (0, 0, 0))
        b.S(n, "To_Max_FLOAT3", (5, 6, 7))
        return (n, "Vector")
    out.append(field_case("MapRange.vector", mk, "FLOAT_VECTOR"))
    for ct in ("MINMAX", "RANGE"):
        def mk(b, ct=ct):
            x, y, z = noisy_xyz(b)
            return b.n("ShaderNodeClamp", {"clamp_type": ct}, Value=x, Min=-0.5, Max=0.8)
        out.append(field_case(f"Clamp.{ct}", mk))
    for op in ("LESS_THAN", "LESS_EQUAL", "GREATER_THAN", "GREATER_EQUAL", "EQUAL", "NOT_EQUAL"):
        def mk(b, op=op):
            x, y, z = noisy_xyz(b)
            return b.n("FunctionNodeCompare", {"operation": op, "data_type": "FLOAT"}, A=x, B=0.3)
        out.append(field_case(f"Compare.FLOAT.{op}", mk, "BOOLEAN"))
        def mk(b, op=op):
            i = b.index()
            return b.n("FunctionNodeCompare", {"operation": op, "data_type": "INT"}, A_INT=(i, "Index"), B_INT=7)
        out.append(field_case(f"Compare.INT.{op}", mk, "BOOLEAN"))
    for mode in ("ELEMENT", "LENGTH", "AVERAGE", "DOT_PRODUCT"):
        def mk(b, mode=mode):
            p = b.pos()
            n = b.n("FunctionNodeCompare", {"operation": "GREATER_THAN", "data_type": "VECTOR", "mode": mode}, A_VEC3=(p, "Position"), B_VEC3=(0.2, -0.3, 0.1))
            return n
        out.append(field_case(f"Compare.VECTOR.{mode}", mk, "BOOLEAN"))
    for op in ("AND", "OR", "NOT", "NAND", "NOR", "XNOR", "XOR", "IMPLY", "NIMPLY"):
        def mk(b, op=op):
            x, y, z = noisy_xyz(b)
            c1 = b.n("FunctionNodeCompare", {"operation": "GREATER_THAN"}, A=x, B=0.0)
            c2 = b.n("FunctionNodeCompare", {"operation": "LESS_THAN"}, A=y, B=0.3)
            return b.n("FunctionNodeBooleanMath", {"operation": op}, Boolean=(c1, "Result"), Boolean_001=(c2, "Result"))
        out.append(field_case(f"BooleanMath.{op}", mk, "BOOLEAN"))
    def mk(b):
        x, y, z = noisy_xyz(b)
        c1 = b.n("FunctionNodeCompare", {"operation": "GREATER_THAN"}, A=x, B=0.0)
        return _switch(b, c1, x, y)
    out.append(field_case("Switch.FLOAT.field", mk))
    def mk(b):
        x, y, z = noisy_xyz(b)
        n = b.n("GeometryNodeSwitch", {"input_type": "FLOAT"})
        n.inputs["Switch"].default_value = True
        n.inputs["False"].default_value = 1.0
        n.inputs["True"].default_value = 2.0
        return n
    out.append(field_case("Switch.FLOAT.const", mk))
    for dt, kw in (("FLOAT", {"Min_001": -1.0, "Max_001": 3.0}), ("INT", {"Min_002": -2, "Max_002": 9}), ("FLOAT_VECTOR", {"Min": (-1, 0, 1), "Max": (1, 2, 3)}), ("BOOLEAN", {"Probability": 0.4})):
        for seed in (0, 7):
            def mk(b, dt=dt, kw=kw, seed=seed):
                n = b.n("FunctionNodeRandomValue", {"data_type": dt}, **kw)
                n.inputs["Seed"].default_value = seed
                return n
            odt = {"FLOAT": "FLOAT", "INT": "INT", "FLOAT_VECTOR": "FLOAT_VECTOR", "BOOLEAN": "BOOLEAN"}[dt]
            out.append(field_case(f"RandomValue.{dt}.seed{seed}", mk, odt))
    for pts in ([(0, 0), (1, 1)], [(0, 0), (0.3, 0.8), (1, 0.4)], [(0, 0.1), (0.25, 0.9), (0.6, 0.2), (1, 1)]):
        def mk(b, pts=pts):
            x, y, z = noisy_xyz(b)
            m = b.n("ShaderNodeMapRange", {"clamp": True}, Value=x)
            m.inputs["From Min"].default_value = -1.9
            m.inputs["From Max"].default_value = 1.9
            n = b.n("ShaderNodeFloatCurve", Value=m)
            c = n.mapping.curves[0]
            for i, p in enumerate(pts):
                if i < 2:
                    c.points[i].location = p
                else:
                    c.points.new(*p)
            n.mapping.update()
            return n
        out.append(field_case(f"FloatCurve.{len(pts)}pts", mk))
    def mk(b):
        x, y, z = noisy_xyz(b)
        m = b.n("ShaderNodeMapRange", {"clamp": True}, Value=x)
        m.inputs["From Min"].default_value = -1.9
        m.inputs["From Max"].default_value = 1.9
        n = b.n("ShaderNodeValToRGB", Fac=m)
        r = n.color_ramp
        r.elements[0].color = (0.1, 0.2, 0.3, 1)
        r.elements[1].color = (0.9, 0.5, 0.1, 1)
        e = r.elements.new(0.4)
        e.color = (0.0, 1.0, 0.5, 1)
        return (n, "Color")
    out.append(field_case("ColorRamp.linear", mk, "FLOAT_COLOR"))
    for dt, a, bb in (("FLOAT", 1.5, -2.0), ("VECTOR", (1, 2, 3), (-1, 0, 4))):
        def mk(b, dt=dt, a=a, bb=bb):
            x, y, z = noisy_xyz(b)
            n = b.n("ShaderNodeMix", {"data_type": dt, "clamp_factor": True}, Factor_Float=x)
            ia, ib = ("A_Float", "B_Float") if dt == "FLOAT" else ("A_Vector", "B_Vector")
            b.S(n, ia, a)
            b.S(n, ib, bb)
            return (n, "Result_Float" if dt == "FLOAT" else "Result_Vector")
        out.append(field_case(f"Mix.{dt}", mk, "FLOAT" if dt == "FLOAT" else "FLOAT_VECTOR"))
    for bl in ("MIX", "ADD", "MULTIPLY", "SUBTRACT", "SCREEN", "DIVIDE", "DIFFERENCE", "DARKEN", "LIGHTEN", "OVERLAY"):
        def mk(b, bl=bl):
            x, y, z = noisy_xyz(b)
            m = b.n("ShaderNodeMapRange", {"clamp": True}, Value=x)
            m.inputs["From Min"].default_value = -1.9
            m.inputs["From Max"].default_value = 1.9
            n = b.n("ShaderNodeMix", {"data_type": "RGBA", "blend_type": bl}, Factor_Float=m)
            b.S(n, "A_Color", (0.2, 0.7, 0.4, 1))
            b.S(n, "B_Color", (0.9, 0.1, 0.6, 1))
            return (n, "Result_Color")
        out.append(field_case(f"Mix.RGBA.{bl}", mk, "FLOAT_COLOR"))
    return out


def _switch(b, cond, x, y):
    n = b.n("GeometryNodeSwitch", {"input_type": "FLOAT"})
    b.ng.links.new(b.out_sock((cond, "Result")), n.inputs["Switch"])
    b._set(n.inputs["False"], x)
    b._set(n.inputs["True"], y)
    return n


def input_cases():
    out = []
    def mk(b):
        return b.pos()
    out.append(field_case("InputPosition", mk, "FLOAT_VECTOR"))
    out.append(field_case("InputIndex", lambda b: b.index(), "INT"))
    out.append(field_case("InputNormal.face", lambda b: b.n("GeometryNodeInputNormal"), "FLOAT_VECTOR", "FACE", base=lambda b: b.n("GeometryNodeMeshUVSphere", Segments=7, Rings=5, Radius=1.3)))
    out.append(field_case("InputNormal.point", lambda b: b.n("GeometryNodeInputNormal"), "FLOAT_VECTOR", "POINT", base=lambda b: b.n("GeometryNodeMeshUVSphere", Segments=7, Rings=5, Radius=1.3)))
    out.append(field_case("InputNormal.cube.point", lambda b: b.n("GeometryNodeInputNormal"), "FLOAT_VECTOR", "POINT", base=lambda b: b.n("GeometryNodeMeshCube", Size=(1.0, 2.0, 3.0))))
    out.append(field_case("pos.on.face", lambda b: b.pos(), "FLOAT_VECTOR", "FACE"))
    out.append(field_case("pos.on.edge", lambda b: b.pos(), "FLOAT_VECTOR", "EDGE"))
    out.append(field_case("pos.on.corner", lambda b: b.pos(), "FLOAT_VECTOR", "CORNER"))
    out.append(field_case("index.on.face", lambda b: b.index(), "INT", "FACE"))
    out.append(field_case("index.on.edge", lambda b: b.index(), "INT", "EDGE"))
    return out


def primitive_cases():
    out = []
    def prim(name, idname, props=None, **ins):
        def build(b):
            n = b.n(idname, props, **ins)
            return (n, "Mesh")
        out.append((name, build))
    for n in ((2, 2, 2), (3, 2, 4), (4, 4, 4), (2, 5, 3)):
        prim(f"MeshCube.{n}", "GeometryNodeMeshCube", None, Size=(1.5, 2.0, 0.7), Vertices__X=n[0], Vertices__Y=n[1], Vertices__Z=n[2])
    for fill in ("NGON", "NONE", "TRIANGLE_FAN"):
        for sides, fills in ((8, 1), (5, 3), (3, 2)):
            prim(f"MeshCylinder.{fill}.{sides}.{fills}", "GeometryNodeMeshCylinder", {"fill_type": fill}, Vertices=sides, Side__Segments=fills, Fill__Segments=fills, Radius=0.8, Depth=2.2)
        prim(f"MeshCone.{fill}", "GeometryNodeMeshCone", {"fill_type": fill}, Vertices=6, Side__Segments=2, Fill__Segments=1, Radius__Top=0.3, Radius__Bottom=0.9, Depth=1.7)
        prim(f"MeshCone.point.{fill}", "GeometryNodeMeshCone", {"fill_type": fill}, Vertices=6, Side__Segments=2, Fill__Segments=1, Radius__Top=0.0, Radius__Bottom=0.9, Depth=1.7)
    prim("MeshCone.bottompoint", "GeometryNodeMeshCone", {"fill_type": "NGON"}, Vertices=5, Side__Segments=1, Fill__Segments=1, Radius__Top=0.9, Radius__Bottom=0.0, Depth=1.7)
    for s, r in ((8, 5), (5, 3), (32, 16), (3, 2)):
        prim(f"MeshUVSphere.{s}.{r}", "GeometryNodeMeshUVSphere", None, Segments=s, Rings=r, Radius=1.3)
    for mode, cm in (("OFFSET", "TOTAL"), ("END_POINTS", "TOTAL"), ("END_POINTS", "RESOLUTION")):
        prim(f"MeshLine.{mode}.{cm}", "GeometryNodeMeshLine", {"mode": mode, "count_mode": cm}, Count=6, Resolution=0.4, Start__Location=(0.1, 0.2, 0.3), Offset=(1.0, -0.5, 0.25))
    prim("MeshGrid", "GeometryNodeMeshGrid", None, Size__X=2.5, Size__Y=1.5, Vertices__X=4, Vertices__Y=3)
    prim("MeshGrid.1", "GeometryNodeMeshGrid", None, Size__X=2.5, Size__Y=1.5, Vertices__X=2, Vertices__Y=2)
    for fill in ("NONE", "NGON", "TRIANGLE_FAN"):
        prim(f"MeshCircle.{fill}", "GeometryNodeMeshCircle", {"fill_type": fill}, Vertices=7, Radius=1.2)
    return out


def curve_cases():
    out = []
    def cm(name, mk_curve, prof=None, caps=False, base=None):
        def build(b):
            c = mk_curve(b)
            p = prof(b) if prof else None
            n = b.n("GeometryNodeCurveToMesh", Curve=(c, "Curve"), Fill__Caps=caps)
            if p is not None:
                b.ng.links.new(b.out_sock((p, "Curve")), n.inputs["Profile Curve"])
            return (n, "Mesh")
        out.append((name, build))
    line = lambda b: b.n("GeometryNodeCurvePrimitiveLine", Start=(0, 0, 0), End=(0.3, 0.4, 2.0))
    circ = lambda b: b.n("GeometryNodeCurvePrimitiveCircle", {"mode": "RADIUS"}, Resolution=6, Radius=0.5)
    circ_big = lambda b: b.n("GeometryNodeCurvePrimitiveCircle", {"mode": "RADIUS"}, Resolution=12, Radius=2.0)
    bez = lambda b: b.n("GeometryNodeCurveQuadraticBezier", Resolution=8, Start=(0, 0, 0), Middle=(1, 0.2, 1.5), End=(2.5, 1.0, 1.0))
    cm("CurveToMesh.line.noprofile", line)
    cm("CurveToMesh.circle.noprofile", circ)
    cm("CurveToMesh.line.circle", line, circ)
    cm("CurveToMesh.line.circle.caps", line, circ, True)
    cm("CurveToMesh.bezier.circle", bez, circ, True)
    cm("CurveToMesh.circle.circle", circ_big, circ)
    cm("CurveToMesh.bezier.line", bez, line)
    cm("CurveToMesh.bezier.rect", bez, lambda b: b.n("GeometryNodeCurvePrimitiveQuadrilateral", Width=0.4, Height=0.2), True)
    for res in (2, 3, 8):
        cm(f"QuadraticBezier.{res}", lambda b, res=res: b.n("GeometryNodeCurveQuadraticBezier", Resolution=res, Start=(0, 0, 0), Middle=(1, 0.2, 1.5), End=(2.5, 1.0, 1.0)))
    cm("CurveQuad.wire", lambda b: b.n("GeometryNodeCurvePrimitiveQuadrilateral", Width=0.4, Height=0.2))
    cm("CurveCircle.wire", lambda b: b.n("GeometryNodeCurvePrimitiveCircle", {"mode": "RADIUS"}, Resolution=5, Radius=0.7))
    rectc = lambda b: b.n("GeometryNodeCurvePrimitiveQuadrilateral", Width=2.0, Height=1.0)
    filc = lambda b: b.n("GeometryNodeFilletCurve", {"mode": "POLY"}, Curve=(rectc(b), "Curve"), Radius=0.3, Limit__Radius=True)
    cm("CurveToMesh.cyc.rect.line", rectc, line)
    cm("CurveToMesh.cyc.rect.circle", rectc, circ)
    cm("CurveToMesh.cyc.fillet.line", filc, line)
    cm("CurveToMesh.cyc.fillet.circle", filc, circ)
    cm("CurveLine.direction", lambda b: b.n("GeometryNodeCurvePrimitiveLine", {"mode": "DIRECTION"}, Start=(1, 0, 0), Direction=(0, 3, 4), Length=2.0))
    # fields on curves
    def curve_field(name, mk_field, dt="FLOAT", dom="POINT"):
        def build(b):
            c = bez(b)
            f = mk_field(b)
            s = b.store((c, "Curve"), f, "r", dom, dt)
            n = b.n("GeometryNodeCurveToMesh", Curve=s)
            return (n, "Mesh")
        out.append((name, build))
    return out


def geometry_cases():
    out = []
    def case(name, build):
        out.append((name, build))
    def cube(b, s=(1.0, 2.0, 0.5)):
        return b.n("GeometryNodeMeshCube", Size=s)
    for rot in ((0, 0, 0), (0.3, 0.0, 0.0), (0.2, -0.4, 1.1), (0, 0, math.pi / 2)):
        for sc in ((1, 1, 1), (2, 0.5, 1.5)):
            case(f"Transform.{rot}.{sc}", lambda b, rot=rot, sc=sc: b.n("GeometryNodeTransform", Geometry=(cube(b), "Mesh"), Translation=(0.5, -1, 2), Rotation=rot, Scale=sc))
    case("Transform.translate", lambda b: b.n("GeometryNodeTransform", Geometry=(cube(b), "Mesh"), Translation=(0.5, -1, 2)))
    def join(b):
        c1 = cube(b)
        c2 = b.n("GeometryNodeMeshUVSphere", Segments=5, Rings=3, Radius=0.4)
        c3 = b.n("GeometryNodeMeshCylinder", Vertices=4)
        return b.n("GeometryNodeJoinGeometry", Geometry=[(c1, "Mesh"), (c2, "Mesh"), (c3, "Mesh")])
    case("JoinGeometry.3", join)
    def join_attr(b):
        c1 = b.store((cube(b), "Mesh"), 2.0, "r", "POINT")
        c2 = b.n("GeometryNodeMeshUVSphere", Segments=5, Rings=3, Radius=0.4)
        c3 = b.store((b.n("GeometryNodeMeshCylinder", Vertices=4), "Mesh"), 1, "q", "FACE", "INT")
        return b.n("GeometryNodeJoinGeometry", Geometry=[c1, (c2, "Mesh"), c3])
    case("JoinGeometry.attrs", join_attr)
    for cnt in (23,):
        def rs_attr(b, cnt=cnt, what="noise"):
            circ = b.n("GeometryNodeCurvePrimitiveCircle", {"mode": "RADIUS"}, Resolution=32, Radius=0.07)
            rc = b.n("GeometryNodeResampleCurve", {"mode": "COUNT"}, Curve=(circ, "Curve"), Count=cnt)
            r1 = b.n("FunctionNodeRandomValue", {"data_type": "FLOAT"}, Min_001=-0.021, Max_001=0.021)
            r2 = b.n("FunctionNodeRandomValue", {"data_type": "FLOAT"}, Min_001=-0.021, Max_001=0.021)
            cb = b.n("ShaderNodeCombineXYZ", X=(r1, "Value_001"), Y=(r2, "Value_001"))
            sp = b.n("GeometryNodeSetPosition", Geometry=(rc, "Curve"), Offset=(cb, "Vector"))
            if what == "noise":
                nt = b.n("ShaderNodeTexNoise")
                st = b.store((sp, "Geometry"), (nt, "Fac"), "r", "POINT", "FLOAT")
            elif what == "normal":
                nor = b.n("GeometryNodeInputNormal")
                st = b.store((sp, "Geometry"), (nor, "Normal"), "r", "POINT", "FLOAT_VECTOR")
            else:
                nor = b.n("GeometryNodeInputNormal")
                al = b.n("FunctionNodeAlignEulerToVector", {"axis": "Y"}, Vector=(nor, "Normal"))
                st = b.store((sp, "Geometry"), (al, "Rotation"), "r", "POINT", "FLOAT_VECTOR")
            return b.n("GeometryNodeCurveToMesh", Curve=(st, "Geometry"))
        for what in ("noise", "normal", "align"):
            case(f"ResampleAttr.{what}.{cnt}", lambda b, what=what: rs_attr(b, what=what))
    for cnt in (23, 29):
        def iop_circle(b, cnt=cnt):
            circ = b.n("GeometryNodeCurvePrimitiveCircle", {"mode": "RADIUS"}, Resolution=32, Radius=0.07)
            rc = b.n("GeometryNodeResampleCurve", {"mode": "COUNT"}, Curve=(circ, "Curve"), Count=cnt)
            r1 = b.n("FunctionNodeRandomValue", {"data_type": "FLOAT"}, Min_001=-0.021, Max_001=0.021)
            r2 = b.n("FunctionNodeRandomValue", {"data_type": "FLOAT"}, Min_001=-0.021, Max_001=0.021)
            cb = b.n("ShaderNodeCombineXYZ", X=(r1, "Value_001"), Y=(r2, "Value_001"))
            sp = b.n("GeometryNodeSetPosition", Geometry=(rc, "Curve"), Offset=(cb, "Vector"))
            nor = b.n("GeometryNodeInputNormal")
            al = b.n("FunctionNodeAlignEulerToVector", {"axis": "Y"}, Vector=(nor, "Normal"))
            line = b.n("GeometryNodeMeshLine", {"mode": "OFFSET"}, Count=2, Offset=(0, 0, 0.5))
            nt = b.n("ShaderNodeTexNoise")
            mr = b.n("ShaderNodeMapRange", {"clamp": True}, Value=(nt, "Fac"))
            b.S(mr, "To Min", 0.6)
            b.S(mr, "To Max", 1.1)
            iop = b.n("GeometryNodeInstanceOnPoints", Points=(sp, "Geometry"), Instance=(line, "Mesh"), Rotation=(al, "Rotation"), Scale=(mr, "Result"))
            return b.n("GeometryNodeRealizeInstances", Geometry=(iop, "Instances"))
        case(f"InstanceOnPoints.circle_resample.{cnt}", iop_circle)
    def setpos(b):
        g = base_grid(b)
        x, y, z = noisy_xyz(b)
        off = b.n("ShaderNodeCombineXYZ", X=0.0, Y=0.0, Z=b.math("MULTIPLY", (b.sep((b.pos(), "Position")), "X"), 0.5))
        return b.n("GeometryNodeSetPosition", Geometry=(g, "Mesh"), Offset=off)
    case("SetPosition.offset", setpos)
    def setpos2(b):
        g = base_grid(b)
        p = b.pos()
        pp = b.vmath("MULTIPLY", (p, "Position"), (1.5, 0.5, 2.0))
        sel = b.n("FunctionNodeCompare", {"operation": "GREATER_THAN"}, A=(b.sep((p, "Position")), "X"), B=0.0)
        return b.n("GeometryNodeSetPosition", Geometry=(g, "Mesh"), Selection=(sel, "Result"), Position=(pp, "Vector"))
    case("SetPosition.position.sel", setpos2)
    for dom, dt, val in (("POINT", "FLOAT", 1.5), ("FACE", "INT", 3), ("EDGE", "FLOAT_VECTOR", (1, 2, 3)), ("CORNER", "BOOLEAN", True), ("FACE", "FLOAT_COLOR", (0.1, 0.2, 0.3, 1.0))):
        def mk(b, dom=dom, dt=dt, val=val):
            return b.store((cube(b), "Mesh"), val, "attr", dom, dt)
        case(f"StoreNamedAttribute.{dom}.{dt}", mk)
    def store_sel(b):
        g = base_grid(b)
        sel = b.n("FunctionNodeCompare", {"operation": "GREATER_THAN"}, A=(b.sep((b.pos(), "Position")), "X"), B=0.0)
        return b.store((g, "Mesh"), 5.0, "r", "POINT", "FLOAT", (sel, "Result"))
    case("StoreNamedAttribute.selection", store_sel)
    def capture(b):
        g = base_grid(b)
        cap = b.ng.nodes.new("GeometryNodeCaptureAttribute")
        cap.domain = "POINT"
        cap.capture_items.new("FLOAT", "a")
        b.ng.links.new(b.out_sock((g, "Mesh")), cap.inputs["Geometry"])
        b.ng.links.new(b.out_sock((b.sep((b.pos(), "Position")), "X")), cap.inputs["Value"])
        # move points, then store the captured field
        sp = b.n("GeometryNodeSetPosition", Geometry=(cap, "Geometry"), Offset=(0.3, 0.2, 0.1))
        return b.store((sp, "Geometry"), (cap, "Attribute"), "r", "POINT", "FLOAT")
    case("CaptureAttribute.point", capture)
    def setmat(b):
        m = bpy.data.materials.new("matA")
        m2 = bpy.data.materials.new("matB")
        g = cube(b)
        s1 = b.n("GeometryNodeSetMaterial", Geometry=(g, "Mesh"), Material=m)
        sel = b.n("FunctionNodeCompare", {"operation": "GREATER_THAN", "data_type": "INT"})
        b.S(sel, "A_INT", (b.index(), "Index"))
        b.S(sel, "B_INT", 2)
        return b.n("GeometryNodeSetMaterial", Geometry=s1, Selection=(sel, "Result"), Material=m2)
    case("SetMaterial.sel", setmat)
    case("SetMaterialIndex", lambda b: b.n("GeometryNodeSetMaterialIndex", Geometry=(cube(b), "Mesh"), Material__Index=3))
    case("BoundBox", lambda b: (b.n("GeometryNodeBoundBox", Geometry=(b.n("GeometryNodeTransform", Geometry=(cube(b), "Mesh"), Translation=(1, 2, 3), Rotation=(0.3, 0.2, 0.1)), "Geometry")), "Bounding Box"))
    return out


def inst_cases():
    out = []
    def case(name, build):
        out.append((name, build))
    def cube(b, s=(0.3, 0.2, 0.5)):
        return b.n("GeometryNodeMeshCube", Size=s)
    def iop(b, rot=(0, 0, 0), scale=(1, 1, 1), sel=None, pick=False):
        g = b.n("GeometryNodeMeshGrid", Size__X=2.0, Size__Y=1.0, Vertices__X=3, Vertices__Y=2)
        n = b.n("GeometryNodeInstanceOnPoints", Points=(g, "Mesh"), Instance=(cube(b), "Mesh"), Rotation=rot, Scale=scale)
        return n
    def realize(b, g):
        return b.n("GeometryNodeRealizeInstances", Geometry=g)
    case("InstanceOnPoints.basic", lambda b: realize(b, iop(b)))
    case("InstanceOnPoints.rot.scale", lambda b: realize(b, iop(b, (0.3, 0.2, 0.9), (1.5, 1, 0.5))))
    def iop_field(b):
        g = b.n("GeometryNodeMeshGrid", Size__X=2.0, Size__Y=1.0, Vertices__X=3, Vertices__Y=2)
        x, y, z = noisy_xyz(b)
        sc = b.n("ShaderNodeCombineXYZ", X=b.math("ADD", x, 2.0), Y=1.0, Z=b.math("ADD", y, 2.0))
        rt = b.n("ShaderNodeCombineXYZ", X=0.0, Y=0.0, Z=x)
        sel = b.n("FunctionNodeCompare", {"operation": "LESS_THAN", "data_type": "INT"})
        b.S(sel, "A_INT", (b.index(), "Index"))
        b.S(sel, "B_INT", 4)
        n = b.n("GeometryNodeInstanceOnPoints", Points=(g, "Mesh"), Instance=(cube(b), "Mesh"), Rotation=rt, Scale=sc, Selection=(sel, "Result"))
        return realize(b, n)
    case("InstanceOnPoints.fields.sel", iop_field)
    case("BoundingBox.instances.rot", lambda b: realize(b, b.n("GeometryNodeRotateInstances", Instances=(b.n("GeometryNodeBoundBox", Geometry=iop(b, (0.1, 0.2, 0.3))), "Bounding Box"), Rotation=(0.2, 0.4, 0.6), Pivot__Point=(0.3, 0.1, 0.2))))
    case("RotateInstances.global", lambda b: realize(b, b.n("GeometryNodeRotateInstances", Instances=iop(b), Rotation=(0.2, 0.4, 0.6), Pivot__Point=(0.3, 0.1, 0.2), Local__Space=False)))
    case("RotateInstances.local", lambda b: realize(b, b.n("GeometryNodeRotateInstances", Instances=iop(b, (0.1, 0.2, 0.3)), Rotation=(0.2, 0.4, 0.6), Pivot__Point=(0.3, 0.1, 0.2), Local__Space=True)))
    case("ScaleInstances.global", lambda b: realize(b, b.n("GeometryNodeScaleInstances", Instances=iop(b, (0.1, 0.2, 0.3)), Scale=(1.5, 0.7, 2.0), Center=(0.3, 0.1, 0.2), Local__Space=False)))
    case("ScaleInstances.local", lambda b: realize(b, b.n("GeometryNodeScaleInstances", Instances=iop(b, (0.1, 0.2, 0.3)), Scale=(1.5, 0.7, 2.0), Center=(0.3, 0.1, 0.2), Local__Space=True)))
    case("TranslateInstances.global", lambda b: realize(b, b.n("GeometryNodeTranslateInstances", Instances=iop(b, (0.1, 0.2, 0.3)), Translation=(1.5, 0.7, 2.0), Local__Space=False)))
    case("TranslateInstances.local", lambda b: realize(b, b.n("GeometryNodeTranslateInstances", Instances=iop(b, (0.1, 0.2, 0.3)), Translation=(1.5, 0.7, 2.0), Local__Space=True)))
    for dt, mk in (("FLOAT", lambda b: (b.sep((b.pos(), "Position")), "X")), ("FLOAT_VECTOR", lambda b: (b.pos(), "Position"))):
        for dom in ("POINT", "FACE", "EDGE"):
            def build(b, dt=dt, mk=mk, dom=dom):
                g = base_grid(b)
                st = b.n("GeometryNodeAttributeStatistic", {"data_type": dt, "domain": dom}, Geometry=(g, "Mesh"), Attribute=mk(b))
                # store each statistic as a point attribute on a 1-point mesh via combine
                outs_ = ["Mean", "Median", "Sum", "Min", "Max", "Range", "Standard__Deviation", "Variance"]
                res = b.n("GeometryNodeMeshLine", Count=1)
                cur = (res, "Mesh")
                for o in ("Mean", "Median", "Sum", "Min", "Max", "Range", "Standard Deviation", "Variance"):
                    cur = (b.store(cur, (st, o), o.replace(" ", "_"), "POINT", dt), "Geometry")
                return cur
            case(f"AttributeStatistic.{dt}.{dom}", build)
    def sel_x(b):
        return (b.n("FunctionNodeCompare", {"operation": "GREATER_THAN"}, A=(b.sep((b.pos(), "Position")), "X"), B=0.1), "Result")
    for dom in ("POINT", "FACE", "EDGE"):
        def build_sep(b, dom=dom):
            g = b.n("GeometryNodeMeshGrid", Size__X=3.7, Size__Y=2.9, Vertices__X=5, Vertices__Y=4)
            n = b.n("GeometryNodeSeparateGeometry", {"domain": dom}, Geometry=(g, "Mesh"), Selection=sel_x(b))
            return (n, "Selection")
        case(f"SeparateGeometry.{dom}", build_sep)
        def build_sep2(b, dom=dom):
            g = b.n("GeometryNodeMeshGrid", Size__X=3.7, Size__Y=2.9, Vertices__X=5, Vertices__Y=4)
            n = b.n("GeometryNodeSeparateGeometry", {"domain": dom}, Geometry=(g, "Mesh"), Selection=sel_x(b))
            return (n, "Inverted")
        case(f"SeparateGeometry.inv.{dom}", build_sep2)
        def build_del(b, dom=dom):
            g = b.n("GeometryNodeMeshCube", Size=(1, 2, 3), Vertices__X=3, Vertices__Y=3, Vertices__Z=3)
            n = b.n("GeometryNodeDeleteGeometry", {"domain": dom}, Geometry=(g, "Mesh"), Selection=sel_x(b))
            return n
        case(f"DeleteGeometry.{dom}", build_del)
    for mode in ("EDGE_FACE", "ONLY_FACE"):
        def build_del2(b, mode=mode):
            g = b.n("GeometryNodeMeshCube", Size=(1, 2, 3), Vertices__X=3, Vertices__Y=3, Vertices__Z=3)
            sel = b.n("FunctionNodeCompare", {"operation": "GREATER_THAN", "data_type": "INT"})
            b.S(sel, "A_INT", (b.index(), "Index"))
            b.S(sel, "B_INT", 10)
            return b.n("GeometryNodeDeleteGeometry", {"domain": "FACE", "mode": mode}, Geometry=(g, "Mesh"), Selection=(sel, "Result"))
        case(f"DeleteGeometry.FACE.{mode}", build_del2)
    for dom in ("POINT", "EDGE", "FACE"):
        for sel_name in ("Top", "Side", "Bottom"):
            def build_del3(b, dom=dom, sel_name=sel_name):
                cyl = b.n("GeometryNodeMeshCylinder", {"fill_type": "NGON"}, Vertices=8, Side__Segments=2)
                return b.n("GeometryNodeDeleteGeometry", {"domain": dom, "mode": "ALL"}, Geometry=(cyl, "Mesh"), Selection=(cyl, sel_name))
            case(f"DeleteGeometry.cyl.{dom}.{sel_name}", build_del3)
    for dom in ("FACE", "EDGE"):
        def build_ss(b, dom=dom):
            g = b.n("GeometryNodeMeshCube", Size=(1, 2, 3))
            return b.n("GeometryNodeSetShadeSmooth", {"domain": dom}, Geometry=(g, "Mesh"), Shade__Smooth=True)
        case(f"SetShadeSmooth.{dom}", build_ss)
    def build_flip(b):
        g = b.n("GeometryNodeMeshCylinder", Vertices=5)
        return b.n("GeometryNodeFlipFaces", Mesh=(g, "Mesh"))
    case("FlipFaces", build_flip)
    def build_flip_attr(b):
        g = b.n("GeometryNodeMeshCylinder", Vertices=5)
        s = b.store((g, "Mesh"), (b.index(), "Index"), "c", "CORNER", "INT")
        return b.n("GeometryNodeFlipFaces", Mesh=s)
    case("FlipFaces.cornerattr", build_flip_attr)
    def build_domsize(b):
        g = b.n("GeometryNodeMeshCylinder", Vertices=5)
        d = b.n("GeometryNodeAttributeDomainSize", {"component": "MESH"}, Geometry=(g, "Mesh"))
        line = b.n("GeometryNodeMeshLine", Count=1)
        cur = (line, "Mesh")
        for o in ("Point Count", "Edge Count", "Face Count", "Face Corner Count"):
            cur = (b.store(cur, (d, o), o.replace(" ", "_"), "POINT", "INT"), "Geometry")
        return cur
    case("DomainSize", build_domsize)
    return out


def curve2_cases():
    out = []
    def case(name, build):
        out.append((name, build))
    def wire(b, curve, attrs=True):
        """curve geometry -> store radius/tilt/tangent/spline params as attributes -> wire mesh"""
        cur = curve
        if attrs:
            cur = (b.store(cur, (b.n("GeometryNodeInputRadius"), "Radius"), "rad", "POINT", "FLOAT"), "Geometry")
            cur = (b.store(cur, (b.n("GeometryNodeInputCurveTilt"), "Tilt"), "til", "POINT", "FLOAT"), "Geometry")
            cur = (b.store(cur, (b.n("GeometryNodeInputTangent"), "Tangent"), "tan", "POINT", "FLOAT_VECTOR"), "Geometry")
            sp = b.n("GeometryNodeSplineParameter")
            cur = (b.store(cur, (sp, "Factor"), "fac", "POINT", "FLOAT"), "Geometry")
            cur = (b.store(cur, (sp, "Length"), "len", "POINT", "FLOAT"), "Geometry")
            cur = (b.store(cur, (sp, "Index"), "idx", "POINT", "INT"), "Geometry")
            cur = (b.store(cur, (b.n("GeometryNodeInputNormal"), "Normal"), "nor", "POINT", "FLOAT_VECTOR"), "Geometry")
        return b.n("GeometryNodeCurveToMesh", Curve=cur)
    def bez(b):
        return (b.n("GeometryNodeCurveQuadraticBezier", Resolution=7, Start=(0, 0, 0), Middle=(1, 0.2, 1.5), End=(2.5, 1.0, 1.0)), "Curve")
    def radius_curve(b):
        c = bez(b)
        idx = b.index()
        rad = b.math("MULTIPLY_ADD", (idx, "Index"), 0.15, 0.5)
        r = b.n("GeometryNodeSetCurveRadius", Curve=c, Radius=rad)
        t = b.n("GeometryNodeSetCurveTilt", Curve=(r, "Curve"), Tilt=b.math("MULTIPLY", (idx, "Index"), 0.1))
        return (t, "Curve")
    circ = lambda b: (b.n("GeometryNodeCurvePrimitiveCircle", {"mode": "RADIUS"}, Resolution=6, Radius=1.5), "Curve")
    case("SplineParameter.bezier", lambda b: wire(b, bez(b)))
    case("SplineParameter.circle", lambda b: wire(b, circ(b)))
    for (a_, b_, cnt_) in (((-1.1234, 0, 0), (1.0987, 0, 0), 200), ((0.3, 0.1, 0), (-1.7, 0.4, 0.2), 33)):
        case(f"ResampleLine.{cnt_}", lambda b, a_=a_, b_=b_, cnt_=cnt_: wire(b, (b.n("GeometryNodeResampleCurve", {"mode": "COUNT"}, Curve=(b.n("GeometryNodeCurvePrimitiveLine", {"mode": "POINTS"}, Start=a_, End=b_), "Curve"), Count=cnt_), "Curve")))
    case("SetCurveRadius.Tilt", lambda b: wire(b, radius_curve(b)))
    for mode, kw in (("COUNT", {"Count": 5}), ("COUNT", {"Count": 11}), ("LENGTH", {"Length": 0.7}), ("LENGTH", {"Length": 0.8})):
        for src in ("bezier", "circle"):
            def mk(b, mode=mode, kw=kw, src=src):
                c = radius_curve(b) if src == "bezier" else circ(b)
                n = b.n("GeometryNodeResampleCurve", {"mode": mode}, Curve=c, **kw)
                if mode == "LENGTH":
                    pass
                return wire(b, (n, "Curve"))
            case(f"ResampleCurve.{mode}.{list(kw.values())[0]}.{src}", mk)
    case("ResampleCurve.EVALUATED", lambda b: wire(b, (b.n("GeometryNodeResampleCurve", {"mode": "EVALUATED"}, Curve=radius_curve(b)), "Curve")))
    for mode, kw in (("COUNT", {"Count": 6}), ("LENGTH", {"Length": 0.6}), ("EVALUATED", {})):
        def mk(b, mode=mode, kw=kw):
            c = radius_curve(b)
            n = b.n("GeometryNodeCurveToPoints", {"mode": mode}, Curve=c, **kw)
            m = b.n("GeometryNodePointsToVertices", Points=(n, "Points"))
            cur = (m, "Mesh")
            for o, dt in (("Tangent", "FLOAT_VECTOR"), ("Normal", "FLOAT_VECTOR"), ("Rotation", "QUATERNION")):
                if dt == "QUATERNION":
                    continue
                cur = (b.store(cur, (n, o), o.lower(), "POINT", dt), "Geometry")
            return cur
        case(f"CurveToPoints.{mode}", mk)
    for mode in ("FACTOR", "LENGTH"):
        def mk(b, mode=mode):
            c = radius_curve(b)
            g = b.n("GeometryNodeMeshGrid", Size__X=2.0, Size__Y=1.0, Vertices__X=3, Vertices__Y=2)
            x = b.sep((b.pos(), "Position"))
            f = b.n("ShaderNodeMapRange", {"clamp": True}, Value=(x, "X"))
            f.inputs["From Min"].default_value = -1.0
            f.inputs["From Max"].default_value = 1.0
            f.inputs["To Min"].default_value = 0.0
            f.inputs["To Max"].default_value = 1.0 if mode == "FACTOR" else 3.0
            sc = b.n("GeometryNodeSampleCurve", {"mode": mode}, Curves=c)
            b.ng.links.new(b.out_sock((f, "Result")), sc.inputs["Factor" if mode == "FACTOR" else "Length"])
            cur = (g, "Mesh")
            for o, dt in (("Position", "FLOAT_VECTOR"), ("Tangent", "FLOAT_VECTOR"), ("Normal", "FLOAT_VECTOR")):
                cur = (b.store(cur, (sc, o), o.lower(), "POINT", dt), "Geometry")
            return cur
        case(f"SampleCurve.{mode}", mk)
    def mesh2curve(b, line=True):
        if line:
            g = b.n("GeometryNodeMeshLine", Count=6, Offset=(0.5, 0.2, -0.3))
        else:
            g = b.n("GeometryNodeMeshCircle", Vertices=7, Radius=1.1)
        return b.n("GeometryNodeMeshToCurve", Mesh=(g, "Mesh"))
    case("MeshToCurve.line", lambda b: wire(b, (mesh2curve(b), "Curve"), False))
    case("MeshToCurve.circle", lambda b: wire(b, (mesh2curve(b, False), "Curve"), False))
    case("CurveLength", lambda b: b.store((b.n("GeometryNodeMeshLine", Count=1), "Mesh"), (b.n("GeometryNodeCurveLength", Curve=bez(b)), "Length"), "L", "POINT", "FLOAT"))
    case("FillCurve.tri", lambda b: b.n("GeometryNodeFillCurve", {"mode": "TRIANGLES"}, Curve=circ(b)))
    case("FillCurve.ngon", lambda b: b.n("GeometryNodeFillCurve", {"mode": "NGONS"}, Curve=circ(b)))
    for rs in (5, 8, 16, 32, 7, 12):
        case(f"FillCurve.circle{rs}", lambda b, rs=rs: b.n("GeometryNodeFillCurve", {"mode": "TRIANGLES"}, Curve=(b.n("GeometryNodeCurvePrimitiveCircle", {"mode": "RADIUS"}, Resolution=rs, Radius=0.5), "Curve")))
    case("FillCurve.star", lambda b: b.n("GeometryNodeFillCurve", {"mode": "TRIANGLES"}, Curve=(b.n("GeometryNodeCurveStar", Points=7, Inner__Radius=0.4, Outer__Radius=1.0), "Curve")))
    case("FilletCurve.quadfill5", lambda b: wire(b, (b.n("GeometryNodeFilletCurve", {"mode": "POLY"}, Curve=(b.n("GeometryNodeCurvePrimitiveQuadrilateral", Width=0.2, Height=0.7), "Curve"), Radius=0.1, Count=5), "Curve")))
    case("FillCurve.quadfillet", lambda b: b.n("GeometryNodeFillCurve", {"mode": "TRIANGLES"}, Curve=(b.n("GeometryNodeFilletCurve", {"mode": "POLY"}, Curve=(b.n("GeometryNodeCurvePrimitiveQuadrilateral", Width=0.2, Height=0.7), "Curve"), Radius=0.1, Count=5), "Curve")))
    case("SubdivideCurve", lambda b: wire(b, (b.n("GeometryNodeSubdivideCurve", Curve=radius_curve(b), Cuts=2), "Curve")))
    for st, en in ((0.2, 0.8), (0.0, 0.55)):
        for mode in ("FACTOR", "LENGTH"):
            def mk(b, st=st, en=en, mode=mode):
                k = {"Start": st, "End": en} if mode == "FACTOR" else {"Start_001": st * 3, "End_001": en * 3}
                n = b.n("GeometryNodeTrimCurve", {"mode": mode}, Curve=radius_curve(b), **k)
                return wire(b, (n, "Curve"))
            case(f"TrimCurve.{mode}.{st}.{en}", mk)
    for lim in (True, False):
        def mk(b, lim=lim):
            q = b.n("GeometryNodeCurvePrimitiveQuadrilateral", Width=2.0, Height=1.0)
            n = b.n("GeometryNodeFilletCurve", {"mode": "POLY"}, Curve=(q, "Curve"), Radius=0.3, Limit__Radius=lim)
            n.inputs["Count"].default_value = 4
            return wire(b, (n, "Curve"), False)
        case(f"FilletCurve.POLY.limit{lim}", mk)
    for res_, rad_, lim_ in ((4, 0.0, True), (4, 0.2, True), (4, 0.5, True), (4, 0.5, False), (6, 0.3, True), (6, 1.5, True)):
        def mk(b, res_=res_, rad_=rad_, lim_=lim_):
            arc = b.n("GeometryNodeCurveArc", {"mode": "RADIUS"}, Resolution=res_, Radius=0.7071, Sweep__Angle=4.7124)
            n = b.n("GeometryNodeFilletCurve", {"mode": "POLY"}, Curve=(arc, "Curve"), Radius=rad_, Limit__Radius=lim_)
            n.inputs["Count"].default_value = 8
            return wire(b, (n, "Curve"), False)
        case(f"FilletCurve.arc.{res_}.{rad_}.{lim_}", mk)
    for sy_, rad_ in ((3.05, 3.05), (1.5, 0.4), (0.5, 2.0)):
        def mk(b, sy_=sy_, rad_=rad_):
            arc = b.n("GeometryNodeCurveArc", {"mode": "RADIUS"}, Resolution=4, Radius=0.7071, Sweep__Angle=4.7124)
            t1 = b.n("GeometryNodeTransform", Geometry=(arc, "Curve"), Rotation=(0, 0, -0.7854))
            t2 = b.n("GeometryNodeTransform", Geometry=t1, Rotation=(0, 1.5708, 0))
            t3 = b.n("GeometryNodeTransform", Geometry=t2, Translation=(0, 0.5, 0))
            t4 = b.n("GeometryNodeTransform", Geometry=t3, Scale=(1, sy_, 1))
            n = b.n("GeometryNodeFilletCurve", {"mode": "POLY"}, Curve=t4, Radius=rad_, Limit__Radius=True)
            n.inputs["Count"].default_value = 8
            return wire(b, (n, "Curve"), False)
        case(f"FilletCurve.scaled.{sy_}.{rad_}", mk)
    for nres_, rad_, cnt_ in ((3, 0.2, 4), (3, 3.0, 8), (5, 0.1, 8), (8, 0.5, 2), (3, -0.2, 4), (5, -0.5, 4)):
        def mk(b, nres_=nres_, rad_=rad_, cnt_=cnt_):
            cc = b.n("GeometryNodeCurvePrimitiveCircle", {"mode": "RADIUS"}, Resolution=nres_, Radius=1.0)
            n = b.n("GeometryNodeFilletCurve", {"mode": "POLY"}, Curve=(cc, "Curve"), Radius=rad_, Limit__Radius=True)
            n.inputs["Count"].default_value = cnt_
            return wire(b, (n, "Curve"), False)
        case(f"FilletCurve.circle.{nres_}.{rad_}.{cnt_}", mk)
    import random as _r
    _rr = _r.Random(5)
    for k_ in range(14):
        cfg_ = (_rr.choice([4, 5, 6]), _rr.uniform(0.2, 3.0), _rr.uniform(0.2, 3.0), _rr.uniform(0.05, 2.0), _rr.choice([2, 4, 8]), _rr.choice([True, False]), _rr.choice([4.7124, 3.0, 6.0]))
        def mk(b, cfg_=cfg_):
            res_, sx_, sy_, rad_, cnt_, lim_, sw_ = cfg_
            arc = b.n("GeometryNodeCurveArc", {"mode": "RADIUS"}, Resolution=res_, Radius=0.7071, Sweep__Angle=sw_)
            t4 = b.n("GeometryNodeTransform", Geometry=(arc, "Curve"), Scale=(sx_, sy_, 1))
            n = b.n("GeometryNodeFilletCurve", {"mode": "POLY"}, Curve=t4, Radius=rad_, Limit__Radius=lim_)
            n.inputs["Count"].default_value = cnt_
            return wire(b, (n, "Curve"), False)
        case(f"FilletCurve.rand.{k_}", mk)
    case("CurveArc", lambda b: wire(b, (b.n("GeometryNodeCurveArc", {"mode": "RADIUS"}, Resolution=7, Radius=1.3, Start__Angle=0.3, Sweep__Angle=4.0), "Curve"), False))
    case("CurveSpiral", lambda b: wire(b, (b.n("GeometryNodeCurveSpiral", Resolution=8, Rotations=2.0, Start__Radius=1.0, End__Radius=2.0, Height=3.0), "Curve"), False))
    case("CurveBezierSegment", lambda b: wire(b, (b.n("GeometryNodeCurvePrimitiveBezierSegment", Resolution=6, Start=(0, 0, 0), Start__Handle=(1, 0, 0), End__Handle=(-1, 1, 0), End=(2, 2, 1)), "Curve"), False))
    case("ReverseCurve", lambda b: wire(b, (b.n("GeometryNodeReverseCurve", Curve=radius_curve(b)), "Curve")))
    for st in ("CATMULL_ROM", "BEZIER", "POLY", "NURBS"):
        for src in ("bezier", "circle"):
            def mk(b, st=st, src=src):
                c = radius_curve(b) if src == "bezier" else circ(b)
                n = b.n("GeometryNodeCurveSplineType", {"spline_type": st}, Curve=c)
                return wire(b, (n, "Curve"))
            case(f"SplineType.{st}.{src}", mk)
    def bez5(b):
        c = (b.n("GeometryNodeCurveSplineType", {"spline_type": "BEZIER"}, Curve=(b.n("GeometryNodeMeshToCurve", Mesh=(b.n("GeometryNodeMeshCircle", Vertices=6, Radius=1.2), "Mesh")), "Curve")), "Curve")
        return c
    for ht in ("AUTO", "VECTOR", "ALIGN", "FREE"):
        for md in ({"LEFT"}, {"RIGHT"}, {"LEFT", "RIGHT"}):
            def mk(b, ht=ht, md=md):
                n = b.n("GeometryNodeCurveSetHandles", {"handle_type": ht, "mode": md}, Curve=bez5(b), Selection=b.n("FunctionNodeCompare", {"data_type": "INT", "operation": "LESS_THAN"}, A=(b.index(), "Index"), B=3) if False else True)
                return wire(b, (n, "Curve"), False)
            case(f"SetHandleType.{ht}.{'_'.join(sorted(md))}", mk)
    for side in ("LEFT", "RIGHT"):
        for pre in ("AUTO", "ALIGN", "VECTOR", "FREE"):
            def mk(b, side=side, pre=pre):
                c = bez5(b)
                if pre != "AUTO":
                    c = (b.n("GeometryNodeCurveSetHandles", {"handle_type": pre, "mode": {"LEFT", "RIGHT"}}, Curve=c), "Curve")
                off = b.n("ShaderNodeCombineXYZ", X=0.2, Y=0.35, Z=0.1)
                n = b.n("GeometryNodeSetCurveHandlePositions", {"mode": side}, Curve=c, Offset=(off, "Vector"))
                return wire(b, (n, "Curve"), False)
            case(f"SetHandlePositions.{side}.{pre}", mk)
    def mk_sp(b):
        c = bez5(b)
        idx = b.index()
        off = b.n("ShaderNodeCombineXYZ", X=0.0, Y=(b.math("MULTIPLY", (idx, "Index"), 0.07), "Value"), Z=0.0)
        n = b.n("GeometryNodeSetPosition", Geometry=c, Offset=(off, "Vector"))
        return wire(b, (n, "Geometry"), False)
    case("SetPosition.bezier", mk_sp)
    case("CurveStar", lambda b: wire(b, (b.n("GeometryNodeCurveStar", Points=5, Inner__Radius=0.5, Outer__Radius=1.2, Twist=0.3), "Curve"), False))
    def spl_len(b):
        c = circ(b)
        n = b.n("GeometryNodeSplineLength")
        cur = (b.store(c, (n, "Length"), "L", "POINT", "FLOAT"), "Geometry")
        cur = (b.store(cur, (n, "Point Count"), "pc", "POINT", "INT"), "Geometry")
        return wire(b, cur, False)
    case("SplineLength", spl_len)
    case("CurveEndpointSelection", lambda b: b.store(radius_curve(b), (b.n("GeometryNodeCurveEndpointSelection", Start__Size=2, End__Size=1), "Selection"), "e", "POINT", "BOOLEAN") if False else _endpoint(b, radius_curve(b), wire))
    return out


def _endpoint(b, c, wire):
    n = b.n("GeometryNodeCurveEndpointSelection", Start__Size=2, End__Size=1)
    s = b.store(c, (n, "Selection"), "e", "POINT", "BOOLEAN")
    return wire(b, (s, "Geometry"), False)


def field2_cases():
    out = []
    def case(name, build):
        out.append((name, build))
    def sph(b):
        return b.n("GeometryNodeMeshUVSphere", Segments=7, Rings=5, Radius=1.3)
    def cubeish(b):
        c = b.n("GeometryNodeMeshCube", Size=(1.0, 2.0, 3.0), Vertices__X=3, Vertices__Y=2, Vertices__Z=2)
        return b.n("GeometryNodeSetPosition", Geometry=(c, "Mesh"), Offset=(b.n("ShaderNodeCombineXYZ", X=b.math("MULTIPLY", (b.index(), "Index"), 0.013), Y=0.0, Z=b.math("MULTIPLY", (b.index(), "Index"), 0.021))))
    for dom in ("POINT", "FACE"):
        def mk(b, dom=dom):
            g = sph(b)
            other = b.n("GeometryNodeMeshGrid", Size__X=2.0, Size__Y=1.0, Vertices__X=3, Vertices__Y=2)
            # sample position.x of the sphere at index = (index*3)%7 on dom
            ix = b.math("MODULO", b.math("MULTIPLY", (b.index(), "Index"), 3.0), 7.0)
            ixi = b.n("FunctionNodeFloatToInt", Float=ix)
            sx = b.sep((b.pos(), "Position"))
            si = b.n("GeometryNodeSampleIndex", {"domain": dom, "data_type": "FLOAT"}, Geometry=(g, "Mesh"), Value=(sx, "X"), Index=(ixi, "Integer"))
            return b.store((other, "Mesh"), (si, "Value"), "r", "POINT", "FLOAT")
        case(f"SampleIndex.{dom}", mk)
    def mk_named(b):
        g = base_grid(b)
        s1 = b.store((g, "Mesh"), b.math("MULTIPLY", (b.index(), "Index"), 0.5), "src", "POINT", "FLOAT")
        na = b.n("GeometryNodeInputNamedAttribute", {"data_type": "FLOAT"}, Name="src")
        nb_ = b.n("GeometryNodeInputNamedAttribute", {"data_type": "FLOAT"}, Name="missing")
        cur = (b.store((s1, "Geometry"), (na, "Attribute"), "a", "POINT", "FLOAT"), "Geometry")
        cur = (b.store(cur, (na, "Exists"), "ae", "POINT", "BOOLEAN"), "Geometry")
        cur = (b.store(cur, (nb_, "Attribute"), "b", "POINT", "FLOAT"), "Geometry")
        cur = (b.store(cur, (nb_, "Exists"), "be", "POINT", "BOOLEAN"), "Geometry")
        return cur
    case("NamedAttribute", mk_named)
    def mk_named_dom(b):
        g = base_grid(b)
        s1 = b.store((g, "Mesh"), b.math("MULTIPLY", (b.index(), "Index"), 0.5), "src", "FACE", "FLOAT")
        na = b.n("GeometryNodeInputNamedAttribute", {"data_type": "FLOAT"}, Name="src")
        return b.store((s1, "Geometry"), (na, "Attribute"), "a", "POINT", "FLOAT")
    case("NamedAttribute.domain", mk_named_dom)
    def mk_sw_geo(b):
        a = b.n("GeometryNodeMeshCube", Size=(1, 1, 1))
        c = b.n("GeometryNodeMeshUVSphere", Segments=5, Rings=3, Radius=0.5)
        sw = b.n("GeometryNodeSwitch", {"input_type": "GEOMETRY"}, Switch=True)
        b.S(sw, "False", (a, "Mesh"))
        b.S(sw, "True", (c, "Mesh"))
        return sw
    case("Switch.GEOMETRY", mk_sw_geo)
    for it, a, bb in (("VECTOR", (1, 2, 3), (4, 5, 6)), ("INT", 3, 8), ("BOOLEAN", True, False), ("RGBA", (0.1, 0.2, 0.3, 1), (0.5, 0.6, 0.7, 1))):
        def mk_sw(b, it=it, a=a, bb=bb):
            g = base_grid(b)
            cond = b.n("FunctionNodeCompare", {"operation": "GREATER_THAN"}, A=(b.sep((b.pos(), "Position")), "X"), B=0.1)
            sw = b.n("GeometryNodeSwitch", {"input_type": it})
            b.ng.links.new(b.out_sock((cond, "Result")), sw.inputs["Switch"])
            b.S(sw, "False", a)
            b.S(sw, "True", bb)
            dt = {"VECTOR": "FLOAT_VECTOR", "INT": "INT", "BOOLEAN": "BOOLEAN", "RGBA": "FLOAT_COLOR"}[it]
            return b.store((g, "Mesh"), (sw, "Output"), "r", "POINT", dt)
        case(f"Switch.{it}.field", mk_sw)
    def mk_group(b):
        sub = bpy.data.node_groups.new("subgrp", "GeometryNodeTree")
        sub.interface.new_socket("Geometry", in_out="INPUT", socket_type="NodeSocketGeometry")
        sub.interface.new_socket("Scale", in_out="INPUT", socket_type="NodeSocketFloat")
        sub.interface.new_socket("Field", in_out="INPUT", socket_type="NodeSocketFloat")
        sub.interface.new_socket("Geometry", in_out="OUTPUT", socket_type="NodeSocketGeometry")
        gi = sub.nodes.new("NodeGroupInput")
        go = sub.nodes.new("NodeGroupOutput")
        sp = sub.nodes.new("GeometryNodeSetPosition")
        cx = sub.nodes.new("ShaderNodeCombineXYZ")
        mu = sub.nodes.new("ShaderNodeMath")
        mu.operation = "MULTIPLY"
        sub.links.new(gi.outputs["Field"], mu.inputs[0])
        sub.links.new(gi.outputs["Scale"], mu.inputs[1])
        sub.links.new(mu.outputs[0], cx.inputs["Z"])
        sub.links.new(gi.outputs["Geometry"], sp.inputs["Geometry"])
        sub.links.new(cx.outputs[0], sp.inputs["Offset"])
        sub.links.new(sp.outputs[0], go.inputs[0])
        g = base_grid(b)
        grp = b.ng.nodes.new("GeometryNodeGroup")
        grp.node_tree = sub
        b.ng.links.new(b.out_sock((g, "Mesh")), grp.inputs["Geometry"])
        grp.inputs["Scale"].default_value = 0.7
        x, y, z = noisy_xyz(b)
        b.ng.links.new(b.out_sock(x), grp.inputs["Field"])
        return grp
    case("GeometryNodeGroup.field", mk_group)
    def mk_clamp(b):
        g = sph(b)
        other = b.n("GeometryNodeMeshGrid", Size__X=2.0, Size__Y=1.0, Vertices__X=4, Vertices__Y=3)
        si = b.n("GeometryNodeSampleIndex", {"domain": "FACE", "data_type": "FLOAT_VECTOR", "clamp": True}, Geometry=(g, "Mesh"), Value=(b.pos(), "Position"), Index=(b.n("ShaderNodeMath", {"operation": "ADD"}), "Value") if False else (b.index(), "Index"))
        return b.store((other, "Mesh"), (si, "Value"), "r", "POINT", "FLOAT_VECTOR")
    case("SampleIndex.clamp.vector", mk_clamp)
    def mk_fai(b):
        g = sph(b)
        sx = b.sep((b.pos(), "Position"))
        fi = b.n("GeometryNodeFieldAtIndex", {"domain": "POINT", "data_type": "FLOAT"}, Index=3, Value=(sx, "X"))
        return b.store((g, "Mesh"), (fi, "Value"), "r", "POINT", "FLOAT")
    case("FieldAtIndex", mk_fai)
    def mk_fod(b):
        g = sph(b)
        fi = b.n("GeometryNodeFieldOnDomain", {"domain": "FACE", "data_type": "FLOAT_VECTOR"}, Value=(b.pos(), "Position"))
        return b.store((g, "Mesh"), (fi, "Value"), "r", "POINT", "FLOAT_VECTOR")
    case("FieldOnDomain", mk_fod)
    for dt in ("FLOAT", "INT", "FLOAT_VECTOR"):
        for dom in ("POINT", "FACE"):
            def mk(b, dt=dt, dom=dom):
                g = sph(b)
                if dt == "FLOAT":
                    v = (b.sep((b.pos(), "Position")), "X")
                elif dt == "INT":
                    v = (b.index(), "Index")
                else:
                    v = (b.pos(), "Position")
                grp = b.math("MODULO", (b.index(), "Index"), 3.0) if False else None
                ac = b.n("GeometryNodeAccumulateField", {"data_type": dt, "domain": dom}, Value=v)
                cur = (g, "Mesh")
                for o in ("Leading", "Trailing", "Total"):
                    cur = (b.store(cur, (ac, o), o.lower(), dom, dt), "Geometry")
                return cur
            case(f"AccumulateField.{dt}.{dom}", mk)
    for nm, o, dom, dt in (("EdgeVertices", "Vertex Index 1", "EDGE", "INT"), ("EdgeVertices", "Vertex Index 2", "EDGE", "INT"), ("EdgeVertices", "Position 1", "EDGE", "FLOAT_VECTOR"), ("EdgeVertices", "Position 2", "EDGE", "FLOAT_VECTOR"),
                           ("FaceArea", "Area", "FACE", "FLOAT"), ("EdgeAngle", "Unsigned Angle", "EDGE", "FLOAT"), ("EdgeAngle", "Signed Angle", "EDGE", "FLOAT"),
                           ("EdgeNeighbors", "Face Count", "EDGE", "INT"), ("FaceNeighbors", "Vertex Count", "FACE", "INT"), ("FaceNeighbors", "Face Count", "FACE", "INT"),
                           ("VertexNeighbors", "Vertex Count", "POINT", "INT"), ("VertexNeighbors", "Face Count", "POINT", "INT")):
        def mk(b, nm=nm, o=o, dom=dom, dt=dt):
            g = cubeish(b)
            n = b.n(f"GeometryNodeInputMesh{nm}")
            return b.store((g, "Geometry"), (n, o), "r", dom, dt)
        case(f"Mesh{nm}.{o}", mk)
    def mk_fa_pt(b):
        g = cubeish(b)
        n = b.n("GeometryNodeInputMeshFaceArea")
        return b.store((g, "Geometry"), (n, "Area"), "r", "POINT", "FLOAT")
    case("MeshFaceArea.point", mk_fa_pt)
    for mode in ("ROUND", "FLOOR", "CEILING", "TRUNCATE"):
        def mk(b, mode=mode):
            g = b.n("GeometryNodeMeshGrid", Size__X=3.7, Size__Y=2.9, Vertices__X=6, Vertices__Y=4)
            f = b.n("FunctionNodeFloatToInt", {"rounding_mode": mode}, Float=(b.sep((b.pos(), "Position")), "X"))
            return b.store((g, "Mesh"), (f, "Integer"), "r", "POINT", "INT")
        case(f"FloatToInt.{mode}", mk)
    for md in ("RGB", "HSV", "HSL"):
        def mk(b, md=md):
            g = b.n("GeometryNodeMeshGrid", Size__X=3.7, Size__Y=2.9, Vertices__X=5, Vertices__Y=4)
            p = b.pos()
            sx = b.sep((p, "Position"))
            cc = b.n("FunctionNodeCombineColor", {"mode": md}, Red=b.math("ABSOLUTE", (sx, "X"), clamp=True), Green=b.math("ABSOLUTE", (sx, "Y"), clamp=True), Blue=0.6, Alpha=0.9)
            sc = b.n("FunctionNodeSeparateColor", {"mode": md}, Color=(cc, "Color"))
            cur = (b.store((g, "Mesh"), (cc, "Color"), "col", "POINT", "FLOAT_COLOR"), "Geometry")
            for o in ("Red", "Green", "Blue", "Alpha"):
                cur = (b.store(cur, (sc, o), o.lower(), "POINT", "FLOAT"), "Geometry")
            return cur
        case(f"CombineSeparateColor.{md}", mk)
    for te in ("POINTS", "EDGES", "FACES"):
        def mk(b, te=te):
            tgt = cubeish(b)
            g = b.n("GeometryNodeMeshGrid", Size__X=3.7, Size__Y=2.9, Vertices__X=5, Vertices__Y=4)
            pos2 = b.vmath("ADD", (b.pos(), "Position"), (0.3, 0.2, 1.1))
            pr = b.n("GeometryNodeProximity", {"target_element": te}, Target=(tgt, "Geometry"), Source__Position=(pos2, "Vector"))
            cur = (b.store((g, "Mesh"), (pr, "Position"), "p", "POINT", "FLOAT_VECTOR"), "Geometry")
            cur = (b.store(cur, (pr, "Distance"), "d", "POINT", "FLOAT"), "Geometry")
            return cur
        case(f"Proximity.{te}", mk)
    for dom in ("POINT", "EDGE", "FACE"):
        def mk(b, dom=dom):
            tgt = cubeish(b)
            g = b.n("GeometryNodeMeshGrid", Size__X=3.7, Size__Y=2.9, Vertices__X=5, Vertices__Y=4)
            pos2 = b.vmath("ADD", (b.pos(), "Position"), (0.3, 0.2, 1.1))
            sn = b.n("GeometryNodeSampleNearest", {"domain": dom}, Geometry=(tgt, "Geometry"), Sample__Position=(pos2, "Vector"))
            return b.store((g, "Mesh"), (sn, "Index"), "i", "POINT", "INT")
        case(f"SampleNearest.{dom}", mk)
    def mk_objinfo(b):
        me = bpy.data.meshes.new("objmesh")
        me.from_pydata([(-1, -1, 0), (1, -1, 0), (1, 1, 0), (-1, 1, 0), (0, 0, 1)], [], [(0, 1, 2, 3), (0, 1, 4), (1, 2, 4), (2, 3, 4), (3, 0, 4)])
        me.update()
        ob = bpy.data.objects.new("TestObj", me)
        bpy.context.scene.collection.objects.link(ob)
        ob.location = (1.0, 2.0, 3.0)
        ob.rotation_euler = (0.3, 0.2, 0.5)
        ob.scale = (1.5, 1.0, 0.7)
        bpy.context.view_layer.update()
        info = b.n("GeometryNodeObjectInfo", {"transform_space": "RELATIVE"}, Object=ob, As__Instance=False)
        cur = info
        g = b.n("GeometryNodeMeshLine", Count=1)
        cur = (b.store((g, "Mesh"), (info, "Location"), "loc", "POINT", "FLOAT_VECTOR"), "Geometry")
        cur = (b.store(cur, (info, "Scale"), "scl", "POINT", "FLOAT_VECTOR"), "Geometry")
        r2e = b.n("FunctionNodeRotationToEuler", Rotation=(info, "Rotation"))
        cur = (b.store(cur, (r2e, "Euler"), "rot", "POINT", "FLOAT_VECTOR"), "Geometry")
        return b.n("GeometryNodeJoinGeometry", Geometry=[(info, "Geometry"), cur])
    case("ObjectInfo.relative", mk_objinfo)
    def mk_objinfo2(b):
        me = bpy.data.meshes.new("objmesh2")
        me.from_pydata([(-1, -1, 0), (1, -1, 0), (1, 1, 0), (-1, 1, 0)], [], [(0, 1, 2, 3)])
        me.update()
        ob = bpy.data.objects.new("TestObj2", me)
        bpy.context.scene.collection.objects.link(ob)
        ob.location = (1.0, 2.0, 3.0)
        ob.rotation_euler = (0.3, 0.2, 0.5)
        bpy.context.view_layer.update()
        info = b.n("GeometryNodeObjectInfo", {"transform_space": "ORIGINAL"}, Object=ob, As__Instance=True)
        return b.n("GeometryNodeRealizeInstances", Geometry=(info, "Geometry"))
    case("ObjectInfo.original.instance", mk_objinfo2)
    def mk_collinfo(b, sep, reset, tag):
        def mkobj(nm, pts, loc, rot=(0, 0, 0), sc=(1, 1, 1)):
            me = bpy.data.meshes.new(nm)
            me.from_pydata(pts, [], [(0, 1, 2)] if len(pts) == 3 else [(0, 1, 2, 3)])
            me.update()
            ob = bpy.data.objects.new(nm, me)
            ob.location = loc
            ob.rotation_euler = rot
            ob.scale = sc
            return ob
        col = bpy.data.collections.new("TCol" + tag)
        child = bpy.data.collections.new("Child" + tag)
        child.instance_offset = (0.5, 0.0, 0.25)
        col.instance_offset = (1.0, -1.0, 0.0)
        col.children.link(child)
        for ob in (mkobj("obj10" + tag, [(0, 0, 0), (1, 0, 0), (0, 1, 0)], (1, 2, 3), (0.1, 0.2, 0.3)),
                   mkobj("Obj2" + tag, [(-1, -1, 0), (1, -1, 0), (1, 1, 0), (-1, 1, 0)], (0, 0, 2), sc=(1.5, 1.0, 0.7))):
            col.objects.link(ob)
        child.objects.link(mkobj("ChildObj" + tag, [(0, 0, 0), (2, 0, 0), (0, 2, 0)], (3, 0, 0), (0, 0, 0.7)))
        info = b.n("GeometryNodeCollectionInfo", {"transform_space": "ORIGINAL"}, Collection=col, Separate__Children=sep, Reset__Children=reset)
        return b.n("GeometryNodeRealizeInstances", Geometry=(info, "Instances"))
    for sep, reset in ((False, False), (True, False), (True, True)):
        case(f"CollectionInfo.{sep}.{reset}", lambda b, sep=sep, reset=reset: mk_collinfo(b, sep, reset, f"{sep}{reset}"))
    def mk_paths(b):
        g = b.n("GeometryNodeMeshGrid", Size__X=2.0, Size__Y=1.0, Vertices__X=5, Vertices__Y=4)
        isend = b.n("FunctionNodeCompare", {"operation": "EQUAL", "data_type": "INT"})
        b.S(isend, "A_INT", (b.index(), "Index"))
        b.S(isend, "B_INT", 0)
        # edge cost: 1 + distance-based variation from edge vertex positions
        ev_ = b.n("GeometryNodeInputMeshEdgeVertices")
        cost = b.math("ADD", 1.0, b.math("MULTIPLY", (b.sep((ev_, "Position 1")), "Z") if False else (b.sep((ev_, "Position 1")), "X"), 0.3))
        sp = b.n("GeometryNodeInputShortestEdgePaths", **{"End Vertex": (isend, "Result"), "Edge Cost": cost})
        cur = (b.store((g, "Mesh"), (sp, "Next Vertex Index"), "nx", "POINT", "INT"), "Geometry")
        cur = (b.store(cur, (sp, "Total Cost"), "tc", "POINT", "FLOAT"), "Geometry")
        return cur
    case("ShortestEdgePaths", mk_paths)
    def mk_epc(b):
        g = b.n("GeometryNodeMeshGrid", Size__X=2.0, Size__Y=1.0, Vertices__X=5, Vertices__Y=4)
        isend = b.n("FunctionNodeCompare", {"operation": "EQUAL", "data_type": "INT"})
        b.S(isend, "A_INT", (b.index(), "Index"))
        b.S(isend, "B_INT", 0)
        sp = b.n("GeometryNodeInputShortestEdgePaths", **{"End Vertex": (isend, "Result"), "Edge Cost": 1.0})
        st = b.n("FunctionNodeCompare", {"operation": "GREATER_THAN", "data_type": "INT"})
        b.S(st, "A_INT", (b.index(), "Index"))
        b.S(st, "B_INT", 16)
        e = b.n("GeometryNodeEdgePathsToCurves", Mesh=(g, "Mesh"), Start__Vertices=(st, "Result"), Next__Vertex__Index=(sp, "Next Vertex Index"))
        return b.n("GeometryNodeCurveToMesh", Curve=(e, "Curves"))
    case("EdgePathsToCurves", mk_epc)
    def mk_ray(b):
        tgt = cubeish(b)
        g = b.n("GeometryNodeMeshGrid", Size__X=1.8, Size__Y=1.4, Vertices__X=5, Vertices__Y=4)
        pos2 = b.vmath("ADD", (b.pos(), "Position"), (0.0, 0.0, 4.0))
        rc = b.n("GeometryNodeRaycast", {"data_type": "FLOAT_VECTOR"}, **{"Target Geometry": (tgt, "Geometry")}, Attribute=(b.pos(), "Position"), Source__Position=(pos2, "Vector"), Ray__Direction=(0.1, 0.05, -1.0), Ray__Length=6.0)
        cur = (g, "Mesh")
        for o, dt in (("Is Hit", "BOOLEAN"), ("Hit Position", "FLOAT_VECTOR"), ("Hit Normal", "FLOAT_VECTOR"), ("Hit Distance", "FLOAT"), ("Attribute", "FLOAT_VECTOR")):
            cur = (b.store(cur, (rc, o), o.replace(" ", "_"), "POINT", dt), "Geometry")
        return cur
    case("Raycast", mk_ray)
    def mk_sns(b):
        tgt = b.n("GeometryNodeMeshUVSphere", Segments=6, Rings=4, Radius=1.0)
        g = b.n("GeometryNodeMeshGrid", Size__X=3.7, Size__Y=2.9, Vertices__X=5, Vertices__Y=4)
        pos2 = b.vmath("ADD", (b.pos(), "Position"), (0.3, 0.2, 1.1))
        sn = b.n("GeometryNodeSampleNearestSurface", {"data_type": "FLOAT"}, Mesh=(tgt, "Mesh"), Value=(b.sep((b.pos(), "Position")), "X"), Sample__Position=(pos2, "Vector"))
        return b.store((g, "Mesh"), (sn, "Value"), "v", "POINT", "FLOAT")
    case("SampleNearestSurface", mk_sns)
    return out


def _boolean(b, op, m1, others, solver="EXACT"):
    if solver == "FLOAT" and op != "DIFFERENCE" and m1 is not None:
        # UNION / INTERSECT take all operands through the multi-input "Mesh 2" socket
        others = [m1] + list(others)
        m1 = None
    n = b.n("GeometryNodeMeshBoolean", {"operation": op, "solver": solver}, **({"Mesh 1": m1} if m1 is not None else {}))
    for o in others:
        b.ng.links.new(b.out_sock(o), n.inputs["Mesh 2"])
    return n


def mesh2_cases():
    out = []
    def case(name, build):
        out.append((name, build))
    def up_sel(b):
        n = b.n("GeometryNodeInputNormal")
        d = b.vmath("DOT_PRODUCT", (n, "Normal"), (0, 0, 1))
        return (b.n("FunctionNodeCompare", {"operation": "GREATER_THAN"}, A=(d, "Value"), B=0.5), "Result")
    case("Extrude.FACES.quad", lambda b: b.n("GeometryNodeExtrudeMesh", {"mode": "FACES"}, Mesh=(b.n("GeometryNodeMeshGrid", Size__X=2.0, Size__Y=1.0, Vertices__X=2, Vertices__Y=2), "Mesh"), Offset__Scale=0.5))
    case("Extrude.FACES.ngon", lambda b: b.n("GeometryNodeExtrudeMesh", {"mode": "FACES"}, Mesh=(b.n("GeometryNodeMeshCircle", {"fill_type": "NGON"}, Vertices=6, Radius=1.0), "Mesh"), Offset__Scale=0.5))
    case("Extrude.FACES.fan", lambda b: b.n("GeometryNodeExtrudeMesh", {"mode": "FACES"}, Mesh=(b.n("GeometryNodeMeshCircle", {"fill_type": "TRIANGLE_FAN"}, Vertices=6, Radius=1.0), "Mesh"), Offset__Scale=0.5))
    case("Extrude.FACES.grid3", lambda b: b.n("GeometryNodeExtrudeMesh", {"mode": "FACES"}, Mesh=(b.n("GeometryNodeMeshGrid", Size__X=2.0, Size__Y=2.0, Vertices__X=3, Vertices__Y=3), "Mesh"), Offset__Scale=0.5))
    case("Extrude.FACES.cube.top", lambda b: b.n("GeometryNodeExtrudeMesh", {"mode": "FACES"}, Mesh=(b.n("GeometryNodeMeshCube", Size=(1, 1, 1)), "Mesh"), Selection=up_sel(b), Offset__Scale=0.7))
    case("Extrude.FACES.cube.all", lambda b: b.n("GeometryNodeExtrudeMesh", {"mode": "FACES"}, Mesh=(b.n("GeometryNodeMeshCube", Size=(1, 1, 1)), "Mesh"), Offset__Scale=0.3))
    case("Extrude.FACES.offset", lambda b: b.n("GeometryNodeExtrudeMesh", {"mode": "FACES"}, Mesh=(b.n("GeometryNodeMeshCube", Size=(1, 1, 1)), "Mesh"), Selection=up_sel(b), Offset=(0.2, 0.1, 1.0), Offset__Scale=0.7))
    case("Extrude.FACES.individual", lambda b: b.n("GeometryNodeExtrudeMesh", {"mode": "FACES"}, Mesh=(b.n("GeometryNodeMeshGrid", Size__X=2.0, Size__Y=2.0, Vertices__X=3, Vertices__Y=3), "Mesh"), Individual=True, Offset__Scale=0.5))
    case("Extrude.EDGES.circle", lambda b: b.n("GeometryNodeExtrudeMesh", {"mode": "EDGES"}, Mesh=(b.n("GeometryNodeMeshCircle", Vertices=5, Radius=1.0), "Mesh"), Offset=(0, 0, 1.0)))
    case("Extrude.EDGES.line", lambda b: b.n("GeometryNodeExtrudeMesh", {"mode": "EDGES"}, Mesh=(b.n("GeometryNodeMeshLine", Count=4, Offset=(1, 0, 0)), "Mesh"), Offset=(0, 0.3, 1.0)))
    case("Extrude.VERTICES", lambda b: b.n("GeometryNodeExtrudeMesh", {"mode": "VERTICES"}, Mesh=(b.n("GeometryNodeMeshLine", Count=4, Offset=(1, 0, 0)), "Mesh"), Offset=(0, 0.3, 1.0)))
    def chain(b):
        e1 = b.n("GeometryNodeExtrudeMesh", {"mode": "EDGES"}, Mesh=(b.n("GeometryNodeMeshCircle", Vertices=5, Radius=1.0), "Mesh"), Offset=(0, 0, 1.0))
        e2 = b.n("GeometryNodeExtrudeMesh", {"mode": "FACES"}, Mesh=(e1, "Mesh"), Selection=(e1, "Side"), Offset__Scale=0.2)
        return b.store((e2, "Mesh"), (b.n("GeometryNodeInputNormal"), "Normal"), "nor", "FACE", "FLOAT_VECTOR")
    case("Extrude.chain", chain)
    def fillext(b, shape):
        if shape == "circle":
            c = b.n("GeometryNodeCurvePrimitiveCircle", {"mode": "RADIUS"}, Resolution=12, Radius=0.5)
        else:
            q = b.n("GeometryNodeCurvePrimitiveQuadrilateral", Width=0.2, Height=0.7)
            c = b.n("GeometryNodeFilletCurve", {"mode": "POLY"}, Curve=(q, "Curve"), Radius=0.1)
            c.inputs["Count"].default_value = 5
        f = b.n("GeometryNodeFillCurve", Curve=(c, "Curve"))
        return b.n("GeometryNodeExtrudeMesh", {"mode": "FACES"}, Mesh=(f, "Mesh"), Offset__Scale=0.15)
    def s2c(b, text, size, ax):
        return b.n("GeometryNodeStringToCurves", {"align_x": ax, "align_y": "BOTTOM_BASELINE"}, String=text, Size=size)
    def real(b, n, sock):
        return b.n("GeometryNodeRealizeInstances", Geometry=(n, sock))
    def fillr(b, text, size, ax="CENTER"):
        return real(b, b.n("GeometryNodeFillCurve", Curve=(s2c(b, text, size, ax), "Curve Instances")), "Mesh")
    case("Text.curves.Bd", lambda b: real(b, s2c(b, "Bd", 1.0, "CENTER"), "Curve Instances"))
    case("Text.curves.BrandName", lambda b: real(b, s2c(b, "BrandName", 0.05, "CENTER"), "Curve Instances"))
    case("Text.fill.Brand", lambda b: fillr(b, "Brand8", 0.37))
    for ch in "BrandNmeabcdefghijklopqstuvwxyzACDEFGHIJKLMOPQRSTUVWXYZ0123456789":
        case(f"Text.fillch.{ch}", lambda b, ch=ch: fillr(b, ch, 0.05))
    for w in ["Br", "Bran", "BrandN", "BrandNam", "BrandName"]:
        case(f"Text.fillw.{w}", lambda b, w=w: fillr(b, w, 0.05))
    for sz in (0.0453, 0.0527, 0.058, 0.04736, 0.047359142):
        case(f"Text.fillsz.{sz}", lambda b, sz=sz: fillr(b, "BrandName", sz))
    case("Text.fill.left", lambda b: fillr(b, "oiQ", 2.0, "LEFT"))
    def fillext(b, t):
        return real(b, b.n("GeometryNodeExtrudeMesh", {"mode": "FACES"}, Mesh=(b.n("GeometryNodeFillCurve", Curve=(s2c(b, t, 0.5, "CENTER"), "Curve Instances")), "Mesh"), Offset__Scale=0.05), "Mesh")
    for t in "lTHoRa":
        case(f"Text.fillext1.{t}", lambda b, t=t: fillext(b, t))
    case("Text.fillext", lambda b: real(b, b.n("GeometryNodeExtrudeMesh", {"mode": "FACES"}, Mesh=(b.n("GeometryNodeFillCurve", Curve=(s2c(b, "Ra", 0.5, "CENTER"), "Curve Instances")), "Mesh"), Offset__Scale=0.05), "Mesh"))
    case("Extrude.fill.circle", lambda b: fillext(b, "circle"))
    case("Extrude.fill.fillet", lambda b: fillext(b, "fillet"))
    def cube_at(b, size, t, rot=(0, 0, 0)):
        c = b.n("GeometryNodeMeshCube", Size=size)
        return b.n("GeometryNodeTransform", Geometry=(c, "Mesh"), Translation=t, Rotation=rot)
    for op in ("UNION", "INTERSECT", "DIFFERENCE"):
        case(f"MeshBoolean.{op}.cubes", lambda b, op=op: b.n("GeometryNodeMeshBoolean", {"operation": op, "solver": "EXACT"}, Mesh_1=(cube_at(b, (1, 1, 1), (0, 0, 0)), "Geometry"), Mesh_2=[(cube_at(b, (1, 1, 1), (0.3, 0.2, 0.1), (0.2, 0.3, 0.4)), "Geometry")]) if False else _boolean(b, op, cube_at(b, (1, 1, 1), (0, 0, 0)), [cube_at(b, (1, 1, 1), (0.3, 0.2, 0.1), (0.2, 0.3, 0.4))]))
    def thin(b, n=(4, 6, 10), solver="EXACT"):
        c = b.n("GeometryNodeMeshCube", Size=(0.012, 0.05, 0.1), Vertices__X=n[0], Vertices__Y=n[1], Vertices__Z=n[2])
        cy = b.n("GeometryNodeMeshCylinder", Radius=0.004, Depth=0.1)
        t = b.n("GeometryNodeTransform", Geometry=(cy, "Mesh"), Translation=(0, 0, 0.02), Rotation=(0.0, 1.5708, 0.0), Scale=(1.2, 1, 1))
        return _boolean(b, "DIFFERENCE", c, [t], solver)
    case("MeshBoolean.DIFFERENCE.thin", thin)
    case("MeshBoolean.DIFFERENCE.thin2", lambda b: thin(b, (2, 2, 2)))
    case("MeshBoolean.DIFFERENCE.hole", lambda b: _boolean(b, "DIFFERENCE", cube_at(b, (2, 2, 2), (0, 0, 0)), [b.n("GeometryNodeMeshCylinder", Vertices=12, Radius=0.4, Depth=4.0)]))
    case("MeshBoolean.DIFFERENCE.multi", lambda b: _boolean(b, "DIFFERENCE", cube_at(b, (2, 2, 2), (0, 0, 0)), [cube_at(b, (0.5, 0.5, 4), (0.3, 0.3, 0)), cube_at(b, (0.5, 4, 0.5), (-0.3, 0, 0.2))]))
    # FLOAT solver (BMesh intersect)
    for op in ("UNION", "INTERSECT", "DIFFERENCE"):
        case(f"MeshBoolean.FLOAT.{op}.cubes", lambda b, op=op: _boolean(b, op, cube_at(b, (1, 1, 1), (0, 0, 0)), [cube_at(b, (1, 1, 1), (0.3, 0.2, 0.1), (0.2, 0.3, 0.4))], "FLOAT"))
        case(f"MeshBoolean.FLOAT.{op}.cylcube", lambda b, op=op: _boolean(b, op, cube_at(b, (2, 2, 2), (0, 0, 0)), [b.n("GeometryNodeMeshCylinder", Vertices=12, Radius=0.7, Depth=1.0)], "FLOAT"))
        case(f"MeshBoolean.FLOAT.{op}.sphere", lambda b, op=op: _boolean(b, op, b.n("GeometryNodeMeshUVSphere", Segments=12, Rings=8, Radius=1.0), [cube_at(b, (1.2, 1.2, 1.2), (0.5, 0.3, 0.2), (0.0, 0.0, 0.3))], "FLOAT"))
    for nv_ in (3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 14, 16):
        case(f"MeshBoolean.FLOATH.hole{nv_}", lambda b, nv_=nv_: _boolean(b, "DIFFERENCE", cube_at(b, (2, 2, 2), (0, 0, 0)), [b.n("GeometryNodeMeshCylinder", Vertices=nv_, Radius=0.4, Depth=4.0)], "FLOAT"))
    case("MeshBoolean.FLOAT.DIFFERENCE.hole", lambda b: _boolean(b, "DIFFERENCE", cube_at(b, (2, 2, 2), (0, 0, 0)), [b.n("GeometryNodeMeshCylinder", Vertices=12, Radius=0.4, Depth=4.0)], "FLOAT"))
    case("MeshBoolean.FLOAT.DIFFERENCE.holegrid", lambda b: _boolean(b, "DIFFERENCE", b.n("GeometryNodeMeshCube", Size=(2, 2, 2), Vertices__X=4, Vertices__Y=4, Vertices__Z=4), [b.n("GeometryNodeMeshCylinder", Vertices=12, Radius=0.4, Depth=4.0)], "FLOAT"))
    case("MeshBoolean.FLOAT.DIFFERENCE.multi", lambda b: _boolean(b, "DIFFERENCE", cube_at(b, (2, 2, 2), (0, 0, 0)), [cube_at(b, (0.5, 0.5, 4), (0.3, 0.3, 0)), cube_at(b, (0.5, 4, 0.5), (-0.3, 0, 0.2))], "FLOAT"))
    case("MeshBoolean.FLOAT.UNION.multi", lambda b: _boolean(b, "UNION", None, [cube_at(b, (1, 1, 1), (0, 0, 0)), cube_at(b, (1, 1, 1), (0.3, 0.2, 0.1), (0.2, 0.3, 0.4)), cube_at(b, (1, 1, 1), (-0.3, 0.1, 0.5), (0.4, 0.1, 0.2))], "FLOAT"))
    def inst_cutters(b, nx=4, nz=3):
        # Microwave-style button grid: a small cube -> instance -> DuplicateElements (INSTANCE) x2 -> SetPosition offsets
        c = cube_at(b, (0.03, 0.03, 0.01), (0.1, 0.0, 0.05))
        gi = b.n("GeometryNodeGeometryToInstance")
        b.ng.links.new(b.out_sock((c, "Geometry")), gi.inputs["Geometry"])
        d1 = b.n("GeometryNodeDuplicateElements", {"domain": "INSTANCE"}, Geometry=(gi, "Instances"), Amount=nx)
        off1 = b.n("ShaderNodeCombineXYZ", X=b.math("MULTIPLY", (d1, "Duplicate Index"), 0.04))
        sp1 = b.n("GeometryNodeSetPosition", Geometry=(d1, "Geometry"), Offset=off1)
        d2 = b.n("GeometryNodeDuplicateElements", {"domain": "INSTANCE"}, Geometry=(sp1, "Geometry"), Amount=nz)
        off2 = b.n("ShaderNodeCombineXYZ", Z=b.math("MULTIPLY", (d2, "Duplicate Index"), 0.02))
        sp2 = b.n("GeometryNodeSetPosition", Geometry=(d2, "Geometry"), Offset=off2)
        return d2, sp2
    def mk_inst(b):
        base = cube_at(b, (0.5, 0.2, 0.3), (0.2, 0.0, 0.1))
        d2, sp2 = inst_cutters(b)
        return _boolean(b, "DIFFERENCE", base, [(d2, "Geometry"), sp2], "FLOAT")
    case("MeshBoolean.FLOAT.DIFFERENCE.instances", mk_inst)
    def mk_pattr(b):
        c = b.n("GeometryNodeMeshCube", Size=(2, 2, 2), Vertices__X=4, Vertices__Y=4, Vertices__Z=4)
        st = b.store((c, "Mesh"), (b.index(), "Index"), "pi", "POINT", "INT")
        st2 = b.store((st, "Geometry"), (b.index(), "Index"), "ei", "EDGE", "INT")
        st3 = b.store((st2, "Geometry"), (b.index(), "Index"), "fi", "FACE", "INT")
        return _boolean(b, "DIFFERENCE", st3, [b.n("GeometryNodeMeshCylinder", Vertices=8, Radius=0.4, Depth=4.0)], "FLOAT")
    case("MeshBoolean.FLOAT.DIFFERENCE.pattr", mk_pattr)
    case("MeshBoolean.FLOAT.DIFFERENCE.thin", lambda b: thin(b, solver="FLOAT"))
    case("MeshBoolean.FLOAT.DIFFERENCE.thin2", lambda b: thin(b, (2, 2, 2), "FLOAT"))
    case("MeshBoolean.FLOAT.UNION.attr", lambda b: _boolean(b, "UNION", b.n("GeometryNodeSetMaterialIndex", Geometry=(cube_at(b, (1, 1, 1), (0, 0, 0)), "Geometry"), Material__Index=2), [cube_at(b, (1, 1, 1), (0.3, 0.2, 0.1), (0.2, 0.3, 0.4))], "FLOAT"))
    case("MeshBoolean.UNION.attr", lambda b: _boolean(b, "UNION", b.n("GeometryNodeSetMaterialIndex", Geometry=(cube_at(b, (1, 1, 1), (0, 0, 0)), "Geometry"), Material__Index=2), [cube_at(b, (1, 1, 1), (0.3, 0.2, 0.1), (0.2, 0.3, 0.4))]))
    def two_cubes(b):
        c1 = b.n("GeometryNodeMeshCube", Size=(1, 1, 1))
        c2 = b.n("GeometryNodeTransform", Geometry=(b.n("GeometryNodeMeshCube", Size=(1, 1, 1)), "Mesh"), Translation=(0.5, 0, 0))
        return b.n("GeometryNodeJoinGeometry", Geometry=[(c1, "Mesh"), c2])
    case("MergeByDistance.cubes", lambda b: b.n("GeometryNodeMergeByDistance", Geometry=two_cubes(b), Distance=0.001))
    case("MergeByDistance.cube", lambda b: b.n("GeometryNodeMergeByDistance", Geometry=(b.n("GeometryNodeMeshCube", Size=(1, 1, 1), Vertices__X=3, Vertices__Y=3, Vertices__Z=3), "Mesh"), Distance=0.001))
    case("MergeByDistance.attr", lambda b: b.n("GeometryNodeMergeByDistance", Geometry=(b.store(two_cubes(b), (b.index(), "Index"), "pi", "POINT", "INT"), "Geometry"), Distance=0.001))
    for lvl in (1, 2):
        case(f"SubdivideMesh.cube.{lvl}", lambda b, lvl=lvl: b.n("GeometryNodeSubdivideMesh", Mesh=(b.n("GeometryNodeMeshCube", Size=(1, 2, 3)), "Mesh"), Level=lvl))
        case(f"SubdivideMesh.cyl.{lvl}", lambda b, lvl=lvl: b.n("GeometryNodeSubdivideMesh", Mesh=(b.n("GeometryNodeMeshCylinder", Vertices=5), "Mesh"), Level=lvl))
        case(f"SubdivisionSurface.cube.{lvl}", lambda b, lvl=lvl: b.n("GeometryNodeSubdivisionSurface", Mesh=(b.n("GeometryNodeMeshCube", Size=(1, 2, 3)), "Mesh"), Level=lvl))
        case(f"SubdivisionSurface.cyl.{lvl}", lambda b, lvl=lvl: b.n("GeometryNodeSubdivisionSurface", Mesh=(b.n("GeometryNodeMeshCylinder", Vertices=5), "Mesh"), Level=lvl))
    case("Triangulate.cube", lambda b: b.n("GeometryNodeTriangulate", Mesh=(b.n("GeometryNodeMeshCube", Size=(1, 2, 3)), "Mesh")))
    case("Triangulate.cyl", lambda b: b.n("GeometryNodeTriangulate", Mesh=(b.n("GeometryNodeMeshCylinder", Vertices=7), "Mesh")))
    case("Triangulate.minv", lambda b: b.n("GeometryNodeTriangulate", Mesh=(b.n("GeometryNodeMeshCylinder", Vertices=7), "Mesh"), Minimum__Vertices=5))
    def hull(b):
        g = b.n("GeometryNodeMeshGrid", Size__X=3.7, Size__Y=2.9, Vertices__X=5, Vertices__Y=4)
        x, y, z = noisy_xyz(b)
        h = b.math("MULTIPLY", b.math("SINE", z), 0.8)
        sp = b.n("GeometryNodeSetPosition", Geometry=(g, "Mesh"), Offset=b.n("ShaderNodeCombineXYZ", X=0.0, Y=0.0, Z=h))
        return b.n("GeometryNodeConvexHull", Geometry=(sp, "Geometry"))
    case("ConvexHull.grid", lambda b: (hull(b), "Convex Hull"))
    case("ConvexHull.sphere", lambda b: (b.n("GeometryNodeConvexHull", Geometry=(b.n("GeometryNodeMeshUVSphere", Segments=8, Rings=5, Radius=1.0), "Mesh")), "Convex Hull"))
    case("SplitEdges.cube", lambda b: b.n("GeometryNodeSplitEdges", Mesh=(b.n("GeometryNodeMeshCube", Size=(1, 2, 3), Vertices__X=3, Vertices__Y=3, Vertices__Z=3), "Mesh")))
    def split_sel(b):
        g = b.n("GeometryNodeMeshGrid", Size__X=2.0, Size__Y=2.0, Vertices__X=4, Vertices__Y=4)
        sel = b.n("FunctionNodeCompare", {"operation": "GREATER_THAN", "data_type": "INT"})
        b.S(sel, "A_INT", (b.index(), "Index"))
        b.S(sel, "B_INT", 8)
        return b.n("GeometryNodeSplitEdges", Mesh=(g, "Mesh"), Selection=(sel, "Result"))
    case("SplitEdges.grid.sel", split_sel)
    case("ScaleElements.face", lambda b: b.n("GeometryNodeScaleElements", {"domain": "FACE"}, Geometry=(b.n("GeometryNodeMeshGrid", Size__X=2.0, Size__Y=2.0, Vertices__X=3, Vertices__Y=3), "Mesh"), Scale=0.5))
    case("ScaleElements.edge", lambda b: b.n("GeometryNodeScaleElements", {"domain": "EDGE"}, Geometry=(b.n("GeometryNodeMeshGrid", Size__X=2.0, Size__Y=2.0, Vertices__X=3, Vertices__Y=3), "Mesh"), Scale=0.5))
    return out


def dup_cases():
    out = []
    def case(name, build):
        out.append((name, build))
    def grid(b):
        return b.n("GeometryNodeMeshGrid", Size__X=2.0, Size__Y=1.0, Vertices__X=3, Vertices__Y=2)
    amt = lambda b: b.math("MODULO", (b.index(), "Index"), 3.0)
    for dom in ("POINT", "EDGE", "FACE"):
        def mk(b, dom=dom):
            g = grid(b)
            d = b.n("GeometryNodeDuplicateElements", {"domain": dom}, Geometry=(g, "Mesh"), Amount=b.n("FunctionNodeFloatToInt", Float=amt(b)))
            return b.store((d, "Geometry"), (d, "Duplicate Index"), "di", dom, "INT")
        case(f"DuplicateElements.{dom}", mk)
    def mk_inst(b):
        c = b.n("GeometryNodeMeshCube", Size=(0.3, 0.2, 0.1))
        gi = b.n("GeometryNodeGeometryToInstance")
        b.ng.links.new(b.out_sock((c, "Mesh")), gi.inputs["Geometry"])
        d = b.n("GeometryNodeDuplicateElements", {"domain": "INSTANCE"}, Geometry=(gi, "Instances"), Amount=6)
        di = (d, "Duplicate Index")
        cx = b.n("ShaderNodeCombineXYZ", X=b.math("MODULO", di, 3.0), Y=b.math("FLOOR", b.math("DIVIDE", di, 3.0)), Z=b.math("MULTIPLY", b.math("FLOOR", b.math("DIVIDE", di, 3.0)), -0.2))
        sp = b.n("GeometryNodeSetPosition", Geometry=(d, "Geometry"), Offset=cx)
        r = b.n("GeometryNodeRealizeInstances", Geometry=(sp, "Geometry"))
        return b.store((r, "Geometry"), (b.index(), "Index"), "i", "POINT", "INT")
    case("DuplicateElements.INSTANCE", mk_inst)
    def mk_c(b):
        g = b.n("GeometryNodeCurvePrimitiveCircle", {"mode": "RADIUS"}, Resolution=4, Radius=1.0)
        d = b.n("GeometryNodeDuplicateElements", {"domain": "SPLINE"}, Geometry=(g, "Curve"), Amount=2)
        m = b.n("GeometryNodeCurveToMesh", Curve=(d, "Geometry"))
        return m
    case("DuplicateElements.SPLINE", mk_c)
    def mk_inst(b, nested):
        cl = b.n("GeometryNodeCurvePrimitiveLine", Start=(0, -1, 0), End=(0, 1, 0.5))
        g2i = b.n("GeometryNodeGeometryToInstance", Geometry=(cl, "Curve"))
        d = b.n("GeometryNodeDuplicateElements", {"domain": "INSTANCE"}, Geometry=(g2i, "Instances"), Amount=4)
        off = b.n("ShaderNodeCombineXYZ", X=b.math("MULTIPLY", b.math("SUBTRACT", (d, "Duplicate Index"), 2.0), 0.5))
        sp = b.n("GeometryNodeSetPosition", Geometry=(d, "Geometry"), Offset=(off, "Vector"))
        g = sp
        if nested:
            j = b.n("GeometryNodeJoinGeometry", Geometry=[(cl, "Curve"), (sp, "Geometry")])
            g2 = b.n("GeometryNodeGeometryToInstance", Geometry=(j, "Geometry"))
            d2 = b.n("GeometryNodeDuplicateElements", {"domain": "INSTANCE"}, Geometry=(g2, "Instances"), Amount=3)
            off2 = b.n("ShaderNodeCombineXYZ", Y=b.math("MULTIPLY", b.math("SUBTRACT", (d2, "Duplicate Index"), 1.0), 0.7))
            g = b.n("GeometryNodeSetPosition", Geometry=(d2, "Geometry"), Offset=(off2, "Vector"))
        r = b.n("GeometryNodeRealizeInstances", Geometry=(g, "Geometry"))
        return b.n("GeometryNodeCurveToMesh", Curve=(r, "Geometry"))
    case("DuplicateElements.INSTANCE.setpos", lambda b: mk_inst(b, False))
    case("DuplicateElements.INSTANCE.nested", lambda b: mk_inst(b, True))
    return out


def noise_cases():
    out = []
    def case(name, build):
        out.append((name, build))
    for dim in ("1D", "2D", "3D", "4D"):
        for norm in (True, False):
            def mk(b, dim=dim, norm=norm):
                g = base_grid(b)
                n = b.n("ShaderNodeTexNoise", {"noise_dimensions": dim, "normalize": norm}, Vector=(b.pos(), "Position"), Scale=2.3, Detail=2.0, Roughness=0.5, Lacunarity=2.0)
                if dim in ("1D", "4D"):
                    b.S(n, "W", 0.37)
                return b.store((g, "Mesh"), (n, "Fac"), "r", "POINT", "FLOAT")
            case(f"NoiseTexture.{dim}.{'norm' if norm else 'raw'}", mk)
    for det, rough, lac in ((0.0, 0.5, 2.0), (3.5, 0.7, 1.7), (7.0, 0.3, 2.5), (1.0, 1.0, 2.0)):
        def mk(b, det=det, rough=rough, lac=lac):
            g = base_grid(b)
            n = b.n("ShaderNodeTexNoise", Vector=(b.pos(), "Position"), Scale=1.9, Detail=det, Roughness=rough, Lacunarity=lac)
            return b.store((g, "Mesh"), (n, "Fac"), "r", "POINT", "FLOAT")
        case(f"NoiseTexture.d{det}.r{rough}.l{lac}", mk)
    def mk_dist(b):
        g = base_grid(b)
        n = b.n("ShaderNodeTexNoise", Vector=(b.pos(), "Position"), Scale=1.9, Detail=2.0, Roughness=0.5, Lacunarity=2.0, Distortion=0.4)
        return b.store((g, "Mesh"), (n, "Fac"), "r", "POINT", "FLOAT")
    case("NoiseTexture.distortion", mk_dist)
    for nt in ("MULTIFRACTAL", "RIDGED_MULTIFRACTAL", "HYBRID_MULTIFRACTAL", "HETERO_TERRAIN"):
        def mk(b, nt=nt):
            g = base_grid(b)
            n = b.n("ShaderNodeTexNoise", {"noise_type": nt, "normalize": False}, Vector=(b.pos(), "Position"), Scale=1.9, Detail=3.0, Roughness=0.5, Lacunarity=2.0)
            b.S(n, "Offset", 0.7)
            b.S(n, "Gain", 1.3)
            return b.store((g, "Mesh"), (n, "Fac"), "r", "POINT", "FLOAT")
        case(f"NoiseTexture.{nt}", mk)
    return out


def rot_cases():
    out = []
    def case(name, build):
        out.append((name, build))
    def vec_field(b):
        x, y, z = noisy_xyz(b)
        return b.n("ShaderNodeCombineXYZ", X=x, Y=y, Z=z)
    for ax in ("X", "Y", "Z"):
        for pv in ("AUTO", "X", "Y", "Z"):
            for rot in ((0.0, 0.0, 0.0), (0.3, 0.2, 0.9)):
                def mk(b, ax=ax, pv=pv, rot=rot):
                    n = b.n("FunctionNodeAlignEulerToVector", {"axis": ax, "pivot_axis": pv}, Rotation=rot, Factor=0.8, Vector=vec_field(b))
                    return n
                case(f"AlignEulerToVector.{ax}.{pv}.{rot[0]}", lambda b, mk=mk: None) if False else None
                out.append((f"AlignEulerToVector.{ax}.{pv}.{rot[0]}", None))
                out.pop()
                def build(b, mk=mk):
                    g = base_grid(b)
                    return b.store((g, "Mesh"), mk(b), "r", "POINT", "FLOAT_VECTOR")
                out.append((f"AlignEulerToVector.{ax}.{pv}.{rot[0]}", build))
    for mode in ("AXIS_ANGLE", "X_AXIS", "Y_AXIS", "Z_AXIS", "EULER_XYZ"):
        for inv in (False, True):
            def build(b, mode=mode, inv=inv):
                g = base_grid(b)
                vr = b.n("ShaderNodeVectorRotate", {"rotation_type": mode, "invert": inv}, Vector=vec_field(b), Center=(0.3, 0.2, 0.1), Axis=(0.3, 0.8, -0.5), Angle=0.7)
                if mode == "EULER_XYZ":
                    b.S(vr, "Rotation", (0.2, 0.5, -0.7))
                return b.store((g, "Mesh"), vr, "r", "POINT", "FLOAT_VECTOR")
            out.append((f"VectorRotate.{mode}.{inv}", build))
    for rt in ("EULER", "AXIS_ANGLE"):
        for sp in ("OBJECT", "LOCAL"):
            def build(b, rt=rt, sp=sp):
                g = base_grid(b)
                kw = {"Rotation": vec_field(b)}
                n = b.n("FunctionNodeRotateEuler", {"rotation_type": rt, "space": sp}, **kw)
                if rt == "EULER":
                    b.S(n, "Rotate By", (0.3, 0.5, 0.2))
                else:
                    b.S(n, "Axis", (0.3, 0.5, 0.2))
                    b.S(n, "Angle", 0.6)
                return b.store((g, "Mesh"), n, "r", "POINT", "FLOAT_VECTOR")
            out.append((f"RotateEuler.{rt}.{sp}", build))
    def build_q(b):
        g = base_grid(b)
        e2r = b.n("FunctionNodeEulerToRotation", Euler=vec_field(b))
        aa = b.n("FunctionNodeAxisAngleToRotation", Axis=(0.3, 0.4, 0.8), Angle=0.9)
        inv = b.n("FunctionNodeInvertRotation", Rotation=e2r)
        rv = b.n("FunctionNodeRotateVector", Vector=(b.pos(), "Position"), Rotation=(aa, "Rotation"))
        r2e = b.n("FunctionNodeRotationToEuler", Rotation=inv)
        cur = (b.store((g, "Mesh"), rv, "rv", "POINT", "FLOAT_VECTOR"), "Geometry")
        cur = (b.store(cur, r2e, "re", "POINT", "FLOAT_VECTOR"), "Geometry")
        return cur
    out.append(("RotationNodes", build_q))
    def build_mixrot(b):
        g = base_grid(b)
        a = b.n("FunctionNodeEulerToRotation", Euler=(0.3, 0.2, 0.5))
        c = b.n("FunctionNodeEulerToRotation", Euler=vec_field(b))
        mx = b.n("ShaderNodeMix", {"data_type": "ROTATION"}, Factor_Float=0.4)
        b.S(mx, "A_Rotation", (a, "Rotation"))
        b.S(mx, "B_Rotation", (c, "Rotation"))
        r2e = b.n("FunctionNodeRotationToEuler", Rotation=(mx, "Result_Rotation"))
        return b.store((g, "Mesh"), r2e, "r", "POINT", "FLOAT_VECTOR")
    out.append(("MixRotation", build_mixrot))
    def g2i(b):
        c = b.n("GeometryNodeMeshCube", Size=(0.5, 0.5, 0.5))
        s = b.n("GeometryNodeMeshUVSphere", Segments=5, Rings=3, Radius=0.3)
        g = b.n("GeometryNodeGeometryToInstance")
        b.ng.links.new(b.out_sock((c, "Mesh")), g.inputs["Geometry"])
        b.ng.links.new(b.out_sock((s, "Mesh")), g.inputs["Geometry"])
        return b.n("GeometryNodeRealizeInstances", Geometry=g)
    out.append(("GeometryToInstance", g2i))
    for seed, dens in ((0, 400.0), (7, 150.0)):
        for src in ("grid", "sphere", "cube"):
            def dist(b, seed=seed, dens=dens, src=src):
                g = {"grid": lambda: b.n("GeometryNodeMeshGrid", Size__X=1.3, Size__Y=0.9, Vertices__X=4, Vertices__Y=3),
                     "sphere": lambda: b.n("GeometryNodeMeshUVSphere", Segments=8, Rings=5, Radius=0.7),
                     "cube": lambda: b.n("GeometryNodeMeshCube", Size=(1.0, 0.8, 0.6))}[src]()
                d = b.n("GeometryNodeDistributePointsOnFaces", {"distribute_method": "RANDOM"}, Mesh=(g, "Mesh"), Density=dens, Seed=seed)
                m = b.n("GeometryNodePointsToVertices", Points=(d, "Points"))
                cur = (b.store((m, "Mesh"), (d, "Normal"), "nor", "POINT", "FLOAT_VECTOR"), "Geometry")
                r2e = b.n("FunctionNodeRotationToEuler", Rotation=(d, "Rotation"))
                cur = (b.store(cur, (r2e, "Euler"), "rot", "POINT", "FLOAT_VECTOR"), "Geometry")
                return cur
            out.append((f"DistributePointsOnFaces.{src}.{seed}", dist))
    for src in ("grid", "sphere"):
        for seed in (0, 3):
            def distp(b, src=src, seed=seed):
                g = {"grid": lambda: b.n("GeometryNodeMeshGrid", Size__X=1.3, Size__Y=0.9, Vertices__X=4, Vertices__Y=3),
                     "sphere": lambda: b.n("GeometryNodeMeshUVSphere", Segments=8, Rings=5, Radius=0.7)}[src]()
                d = b.n("GeometryNodeDistributePointsOnFaces", {"distribute_method": "POISSON"}, Mesh=(g, "Mesh"), Distance__Min=0.12, Density__Max=60.0, Density__Factor=0.9, Seed=seed)
                m = b.n("GeometryNodePointsToVertices", Points=(d, "Points"))
                cur = (b.store((m, "Mesh"), (d, "Normal"), "nor", "POINT", "FLOAT_VECTOR"), "Geometry")
                return cur
            out.append((f"DistributePointsOnFaces.poisson.{src}.{seed}", distp))
    def pts(b):
        p = b.n("GeometryNodePoints", Count=5, Position=b.n("ShaderNodeCombineXYZ", X=b.math("MULTIPLY", (b.index(), "Index"), 0.5), Y=1.0, Z=0.0), Radius=0.2)
        return b.n("GeometryNodePointsToVertices", Points=(p, "Geometry"))
    out.append(("Points", pts))
    for sub in (1, 2, 3):
        out.append((f"IcoSphere.{sub}", lambda b, sub=sub: b.n("GeometryNodeMeshIcoSphere", Radius=1.3, Subdivisions=sub)))
    return out


def tex_cases():
    out = []
    def case(name, build):
        out.append((name, build))
    for pts, ht in (([(0, 0), (0.3, 0.8), (1, 0.4)], "AUTO"), ([(0, 0), (0.3, 0.8), (1, 0.4)], "VECTOR")):
        def mk(b, pts=pts, ht=ht):
            g = base_grid(b)
            x, y, z = noisy_xyz(b)
            n = b.n("ShaderNodeFloatCurve", Value=x)
            c = n.mapping.curves[0]
            for i, p in enumerate(pts):
                if i < 2:
                    c.points[i].location = p
                else:
                    c.points.new(*p)
            for pt in c.points:
                pt.handle_type = ht
            n.mapping.update()
            return b.store((g, "Mesh"), n, "r", "POINT", "FLOAT")
        case(f"FloatCurve.unclamped.{ht}", mk)
    def mixed(b):
        g = base_grid(b)
        x, y, z = noisy_xyz(b)
        n = b.n("ShaderNodeFloatCurve", Value=x)
        c = n.mapping.curves[0]
        pts = [(0.0, 0.1), (0.2, 0.13), (0.4, 0.13), (0.6, 0.1), (0.8, 0.07), (1.0, 0.0)]
        for i, p in enumerate(pts):
            if i < 2:
                c.points[i].location = p
            else:
                c.points.new(*p)
        for i, pt in enumerate(c.points):
            pt.handle_type = "VECTOR" if i == len(pts) - 1 else "AUTO"
        n.mapping.update()
        return b.store((g, "Mesh"), n, "r", "POINT", "FLOAT")
    case("FloatCurve.mixed_auto_vector_end", mixed)
    def vc(b):
        g = base_grid(b)
        n = b.n("ShaderNodeVectorCurve", Vector=(b.pos(), "Position"), Fac=0.8)
        c = n.mapping.curves[0]
        c.points.new(0.3, 0.6)
        n.mapping.update()
        return b.store((g, "Mesh"), n, "r", "POINT", "FLOAT_VECTOR")
    case("VectorCurve", vc)
    for ft in ("F1", "F2", "SMOOTH_F1", "DISTANCE_TO_EDGE", "N_SPHERE_RADIUS"):
        for dm in ("EUCLIDEAN", "MANHATTAN", "CHEBYCHEV", "MINKOWSKI"):
            if ft in ("DISTANCE_TO_EDGE", "N_SPHERE_RADIUS") and dm != "EUCLIDEAN":
                continue
            def mk(b, ft=ft, dm=dm):
                g = base_grid(b)
                n = b.n("ShaderNodeTexVoronoi", {"feature": ft, "distance": dm}, Vector=(b.pos(), "Position"), Scale=3.1, Randomness=0.9)
                if ft == "SMOOTH_F1":
                    b.S(n, "Smoothness", 0.4)
                if dm == "MINKOWSKI":
                    b.S(n, "Exponent", 1.7)
                cur = (g, "Mesh")
                osock = "Radius" if ft == "N_SPHERE_RADIUS" else "Distance"
                cur = (b.store(cur, (n, osock), "r", "POINT", "FLOAT"), "Geometry")
                if ft in ("F1", "F2", "SMOOTH_F1"):
                    cur = (b.store(cur, (n, "Color"), "c", "POINT", "FLOAT_COLOR"), "Geometry")
                    cur = (b.store(cur, (n, "Position"), "p", "POINT", "FLOAT_VECTOR"), "Geometry")
                return cur
            case(f"Voronoi.{ft}.{dm}", mk)
    for kb in (False, True):
        case(f"DualMesh.ico.{kb}", lambda b, kb=kb: b.n("GeometryNodeDualMesh", Mesh=(b.n("GeometryNodeMeshIcoSphere", Subdivisions=2, Radius=1.0), "Mesh"), Keep__Boundaries=kb))
        case(f"DualMesh.grid.{kb}", lambda b, kb=kb: b.n("GeometryNodeDualMesh", Mesh=(b.n("GeometryNodeMeshGrid", Size__X=2.0, Size__Y=1.5, Vertices__X=4, Vertices__Y=5), "Mesh"), Keep__Boundaries=kb))
        case(f"DualMesh.uvsphere.{kb}", lambda b, kb=kb: b.n("GeometryNodeDualMesh", Mesh=(b.n("GeometryNodeMeshUVSphere", Segments=7, Rings=5, Radius=1.0), "Mesh"), Keep__Boundaries=kb))
        case(f"DualMesh.cylinder.{kb}", lambda b, kb=kb: b.n("GeometryNodeDualMesh", Mesh=(b.n("GeometryNodeMeshCylinder", Vertices=6, Radius=1.0, Depth=2.0), "Mesh"), Keep__Boundaries=kb))
        case(f"DualMesh.cube.{kb}", lambda b, kb=kb: b.n("GeometryNodeDualMesh", Mesh=(b.n("GeometryNodeMeshCube", Size=(1, 1, 1), Vertices__X=3, Vertices__Y=3, Vertices__Z=3), "Mesh"), Keep__Boundaries=kb))
    def sd_attr_case(name, base, level, node="GeometryNodeSubdivideMesh"):
        def mk(b):
            cur = (base(b), "Mesh")
            idx = b.index()
            for nm, dom, k in (("ap", "POINT", 0.37), ("ae", "EDGE", 0.11), ("ac", "CORNER", 0.05), ("af", "FACE", 0.2)):
                cur = (b.store(cur, b.math("MULTIPLY", (idx, "Index"), k), nm, dom, "FLOAT"), "Geometry")
            return b.n(node, Mesh=cur, Level=level)
        case(name, mk)
    for lv in (1, 2):
        sd_attr_case(f"SubdivAttrs.grid.{lv}", lambda b: b.n("GeometryNodeMeshGrid", Size__X=2.0, Size__Y=1.5, Vertices__X=4, Vertices__Y=3), lv)
        sd_attr_case(f"SubdivAttrs.cyl.{lv}", lambda b: b.n("GeometryNodeMeshCylinder", Vertices=5), lv)
    for lv in (1, 2):
        sd_attr_case(f"SubsurfAttrs.grid.{lv}", lambda b: b.n("GeometryNodeMeshGrid", Size__X=2.0, Size__Y=1.5, Vertices__X=4, Vertices__Y=3), lv, "GeometryNodeSubdivisionSurface")
        sd_attr_case(f"SubsurfAttrs.cyl.{lv}", lambda b: b.n("GeometryNodeMeshCylinder", Vertices=5), lv, "GeometryNodeSubdivisionSurface")
    case("Subsurf.crease", lambda b: b.n("GeometryNodeSubdivisionSurface", Mesh=(b.n("GeometryNodeMeshCube", Size=(1, 2, 3), Vertices__X=3, Vertices__Y=3, Vertices__Z=3), "Mesh"), Level=2, Edge__Crease=(b.n("FunctionNodeCompare", {"data_type": "INT", "operation": "LESS_THAN"}, A=(b.index(), "Index"), B=20) if False else 1.0)))
    def mat_case(name, f):
        def mk(b):
            g = base_grid(b)
            return b.store((g, "Mesh"), f(b), "r", "POINT", "FLOAT_VECTOR")
        case(name, mk)
    def cm(b, vals):
        n = b.n("FunctionNodeCombineMatrix")
        for k, v in vals.items():
            b.S(n, k, v)
        return n
    M1 = {"Column 1 Row 1": 0.9, "Column 1 Row 2": 0.3, "Column 2 Row 1": -0.2, "Column 2 Row 2": 1.1, "Column 3 Row 3": 1.3, "Column 4 Row 1": 0.5, "Column 4 Row 2": -0.4, "Column 4 Row 3": 0.2, "Column 3 Row 1": 0.1}
    mat_case("Matrix.point", lambda b: (b.n("FunctionNodeTransformPoint", Vector=(b.pos(), "Position"), Transform=(cm(b, M1), "Matrix")), "Vector"))
    mat_case("Matrix.direction", lambda b: (b.n("FunctionNodeTransformDirection", Direction=(b.pos(), "Position"), Transform=(cm(b, M1), "Matrix")), "Direction"))
    mat_case("Matrix.transpose", lambda b: (b.n("FunctionNodeTransformPoint", Vector=(b.pos(), "Position"), Transform=(b.n("FunctionNodeTransposeMatrix", Matrix=(cm(b, M1), "Matrix")), "Matrix")), "Vector"))
    mat_case("Matrix.invert", lambda b: (b.n("FunctionNodeTransformPoint", Vector=(b.pos(), "Position"), Transform=(b.n("FunctionNodeInvertMatrix", Matrix=(cm(b, M1), "Matrix")), "Matrix")), "Vector"))
    mat_case("Matrix.multiply", lambda b: (b.n("FunctionNodeTransformPoint", Vector=(b.pos(), "Position"), Transform=(b.n("FunctionNodeMatrixMultiply", Matrix=(cm(b, M1), "Matrix"), Matrix_001=(cm(b, {"Column 1 Row 1": 2.0, "Column 2 Row 3": 0.5, "Column 4 Row 3": 1.0}), "Matrix")), "Matrix")), "Vector"))
    mat_case("Matrix.sepcomb", lambda b: (b.n("FunctionNodeTransformPoint", Vector=(b.pos(), "Position"), Transform=(b.n("FunctionNodeTransposeMatrix", Matrix=(b.n("FunctionNodeTransposeMatrix", Matrix=(cm(b, M1), "Matrix")), "Matrix")), "Matrix")), "Vector"))
    def strjoin(b):
        g = base_grid(b)
        j = b.n("GeometryNodeStringJoin", Delimiter="-")
        for t in ("aa", "bb", "cc"):
            b.ng.links.new(b.out_sock(b.n("FunctionNodeInputString", {"string": t})), j.inputs["Strings"])
        st = b.n("GeometryNodeStoreNamedAttribute", {"data_type": "FLOAT", "domain": "POINT"}, Geometry=(g, "Mesh"), Name=(j, "String"), Value=1.0)
        return st
    case("StringJoin", strjoin)
    for ax in ("X", "Z"):
        for pv in ("AUTO", "Y"):
            if ax == "X" and pv == "X":
                continue
            def mk(b, ax=ax, pv=pv):
                g = base_grid(b)
                e2r = b.n("FunctionNodeEulerToRotation", Euler=(b.pos(), "Position"))
                n = b.n("FunctionNodeAlignRotationToVector", {"axis": ax, "pivot_axis": pv}, Rotation=(e2r, "Rotation"), Factor=0.8, Vector=(0.3, -0.7, 0.5))
                r2e = b.n("FunctionNodeRotationToEuler", Rotation=(n, "Rotation"))
                return b.store((g, "Mesh"), (r2e, "Euler"), "r", "POINT", "FLOAT_VECTOR")
            case(f"AlignRotationToVector.{ax}.{pv}", mk)
    def uv_case(name, mk_node):
        def mk(b):
            n = mk_node(b)
            return b.store((n, "Mesh"), (n, "UV Map"), "uvm", "CORNER", "FLOAT_VECTOR")
        case(name, mk)
    uv_case("UV.Cube", lambda b: b.n("GeometryNodeMeshCube", Size=(1.2, 0.7, 2.0), Vertices__X=3, Vertices__Y=4, Vertices__Z=2))
    uv_case("UV.Grid", lambda b: b.n("GeometryNodeMeshGrid", Size__X=2.0, Size__Y=1.5, Vertices__X=4, Vertices__Y=5))
    uv_case("UV.Sphere", lambda b: b.n("GeometryNodeMeshUVSphere", Segments=7, Rings=5, Radius=1.0))
    for ft in ("NGON", "TRIANGLE_FAN", "NONE"):
        uv_case(f"UV.Cylinder.{ft}", lambda b, ft=ft: b.n("GeometryNodeMeshCylinder", {"fill_type": ft}, Vertices=6, Side__Segments=2, Fill__Segments=2))
        uv_case(f"UV.Cone.{ft}", lambda b, ft=ft: b.n("GeometryNodeMeshCone", {"fill_type": ft}, Vertices=6, Side__Segments=2, Fill__Segments=2, Radius__Top=0.4))
        uv_case(f"UV.ConeTip.{ft}", lambda b, ft=ft: b.n("GeometryNodeMeshCone", {"fill_type": ft}, Vertices=6, Side__Segments=2, Fill__Segments=2))
    def vor_case(name, dim, ft, dm, extra=None, props=None):
        def mk(b):
            g = base_grid(b)
            pr = {"feature": ft, "distance": dm, "voronoi_dimensions": dim}
            pr.update(props or {})
            kw = {"Scale": 3.1, "Randomness": 0.9}
            kw.update(extra or {})
            n = b.n("ShaderNodeTexVoronoi", pr, Vector=(b.pos(), "Position"), **kw)
            if dim in ("1D", "4D"):
                b.S(n, "W", 0.37)
            if ft == "SMOOTH_F1":
                b.S(n, "Smoothness", 0.4)
            if dm == "MINKOWSKI" and dim != "1D":
                b.S(n, "Exponent", 1.7)
            cur = (g, "Mesh")
            osock = "Radius" if ft == "N_SPHERE_RADIUS" else "Distance"
            cur = (b.store(cur, (n, osock), "r", "POINT", "FLOAT"), "Geometry")
            if ft in ("F1", "F2", "SMOOTH_F1"):
                cur = (b.store(cur, (n, "Color"), "c", "POINT", "FLOAT_COLOR"), "Geometry")
                if dim != "1D":
                    cur = (b.store(cur, (n, "Position"), "p", "POINT", "FLOAT_VECTOR"), "Geometry")
                if dim in ("1D", "4D"):
                    cur = (b.store(cur, (n, "W"), "w", "POINT", "FLOAT"), "Geometry")
            return cur
        case(name, mk)
    for dim in ("1D", "2D", "4D"):
        for ft in ("F1", "F2", "SMOOTH_F1", "DISTANCE_TO_EDGE", "N_SPHERE_RADIUS"):
            vor_case(f"Voronoi.{dim}.{ft}", dim, ft, "EUCLIDEAN")
        if dim != "1D":
            vor_case(f"Voronoi.{dim}.F1.MANHATTAN", dim, "F1", "MANHATTAN")
            vor_case(f"Voronoi.{dim}.F2.CHEBYCHEV", dim, "F2", "CHEBYCHEV")
            vor_case(f"Voronoi.{dim}.SMOOTH_F1.MINKOWSKI", dim, "SMOOTH_F1", "MINKOWSKI")
    for ft in ("F1", "F2", "SMOOTH_F1", "DISTANCE_TO_EDGE"):
        for nm in (False, True):
            vor_case(f"Voronoi.3D.{ft}.fractal.{nm}", "3D", ft, "EUCLIDEAN", {"Detail": 2.5, "Roughness": 0.6, "Lacunarity": 2.1}, {"normalize": nm})
    vor_case("Voronoi.2D.F1.fractal", "2D", "F1", "EUCLIDEAN", {"Detail": 1.0, "Roughness": 0.5, "Lacunarity": 2.0}, {"normalize": True})
    for dim in ("1D", "2D", "3D", "4D"):
        def mk(b, dim=dim):
            g = base_grid(b)
            n = b.n("ShaderNodeTexWhiteNoise", {"noise_dimensions": dim}, Vector=(b.pos(), "Position"))
            b.S(n, "W", 0.4)
            return b.store((g, "Mesh"), (n, "Value"), "r", "POINT", "FLOAT")
        case(f"WhiteNoise.{dim}", mk)
    for wt, d1 in (("BANDS", "X"), ("BANDS", "DIAGONAL"), ("RINGS", "X"), ("RINGS", "SPHERICAL")):
        for prof in ("SIN", "SAW", "TRI"):
            for dist in (0.0, 1.5):
                def mk(b, wt=wt, d1=d1, prof=prof, dist=dist):
                    g = base_grid(b)
                    props = {"wave_type": wt, "wave_profile": prof}
                    props["bands_direction" if wt == "BANDS" else "rings_direction"] = d1
                    n = b.n("ShaderNodeTexWave", props, Vector=(b.pos(), "Position"), Scale=1.3, Distortion=dist, Detail=2.0, Detail__Scale=1.0, Detail__Roughness=0.5, Phase__Offset=0.3)
                    return b.store((g, "Mesh"), (n, "Fac"), "r", "POINT", "FLOAT")
                case(f"Wave.{wt}.{d1}.{prof}.{dist}", mk)
    return out


GROUPS = {
    "math": math_cases, "vmath": vector_math_cases, "fn": misc_fn_cases, "input": input_cases,
    "prim": primitive_cases, "curve": curve_cases, "geo": geometry_cases, "inst": inst_cases, "curve2": curve2_cases, "field2": field2_cases, "mesh2": mesh2_cases, "dup": dup_cases, "noise": noise_cases, "rot": rot_cases, "tex": tex_cases,
}


def build_cases(out, filt):
    cases = []
    allc = []
    for gname, fn in GROUPS.items():
        for c in fn():
            allc.append((gname, c))
    for gname, (name, build) in allc:
        full = f"{gname}:{name}"
        if filt and not any(f in full for f in filt):
            continue
        try:
            b = B(full)
            geo = build(b)
            ng = b.finish(geo)
            # group input geometry is unused: the base mesh is created inside the group
            GX.export_group(ng, out)
            exp = GC.eval_blender(ng, {})
            objs = {}
            for nm, o in b.objects.items():
                objs[nm] = {"mesh": GX.export_mesh(o.data) if o.type == "MESH" else None, "matrix": [x for r in o.matrix_world for x in r]}
            objs = {**out.get("objects", {}), **objs}
            cases.append({"name": full, "group": ng.name, "inputs": {}, "expect": exp, "objects": objs, "collections": out.get("collections")})
        except Exception as e:
            print("BUILD ERROR", full, repr(e))
    return cases
