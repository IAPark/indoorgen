//! tableware/pot.py add_bar: two tori (primitive_torus_add 48x12, SIMPLE_DEFORM taper on X, rotation about Y/Z, applied) minus the pot with the
//! exact boolean, the loose piece farthest out along the bar's side kept, SUBSURF level 1, joined to the pot.
use crate::gn::xbool::{boolean, components, Op};
use crate::mesh::Mesh;

fn dump(name: &str, m: &Mesh) {
    if let Ok(dir) = std::env::var("PANDBG") {
        let j = serde_json::json!({"v": m.pos, "p": m.faces});
        let _ = std::fs::write(format!("{dir}/rs_{name}.json"), j.to_string());
    }
}

pub fn add_bars(pot: &Mesh, bar_radius: f64, bar_inner: f64, x_offset: f64, bar_height: f64, scale: [f64; 3], taper: f64, y_rot: f64) -> Mesh {
    let mut out = pot.clone();
    for side in [-1.0f64, 1.0] {
        let loc = [side * (1.0 + x_offset), 0.0, bar_height];
        let loc32 = loc.map(|c| c as f32);
        let g = super::hwexact::bpy_torus(48, 12, bar_radius, bar_inner);
        let mut v = g.v.clone();
        super::bpymod::simple_deform(&mut v, super::bpymod::Deform::Taper, taper, 0);
        // apply_transform: rotation (euler 0, y_rot, 0|pi) times scale as a float matrix
        let (ey, ez) = (y_rot as f32 as f64, if side == 1.0 { 0.0 } else { std::f64::consts::PI as f32 as f64 });
        let (cy, sy, cz, sz) = (ey.cos(), ey.sin(), ez.cos(), ez.sin());
        // R = Rz * Ry
        let r = [[cz * cy, -sz, cz * sy], [sz * cy, cz, sz * sy], [-sy, 0.0, cy]];
        let sc = scale.map(|c| c as f32 as f64);
        let m: [[f32; 3]; 3] = std::array::from_fn(|i| std::array::from_fn(|j| (r[i][j] * sc[j]) as f32));
        let mut torus = Mesh::new();
        for p in &v {
            let q = [
                m[0][0] * p[0] + m[0][1] * p[1] + m[0][2] * p[2],
                m[1][0] * p[0] + m[1][1] * p[1] + m[1][2] * p[2],
                m[2][0] * p[0] + m[2][1] * p[1] + m[2][2] * p[2],
            ];
            // the pan is mirror symmetric about y = 0 like the torus: a hair of offset keeps the operands in generic position
            torus.pos.push([q[0] as f64, q[1] as f64 + 3.1e-9, q[2] as f64]);
        }
        torus.faces = g.p.clone();
        let mut potl = pot.clone();
        for p in potl.pos.iter_mut() {
            *p = [(p[0] as f32 - loc32[0]) as f64, (p[1] as f32 - loc32[1]) as f64, (p[2] as f32 - loc32[2]) as f64];
        }
        dump(&format!("torus{}", side as i32), &torus);
        let res = boolean(Op::Difference, &torus, &potl);
        dump(&format!("bool{}", side as i32), &res);
        let comps = components(&res);
        let mut best = (f64::NEG_INFINITY, usize::MAX);
        for (i, c) in comps.iter().enumerate() {
            let mx = c.pos.iter().map(|p| p[0] * side).fold(f64::NEG_INFINITY, f64::max);
            if mx > best.0 {
                best = (mx, i);
            }
        }
        if best.1 == usize::MAX {
            continue;
        }
        dump(&format!("piece{}", side as i32), &comps[best.1]);
        let mut bar = super::subsurf::subsurf(&comps[best.1], 1, false);
        dump(&format!("bar{}", side as i32), &bar);
        for p in bar.pos.iter_mut() {
            *p = [(p[0] as f32 + loc32[0]) as f64, (p[1] as f32 + loc32[1]) as f64, (p[2] as f32 + loc32[2]) as f64];
        }
        let base = out.pos.len() as u32;
        out.pos.extend(bar.pos);
        for f in bar.faces {
            out.faces.push(f.iter().map(|x| x + base).collect());
        }
    }
    out
}
