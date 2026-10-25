"""table_tpl.py: post-process the gn_export.py templates of TableDiningFactory (data/gn/table_{square,stand,straight}.json):
add patches for derived parameters (anchor width, square leg width, stretcher width, radius-curve points), drop the python expected meshes.
  gn_export.py template tables.dining_table.TableDiningFactory 0 3 data/gn/table_square.json   (stand: 1 5, straight: 2 6)  then run this."""
import json, sys
EXTRA = {
    "square": [("geometry_create_legs", "Group", ["in", 1, "v"], "anchor_w"), ("geometry_create_legs", "Group.001", ["in", 0, "v"], "square_w")],
    "stand": [("geometry_create_legs", "Group", ["in", 1, "v"], "anchor_w")] + [("nodegroup_generate_radius_curve", "Float Curve", ["p", "curve", "pts", i, 1], "curve_y%d" % i) for i in range(3)],
    "straight": [("geometry_create_legs", "Group", ["in", 1, "v"], "anchor_w"), ("geometry_create_legs", "Group.002", ["in", 1, "v"], "strecher_w")]
    + [("nodegroup_generate_radius_curve", "Float Curve", ["p", "curve", "pts", i, 1], "curve_y%d" % i) for i in (1, 2)],
}
for style, extra in EXTRA.items():
    p = "data/gn/table_%s.json" % style
    d = json.load(open(p))
    d.pop("final", None)
    for s in d["steps"]:
        s.pop("expect", None)
    d.pop("params_a", None); d.pop("params_b", None); d["unmapped"] = []
    have = {(q["g"], q["n"], tuple(q["at"])) for q in d["patches"]}
    for g, n, at, name in extra:
        if (g, n, tuple(at)) not in have:
            d["patches"].append({"g": g, "n": n, "at": at, "param": name})
    json.dump(d, open(p, "w"), separators=(",", ":"))
    print(style, len(d["patches"]))
