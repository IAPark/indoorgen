import bpy, sys, numpy as np, json
bpy.ops.wm.read_factory_settings(use_empty=True)
bpy.ops.mesh.primitive_plane_add(size=2); obj = bpy.context.active_object
cases = json.loads(sys.argv[-1])  # list of [axis, pivot, vector, rotation]
out = []
for axis, pivot, vec, rot in cases:
    tree = bpy.data.node_groups.new("t", "GeometryNodeTree")
    tree.interface.new_socket("Geometry", in_out="INPUT", socket_type="NodeSocketGeometry")
    tree.interface.new_socket("Geometry", in_out="OUTPUT", socket_type="NodeSocketGeometry")
    N = tree.nodes
    gi = N.new("NodeGroupInput"); go = N.new("NodeGroupOutput")
    al = N.new("FunctionNodeAlignEulerToVector"); al.axis = axis; al.pivot_axis = pivot
    al.inputs["Vector"].default_value = vec; al.inputs["Rotation"].default_value = rot
    st = N.new("GeometryNodeStoreNamedAttribute"); st.data_type = "FLOAT_VECTOR"; st.domain = "POINT"; st.inputs["Name"].default_value = "rot"
    tree.links.new(gi.outputs[0], st.inputs["Geometry"]); tree.links.new(al.outputs[0], st.inputs["Value"]); tree.links.new(st.outputs[0], go.inputs[0])
    for m in list(obj.modifiers): obj.modifiers.remove(m)
    m = obj.modifiers.new("g", "NODES"); m.node_group = tree
    dg = bpy.context.evaluated_depsgraph_get(); e = obj.evaluated_get(dg).to_mesh()
    out.append([float(x) for x in e.attributes["rot"].data[0].vector])
print("ALIGN", json.dumps(out))
