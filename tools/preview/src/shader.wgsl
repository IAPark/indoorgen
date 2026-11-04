/*LIB*/
struct G {
  viewproj: mat4x4<f32>,
  inv_viewproj: mat4x4<f32>,
  sun_vp: mat4x4<f32>,
  sun_dir: vec4<f32>,      // toward the sun
  sun_col: vec4<f32>,
  sky: vec4<f32>,
  ground: vec4<f32>,
  cam_pos: vec4<f32>,
  cam_fwd: vec4<f32>,
  params: vec4<f32>,       // x = number of lights, y = exposure, z = ao strength, w = ao radius (m)
  clip2: vec4<f32>,
  clip: vec4<f32>,         // a cutaway: what lies behind this vertical plane is not drawn (xyz normal, w offset; all 0: none)
  params2: vec4<f32>,      // x = shadow texel (1/size), y = ss factor, z = fill light strength
  lpos: array<vec4<f32>, 32>,
  lcol: array<vec4<f32>, 32>,
};
struct M { color: vec4<f32>, emissive: vec4<f32>, flags: vec4<f32>, obj: mat4x4<f32>, p: array<vec4<f32>, 48> }; // flags.x: 1 = alpha mask (hair), 2 = glass; obj: world -> object frame; p: the material graph's numbers

@group(0) @binding(0) var<uniform> g: G;
@group(0) @binding(1) var shadow_tex: texture_depth_2d;
@group(0) @binding(2) var shadow_smp: sampler_comparison;
@group(1) @binding(0) var<uniform> m: M;
@group(1) @binding(1) var base_tex: texture_2d<f32>;
@group(1) @binding(2) var base_smp: sampler;

struct VIn { @location(0) pos: vec3<f32>, @location(1) nrm: vec3<f32>, @location(2) uv: vec2<f32> };
struct VOut { @builtin(position) clip: vec4<f32>, @location(0) wpos: vec3<f32>, @location(1) nrm: vec3<f32>, @location(2) uv: vec2<f32> };

@vertex fn vs_shadow(@location(0) pos: vec3<f32>) -> @builtin(position) vec4<f32> { return g.sun_vp * vec4<f32>(pos, 1.0); }

@vertex fn vs_main(v: VIn) -> VOut {
  var o: VOut;
  o.clip = g.viewproj * vec4<f32>(v.pos, 1.0);
  o.wpos = v.pos; o.nrm = v.nrm; o.uv = v.uv;
  return o;
}

struct FOut { @location(0) color: vec4<f32>, @location(1) nrm: vec4<f32>, @location(2) alb: vec4<f32>, @location(3) mat: vec4<f32> };

fn shadow_at(p: vec3<f32>, n: vec3<f32>) -> f32 {
  let ndl = clamp(dot(n, g.sun_dir.xyz), 0.0, 1.0);
  let q = p + n * (0.012 + 0.03 * (1.0 - ndl));
  let c = g.sun_vp * vec4<f32>(q, 1.0);
  let uv = vec2<f32>(c.x * 0.5 + 0.5, 0.5 - c.y * 0.5);
  if (uv.x < 0.0 || uv.x > 1.0 || uv.y < 0.0 || uv.y > 1.0 || c.z > 1.0) { return 1.0; }
  let t = g.params2.x;
  var s = 0.0;
  for (var i = -1; i <= 1; i++) { for (var j = -1; j <= 1; j++) {
    s += textureSampleCompareLevel(shadow_tex, shadow_smp, uv + vec2<f32>(f32(i), f32(j)) * t, c.z - 0.0008);
  } }
  return s / 9.0;
}

