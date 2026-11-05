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
// ---- denoise: a-trous, edges from the G-buffer
@group(0) @binding(0) var<uniform> p: P;
@group(0) @binding(1) var<uniform> stp: vec4<u32>;
@group(0) @binding(2) var d_depth: texture_depth_2d;
@group(0) @binding(3) var d_nrm: texture_2d<f32>;
@group(0) @binding(4) var i_irr: texture_2d<f32>;
@group(0) @binding(5) var i_spec: texture_2d<f32>;
@group(0) @binding(6) var o_irr: texture_storage_2d<rgba16float, write>;
@group(0) @binding(13) var d_cnt: texture_2d<f32>;
@group(0) @binding(7) var o_spec: texture_storage_2d<rgba16float, write>;
@compute @workgroup_size(8, 8) fn denoise(@builtin(global_invocation_id) id: vec3<u32>) {
  let ss = i32(p.dims.z); let w = i32(p.dims.x); let h = i32(p.dims.y);
  let px = vec2<i32>(id.xy);
  if (px.x >= w || px.y >= h) { return; }
  let dims = vec2<f32>(f32(w * ss), f32(h * ss));
  let g = px * ss + vec2<i32>(ss / 2);
  let d0 = textureLoad(d_depth, g, 0);
  var c = textureLoad(i_irr, px, 0); let sc = textureLoad(i_spec, px, 0);
  // (first round: a traced pixel unlike all around it started inside a wall's corner, and saw the outside)
  var av = vec4<f32>(0.0);
  let lone_round = stp.z == 0u && (stp.y == 0u || stp.y == 3u);
  if (lone_round) {
    var hi = 0.0; var cn = 0.0;
    for (var j = -1; j <= 1; j++) { for (var i = -1; i <= 1; i++) {
      if (i == 0 && j == 0) { continue; }
      let q = px + vec2<i32>(i, j);
      if (q.x < 0 || q.y < 0 || q.x >= w || q.y >= h) { continue; }
      let t = textureLoad(i_irr, q, 0);
      hi = max(hi, dot(t.rgb, vec3<f32>(0.333))); av += t; cn += 1.0;
    } }
    if (cn > 0.0) {
      av /= cn;
      if (stp.y == 0u && dot(c.rgb, vec3<f32>(0.333)) > hi * 1.3 + 0.02) { c = vec4<f32>(av.rgb, c.a); }
    }
  }
  if (d0 >= 1.0) { textureStore(o_irr, px, c); textureStore(o_spec, px, sc); return; }
  let n0 = normalize(textureLoad(d_nrm, g, 0).xyz + vec3<f32>(1e-5));
  let p0 = world_of(g, d0, dims, p.inv_vp);
  var kw = array<f32, 3>(0.375, 0.25, 0.0625);
  let stride = i32(stp.x); let it = stp.y;
  var e = vec3<f32>(0.0); var ew = 0.0; var s = vec3<f32>(0.0); var sw = 0.0; var el = vec3<f32>(0.0); var ewl = 0.0;
  // (what is typical: the mean of compressed brightness, which a few bright pixels hardly move)
  var out_cap = 1e9;
  if (it == 0u && stp.z == 1u) {
    var ks = 0.0; var kn = 0.0;
    for (var j = -2; j <= 2; j++) { for (var i = -2; i <= 2; i++) {
      let q = px + vec2<i32>(i, j);
      if (q.x < 0 || q.y < 0 || q.x >= w || q.y >= h) { continue; }
      let l = dot(textureLoad(i_spec, q, 0).rgb, vec3<f32>(0.333));
      ks += l / (1.0 + l); kn += 1.0;
    } }
    let km = ks / max(kn, 1.0);
    out_cap = 3.0 * km / max(1.0 - km, 1e-3) + 0.02;
  }
  // (the light's own edges fade from the filter as it widens: first it takes the noise out, then the room's low frequencies)
  for (var j = -2; j <= 2; j++) { for (var i = -2; i <= 2; i++) {
    let q = px + vec2<i32>(i, j) * stride;
    if (q.x < 0 || q.y < 0 || q.x >= w || q.y >= h) { continue; }
    let gq = q * ss + vec2<i32>(ss / 2);
    let dq = textureLoad(d_depth, gq, 0);
    if (dq >= 1.0) { continue; }
    let nq = normalize(textureLoad(d_nrm, gq, 0).xyz + vec3<f32>(1e-5));
    let pq = world_of(gq, dq, dims, p.inv_vp);
    let kk = kw[abs(i)] * kw[abs(j)];
    let wn = pow(max(dot(n0, nq), 0.0), 32.0);
    let wp = exp(-abs(dot(n0, pq - p0)) / (0.03 + 0.012 * f32(stride)));
    // (the pixel itself as the first round corrected it, or a sliver alone on its surface stays as it was: a bright dot)
    let ci = select(textureLoad(i_irr, q, 0), c, i == 0 && j == 0);
    let wg = kk * wn * wp;
    // (no test on brightness: in a room lit through a window most paths find nothing, and such a test keeps exactly the noise it should take out. No clamp either: what it cut was the sun's bounce)
    e += ci.rgb * wg; ew += wg;
    let wg_l = kk * pow(max(dot(n0, nq), 0.0), 4.0) * wp; el += ci.rgb * wg_l; ewl += wg_l;
    let si = textureLoad(i_spec, q, 0);
    // (reflections on a little relief: its normals differ pixel to pixel while what they reflect does not. A looser test, or a brushed fridge stayed in grains)
    // (first round: a pixel far brighter than what is typical around it counts for less. A rare ray to a small sunlit patch would spread into a disc, and every neighbour reads it, so limiting the pixel alone is not enough. Karis's average on every pixel took the light out of all reflections)
    let l_si = dot(si.rgb, vec3<f32>(0.333));
    let kar = select(1.0, min(1.0, out_cap / max(l_si, 1e-5)), it == 0u);
    let ws = kar * kk * pow(max(dot(n0, nq), 0.0), 24.0) * wp; // (but not across a real crease: on near-black louvres the reflection is all there is, and it smeared them flat)
    s += si.rgb * ws; sw += ws;
  } }
  // (one round is either the light's or the reflections': they are traced in turn)
  let spec_only = stp.z == 1u;
  // (a curved moulding: each pixel's normal has few others to share with, and its few paths stayed as blotches. There the normals may differ more)
  let few = stp.y >= 1u && ew < 0.6 && ewl > 1e-5;
  let ee = select(select(select(c.rgb, e / max(ew, 1e-5), ew > 1e-5), el / max(ewl, 1e-5), few), c.rgb, spec_only);
  // (a mirror's picture is no noise: only rough reflections are smoothed, more the rougher)
  // (not by each pixel's own roughness alone: where a brushed metal's roughness jumps, neighbours got different blur, and a fridge came out in hard patches)
  let spec_rounds = select(0u, select(1u, stp.w, sc.a > 0.15), sc.a > 0.08);
  let sv = select(sc.rgb, s / max(sw, 1e-5), spec_only && sw > 1e-5 && it < spec_rounds);
  // (a sliver, alone on its surface: nothing to average its few paths with, so one lucky path is a bright dot on an edge. It takes what is around it)
  // (nor darker: along a moulding's top face a few paths each drew dashes of both. Again once the neighbours are smooth, for at first their mean is as noisy as the sliver)
  let lone = lone_round && ew < 0.45 && dot(av.rgb, av.rgb) > 0.0;
  // (a video: what has been seen for frames is clean already, and keeps its detail. The filter is for what is new)
  let keep = select(0.0, clamp((textureLoad(d_cnt, px, 0).r - 2.0) / 14.0, 0.0, 1.0), (u32(p.cam.w) & 64u) != 0u);
  textureStore(o_irr, px, vec4<f32>(mix(select(ee, av.rgb, lone), c.rgb, select(keep, 0.0, spec_only)), 0.0));
  textureStore(o_spec, px, vec4<f32>(mix(sv, sc.rgb, select(0.0, keep, spec_only)), sc.a));
}

