"""bl5compat.py: Blender 5 compatibility for the infinigen code that render_rust.py borrows (materials, lighting).

Blender 5 removed ID-property access on geometry-nodes modifiers (`mod["Socket_2"] = v`, `mod["Socket_2_attribute_name"]`,
`mod["Socket_2_use_attribute"]`); values now live under `mod.properties.inputs.<id>.value / .type / .attribute_name` and
`mod.properties.outputs.<id>.attribute_name`. Infinigen (pinned to Blender 4.2) still uses the old form. `install()` re-execs
the few infinigen functions that do this with the subscripts rewritten to `_bl5_set` / `_bl5_get`, in this process only
(infinigen's files stay untouched). No-op on Blender < 5."""
import inspect
import re
import textwrap

import bpy


def _split(key):
    for suf, kind in (("_attribute_name", "attribute_name"), ("_use_attribute", "use_attribute")):
        if key.endswith(suf):
            return key[: -len(suf)], kind
    return key, "value"


def _slot(mod, sid):
    props = mod.properties
    if hasattr(props.inputs, sid):
        return getattr(props.inputs, sid), "in"
    if hasattr(props.outputs, sid):
        return getattr(props.outputs, sid), "out"
    raise KeyError(f"{mod.name}: no modifier socket {sid}")


def _bl5_set(mod, key, val):
    if getattr(mod, "type", None) != "NODES" or bpy.app.version < (5, 0, 0):
        mod[key] = val
        return
    sid, kind = _split(key)
    slot, _ = _slot(mod, sid)
    if kind == "attribute_name":
        slot.attribute_name = val
    elif kind == "use_attribute":
        slot.type = "ATTRIBUTE" if val else "VALUE"
    else:
        cur = slot.value
        if hasattr(cur, "__len__") and not isinstance(val, (str, bytes)) and not isinstance(cur, str):
            slot.value = tuple(val)
        else:
            slot.value = val


def _bl5_get(mod, key):
    if getattr(mod, "type", None) != "NODES" or bpy.app.version < (5, 0, 0):
        return mod[key]
    sid, kind = _split(key)
    slot, _ = _slot(mod, sid)
    if kind == "attribute_name":
        return slot.attribute_name
    if kind == "use_attribute":
        return slot.type == "ATTRIBUTE"
    return slot.value


_SET = re.compile(r"^(\s*)((?:self\.)?(?:mod|modifier))\[(.+)\]\s*=\s*(.+?)\s*$")
_GET = re.compile(r"((?:self\.)?(?:mod|modifier))\[([^\]\n]+)\]")


def _rewrite(src):
    out = []
    for line in src.splitlines():
        m = _SET.match(line)
        if m and "==" not in line:
            line = f"{m.group(1)}_bl5_set({m.group(2)}, {m.group(3)}, {m.group(4)})"
        else:
            line = _GET.sub(lambda g: f"_bl5_get({g.group(1)}, {g.group(2)})", line)
        out.append(line)
    return "\n".join(out) + "\n"


def _patch_function(module, owner, name):
    fn = getattr(owner, name)
    fn = getattr(fn, "__wrapped__", fn)
    src = textwrap.dedent(inspect.getsource(fn))
    src = re.sub(r"^@.*\n", "", src, flags=re.M)  # decorators (gin) are re-applied by keeping the original wrapper below
    g = module.__dict__
    g["_bl5_set"], g["_bl5_get"] = _bl5_set, _bl5_get
    loc = {}
    exec(compile(_rewrite(src), inspect.getsourcefile(fn), "exec"), g, loc)
    new = loc[fn.__name__]
    orig = getattr(owner, name)
    if hasattr(orig, "__wrapped__"):  # functools.wraps-style wrapper (gin): swap the wrapped function inside it
        try:
            orig.__wrapped__ = new
            if hasattr(orig, "__code__") and orig.__code__.co_freevars:
                for cell, var in zip(orig.__closure__ or (), orig.__code__.co_freevars):
                    if cell.cell_contents is fn:
                        cell.cell_contents = new
            return
        except (AttributeError, ValueError):
            pass
    setattr(owner, name, new)


_INSTALLED = [False]


def install():
    # once per process (the Blender server runs render_rust.py many times: patching twice broke the patched sources)
    if bpy.app.version < (5, 0, 0) or _INSTALLED[0]:
        return
    _INSTALLED[0] = True
    from infinigen.core import surface
    from infinigen.core.nodes import node_wrangler
    from infinigen.core.util import blender as butil
    _patch_function(surface, surface, "add_geomod")
    _patch_function(node_wrangler, node_wrangler.NodeWrangler, "expose_input")
    for name, fn in list(vars(butil).items()):
        if inspect.isfunction(fn) and fn.__module__ == butil.__name__:
            try:
                src = inspect.getsource(fn)
            except OSError:
                continue
            if re.search(r"\bmod\[soc\.identifier\]", src):
                _patch_function(butil, butil, name)
