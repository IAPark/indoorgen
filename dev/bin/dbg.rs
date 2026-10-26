use indoorgen::furn::dof::*;
use indoorgen::furn::geom::*;
use indoorgen::furn::relations::*;
use indoorgen::furn::state::*;
use indoorgen::furn::tags::*;
use indoorgen::mesh::*;
use indoorgen::rng::Rng;
use std::sync::Arc;

fn main() {
    // a simple room: floor z=0.14 plane, wall x=0 (normal +x), as tagged triangles
    let mut tris = vec![];
    let q = |a: V3, b: V3, c: V3, d: V3, tags: u32| vec![Tri { v: [a, b, c], tags }, Tri { v: [a, c, d], tags }];
    tris.extend(q([0., 0., 0.14], [5., 0., 0.14], [5., 4., 0.14], [0., 4., 0.14], SUPPORT | VISIBLE));
    // wall at x=0, normal +x: a=(0,4,0.14), b=(0,0,0.14), c=(0,0,3), d=(0,4,3): (b-a)x(c-a)
    tris.extend(q([0., 0., 0.14], [0., 4., 0.14], [0., 4., 3.], [0., 0., 3.], WALL | VISIBLE));
    let room = ObjGeom::from_tris(tris, vec![]);
    let mut st = State::new();
    st.objs.insert("room".into(), ObjState::new(Arc::new(room), Pose::identity(), TagSet::from([Tag::sem("Room")])));
    st.touch("room");
    let bx = ObjGeom::cuboid([-0.25, -0.2, -0.4], [0.25, 0.2, 0.5], 0);
    let mut o = ObjState::new(Arc::new(bx), Pose::identity(), TagSet::new());
    let bottom = TagSet::from([Tag::sub("Bottom"), Tag::sub("Top").negated(), Tag::sub("Front").negated(), Tag::sub("Back").negated()]);
    let floort = TagSet::from([Tag::sub("SupportSurface"), Tag::sub("Visible"), Tag::sub("Wall").negated(), Tag::sub("Ceiling").negated()]);
    let back = TagSet::from([Tag::sub("Back"), Tag::sub("Top").negated(), Tag::sub("Front").negated()]);
    let wallt = TagSet::from([Tag::sub("Wall"), Tag::sub("Visible"), Tag::sub("SupportSurface").negated(), Tag::sub("Ceiling").negated()]);
    let mk = |c: &TagSet, p: &TagSet, m: f64| Relation::Geometry(Geo { kind: GeoKind::StableAgainst, child: c.clone(), parent: p.clone(), margin: m, check_z: true, rev_normal: false });
    o.relations = vec![
        RelState { relation: mk(&bottom, &floort, 0.01), target: "room".into(), child_plane: 0, parent_plane: 0 },
        RelState { relation: mk(&back, &wallt, 0.07), target: "room".into(), child_plane: 0, parent_plane: 0 },
    ];
    st.objs.insert("toilet".into(), o);
    st.touch("toilet");
    let mut rng = Rng::seeded(1);
    println!("planes obj bottom {:?} back {:?}", st.planes("toilet", &bottom), st.planes("toilet", &back));
    println!("planes room floor {:?} wall {:?}", st.planes("room", &floort), st.planes("room", &wallt));
    println!("toilet back plane nd {:?}", st.plane_nd("toilet", 0));
    println!("wall plane nd {:?}", st.plane_nd("room", 2));
    let r = apply_relations_surfacesample(&mut st, "toilet", &mut rng);
    println!("apply -> {:?}  pose {:?}", r, st.objs["toilet"].pose);
    println!("valid rel {}", all_relations_valid(&st, "toilet"));
}