// ---- a video's light and reflections, built up over its frames: before the filter, which then has less to do the longer a surface has been seen
struct TU { prev_vp: mat4x4<f32>, prev_eye: vec4<f32>, al: vec4<f32> }; // al: 1 on the first frame; the history's size; 1 for reflections
@group(0) @binding(8) var<uniform> tu: TU;
@group(0) @binding(9) var t_lamp: texture_2d<f32>;
@group(0) @binding(10) var t_n: texture_2d<f32>;
@group(0) @binding(11) var o_n: texture_storage_2d<rgba16float, write>;
@group(0) @binding(12) var t_hit: texture_2d<f32>;
fn prev_uv(q: vec3<f32>) -> vec3<f32> {
  let c = tu.prev_vp * vec4<f32>(q, 1.0);
  return vec3<f32>(c.x / c.w * 0.5 + 0.5, 0.5 - c.y / c.w * 0.5, c.w);
}
@compute @workgroup_size(8, 8) fn temporal(@builtin(global_invocation_id) id: vec3<u32>) {
  let w = i32(p.dims.x); let h = i32(p.dims.y); let ss = i32(p.dims.z); let px = vec2<i32>(id.xy);
  if (px.x >= w || px.y >= h) { return; }
  var cur4 = textureLoad(i_irr, px, 0);
  let refl = tu.al.w > 0.5;
  let cur = cur4.rgb;
  let g = px * ss + vec2<i32>(ss / 2);
  let d = textureLoad(d_depth, g, 0);
  if (d >= 1.0) { textureStore(o_irr, px, cur4); textureStore(o_n, px, vec4<f32>(0.0)); return; }
  let pos = world_of(g, d, vec2<f32>(f32(w * ss), f32(h * ss)), p.inv_vp);
  let dist = length(pos - p.cam.xyz);
  let n0 = normalize(textureLoad(d_nrm, g, 0).xyz + vec3<f32>(1e-5));
  // (the first frame is a clean picture: as good as many frames' worth. It traced its nearest lamps apart, where later frames have them all in this light)
  if (tu.al.x >= 1.0) { textureStore(o_irr, px, vec4<f32>(cur + select(textureLoad(t_lamp, px, 0).rgb, vec3<f32>(0.0), refl), cur4.a)); textureStore(o_n, px, vec4<f32>(16.0, dist, n0.x, n0.y)); return; }
  // (a mirror's picture is sharp and already right)
  if (refl && cur4.a < 0.03) { textureStore(o_irr, px, cur4); textureStore(o_n, px, vec4<f32>(1.0, dist, n0.x, n0.y)); return; }
  let hd = vec2<i32>(i32(tu.al.y), i32(tu.al.z)); let hdf = vec2<f32>(hd);
  let su = prev_uv(pos);
  if (su.z <= 0.0 || any(su.xy < vec2<f32>(0.0)) || any(su.xy >= vec2<f32>(1.0))) { textureStore(o_irr, px, cur4); textureStore(o_n, px, vec4<f32>(1.0, dist, n0.x, n0.y)); return; }
  // (from the same surface only: what the camera has just come round the corner of has no past)
  let expect = length(pos - tu.prev_eye.xyz);
  let f = su.xy * hdf - 0.5; let b = floor(f); let t = f - b;
  var acc = vec3<f32>(0.0); var sum = 0.0; var nn = 0.0;
  for (var j = 0; j < 2; j++) { for (var i = 0; i < 2; i++) {
    let q = clamp(vec2<i32>(b) + vec2<i32>(i, j), vec2<i32>(0), hd - vec2<i32>(1));
    let hn = textureLoad(t_n, q, 0);
    // (and facing the same way: a door's jamb beside the wall it is set in lies at nearly the same distance, and took the wall's light)
    if (abs(hn.g - expect) > 0.02 + 0.005 * expect || abs(hn.b - n0.x) + abs(hn.a - n0.y) > 0.3) { continue; }
    let wq = select(1.0 - t.x, t.x, i == 1) * select(1.0 - t.y, t.y, j == 1);
    acc += textureLoad(i_spec, q, 0).rgb * wq; nn += hn.r * wq; sum += wq;
  } }
  if (sum < 0.2) { textureStore(o_irr, px, cur4); textureStore(o_n, px, vec4<f32>(1.0, dist, n0.x, n0.y)); return; }
  acc /= sum; nn /= sum;
  // (a reflection moves as what it shows does, behind the surface: as far back as the rays went. A rough one is a wide blur, and stays nearer the surface)
  if (refl) {
    let k = 1.0 - smoothstep(0.1, 0.5, cur4.a);
    let vu = prev_uv(pos + normalize(pos - p.cam.xyz) * max(textureLoad(t_hit, px, 0).a, 0.0) * k);
    if (vu.z > 0.0 && all(vu.xy >= vec2<f32>(0.0)) && all(vu.xy < vec2<f32>(1.0))) {
      let f2 = vu.xy * hdf - 0.5; let b2 = floor(f2); let t2 = f2 - b2; var a2 = vec3<f32>(0.0); var w2_ = 0.0;
      // (only where that lands on this same surface: a white door frame's history bled onto the wall beside it, in a halo)
      for (var j = 0; j < 2; j++) { for (var i = 0; i < 2; i++) {
        let q = clamp(vec2<i32>(b2) + vec2<i32>(i, j), vec2<i32>(0), hd - vec2<i32>(1));
        let hv = textureLoad(t_n, q, 0); if (abs(hv.g - expect) > 0.02 + 0.005 * expect || abs(hv.b - n0.x) + abs(hv.a - n0.y) > 0.3) { continue; }
        let wq = select(1.0 - t2.x, t2.x, i == 1) * select(1.0 - t2.y, t2.y, j == 1);
        a2 += textureLoad(i_spec, q, 0).rgb * wq; w2_ += wq;
      } }
      if (w2_ > 0.5) { acc = a2 / w2_; }
    }
  }
  // (the longer a surface has been seen the less each frame counts, down to a floor: new ones clean up at once)
  // (a noisy sample would stay in the picture at the floor. What the cache answered has no noise, and the first frame's detail stays as long as it can)
  let idle = refl && textureLoad(t_hit, px, 0).a < 0.0;
  let al = select(max(1.0 / (nn + 1.0), select(select(0.05, 0.0, cur4.a <= 0.0), 0.05, refl)), 0.0, idle);
  // (a lone bright ray is a speck for many frames once it is in the history: what history has seen says how bright a new one can be. The more frames, the surer)
  let lh = dot(acc, vec3<f32>(0.333)); let lc = dot(cur, vec3<f32>(0.333));
  let cap = lh * 8.0 + 0.1;
  let cur_c = cur * min(1.0, cap / max(lc, 1e-5));
  textureStore(o_irr, px, vec4<f32>(mix(acc, cur_c, al), cur4.a));
  textureStore(o_n, px, vec4<f32>(select(min(nn + 1.0, 64.0), nn, idle), dist, n0.x, n0.y));
}