@fragment fn fs_main(v: VOut, @builtin(front_facing) ff: bool) -> FOut {
  if (dot(g.clip.xyz, g.clip.xyz) > 0.5 && dot(g.clip.xyz, v.wpos) < g.clip.w) { discard; }
  if (dot(g.clip2.xyz, g.clip2.xyz) > 0.5 && dot(g.clip2.xyz, v.wpos) < g.clip2.w) { discard; }
  var albedo = m.color;
  let tex = textureSample(base_tex, base_smp, v.uv);
  // alpha-masked cards: coverage from the full-resolution level (mip averaging dropped the leaves below the cutoff at a distance)
  let a0 = textureSampleLevel(base_tex, base_smp, v.uv, 0.0).a;
  albedo = vec4<f32>(albedo.rgb * tex.rgb, albedo.a * select(tex.a, a0, m.flags.x > 0.5));
  if (m.flags.x > 0.5 && albedo.a < 0.5) { discard; }
  var n = normalize(v.nrm);
  if (dot(v.nrm, v.nrm) < 0.25) {
    n = normalize(cross(dpdx(v.wpos), dpdy(v.wpos)));
  }
  let vdir = normalize(g.cam_pos.xyz - v.wpos);
  if (dot(n, vdir) < 0.0) { n = -n; }
  // the material graph, where the material has one (ig_shade)
  var pbr = false; var surf_color = albedo.rgb; var surf_rough = 0.5; var surf_metal = 0.0; var surf_coat = 0.0; var surf_height = 0.0;
  /*SURF*/
  if (pbr) {
    albedo = vec4<f32>(surf_color, 1.0);
    // bump from the screen-space slope of the height (surface gradient)
    let dpx = dpdx(v.wpos); let dpy = dpdy(v.wpos);
    let r1 = cross(dpy, n); let r2 = cross(n, dpx);
    let det = dot(dpx, r1);
    let grad = sign(det) * (dpdx(surf_height) * r1 + dpdy(surf_height) * r2);
    // (a limit on the slope, and none past what a pixel can show: a joint's step within a pixel is noise, and drew dashes along the tile)
    let adet = max(abs(det), 1e-9);
    let g0 = grad / adet;
    if (m.flags.y < 0.5) { let sl = length(g0); n = normalize(n - g0 * min(1.0, 0.35 / max(sl, 1e-6)) * (1.0 - smoothstep(0.2, 0.5, sl))); }
  }
  let sun = g.sun_dir.xyz;
  let ndl = max(dot(n, sun), 0.0);
  let sh = select(1.0, shadow_at(v.wpos, n), ndl > 0.0);
  var lit = g.sun_col.rgb * ndl * sh;
  let amb = mix(g.ground.rgb, g.sky.rgb, n.z * 0.5 + 0.5);
  lit += amb;
  let nl = i32(g.params.x);
  for (var i = 0; i < nl; i++) {
    let d = g.lpos[i].xyz - v.wpos;
    let r2 = max(dot(d, d), 0.04);
    let l = d * inverseSqrt(r2);
    lit += g.lcol[i].rgb * max(dot(n, l), 0.0) / r2;
  }
  // soft head-light fill so unlit corners stay readable
  lit += g.params2.z * g.sun_col.rgb * max(dot(n, vdir), 0.0) * 0.15;
  let h = normalize(sun + vdir);
  let spec = pow(max(dot(n, h), 0.0), 40.0) * 0.06 * sh * ndl;
  var col = albedo.rgb * lit + g.sun_col.rgb * spec + m.emissive.rgb;
  if (pbr) {
    // Blinn-Phong sized by roughness, and a dim reflection of the ambient light, stronger the smoother and the more metal
    let r_ = clamp(surf_rough, 0.04, 1.0);
    let pw = clamp(2.0 / (r_ * r_ * r_ * r_), 4.0, 4000.0);
    let f0 = mix(vec3<f32>(0.04), albedo.rgb, surf_metal);
    let ndh = max(dot(n, h), 0.0);
    let fres = pow(1.0 - max(dot(n, vdir), 0.0), 5.0);
    let spc = f0 * pow(ndh, pw) * (pw + 8.0) / 25.0 + surf_coat * 0.04 * pow(ndh, 1200.0) * 48.0;
    // (no environment to reflect here: metal keeps part of its colour as diffuse, and sees neutral room light, or ovens came out dark blue and mottled)
    let room = vec3<f32>(dot(amb, vec3<f32>(0.333)));
    col = albedo.rgb * (1.0 - 0.55 * surf_metal) * lit + g.sun_col.rgb * spc * sh * ndl + (f0 + (1.0 - f0) * fres * (1.0 - r_) + surf_coat * 0.04 * fres) * mix(amb, room, surf_metal) * (1.0 - 0.35 * r_) + m.emissive.rgb;
    // a mirror (no environment to reflect here): the room's light by direction with a soft streak, from the view, so parts with no UVs read as glass too (they came out blank white)
    if (surf_metal > 0.9 && surf_rough < 0.02) {
      let rf = reflect(-vdir, n);
      let streak = pow(0.5 + 0.5 * sin(9.0 * (rf.x + rf.y) + 6.0 * rf.z), 10.0);
      col = mix(vec3<f32>(0.2, 0.2, 0.21), vec3<f32>(0.46, 0.48, 0.5), rf.z * 0.5 + 0.5) + 0.22 * streak;
    }
  }
  var out: FOut;
  var a = 1.0;
  if (m.flags.x > 1.5) { a = select(0.18, m.flags.z, m.flags.z > 0.0); }
  out.color = vec4<f32>(col, a);
  out.nrm = vec4<f32>(n, surf_coat);
  // (path traced: the G-buffer; what a surface gives off is all it draws itself. Glass stays forward, over the traced picture)
  out.alb = vec4<f32>(albedo.rgb, surf_rough);
  out.mat = vec4<f32>(surf_metal, surf_coat, 0.0, 1.0);
  if (g.params2.w > 0.5 && m.flags.x < 1.5) { out.color = vec4<f32>(m.emissive.rgb, 1.0); }
  // (glass, over the traced picture: a little is lost in it; what lies behind is already lit)
  if (g.params2.w > 0.5 && m.flags.x > 1.5) {
    // (and it mirrors the room, more at a glancing angle)
    let fr = 0.04 + 0.96 * pow(1.0 - abs(dot(n, vdir)), 5.0);
    out.color = vec4<f32>(g.ground.rgb, max(fr, select(0.06, m.flags.z * 0.5, m.flags.z > 0.0)));
  }
  return out;
}

