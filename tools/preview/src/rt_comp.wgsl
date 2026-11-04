struct P { inv_vp: mat4x4<f32>, cam: vec4<f32>, sun_dir: vec4<f32>, sun_col: vec4<f32>, dims: vec4<u32>, misc: vec4<u32>, misc2: vec4<u32>, scale: vec4<f32> };
const PI = 3.14159265;
fn world_of(px: vec2<i32>, d: f32, dims: vec2<f32>, inv_vp: mat4x4<f32>) -> vec3<f32> {
  let ndc = vec2<f32>((f32(px.x) + 0.5) / dims.x * 2.0 - 1.0, 1.0 - (f32(px.y) + 0.5) / dims.y * 2.0);
  let w = inv_vp * vec4<f32>(ndc, d, 1.0);
  return w.xyz / w.w;
}
fn sky_uv(d: vec3<f32>, sw: u32, sh: u32) -> vec2<f32> {
  let az = fract(atan2(d.y, d.x) / (2.0 * PI));
  let v = sqrt(asin(clamp(d.z, 0.0, 1.0)) / (PI * 0.5));
  return vec2<f32>(az * f32(sw) - 0.5, v * f32(sh) - 0.5);
}
@group(0) @binding(0) var<uniform> p: P;
@group(0) @binding(1) var<storage, read> sky_t: array<vec4<f32>>;
@group(0) @binding(2) var c_depth: texture_depth_2d;
@group(0) @binding(3) var c_nrm: texture_2d<f32>;
@group(0) @binding(4) var c_alb: texture_2d<f32>;
@group(0) @binding(5) var c_mat: texture_2d<f32>;
@group(0) @binding(6) var c_irr: texture_2d<f32>;
@group(0) @binding(7) var c_spec: texture_2d<f32>;
@group(0) @binding(8) var c_emis: texture_2d<f32>;
@group(0) @binding(9) var o_lit: texture_storage_2d<rgba16float, write>;
@group(0) @binding(10) var c_fine: texture_2d<f32>;
@group(0) @binding(11) var c_glint: texture_2d<f32>;
@group(0) @binding(12) var c_lamp: texture_2d<f32>;
fn sky_at(d: vec3<f32>) -> vec3<f32> {
  // (below the horizon Blender's sky does not stop: the horizon's colour, gone by 24 degrees down, as measured from it. A warm fill for a low sun)
  var dd = d; var fade = 1.0;
  if (d.z <= 0.0) {
    let el = asin(clamp(d.z, -1.0, 0.0));
    fade = max(1.0 + el * 7.5 / PI, 0.0); fade = fade * fade * fade;
    if (fade <= 0.0) { return vec3<f32>(0.0); }
    dd = normalize(vec3<f32>(d.x, d.y, 1e-4));
  }
  let sw = i32(p.misc.w); let sh = i32(p.misc2.x);
  let uv = sky_uv(dd, u32(sw), u32(sh));
  let x0 = i32(floor(uv.x)); let y0 = clamp(i32(floor(uv.y)), 0, sh - 1); let y1 = min(y0 + 1, sh - 1);
  let fx = uv.x - floor(uv.x); let fy = clamp(uv.y - floor(uv.y), 0.0, 1.0);
  let xa = (x0 + sw) % sw; let xb = (x0 + 1 + sw) % sw;
  let a = mix(sky_t[y0 * sw + xa].rgb, sky_t[y0 * sw + xb].rgb, fx);
  let b = mix(sky_t[y1 * sw + xa].rgb, sky_t[y1 * sw + xb].rgb, fx);
  return mix(a, b, fy) * p.scale.y * fade;
}
@compute @workgroup_size(8, 8) fn compose(@builtin(global_invocation_id) id: vec3<u32>) {
  let ss = i32(p.dims.z); let w = i32(p.dims.x) * ss; let h = i32(p.dims.y) * ss;
  let px0 = vec2<i32>(id.xy);
  if (px0.x >= w || px0.y >= h) { return; }
  let dims = vec2<f32>(f32(w), f32(h));
  var px = px0;
  var d = textureLoad(c_depth, px, 0);
  // (a crack between two meshes, where what lies behind the wall -- the sky, a sunlit outside -- came through as white dots: a pixel far behind its neighbours on two opposite sides takes one of them)
  let dist0 = select(length(world_of(px0, d, dims, p.inv_vp) - p.cam.xyz), 1e9, d >= 1.0);
  for (var k = 0; k < 8; k++) {
    let o = array<vec2<i32>, 8>(vec2<i32>(1, 0), vec2<i32>(0, 1), vec2<i32>(1, 1), vec2<i32>(1, -1), vec2<i32>(2, 0), vec2<i32>(0, 2), vec2<i32>(2, 2), vec2<i32>(2, -2))[k];
    let a0 = clamp(px0 + o, vec2<i32>(0), vec2<i32>(w - 1, h - 1)); let b0 = clamp(px0 - o, vec2<i32>(0), vec2<i32>(w - 1, h - 1));
    let da = textureLoad(c_depth, a0, 0); let db = textureLoad(c_depth, b0, 0);
    if (da < 1.0 && db < 1.0 && length(world_of(a0, da, dims, p.inv_vp) - p.cam.xyz) < dist0 - 0.06 && length(world_of(b0, db, dims, p.inv_vp) - p.cam.xyz) < dist0 - 0.06) { px = a0; d = da; break; }
  }
  if (d >= 1.0) {
    let rd = normalize(world_of(px0, 0.9999, dims, p.inv_vp) - p.cam.xyz);
    // (a cutaway stands in no world: plain behind it)
    let bg = select(sky_at(rd), vec3<f32>(0.25), p.misc2.w == 1u);
    textureStore(o_lit, px0, vec4<f32>(bg, 1.0));
    return;
  }
  let pos = world_of(px, d, dims, p.inv_vp);
  let nr = textureLoad(c_nrm, px, 0).xyz;
  let n = select(-normalize(pos - p.cam.xyz), normalize(nr), dot(nr, nr) > 0.25);
  let a = textureLoad(c_alb, px, 0); let mt = textureLoad(c_mat, px, 0);
  // the filtered light, from the traced pixels around this one that lie on the same surface
  let lw = i32(p.dims.x); let lh = i32(p.dims.y);
  let f = (vec2<f32>(px) + 0.5) / f32(ss) - 0.5;
  let b = vec2<i32>(floor(f));
  var e = vec3<f32>(0.0); var s = vec3<f32>(0.0); var gl_ = vec3<f32>(0.0); var dl_ = vec3<f32>(0.0); var tw = 0.0;
  for (var j = 0; j < 2; j++) { for (var i = 0; i < 2; i++) {
    let q = clamp(b + vec2<i32>(i, j), vec2<i32>(0), vec2<i32>(lw - 1, lh - 1));
    let gq = q * ss + vec2<i32>(ss / 2);
    let dq = textureLoad(c_depth, gq, 0);
    if (dq >= 1.0) { continue; }
    let pq = world_of(gq, dq, dims, p.inv_vp);
    let nq = textureLoad(c_nrm, gq, 0).xyz;
    let bw = (1.0 - abs(f.x - f32(b.x + i))) * (1.0 - abs(f.y - f32(b.y + j)));
    let wt = bw * pow(max(dot(n, normalize(nq + vec3<f32>(1e-5))), 0.0), 8.0) * exp(-abs(dot(n, pq - pos)) / 0.02) + 1e-4 * bw;
    let c = textureLoad(c_irr, q, 0);
    dl_ += textureLoad(c_lamp, q, 0).rgb * wt; gl_ += textureLoad(c_glint, q, 0).rgb * wt; e += c.rgb * wt; s += textureLoad(c_spec, q, 0).rgb * wt; tw += wt;
  } }
  e /= max(tw, 1e-5); s /= max(tw, 1e-5); gl_ /= max(tw, 1e-5); dl_ /= max(tw, 1e-5);
  // the sun at this very point, and the light of a surface no traced pixel landed on
  let fine = textureLoad(c_fine, px, 0);
  var v = fine.a;
  // (a shadow's soft edge, from few rays: smoothed along this surface, only where the sun's visibility changes nearby)
  var v_lo = v; var v_hi = v;
  for (var k = 0; k < 4; k++) {
    let o = array<vec2<i32>, 4>(vec2<i32>(2, 0), vec2<i32>(-2, 0), vec2<i32>(0, 2), vec2<i32>(0, -2))[k];
    let t = textureLoad(c_fine, clamp(px + o, vec2<i32>(0), vec2<i32>(w - 1, h - 1)), 0).a;
    v_lo = min(v_lo, t); v_hi = max(v_hi, t);
  }
  // (a sliver no traced pixel landed on, lit by the sun alone among what is around it: a ray slipping past a wall's edge, a broken bright line along a join. Its sun is what its neighbours have)
  if (fine.r >= 0.0 && v > v_lo) {
    v = v_lo; for (var k = 0; k < 4; k++) {
      let o = array<vec2<i32>, 4>(vec2<i32>(ss, 0), vec2<i32>(-ss, 0), vec2<i32>(0, ss), vec2<i32>(0, -ss))[k];
      v = max(v, textureLoad(c_fine, clamp(px + o, vec2<i32>(0), vec2<i32>(w - 1, h - 1)), 0).a);
    }
    v = min(v, fine.a);
  }
  if (v_hi > v_lo) {
    var vs = 0.0; var vc = 0.0;
    for (var j = -2; j <= 2; j++) { for (var i = -2; i <= 2; i++) {
      let q = px + vec2<i32>(i, j);
      if (q.x < 0 || q.y < 0 || q.x >= w || q.y >= h) { continue; }
      let dq = textureLoad(c_depth, q, 0);
      if (dq >= 1.0 || abs(dot(n, world_of(q, dq, dims, p.inv_vp) - pos)) > 0.01) { continue; }
      let wq = f32(3 - abs(i)) * f32(3 - abs(j));
      vs += textureLoad(c_fine, q, 0).a * wq; vc += wq;
    } }
    if (vc > 0.0) { v = vs / vc; }
  }
  // (the filtered light has no strong lamps in it: the nearest lamps' direct light, with its shadows, comes on top. A surface traced on its own has them already)
  let e_room = select(e, min(fine.rgb, (e + dl_) * 1.2), fine.r >= 0.0);
  if (fine.r >= 0.0) { e = e_room; } else { e += dl_; }
  let metal = mt.x;
  // (cloth is matte, as render_rust.py makes it: very rough, a weak specular)
  let cloth = mt.z > 0.0;
  let rough = select(a.a, max(a.a, 0.85), cloth);
  let vdir = normalize(p.cam.xyz - pos);
  let sun = p.sun_dir.xyz;
  let ndl = max(dot(n, sun), 0.0);
  let lit = e + p.sun_col.rgb * ndl * v / PI;
  // specular: what the reflection rays found, by the share a rough surface returns (Karis's fit), and a clear coat's own Fresnel on top
  let f0 = mix(vec3<f32>(select(0.04, 0.02, cloth)), a.rgb, metal);
  // specular anti-aliasing: Fresnel over what the whole output pixel sees. A ledge nearly edge-on, thinner than a pixel, was at full Fresnel on its own and drew a broken bright line
  var nbar = n;
  for (var k = 0; k < 4; k++) {
    let o = array<vec2<i32>, 4>(vec2<i32>(ss, 0), vec2<i32>(-ss, 0), vec2<i32>(0, ss), vec2<i32>(0, -ss))[k];
    let nq = textureLoad(c_nrm, clamp(px + o, vec2<i32>(0), vec2<i32>(w - 1, h - 1)), 0).xyz;
    if (dot(nq, nq) > 0.25) { nbar += nq; } else { nbar += n; }
  }
  let nv = max(max(dot(n, vdir), dot(normalize(nbar), vdir)), 0.001);
  let r_ = rough * vec4<f32>(-1.0, -0.0275, -0.572, 0.022) + vec4<f32>(1.0, 0.0425, 1.04, -0.04);
  let a004 = min(r_.x * r_.x, pow(2.0, -9.28 * nv)) * r_.x + r_.y;
  let ab = vec2<f32>(-1.04, 1.04) * a004 + r_.zw;
  // (what bounces more than once off a rough surface's relief, Fdez-Aguera's way: one bounce alone keeps half the light at high roughness, and rough metal came out far too dark. Cycles counts it)
  let ess_ = clamp(ab.x + ab.y, 0.0, 1.0); let favg_ = f0 + (vec3<f32>(1.0) - f0) / 21.0;
  let fr = (f0 * ab.x + vec3<f32>(ab.y)) * (vec3<f32>(1.0) + favg_ * (1.0 - ess_) / max(vec3<f32>(1.0) - favg_ * (1.0 - ess_), vec3<f32>(0.01)) / max(ess_, 0.05) * 1.0);
  let coat = textureLoad(c_nrm, px, 0).a;
  let fc = coat * (0.04 + 0.96 * pow(1.0 - nv, 5.0));
  // the sun's highlight, on the normal as this part of the picture sees it: relief finer than a pixel widens the highlight (Toksvig), where taken point by point it sparkled, and leather came out speckled
  var nb = n;
  if (v > 0.0 && ndl > 0.0 && rough < 0.7) {
    for (var k = 0; k < 8; k++) {
      let o = array<vec2<i32>, 8>(vec2<i32>(1, 0), vec2<i32>(-1, 0), vec2<i32>(0, 1), vec2<i32>(0, -1), vec2<i32>(2, 2), vec2<i32>(-2, 2), vec2<i32>(2, -2), vec2<i32>(-2, -2))[k];
      let nq = textureLoad(c_nrm, clamp(px + o, vec2<i32>(0), vec2<i32>(w - 1, h - 1)), 0).xyz;
      if (dot(nq, n) > 0.5) { nb += nq; } else { nb += n; }
    }
    nb /= 9.0;
  }
  let nbl = max(length(nb), 0.01); let ns = nb / nbl;
  let h_ = normalize(sun + vdir);
  let r2 = sqrt(max(rough * rough * rough * rough + (1.0 - nbl) / nbl, 0.000004)); let ndh = max(dot(ns, h_), 0.0);
  let ndl_s = max(dot(ns, sun), 0.0);
  let dn = ndh * ndh * (r2 * r2 - 1.0) + 1.0;
  let ggx = r2 * r2 / (PI * dn * dn + 1e-6);
  let vf = 0.25 / max(mix(nv, 1.0, r2 * 0.5) * mix(ndl_s, 1.0, r2 * 0.5), 0.05);
  let spec_sun = (f0 + (vec3<f32>(1.0) - f0) * pow(1.0 - max(dot(h_, vdir), 0.0), 5.0)) * min(ggx * vf, 40.0) * ndl_s * p.sun_col.rgb * v;
  // (rays only for the truly glossy: anything rougher, or a coat over fine relief as Blender's surfaces have, gives back the room's light, blurred. Traced, floors came out in sharp glare)
  // (and not on a thin rail no traced pixel landed on: what it would borrow is its neighbours', who traced nothing, and bright metal came out black)
  // (brushed metal that is thin or curved too: a sharp ray per pixel put hard glints on rails and frames that Blender, with its fine relief, draws soft. Flat panels keep their reflections)
  var flat_here = true;
  if (metal >= 0.5 && rough >= 0.03) {
    // (one side is only an edge nearby. Not in a mirror's picture, at one sample a pixel: there a speck of relief counted, and left dots across a door)
    // (and by what lies around it: a picture frame's face lies flat on its wall with the same normal, and was taken for a panel. Metal with something else on both sides is a thin part)
    var off_ = 0; let r_ = ss * 2 + 2;
    for (var k = 0; k < 4; k++) {
      let o = array<vec2<i32>, 4>(vec2<i32>(r_, 0), vec2<i32>(-r_, 0), vec2<i32>(0, r_), vec2<i32>(0, -r_))[k];
      let qq = clamp(px + o, vec2<i32>(0), vec2<i32>(w - 1, h - 1));
      let nq = textureLoad(c_nrm, qq, 0).xyz;
      if ((ss > 1 && dot(n, nq) < 0.98 * length(nq)) || textureLoad(c_mat, qq, 0).x < 0.5 || textureLoad(c_alb, qq, 0).a < 0.001) { off_++; } // (a mirror beside it is no more of the same metal)
    }
    flat_here = off_ < 2;
  }
  // (not by roughness alone under a coat: where a ceramic's roughness varies across 0.03, a hard switch between sharp and blurred drew camouflage on a sink)
  // (blended out by roughness, not cut: a surface whose roughness wanders across a limit came out in hard patches, on a metal door and a tile floor)
  // (at any roughness: a material whose roughness jumps between two values came out in hard patches when one of them took the blurred room light, on a metal door and a tile floor)
  let traced = select(0.0, 1.0, ((rough < 0.03 && (metal >= 0.5 || coat < 0.25)) || (coat < 0.25 && flat_here)) && fine.r < 0.0);
  // (what a rough surface reflects is the room: a lamp's own highlight is the glint below. With its direct light here too, tile seen at a glancing angle near a lamp washed out)
  // (thin brushed metal takes the lamps too: its wide highlights are mostly them, and frames came out black)
  var refl = mix(select(e_room, e, metal >= 0.5 && !flat_here), s, traced) * (fr + vec3<f32>(fc));
  // cloth's sheen: fine fibres send back a share of the light uncoloured, more at grazing angles, taking it from the colour underneath
  let sheen = mt.z * (0.2 + 0.6 * pow(1.0 - nv, 4.0));
  let col = textureLoad(c_emis, px, 0).rgb + a.rgb * (1.0 - metal) * lit * (1.0 - fc) * (1.0 - sheen) + lit * sheen + refl + spec_sun + select(gl_ * (fr + vec3<f32>(fc)), vec3<f32>(0.0), cloth);
  // (RT_SHOW: one ingredient of the picture alone, to find which is wrong)
  if (p.misc2.w == 9u) { textureStore(o_lit, px0, vec4<f32>(mt.w - 1.0, mt.z, a.a, 1.0)); return; }
  if (p.misc2.w == 7u) { textureStore(o_lit, px0, vec4<f32>(select(0.0, 3.0, fine.r >= 0.0), min(tw, 1.0) * 0.5, select(0.0, 3.0, any(px != px0)), 1.0)); return; }
  if (p.misc2.w == 5u) { textureStore(o_lit, px0, vec4<f32>(vec3<f32>(v), 1.0)); return; }
  if (p.misc2.w == 6u) { textureStore(o_lit, px0, vec4<f32>(n * 0.5 + 0.5, 1.0)); return; }
  if (p.misc2.w == 2u) { textureStore(o_lit, px0, vec4<f32>(a.rgb, 1.0)); return; }
  if (p.misc2.w == 3u) { textureStore(o_lit, px0, vec4<f32>(lit, 1.0)); return; }
  if (p.misc2.w == 4u) { textureStore(o_lit, px0, vec4<f32>(refl + spec_sun, 1.0)); return; }
  textureStore(o_lit, px0, vec4<f32>(col, 1.0));
}