// ---- a video's picture over frames: its edges, by the shifted camera. What was there kept only as far as it fits what is around this pixel now, or what moved would trail
fn taa_c(c: vec3<f32>) -> vec3<f32> { // (compressed, in luma and chroma: a bright window's edge would otherwise win every pixel it touches)
  let l = c / (1.0 + dot(c, vec3<f32>(0.2126, 0.7152, 0.0722)));
  return vec3<f32>(dot(l, vec3<f32>(0.25, 0.5, 0.25)), dot(l, vec3<f32>(0.5, 0.0, -0.5)), dot(l, vec3<f32>(-0.25, 0.5, -0.25)));
}
fn taa_d(y: vec3<f32>) -> vec3<f32> {
  let c = vec3<f32>(y.x + y.y - y.z, y.x + y.z, y.x - y.y - y.z);
  return c / max(1.0 - dot(c, vec3<f32>(0.2126, 0.7152, 0.0722)), 1e-3);
}
@compute @workgroup_size(8, 8) fn taa(@builtin(global_invocation_id) id: vec3<u32>) {
  let w = i32(tu.al.y); let h = i32(tu.al.z); let px = vec2<i32>(id.xy);
  if (px.x >= w || px.y >= h) { return; }
  let sc = i32(textureDimensions(i_irr).x) / w;
  let dims = vec2<f32>(f32(w * sc), f32(h * sc));
  let g = px * sc + vec2<i32>(sc / 2);
  let d = textureLoad(d_depth, g, 0);
  let pos = select(world_of(g, d, dims, p.inv_vp), p.cam.xyz + normalize(world_of(g, 0.9999, dims, p.inv_vp) - p.cam.xyz) * 1000.0, d >= 1.0);
  let dist = length(pos - p.cam.xyz);
  if (tu.al.x >= 1.0) {
    var a = vec3<f32>(0.0);
    for (var j = 0; j < sc; j++) { for (var i = 0; i < sc; i++) { a += textureLoad(i_irr, px * sc + vec2<i32>(i, j), 0).rgb; } }
    textureStore(o_irr, px, vec4<f32>(a / f32(sc * sc), dist)); return;
  }
  let cur = textureLoad(i_irr, px, 0).rgb;
  let u = prev_uv(pos);
  if (u.z <= 0.0 || any(u.xy < vec2<f32>(0.0)) || any(u.xy >= vec2<f32>(1.0))) { textureStore(o_irr, px, vec4<f32>(cur, dist)); return; }
  var m1 = vec3<f32>(0.0); var m2 = vec3<f32>(0.0);
  for (var j = -1; j <= 1; j++) { for (var i = -1; i <= 1; i++) {
    let t = taa_c(textureLoad(i_irr, clamp(px + vec2<i32>(i, j), vec2<i32>(0), vec2<i32>(w - 1, h - 1)), 0).rgb);
    m1 += t; m2 += t * t;
  } }
  m1 /= 9.0; m2 /= 9.0;
  let sg = sqrt(max(m2 - m1 * m1, vec3<f32>(0.0))) * 1.25;
  // (a sharper reading of the history than four pixels' average, which would blur a little more every frame)
  let f = u.xy * vec2<f32>(f32(w), f32(h)) - 0.5; let b = floor(f); let t = f - b;
  let w0 = t * (-0.5 + t * (1.0 - 0.5 * t)); let w1 = 1.0 + t * t * (-2.5 + 1.5 * t); let w2 = t * (0.5 + t * (2.0 - 1.5 * t)); let w3 = t * t * (-0.5 + 0.5 * t);
  var hc = vec3<f32>(0.0); var hw = 0.0;
  for (var j = 0; j < 4; j++) { for (var i = 0; i < 4; i++) {
    let q = clamp(vec2<i32>(b) + vec2<i32>(i - 1, j - 1), vec2<i32>(0), vec2<i32>(w - 1, h - 1));
    var wa = array<f32, 4>(w0.x, w1.x, w2.x, w3.x); var wb = array<f32, 4>(w0.y, w1.y, w2.y, w3.y);
    let wx = wa[i]; let wy = wb[j];
    hc += textureLoad(i_spec, q, 0).rgb * wx * wy; hw += wx * wy;
  } }
  hc = max(hc / max(hw, 1e-3), vec3<f32>(0.0));
  let hy = clamp(taa_c(hc), m1 - sg, m1 + sg);
  // (what has just come into view has no past)
  let dh = textureLoad(i_spec, clamp(vec2<i32>(u.xy * vec2<f32>(f32(w), f32(h))), vec2<i32>(0), vec2<i32>(w - 1, h - 1)), 0).a;
  let expect = length(pos - tu.prev_eye.xyz);
  let al = select(0.1, 0.5, abs(dh - expect) > 0.05 * expect + 0.03);
  textureStore(o_irr, px, vec4<f32>(taa_d(mix(hy, taa_c(cur), al)), dist));
}
