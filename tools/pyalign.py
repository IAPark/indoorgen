import bpy, sys, numpy as np
bpy.ops.wm.read_factory_settings(use_empty=True)
bpy.ops.mesh.primitive_plane_add(size=2); obj = bpy.context.active_object
tree = bpy.data.node_groups.new("t", "GeometryNodeTree")
tree.interface.new_socket("Geometry", in_out="INPUT", socket_type="NodeSocketGeometry")
tree.interface.new_socket("Geometry", in_out="OUTPUT", socket_type="NodeSocketGeometry")
N = tree.nodes
gi = N.new("NodeGroupInput"); go = N.new("NodeGroupOutput")
al = N.new("FunctionNodeAlignEulerToVector"); al.axis = "X"; al.pivot_axis = "Z"
mode = sys.argv[-1]
if mode == "vec":
    al.inputs["Vector"].default_value = (0, 0, 0)
st = N.new("GeometryNodeStoreNamedAttribute"); st.data_type = "FLOAT_VECTOR"; st.domain = "POINT"; st.inputs["Name"].default_value = "rot"
tree.links.new(gi.outputs[0], st.inputs["Geometry"]); tree.links.new(al.outputs[0], st.inputs["Value"]); tree.links.new(st.outputs[0], go.inputs[0])
print("default vector", tuple(al.inputs["Vector"].default_value), "rot default", tuple(al.inputs["Rotation"].default_value))
m = obj.modifiers.new("g", "NODES"); m.node_group = tree
dg = bpy.context.evaluated_depsgraph_get(); e = obj.evaluated_get(dg).to_mesh()
print("ROT", [tuple(np.degrees(a.vector)) for a in e.attributes["rot"].data][:2])