// ---------------- post: SSAO + tonemap + downsample ----------------
@group(0) @binding(3) var hdr_tex: texture_2d<f32>;
@group(0) @binding(4) var nrm_tex: texture_2d<f32>;
@group(0) @binding(5) var depth_tex: texture_depth_2d;

@vertex fn vs_full(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
  let x = f32((i << 1u) & 2u); let y = f32(i & 2u);
  return vec4<f32>(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
}

fn world_at(px: vec2<i32>, dims: vec2<f32>) -> vec3<f32> {
  let d = textureLoad(depth_tex, px, 0);
  let ndc = vec2<f32>((f32(px.x) + 0.5) / dims.x * 2.0 - 1.0, 1.0 - (f32(px.y) + 0.5) / dims.y * 2.0);
  let w = g.inv_viewproj * vec4<f32>(ndc, d, 1.0);
  return w.xyz / w.w;
}
fn hash(p: vec2<f32>) -> f32 { return fract(sin(dot(p, vec2<f32>(12.9898, 78.233))) * 43758.5453); }

// AgX, as Blender's view transform, with its Medium High Contrast look (Wrensch's fit)
fn agx(c: vec3<f32>) -> vec3<f32> {
  let ins = mat3x3<f32>(vec3<f32>(0.842479062253094, 0.0423282422610123, 0.0423756549057051), vec3<f32>(0.0784335999999992, 0.878468636469772, 0.0784336), vec3<f32>(0.0792237451477643, 0.0791661274605434, 0.879142973793104));
  let outs = mat3x3<f32>(vec3<f32>(1.19687900512017, -0.0528968517574562, -0.0529716355144438), vec3<f32>(-0.0980208811401368, 1.15190312990417, -0.0980434501171241), vec3<f32>(-0.0990297440797205, -0.0989611768448433, 1.15107367264116));
  var v = ins * max(c, vec3<f32>(1e-10));
  v = clamp((log2(v) + 12.47393) / 16.5, vec3<f32>(0.0), vec3<f32>(1.0));
  let x2 = v * v; let x4 = x2 * x2;
  v = 15.5 * x4 * x2 - 40.14 * x4 * v + 31.96 * x4 - 6.868 * x2 * v + 0.4298 * x2 + 0.1191 * v - 0.00232;
  // look: contrast about mid grey
  let luma = dot(v, vec3<f32>(0.2126, 0.7152, 0.0722));
  v = pow(max(v, vec3<f32>(0.0)), vec3<f32>(g.cam_fwd.w));
  v = outs * v;
  return pow(clamp(v, vec3<f32>(0.0), vec3<f32>(1.0)), vec3<f32>(2.2));
}
fn aces(x: vec3<f32>) -> vec3<f32> {
  return clamp((x * (2.51 * x + 0.03)) / (x * (2.43 * x + 0.59) + 0.14), vec3<f32>(0.0), vec3<f32>(1.0));
}
fn srgb(c: vec3<f32>) -> vec3<f32> {
  return select(1.055 * pow(c, vec3<f32>(1.0 / 2.4)) - 0.055, c * 12.92, c <= vec3<f32>(0.0031308));
}

fn shade_px(px: vec2<i32>, dims: vec2<f32>) -> vec3<f32> {
  let d = textureLoad(depth_tex, px, 0);
  let hdr = textureLoad(hdr_tex, px, 0).rgb;
  var ao = 1.0;
  if (d < 1.0 && g.params.z > 0.0) {
    let p = world_at(px, dims);
    // (a pixel on a crack between two triangles has no normal of its own, and its occlusion came out as a bright line: a neighbour's)
    var n = textureLoad(nrm_tex, px, 0).xyz;
    if (dot(n, n) < 0.25) {
      for (var o = 0; o < 4; o++) {
        let q = px + array<vec2<i32>, 4>(vec2<i32>(1, 0), vec2<i32>(-1, 0), vec2<i32>(0, 1), vec2<i32>(0, -1))[o];
        let m = textureLoad(nrm_tex, q, 0).xyz;
        if (dot(m, m) >= 0.25) { n = m; break; }
      }
      if (dot(n, n) < 0.25) { n = -g.cam_fwd.xyz; }
    }
    let t = normalize(select(vec3<f32>(1.0, 0.0, 0.0), vec3<f32>(0.0, 1.0, 0.0), abs(n.x) > 0.8) - n * dot(select(vec3<f32>(1.0, 0.0, 0.0), vec3<f32>(0.0, 1.0, 0.0), abs(n.x) > 0.8), n));
    let b = cross(n, t);
    // (interleaved gradient noise, not white: it averages out over a few pixels, where a random angle per pixel showed as grain on flat surfaces)
    let rot = fract(52.9829189 * fract(dot(vec2<f32>(px % vec2<i32>(64, 64)), vec2<f32>(0.06711056, 0.00583715)))) * 6.2831853;
    let R = g.params.w;
    var occ = 0.0;
    let N = 24;
    for (var i = 0; i < N; i++) {
      let fi = (f32(i) + 0.5) / f32(N);
      let ang = rot + f32(i) * 2.399963;
      let rr = sqrt(fi);
      let h = sqrt(max(1.0 - fi, 0.0));
      let lenk = mix(0.15, 1.0, fract(fi * 7.31));
      let dirv = (t * cos(ang) * rr + b * sin(ang) * rr + n * h * 0.9 + n * 0.1) * lenk * R;
      let sp = p + dirv;
      let c = g.viewproj * vec4<f32>(sp, 1.0);
      if (c.w <= 0.0) { continue; }
      let nd = c.xy / c.w;
      let spx = vec2<i32>(i32((nd.x * 0.5 + 0.5) * dims.x), i32((0.5 - nd.y * 0.5) * dims.y));
      if (spx.x < 0 || spx.y < 0 || spx.x >= i32(dims.x) || spx.y >= i32(dims.y)) { continue; }
      let sd = textureLoad(depth_tex, spx, 0);
      if (sd >= 1.0) { continue; }
      let sw = world_at(spx, dims);
      let zs = dot(sw - g.cam_pos.xyz, g.cam_fwd.xyz);
      let zq = dot(sp - g.cam_pos.xyz, g.cam_fwd.xyz);
      let diff = zq - zs;
      if (diff > 0.02 * R && diff < R * 1.2) { occ += 1.0; }
    }
    // (a wall seen nearly edge-on has its samples fall behind the depth of its own next pixels: dark grain, no real occlusion)
    let facing = abs(dot(n, normalize(g.cam_pos.xyz - p)));
    ao = 1.0 - g.params.z * smoothstep(0.08, 0.3, facing) * occ / f32(N);
  }
  // ao only attenuates the indirect part; approximated by scaling the whole radiance gently
  var c = hdr * mix(1.0, ao, 0.9) * g.params.y;
  if (g.params2.w > 0.5) { return agx(hdr * g.params.y); }
  // sky where nothing was drawn (outdoor views went black) and aerial perspective toward the horizon colour on geometry
  let ndc = vec2<f32>((f32(px.x) + 0.5) / dims.x * 2.0 - 1.0, 1.0 - (f32(px.y) + 0.5) / dims.y * 2.0);
  let far = g.inv_viewproj * vec4<f32>(ndc, 0.9999, 1.0);
  let rd = normalize(far.xyz / far.w - g.cam_pos.xyz);
  let up = clamp(rd.z, 0.0, 1.0);
  let horizon = vec3<f32>(0.75, 0.83, 0.95) * 1.1;
  let zenith = vec3<f32>(0.22, 0.42, 0.85) * 0.9;
  let sunglow = pow(max(dot(rd, g.sun_dir.xyz), 0.0), 64.0) * 2.0;
  let sky = (mix(horizon, zenith, pow(up, 0.55)) + vec3<f32>(1.0, 0.9, 0.7) * sunglow) * g.params.y * 0.55;
  if (d >= 1.0) { return aces(sky); }
  let dist = length(world_at(px, dims) - g.cam_pos.xyz);
  let haze = 1.0 - exp(-dist * 0.00045);
  c = mix(c, horizon * g.params.y * 0.55, haze);
  return aces(c);
}

@fragment fn fs_post(@builtin(position) fp: vec4<f32>) -> @location(0) vec4<f32> {
  let dims = vec2<f32>(textureDimensions(depth_tex));
  let ss = i32(g.params2.y);
  let base = vec2<i32>(i32(fp.x), i32(fp.y)) * ss;
  var acc = vec3<f32>(0.0);
  for (var i = 0; i < ss; i++) { for (var j = 0; j < ss; j++) {
    acc += shade_px(base + vec2<i32>(i, j), dims);
  } }
  acc = acc / f32(ss * ss);
  return vec4<f32>(srgb(acc), 1.0);
}
