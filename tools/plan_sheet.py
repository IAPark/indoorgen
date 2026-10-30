"""Draw floor plans dumped by `PLAN_DUMP=pN.json gen2 N ...` (rooms + door / window / entrance cutters, no furniture).
uv run --with matplotlib --with shapely tools/plan_sheet.py DIR   -> DIR/png/pN.png and DIR/sheet_*.png (5 x 4 per sheet)"""
import glob, json, math, os, re, sys
import matplotlib
matplotlib.use("Agg")
import matplotlib.pyplot as plt
from matplotlib.patches import Arc, PathPatch, Polygon as MPoly
from matplotlib.path import Path
from shapely import wkt
from shapely.geometry import MultiPoint, Polygon, box
from shapely.affinity import rotate, translate
from shapely.ops import polylabel, unary_union

COL = {"living-room": "#f3dfb8", "dining-room": "#f6c9a8", "kitchen": "#cfe3b4", "bedroom": "#bcd3ec", "bathroom": "#b9e4e1",
       "hallway": "#e4e0d8", "closet": "#d9cfe6", "utility": "#d8d8d8", "balcony": "#e8f0d0", "staircase": "#ffffff", "garage": "#cccccc"}
COL["hallway"] = "#e9e4da"

def kind(name): return name.split("_")[0]

def footprint(c):
    (x, y, _), (dx, dy, _), r = c["loc"], c["dim"], c["rot"]
    return translate(rotate(box(-dx / 2, -dy / 2, dx / 2, dy / 2), r, use_radians=True), x, y)

