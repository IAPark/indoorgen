"""Plausibility score for a layout dump (LAYOUT_DUMP json): penalty points, lower is better, with the reasons.

    uv run --with shapely tools/layout_score.py l1.json [l2.json ...]

Plan terms (room program, shapes) and furniture terms (anchors present, sofa faces the TV and backs onto a wall, beds and counter
runs on walls, chairs at the table). Used to keep the best of several candidate houses (tools/best_of.sh).
"""
import json
import math
import sys

from shapely.geometry import Point, Polygon, box
from shapely import wkt

FLOATING = 0.35  # m from the back of a sofa/bed/run to a wall before it counts as floating


def foot(o):
    pts = [tuple(p) for part in o["parts"] for p in part["xy"]]
    return Polygon(pts).convex_hull if len(pts) >= 3 else None


def score(d):
    pen, why = 0.0, []

    def add(p, msg):
        nonlocal pen
        pen += p
        why.append(f"{p:+.1f} {msg}")

    rooms = {k: wkt.loads(v) for k, v in d["rooms"].items()}
    rtype = {k: v["type"] for k, v in d["room_info"].items()}
    floors = {k: Polygon(v["floor"][0]) for k, v in d["room_info"].items() if v.get("floor")}
    count = lambda t: sum(1 for v in rtype.values() if v == t)
    beds, baths = count("bedroom"), count("bathroom")

    # plan
    if baths > max(1, beds):
        add(2.0 * (baths - max(1, beds)), f"{baths} bathrooms for {beds} bedrooms")
    for k, poly in rooms.items():
        t = rtype.get(k, "")
        mrr = poly.minimum_rotated_rectangle
        xs = mrr.exterior.coords
        e = sorted(math.dist(xs[i], xs[i + 1]) for i in range(2))
        short, long_ = e[0], e[1]
        if t not in ("hallway", "closet") and short < 2.3:
            add(1.5, f"{k} only {short:.1f} m wide")
        if t not in ("hallway",) and short > 0 and long_ / short > 2.6:
            add(1.0, f"{k} aspect {long_ / short:.1f}")
        angled = sum(1 for a, b in zip(poly.exterior.coords, poly.exterior.coords[1:]) if abs(a[0] - b[0]) > 1e-3 and abs(a[1] - b[1]) > 1e-3)
        if angled:
            add(0.5 * angled, f"{k} {angled} angled wall(s)")
        if t == "bathroom" and not 3.5 <= poly.area <= 14:
            add(1.0, f"{k} {poly.area:.0f} m2")
        if t == "hallway" and poly.area > 12:
            add(1.0, f"{k} {poly.area:.0f} m2 hallway")
        if t == "living-room" and poly.area < 14:
            add(1.5, f"{k} living {poly.area:.0f} m2")
        if t == "bedroom" and poly.area < 8:
            add(1.5, f"{k} bedroom {poly.area:.0f} m2")

    # furniture
    objs = [o for o in d["objects"] if o["class"] not in ("Keepout",)]
    by_room = {}
    for o in objs:
        if o.get("room"):
            by_room.setdefault(o["room"], []).append(o)
    has = lambda k, c: [o for o in by_room.get(k, []) if o["class"] == c]

    def wall_gap(o, k):
        # distance from the back edge (opposite `front`) of the footprint to the room's floor boundary
        f, fl = foot(o), floors.get(k)
        if f is None or fl is None:
            return 0.0
        c = f.centroid
        fx, fy = o["front"]
        # the footprint's extent along -front
        back = max(((c.x - x) * fx + (c.y - y) * fy) for x, y in f.exterior.coords)
        p = Point(c.x - fx * back, c.y - fy * back)
        return fl.exterior.distance(p)

    for k, t in rtype.items():
        if t == "bedroom":
            b = has(k, "BedFactory")
            if not b:
                add(3.0, f"{k} has no bed")
            for o in b:
                g = wall_gap(o, k)
                if g > FLOATING:
                    add(1.5, f"{k} bed headboard {g:.1f} m off the wall")
        elif t == "living-room":
            s, tv = has(k, "SofaFactory"), has(k, "TVStandFactory")
            if not s:
                add(2.5, f"{k} has no sofa")
            if not tv:
                add(1.0, f"{k} has no TV")
            if s and tv:
                so, to = s[0], tv[0]
                v = (to["pos"][0] - so["pos"][0], to["pos"][1] - so["pos"][1])
                dist = math.hypot(*v) or 1.0
                cos = (v[0] * so["front"][0] + v[1] * so["front"][1]) / dist
                if cos < 0.8:
                    add(2.0, f"{k} sofa does not face the TV (cos {cos:.2f})")
                if not 1.5 <= dist <= 4.5:
                    add(1.0, f"{k} sofa-TV {dist:.1f} m")
            for o in s:
                g = wall_gap(o, k)
                if g > FLOATING:
                    add(1.0, f"{k} sofa back {g:.1f} m off the wall")
            if s and not has(k, "CoffeeTableFactory"):
                add(0.5, f"{k} no coffee table")
        elif t == "kitchen":
            run = has(k, "KitchenSpaceFactory")
            if not run:
                add(3.0, f"{k} has no counter run")
            for c in ("FridgeFactory", "OvenFactory"):
                if not has(k, c):
                    add(1.5, f"{k} has no {c[:-7].lower()}")
            fr = has(k, "FridgeFactory")
            if run and fr:
                gap = foot(fr[0]).distance(foot(run[0])) if foot(fr[0]) and foot(run[0]) else 0
                if gap > 0.5:
                    add(1.0, f"{k} fridge {gap:.1f} m from the run")
            for o in run:
                if abs(math.remainder(o["yaw_deg"], 90.0)) > 3:
                    add(1.5, f"{k} counter run on an angle")
        elif t == "bathroom":
            for c, p in (("ToiletFactory", 2.0), ("StandingSinkFactory", 1.0)):
                if not has(k, c):
                    add(p, f"{k} has no {c[:-7].lower()}")
            if not has(k, "BathtubFactory") and not has(k, "ShowerFactory"):
                add(2.0, f"{k} has no tub or shower")
        elif t == "dining-room":
            tb = has(k, "TableDiningFactory")
            if not tb:
                add(2.0, f"{k} has no table")
    # chairs anywhere: count children of each dining table
    for o in objs:
        if o["class"] == "TableDiningFactory":
            n = sum(1 for c in objs if c["class"] == "ChairFactory" and c.get("parent") == o["key"])
            if n == 0:
                n = sum(1 for c in objs if c["class"] == "ChairFactory" and c.get("room") == o.get("room"))
            if n < 2:
                add(1.5, f"{o.get('room')} dining table with {n} chair(s)")
    return pen, why


if __name__ == "__main__":
    for p in sys.argv[1:]:
        pen, why = score(json.load(open(p)))
        print(json.dumps({"file": p, "penalty": round(pen, 2), "why": why}))
