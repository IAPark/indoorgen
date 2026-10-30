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


def usable_rect(poly, cell=0.25):
    """largest axis-aligned rectangle inside `poly` on a raster: (area, short side)"""
    import shapely
    x0, y0, x1, y1 = poly.bounds
    nx, ny = max(1, int(math.ceil((x1 - x0) / cell))), max(1, int(math.ceil((y1 - y0) / cell)))
    xs = [x0 + (i + 0.5) * cell for j in range(ny) for i in range(nx)]
    ys = [y0 + (j + 0.5) * cell for j in range(ny) for i in range(nx)]
    ins = shapely.contains_xy(poly, xs, ys)
    h = [0] * nx
    best = (0.0, 0.0)
    for j in range(ny):
        for i in range(nx): h[i] = h[i] + 1 if ins[j * nx + i] else 0
        st = []
        for i in range(nx + 1):
            cur = h[i] if i < nx else 0
            while st and h[st[-1]] > cur:
                t = st.pop(); w = i - st[-1] - 1 if st else i
                a, b = w * cell, h[t] * cell
                if a * b > best[0]: best = (a * b, min(a, b))
            st.append(i)
    return best


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
    if beds >= 1 and not count("dining-room"):
        add(1.5, "no dining room")
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
        if t == "bathroom" and not 5.0 <= poly.area <= 14:
            add(1.0, f"{k} {poly.area:.0f} m2")
        if t == "hallway" and poly.area > 12:
            add(1.0, f"{k} {poly.area:.0f} m2 hallway")
        if t == "living-room" and poly.area > 36:
            add(1.0, f"{k} living {poly.area:.0f} m2 (reads half empty)")
        if t == "living-room" and poly.area < 14:
            add(1.5, f"{k} living {poly.area:.0f} m2")
        if t == "bedroom" and poly.area < 8:
            add(1.5, f"{k} bedroom {poly.area:.0f} m2")

    # circulation: rooms joined by their doors (doors, passages); walking distance from the entrance door to every room's door,
    # through door points; private rooms must be reachable without passing through another bedroom, a bathroom or the kitchen
    import heapq
    doors = [c for c in d["cutters"] if c["kind"] in ("Door", "Passage", "Entrance")]
    ent = [c for c in doors if c["kind"] == "Entrance"]
    nbr = {}
    for c in doors:
        if len(c["rooms"]) == 2:
            a, b = c["rooms"]
            nbr.setdefault(a, set()).add(b); nbr.setdefault(b, set()).add(a)
    if ent:
        e0 = ent[0]; er = e0["rooms"][0] if e0["rooms"] else None
        if er and rtype.get(er) in ("bedroom", "bathroom", "closet"):
            add(2.0, f"entrance opens into the {rtype[er]}")
        # door-point graph: points joined when they serve the same room
        pts = [(tuple(c["loc"][:2]), set(c["rooms"])) for c in doors]
        dist = {0: 0.0} if False else {}
        si = doors.index(e0); dist[si] = 0.0; pq = [(0.0, si)]
        while pq:
            dd, i = heapq.heappop(pq)
            if dd > dist.get(i, 1e9): continue
            for j, (pj, rj) in enumerate(pts):
                if j == i or not (pts[i][1] & rj): continue
                # walking through a bedroom / bathroom to reach another door does not count as circulation
                via = pts[i][1] & rj
                if all(rtype.get(r) in ("bedroom", "bathroom", "closet") for r in via) and i != si: continue
                nd = dd + math.dist(pts[i][0], pj)
                if nd < dist.get(j, 1e9): dist[j] = nd; heapq.heappush(pq, (nd, j))
        for k, t in rtype.items():
            if t in ("closet",): continue
            ds = [dist[i] for i, (_, rs) in enumerate(pts) if k in rs and i in dist]
            if not ds:
                if t == "bedroom": add(2.0, f"{k} reached only through another bedroom or bathroom")
                continue
            if min(ds) > 12: add(0.1 * (min(ds) - 12), f"{k} {min(ds):.0f} m walk from the entrance")
        # reached only through the kitchen (bathroom / bedroom): BFS over rooms without kitchens and private rooms
        if er:
            seen, todo = {er}, [er]
            while todo:
                r = todo.pop()
                for q in nbr.get(r, ()):
                    if q in seen: continue
                    seen.add(q)
                    if rtype.get(q) not in ("kitchen", "bedroom", "bathroom", "closet"): todo.append(q)
            for k, t in rtype.items():
                if t in ("bedroom",) and k not in seen: add(1.5, f"{k} reached only through another room")
                if t == "bathroom" and k not in seen and not any(rtype.get(q) == "bedroom" for q in nbr.get(k, ())): add(1.0, f"{k} reached only through the kitchen")
    for k, t in rtype.items():
        if t == "bathroom" and any(rtype.get(q) in ("living-room", "kitchen", "dining-room") for q in nbr.get(k, ())):
            add(0.5, f"{k} opens off a public room")

    # shape: the share of a main room in its largest axis-aligned rectangle (0.25 m raster), and its short side
    for k, poly in rooms.items():
        t = rtype.get(k, "")
        if t not in ("living-room", "dining-room", "bedroom", "kitchen"): continue
        ra, short = usable_rect(poly)
        if poly.area > 0 and ra / poly.area < 0.7:
            add(round(4 * (0.7 - ra / poly.area), 2), f"{k} only {100 * ra / poly.area:.0f}% in one rectangle")
        need = 2.8 if t == "living-room" else 2.4
        if short < need: add(1.0, f"{k} usable width {short:.1f} m")

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
            dps = [c["loc"][:2] for c in doors if k in c["rooms"]]
            for o in b:
                if dps and min(math.dist(o["pos"][:2], q) for q in dps) < 1.2:
                    add(1.0, f"{k} bed by the door")
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
            # work triangle: sink, range, fridge 3.6-7.9 m around
            tri = [o for c in ("SinkFactory", "OvenFactory", "FridgeFactory") for o in objs if o["class"] == c and (o.get("room") == k or c == "SinkFactory")][:3]
            sinks = [o for o in objs if o["class"] == "SinkFactory"]
            ov, fr = has(k, "OvenFactory"), has(k, "FridgeFactory")
            if sinks and ov and fr:
                sk = min(sinks, key=lambda o: math.dist(o["pos"][:2], ov[0]["pos"][:2]))
                ps = [sk["pos"][:2], ov[0]["pos"][:2], fr[0]["pos"][:2]]
                per = math.dist(ps[0], ps[1]) + math.dist(ps[1], ps[2]) + math.dist(ps[2], ps[0])
                if not 3.6 <= per <= 7.9: add(round(0.3 * min(abs(per - 3.6), abs(per - 7.9)), 2), f"{k} work triangle {per:.1f} m")
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
            n = max(sum(1 for c in objs if c["class"] == "ChairFactory" and c.get("parent") == o["key"]),
                    sum(1 for c in objs if c["class"] == "ChairFactory" and c.get("room") == o.get("room")))
            if n < 4:
                add(1.5 if n < 2 else 1.0, f"{o.get('room')} dining table with {n} chair(s)")
    return pen, why


if __name__ == "__main__":
    for p in sys.argv[1:]:
        pen, why = score(json.load(open(p)))
        print(json.dumps({"file": p, "penalty": round(pen, 2), "why": why}))
