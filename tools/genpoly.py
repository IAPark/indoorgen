"""Random test polygons (some with diagonal edges) for roomcmp verify: python tools/genpoly.py out.json N seed"""
import sys, json, random, math
import shapely
from shapely.geometry import Polygon, MultiPoint
from shapely.ops import orient
n, seed = int(sys.argv[2]), int(sys.argv[3])
random.seed(seed)
rooms = {}
k = 0
while len(rooms) < n:
    kind = random.choice(["hull", "rot", "lshape", "star", "hole"])
    if kind == "hull":
        pts = [(random.randint(0, 12) * 0.5, random.randint(0, 12) * 0.5) for _ in range(random.randint(4, 9))]
        p = MultiPoint(pts).convex_hull
    elif kind == "rot":
        w, h = random.uniform(2, 6), random.uniform(2, 5)
        p = shapely.affinity.rotate(shapely.box(0, 0, w, h), random.uniform(5, 85), origin=(0, 0))
    elif kind == "lshape":
        w, h = random.randint(4, 12) * 0.5, random.randint(4, 12) * 0.5
        cw, ch = random.randint(1, int(w * 2) - 1) * 0.5, random.randint(1, int(h * 2) - 1) * 0.5
        p = shapely.box(0, 0, w, h).difference(shapely.box(w - cw, h - ch, w, h))
        if random.random() < 0.5:
            p = shapely.affinity.rotate(p, random.choice([30, 45, 60]), origin=(0, 0))
    elif kind == "hole":
        w, h = random.randint(8, 16) * 0.5, random.randint(8, 16) * 0.5
        hw, hh = random.randint(1, int(w) - 1) * 0.5, random.randint(1, int(h) - 1) * 0.5
        x0, y0 = random.randint(1, int(w * 2) - int(hw * 2) - 1) * 0.5, random.randint(1, int(h * 2) - int(hh * 2) - 1) * 0.5
        p = shapely.box(0, 0, w, h).difference(shapely.box(x0, y0, x0 + hw, y0 + hh))
        if random.random() < 0.5:
            p = shapely.affinity.rotate(p, random.uniform(5, 85), origin=(0, 0))
    else:
        m = random.randint(5, 9)
        pts = [((2 + random.uniform(0, 2)) * math.cos(2 * math.pi * i / m), (2 + random.uniform(0, 2)) * math.sin(2 * math.pi * i / m)) for i in range(m)]
        p = Polygon(pts)
    if p.geom_type != "Polygon" or p.area < 2 or not p.is_valid:
        continue
    p = shapely.segmentize(p, random.choice([0.7, 1.0, 1.4]))
    p = orient(p)
    rings = [[list(c) for c in p.exterior.coords[:-1]]] + [[list(c) for c in i.coords[:-1]] for i in p.interiors]
    # skip near-duplicate vertices (weld threshold)
    flat = [c for r in rings for c in r]
    if any(math.dist(flat[i], flat[j]) < 0.05 for i in range(len(flat)) for j in range(i)):
        continue
    rooms[f"{kind}_{k}"] = rings
    k += 1
json.dump({"0": {"wt": 0.2799158564216724, "h": 2.9845917449011727, "rooms": rooms}, "1": {"wt": 0.29682615757193975, "h": 2.925369671263697, "rooms": {}}}, open(sys.argv[1], "w"))
