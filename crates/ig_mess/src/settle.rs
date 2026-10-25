//! Rigid-body settling of clutter (owner: mess-clutter worker).
//! One `RoomWorld` per room: static colliders (furniture triangle meshes / boxes, floor, walls) + dynamic clutter bodies (convex hulls
//! of the asset parts). Items are dropped slightly above their target, simulated with rapier until they sleep, and checked for
//! interpenetration with parry afterwards.
use ig_core::mesh::{Mesh, V3};
use rapier3d_f64::parry;
use rapier3d_f64::prelude::*;

pub type Hull = Vec<V3>;

fn v(p: V3) -> Vector {
    Vector::new(p[0], p[1], p[2])
}

/// static collision geometry
pub enum Static {
    Mesh { verts: Vec<V3>, tris: Vec<[u32; 3]> },
    Aabb { lo: V3, hi: V3 },
}

impl Static {
    pub fn from_mesh(m: &Mesh, max_tris: usize) -> Option<Static> {
        if m.is_empty() {
            return None;
        }
        let tris = m.triangles();
        if tris.is_empty() {
            return None;
        }
        if tris.len() > max_tris {
            let (lo, hi) = m.bounds();
            return Some(Static::Aabb { lo, hi });
        }
        Some(Static::Mesh { verts: m.pos.clone(), tris })
    }
}

/// a dynamic clutter body: origin = asset origin, hulls in asset-local coordinates
pub struct BodyIn {
    pub hulls: Vec<Hull>,
    /// initial position of the asset origin
    pub t: V3,
    /// initial orientation: yaw about z, then tilt about x and y
    pub yaw: f64,
    pub tilt: [f64; 2],
    pub density: f64,
}

#[derive(Clone, Debug)]
pub struct Rest {
    /// world matrix of the asset origin (row-major 4x4)
    pub m: [[f64; 4]; 4],
    pub asleep: bool,
    /// linear speed + 0.1 * angular speed at the end of the simulation
    pub speed: f64,
    /// deepest penetration (m, >= 0) against static geometry / against other items
    pub pen_static: f64,
    pub pen_item: f64,
    /// distance of the item to the surface below it (0 when resting)
    pub gap: f64,
}

pub struct RoomWorld {
    pub w: PhysicsWorld,
    n_static: usize,
}

pub fn pose_of(t: V3, yaw: f64, tilt: [f64; 2]) -> Pose {
    Pose::from_translation(v(t)) * Pose::rotation(Vector::new(0.0, 0.0, yaw)) * Pose::rotation(Vector::new(tilt[0], tilt[1], 0.0))
}

pub fn mat_of(p: &Pose) -> [[f64; 4]; 4] {
    let q = p.rotation;
    let (x, y, z, w) = (q.x, q.y, q.z, q.w);
    let t = p.translation;
    [
        [1.0 - 2.0 * (y * y + z * z), 2.0 * (x * y - z * w), 2.0 * (x * z + y * w), t.x],
        [2.0 * (x * y + z * w), 1.0 - 2.0 * (x * x + z * z), 2.0 * (y * z - x * w), t.y],
        [2.0 * (x * z - y * w), 2.0 * (y * z + x * w), 1.0 - 2.0 * (x * x + y * y), t.z],
        [0.0, 0.0, 0.0, 1.0],
    ]
}

pub fn hull_shape(h: &Hull) -> Option<SharedShape> {
    let pts: Vec<Vector> = h.iter().map(|&p| v(p)).collect();
    SharedShape::convex_hull(&pts)
}

impl RoomWorld {
    pub fn new(statics: &[Static]) -> RoomWorld {
        let mut w = PhysicsWorld::default();
        w.gravity = Vector::new(0.0, 0.0, -9.81);
        let mut n = 0;
        for s in statics {
            let b = match s {
                Static::Mesh { verts, tris } => match ColliderBuilder::trimesh(verts.iter().map(|&p| v(p)).collect(), tris.clone()) {
                    Ok(b) => b,
                    Err(_) => continue,
                },
                Static::Aabb { lo, hi } => ColliderBuilder::cuboid((hi[0] - lo[0]) / 2.0, (hi[1] - lo[1]) / 2.0, (hi[2] - lo[2]) / 2.0)
                    .translation(Vector::new((hi[0] + lo[0]) / 2.0, (hi[1] + lo[1]) / 2.0, (hi[2] + lo[2]) / 2.0)),
            };
            w.insert_collider(b.friction(0.7).restitution(0.0), None);
            n += 1;
        }
        // one step builds the broad phase so queries see the static colliders
        w.step();
        RoomWorld { w, n_static: n }
    }

