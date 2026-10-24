//! Door and window assets placed in room cutters (room/decorate.py populate_doors / populate_windows),
//! built with the exact ports in assets/{doors,windows}.rs.
use crate::assets::doors::{DoorKind, DoorSpec};
use crate::assets::windows::{window_pieces, WindowSpec};
use crate::materials::*;
use crate::mesh::*;
use crate::rng::Rng;
use crate::rooms::consts::RoomConstants;
use crate::rooms::solidify::{CutKind, Cutter};
use serde_json::json;

pub struct Piece {
    pub name: String,
    pub class: String,
    pub mesh: Mesh,
    pub mat: Material,
}

fn place(m: &mut Mesh, c: &Cutter, local: &Mat4) {
    let full = Mat4::translation([c.loc[0], c.loc[1], c.loc[2]]).mul(&Mat4::rot_z(c.rot_z)).mul(local);
    m.transform(&full);
}

fn spec(name: &str, cls: &str, seed: u64, rgb: [f32; 3]) -> Material {
    spec_material(name, json!({"cls": cls, "kwargs": {}, "seed": seed}), rgb)
}

/// room_doors stage: python populate_doors (3 door factories, door_chance 1, casing_chance 0)
fn doors(c: &RoomConstants, cutters: &[Cutter], seed: u64, out: &mut Vec<Piece>) {
    let mut r = Rng::seeded(crate::int_hash(&[&seed.to_string(), "room_doors"]));
    let dc: Vec<(usize, &Cutter)> = cutters.iter().enumerate().filter(|(_, c)| c.kind != CutKind::Window).collect();
    let kinds = [DoorKind::Panel, DoorKind::GlassPanel, DoorKind::Louver, DoorKind::Lite];
    let mut facs = vec![];
    for _ in 0..3 {
        let k = kinds[r.choice_p(&[4.0 / 12.0, 2.0 / 12.0, 3.0 / 12.0, 3.0 / 12.0])];
        let s = r.randint(0, 10_000_000) as u64;
        facs.push(DoorSpec::new(k, s, c));
    }
    let idx: Vec<usize> = (0..dc.len()).map(|_| r.randint(0, 3) as usize).collect();
    if std::env::var("OPDBG").is_ok() { eprintln!("door facs {:?} idx {:?}", facs.iter().map(|f| (f.kind, f.fseed)).collect::<Vec<_>>(), idx); }
    for (i, fac) in facs.iter().enumerate() {
        for j in 0..dc.len() {
            if idx[j] != i {
                continue;
            }
            let (ci, ct) = dc[j];
            let dch = r.f64(); // door_chance
            let rot_p = r.f64();
            if std::env::var("OPDBG").is_ok() { eprintln!("dch {dch} rot_p {rot_p}"); }
            let rot_z = if rot_p < 0.5 {
                r.uniform(0.0, 0.1)
            } else if rot_p < 0.7 {
                r.uniform(0.93, 1.03)
            } else {
                r.uniform(0.0, 1.0)
            } * std::f64::consts::FRAC_PI_2;
            if std::env::var("OPDBG").is_ok() { eprintln!("door j={j} ci={ci} fac={i} rot_z={rot_z}"); }
            let pieces = fac.build_door(j as u64);
            r.f64(); // casing_chance
            let local = Mat4::translation([c.door_width / 2.0, c.wall_thickness / 2.0, -c.door_size / 2.0]).mul(&Mat4::rot_z(-rot_z));
            let name = format!("door{ci}");
            let sd = fac.fseed;
            let mut add = |suffix: &str, mut mesh: Mesh, cls: &str, rgb: [f32; 3]| {
                if mesh.is_empty() {
                    return;
                }
                place(&mut mesh, ct, &local);
                let nm = format!("{name}{suffix}");
                out.push(Piece { name: nm.clone(), class: "Door".into(), mesh, mat: spec(&nm, cls, sd, rgb) });
            };
            add("", pieces.body, fac.surface_cls, [0.5, 0.4, 0.3]);
            add(".glass", pieces.glass, fac.glass_cls, [0.8, 0.9, 1.0]);
            add(".louver", pieces.louver, fac.louver_cls, [0.5, 0.4, 0.3]);
            add(".handle", pieces.handle, if fac.handle_type == 3 { fac.bar_cls } else { fac.handle_cls }, [0.7, 0.7, 0.7]);
        }
        // casing_factory.finalize_assets(casings=[]) -> surface.apply([]) still runs the shader function (global draws)
        crate::assets::doors::apply_draws(&mut r, fac.surface_cls);
    }
}

/// room_windows stage: python populate_windows (2 window factories)
fn windows(c: &RoomConstants, cutters: &[Cutter], seed: u64, out: &mut Vec<Piece>) {
    let mut r = Rng::seeded(crate::int_hash(&[&seed.to_string(), "room_windows"]));
    let facs: Vec<WindowSpec> = (0..2).map(|_| WindowSpec::new(r.randint(0, 100_000) as u64)).collect();
    let mut j = 0u64;
    for (ci, ct) in cutters.iter().enumerate() {
        if ct.kind != CutKind::Window {
            continue;
        }
        let parent = ct.rooms.first().cloned().unwrap_or_default();
        let fac = &facs[(crate::int_hash(&[&parent]) % 2) as usize];
        let t = ct.dims[1] * r.uniform(0.1, 0.2);
        let dims = [ct.dims[0], ct.dims[2], t];
        // full-height windows pass open=False; others sample it (curtain/shutter kwargs are swallowed by create_asset)
        let tall = (ct.dims[2] - c.wall_height + c.wall_thickness).abs() < 0.1;
        let mut ir = crate::assets::inst_rng(fac.fseed, j);
        let asset = fac.sample_asset(&mut ir, dims, tall);
        let local = Mat4::translation([0.0, -c.wall_thickness / 2.0, 0.0]).mul(&Mat4::rot_y(std::f64::consts::PI));
        for (tag, mut mesh) in window_pieces(fac, &asset) {
            let (suffix, cls, rgb) = match tag {
                0 => ("", asset.frame_mat, [0.4, 0.3, 0.2]),
                1 => (".glass", "window.glass", [0.8, 0.9, 1.0]),
                2 => (".curtain", asset.cloth_mat, [0.8, 0.7, 0.6]),
                3 => (".rod", asset.rod_mat, [0.6, 0.6, 0.6]),
                _ => (".shutter", asset.frame_mat, [0.5, 0.4, 0.3]),
            };
            place(&mut mesh, ct, &local);
            let nm = format!("window{ci}{suffix}");
            out.push(Piece { name: nm.clone(), class: "Window".into(), mesh, mat: spec(&nm, cls, fac.fseed * 1000 + j, rgb) });
        }
        j += 1;
    }
}

pub fn make_openings(c: &RoomConstants, cutters: &[Cutter], seed: u64) -> Vec<Piece> {
    let mut out = vec![];
    doors(c, cutters, seed, &mut out);
    windows(c, cutters, seed, &mut out);
    out
}