def draw(d, ax, small=False):
    t = d["wall_thickness"]
    rooms = {k: wkt.loads(v) for k, v in d["rooms"].items() if not k.startswith("exterior")}
    stairs = {k: v for k, v in rooms.items() if kind(k) == "staircase"}
    rooms = {k: v for k, v in rooms.items() if k not in stairs}
    for k, p in rooms.items():
        ax.add_patch(MPoly(list(p.exterior.coords), closed=True, fc=COL.get(kind(k), "#eeeeee"), ec="none"))
    # walls: every room outline thickened, doors and the entrance cut out
    walls = unary_union([p.exterior.buffer(t / 2, join_style=2, cap_style=2) for p in rooms.values()])
    solid = walls
    cuts = [c for c in d["cutters"] if c["kind"] in ("Door", "Entrance", "Passage")]
    if cuts: walls = walls.difference(unary_union([footprint(c).buffer(0.02) for c in cuts]))
    for g in getattr(walls, "geoms", [walls]):
        if g.is_empty: continue
        rings = [list(g.exterior.coords)] + [list(h.coords) for h in g.interiors]
        verts = [v for r in rings for v in r]
        codes = [c for r in rings for c in [Path.MOVETO] + [Path.LINETO] * (len(r) - 2) + [Path.CLOSEPOLY]]
        ax.add_patch(PathPatch(Path(verts, codes), fc="#2b2b2b", ec="none"))
    for c in d["cutters"]:
        f = footprint(c)
        dx, dy = c["dim"][0], c["dim"][1]
        if c["kind"] == "Window":
            for g in getattr(f.intersection(solid), "geoms", [f.intersection(solid)]):
                if g.geom_type == "Polygon" and not g.is_empty: ax.add_patch(MPoly(list(g.exterior.coords), closed=True, fc="#9fd4ff", ec="#2a6fb0", lw=0.4))
        elif c["kind"] in ("Door", "Entrance"):
            w = dx; along = c["rot"]  # dim[0] = opening width along the wall (dim[1] is the cutter depth through it)
            cx, cy = c["loc"][0], c["loc"][1]
            col = "#c0392b" if c["kind"] == "Entrance" else "#555"
            ux, uy = math.cos(along), math.sin(along)
            style = c.get("style", "swing")
            if style in ("sliding", "sliding_glass"):
                # two panels in the wall line, overlapping in the middle, one a little behind the other
                glass = style == "sliding_glass"
                wt = d.get("wall_thickness", 0.2)
                th = 0.032 if glass else 0.02
                for k, off in ((-1, -0.03), (1, 0.03)):
                    x0 = -w / 2 if k < 0 else -0.05
                    x1 = 0.05 if k < 0 else w / 2
                    nx, ny = -uy * off, ux * off
                    pts = [(cx + ux * x0 + nx - uy * th, cy + uy * x0 + ny + ux * th), (cx + ux * x1 + nx - uy * th, cy + uy * x1 + ny + ux * th),
                           (cx + ux * x1 + nx + uy * th, cy + uy * x1 + ny - ux * th), (cx + ux * x0 + nx + uy * th, cy + uy * x0 + ny - ux * th)]
                    ax.add_patch(MPoly(pts, closed=True, fc="#9fd4ff" if glass else "#d9d2c5", ec="#2a6fb0" if glass else "#555", lw=0.6, zorder=3))
                continue
            hinges = [(-1, 1.0)] if style != "double" else [(-1, 0.5), (1, 0.5)]
            for side, frac in hinges:
                r_ = w * frac
                hx, hy = cx + side * ux * w / 2, cy + side * uy * w / 2
                if side < 0:
                    ax.add_patch(Arc((hx, hy), 2 * r_, 2 * r_, angle=math.degrees(along), theta1=0, theta2=90, lw=0.6, color=col))
                else:
                    ax.add_patch(Arc((hx, hy), 2 * r_, 2 * r_, angle=math.degrees(along), theta1=90, theta2=180, lw=0.6, color=col))
                ax.plot([hx, hx - uy * r_], [hy, hy + ux * r_], lw=0.8, color=col)
    # labels in the room's largest furniture-free patch (a label over the dining table read as overlapping pieces)
    furn = []
    for o in d.get("objects", []):
        if o["class"] in ("Keepout",) or re.search("Light|Art|Mirror|Rug|Hardware", o["class"]) or not o.get("parts"): continue
        pts = [tuple(q) for part in o["parts"] for q in part["xy"]]
        if len(pts) >= 3: furn.append(MultiPoint(pts).convex_hull.buffer(0.15))
    furn_u = unary_union(furn) if furn else None
    for k, p in rooms.items():
        c = p.representative_point()
        if furn_u is not None:
            free = p.buffer(-0.3).difference(furn_u)
            parts_ = [g for g in getattr(free, "geoms", [free]) if g.geom_type == "Polygon" and not g.is_empty]
            if parts_:
                c = polylabel(max(parts_, key=lambda g: g.area), tolerance=0.05)
        lab = kind(k).replace("-room", "").replace("-", " ")
        ax.text(c.x, c.y, f"{lab}\n{p.area:.0f} m²", ha="center", va="center", fontsize=5 if small else 8, color="#333")
    xs = [x for p in rooms.values() for x in p.bounds[0::2]]; ys = [y for p in rooms.values() for y in p.bounds[1::2]]
    ax.set_xlim(min(xs) - 0.6, max(xs) + 0.6); ax.set_ylim(min(ys) - 0.6, max(ys) + 0.6)
    ax.set_aspect("equal"); ax.axis("off")
    area = sum(p.area for p in rooms.values())
    ax.set_title(f"seed {d['seed']} · {len(rooms)} rooms · {area:.0f} m²", fontsize=7 if small else 10)

def main(dirn):
    files = sorted(glob.glob(os.path.join(dirn, "p*.json")), key=lambda f: int(re.findall(r"\d+", os.path.basename(f))[0]))
    os.makedirs(os.path.join(dirn, "png"), exist_ok=True)
    plans = [json.load(open(f)) for f in files]
    for d in plans:
        fig, ax = plt.subplots(figsize=(6, 6)); draw(d, ax)
        fig.savefig(os.path.join(dirn, "png", f"p{d['seed']}.png"), dpi=130, bbox_inches="tight"); plt.close(fig)
    per = 20
    for s in range(0, len(plans), per):
        fig, axs = plt.subplots(4, 5, figsize=(20, 16))
        for ax in axs.flat: ax.axis("off")
        for ax, d in zip(axs.flat, plans[s:s + per]): draw(d, ax, small=True)
        fig.tight_layout(); fig.savefig(os.path.join(dirn, f"sheet_{s // per + 1}.png"), dpi=110); plt.close(fig)
    print(len(plans), "plans")

if __name__ == "__main__": main(sys.argv[1])