    /// distance along the ray to the first static hit
    pub fn ray(&self, o: V3, d: V3, max: f64) -> Option<f64> {
        let ray = parry::query::Ray::new(v(o), v(d));
        self.w.cast_ray(&ray, max, true, QueryFilter::default()).map(|(_, t)| t)
    }

    /// does the posed hull set penetrate any static collider deeper than `tol`?
    pub fn hits_static(&self, shapes: &[(Pose, SharedShape)], tol: f64) -> bool {
        for (p, s) in shapes {
            for (_, c) in self.w.intersect_shape(*p, &**s, QueryFilter::default()) {
                match parry::query::contact(p, &**s, c.position(), c.shape(), 0.0) {
                    Ok(Some(ct)) if ct.dist < -tol => return true,
                    _ => {}
                }
            }
        }
        false
    }

    pub fn n_static(&self) -> usize {
        self.n_static
    }

    /// Drop all bodies and simulate until they sleep (or `max_steps`). Returns the final pose and contact checks per body.
    pub fn settle(&mut self, items: &[BodyIn], max_steps: usize) -> Vec<Rest> {
        let w = &mut self.w;
        let mut handles = vec![];
        for it in items {
            let p = pose_of(it.t, it.yaw, it.tilt);
            let rb = RigidBodyBuilder::dynamic().pose(p).linear_damping(0.6).angular_damping(3.0).can_sleep(true);
            let bh = w.insert_body(rb);
            let mut cs = vec![];
            for h in &it.hulls {
                if let Some(b) = ColliderBuilder::convex_hull(&h.iter().map(|&q| v(q)).collect::<Vec<_>>()) {
                    cs.push(w.insert_collider(b.density(it.density).friction(0.7).restitution(0.05).contact_skin(0.001), Some(bh)));
                }
            }
            handles.push((bh, cs));
        }
        w.integration_parameters.dt = 1.0 / 60.0;
        for step in 0..max_steps {
            w.step();
            if step % 10 == 9 && handles.iter().all(|(b, _)| w.bodies[*b].is_sleeping()) {
                break;
            }
        }
        self.measure(&handles)
    }

    /// Fixed copies of final item hulls (for the audit of finished scenes); returns the measurements per item.
    pub fn audit(&mut self, items: &[Vec<Hull>]) -> Vec<Rest> {
        let mut handles = vec![];
        for hs in items {
            let bh = self.w.insert_body(RigidBodyBuilder::fixed());
            let mut cs = vec![];
            for h in hs {
                if let Some(b) = ColliderBuilder::convex_hull(&h.iter().map(|&q| v(q)).collect::<Vec<_>>()) {
                    cs.push(self.w.insert_collider(b, Some(bh)));
                }
            }
            handles.push((bh, cs));
        }
        self.w.step();
        self.measure(&handles)
    }

    fn measure(&self, handles: &[(RigidBodyHandle, Vec<ColliderHandle>)]) -> Vec<Rest> {
        let w = &self.w;
        let mut out = vec![];
        for (bh, cs) in handles {
            let body = &w.bodies[*bh];
            let pose = *body.position();
            let mut rest = Rest { m: mat_of(&pose), asleep: body.is_sleeping(), speed: body.linvel().length() + 0.1 * body.angvel().length(), pen_static: 0.0, pen_item: 0.0, gap: f64::MAX };
            for &ch in cs {
                let c = &w.colliders[ch];
                let shape = c.shared_shape().clone();
                let cp = *c.position();
                for (oh, oc) in w.intersect_shape(cp, &*shape, QueryFilter::default()) {
                    if oh == ch || oc.parent() == Some(*bh) {
                        continue;
                    }
                    if let Ok(Some(ct)) = parry::query::contact(&cp, &*shape, oc.position(), oc.shape(), 0.0) {
                        let d = (-ct.dist).max(0.0);
                        if oc.parent().is_some() {
                            rest.pen_item = rest.pen_item.max(d);
                        } else {
                            rest.pen_static = rest.pen_static.max(d);
                        }
                    }
                }
                // shape cast straight down: distance until the hull touches anything else
                let filter = QueryFilter::default().exclude_rigid_body(*bh);
                let opts = parry::query::ShapeCastOptions { max_time_of_impact: 0.5, target_distance: 0.0, stop_at_penetration: true, compute_impact_geometry_on_penetration: false };
                if let Some((_, hit)) = w.cast_shape(&cp, Vector::new(0.0, 0.0, -1.0), &*shape, opts, filter) {
                    rest.gap = rest.gap.min(hit.time_of_impact);
                }
            }
            out.push(rest);
        }
        out
    }
}
