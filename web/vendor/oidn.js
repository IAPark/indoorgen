var Vr = Object.defineProperty;
var jr = (r, t, e) => t in r ? Vr(r, t, { enumerable: !0, configurable: !0, writable: !0, value: e }) : r[t] = e;
var y = (r, t, e) => jr(r, typeof t != "symbol" ? t + "" : t, e);
class Kr {
  constructor() {
    y(this, "dims", []);
    y(this, "paddedDims", []);
    y(this, "layout", "x");
    y(this, "dataType", "Float32");
  }
  getByteSize() {
    let t = 1;
    for (const e of this.paddedDims)
      t *= e;
    return this.dataType === "Float32" ? t *= 4 : this.dataType === "Float16" && (t *= 2), t;
  }
}
class qr {
  constructor(t, e) {
    this.desc = t, this.data = e;
  }
}
class Qr {
  constructor(t) {
    y(this, "offset", 0);
    this._view = t;
  }
  read(t) {
    const e = this._view, i = this.offset;
    switch (this.offset += t, t) {
      case 1:
        return e.getUint8(i);
      case 2:
        return e.getUint16(i, !0);
      case 4:
        return e.getUint32(i, !0);
      case 8:
        return Number(e.getBigUint64(i, !0));
      default:
        throw new Error("unsupported read size");
    }
  }
}
function Zr(r) {
  const t = new Uint8Array(r), e = new Qr(new DataView(r));
  if (e.read(2) !== 16855)
    throw new Error("invalid or corrupted weights blob");
  const n = e.read(1);
  if (e.read(1), n !== 2)
    throw new Error("unsupported weights blob version");
  const o = e.read(8);
  e.offset = o;
  const a = e.read(4), s = /* @__PURE__ */ new Map();
  for (let u = 0; u < a; ++u) {
    const l = new Kr(), p = e.read(2), c = new TextDecoder().decode(
      t.subarray(e.offset, e.offset + p)
    );
    e.offset += p;
    const d = e.read(1);
    for (let m = 0; m < d; ++m)
      l.dims.push(e.read(4));
    l.paddedDims = [...l.dims], new TextDecoder().decode(
      t.subarray(e.offset, e.offset + d)
    ) === "oihw" && (l.layout = "oihw"), e.offset += d;
    const g = String.fromCharCode(e.read(1));
    if (g === "f")
      l.dataType = "Float32";
    else if (g === "h")
      l.dataType = "Float16";
    else
      throw new Error("invalid tensor data type");
    const f = e.read(8), _ = t.slice(
      f,
      f + l.getByteSize()
    );
    s.set(c, new qr(l, _));
  }
  return s;
}
function Jr(r, t) {
  return r.channels === t.channels;
}
const he = 8;
class fe {
  constructor(t, e, i) {
    y(this, "autoUpdateOutputBuffer", !0);
    y(this, "_label");
    y(this, "_device");
    y(this, "_outputBuffers", {});
    y(this, "_pipeline");
    y(this, "_bindGroups", []);
    y(this, "_needsUpdatePipeline", !0);
    y(this, "_needsResizeBuffer", !0);
    y(this, "_inputs", []);
    y(this, "_outputs", []);
    y(this, "_uniforms", []);
    y(this, "_uniformBuffers", {});
    y(this, "_width", 10);
    y(this, "_height", 10);
    y(this, "_execWidth");
    y(this, "_execHeight");
    y(this, "_csCode", "");
    y(this, "_csMain");
    y(this, "_csDefine");
    y(this, "_groupOffsets", {
      inputs: 0,
      uniforms: 1,
      outputs: 2
    });
    this._label = t, this._device = e, this._csMain = i.csMain, this._csDefine = i.csDefine, this._inputs = i.inputs, this._outputs = i.outputs, this._uniforms = i.uniforms, this.autoUpdateOutputBuffer = i.autoUpdateOutputBuffer ?? !0, i.uniforms.forEach((n) => {
      this._uniformBuffers[n.label] = e.createBuffer({
        label: this._label,
        size: n.data.byteLength,
        usage: GPUBufferUsage.UNIFORM | GPUBufferUsage.COPY_DST
      }), this._device.queue.writeBuffer(
        this._uniformBuffers[n.label],
        0,
        n.data
      );
    });
  }
  setCSCode({ csDefine: t, csMain: e }) {
    this._csDefine = t, this._csMain = e, this._needsUpdatePipeline = !0;
  }
  setSize(t, e) {
    t = Math.ceil(t), e = Math.ceil(e);
    const i = t !== this._width || e !== this._height;
    this._width = t, this._height = e, i && (this._needsResizeBuffer = !0, this._needsUpdatePipeline = !0);
  }
  setExecuteSize(t, e) {
    t = Math.ceil(t), e = Math.ceil(e), this._execWidth = t, this._execHeight = e;
  }
  setOutputParams(t) {
    this.autoUpdateOutputBuffer && this._updateOutputBuffers(t), this._needsUpdatePipeline = !0;
  }
  setOutputBuffers(t) {
    this._outputBuffers = Object.keys(t).reduce((e, i) => (e[i] = {
      buffer: t[i],
      params: { channels: 4 }
    }, e), {});
  }
  setUniform(t, e) {
    const i = this._uniformBuffers[t];
    this._device.queue.writeBuffer(i, 0, e);
  }
  getOutput(t) {
    return this._needsResizeBuffer && this.autoUpdateOutputBuffer && (this._resizeOutputBuffers(), this._needsResizeBuffer = !1), this._outputBuffers[t].buffer;
  }
  dispose(t = !0) {
    Object.keys(this._uniformBuffers).forEach((e) => {
      this._uniformBuffers[e].destroy();
    }), t && Object.keys(this._outputBuffers).forEach((e) => {
      this._outputBuffers[e].buffer.destroy();
    });
  }
  _createBuffer(t) {
    const e = this._width * this._height * 4 * 4;
    return this._device.createBuffer({
      label: this._label,
      // webgpu needs buffer at least 80 bytes.
      size: Math.max(e, 80),
      usage: GPUBufferUsage.STORAGE | GPUBufferUsage.COPY_DST | GPUBufferUsage.COPY_SRC
    });
  }
  _resizeOutputBuffers() {
    const t = this._outputBuffers;
    for (const e in t) {
      const { buffer: i, params: n } = t[e];
      i.destroy(), t[e].buffer = this._createBuffer(n);
    }
  }
  _updateOutputBuffers(t) {
    var i, n;
    const e = this._outputBuffers;
    for (const o in t) {
      const a = t[o];
      if (!Jr(
        a,
        ((i = e[o]) == null ? void 0 : i.params) || {}
      )) {
        (n = e[o]) == null || n.buffer.destroy();
        const s = this._createBuffer(a);
        e[o] = {
          buffer: s,
          params: a
        };
      }
    }
  }
  _updatePipeline(t, e) {
    if (!this._needsUpdatePipeline)
      return;
    this._needsUpdatePipeline = !1;
    const i = this._device, n = this._getFullCs(t, e);
    n !== this._csCode && (this._csCode = n, this._pipeline = i.createComputePipeline({
      label: this._label,
      layout: "auto",
      compute: {
        module: i.createShaderModule({
          label: this._label,
          code: n
        }),
        entryPoint: "main"
      }
    }), this._updateBindGroups());
  }
  _getFullCs(t, e) {
    const i = this._inputs, n = this._uniforms;
    let o = 0;
    const a = this._groupOffsets = {
      inputs: 0,
      uniforms: 0,
      outputs: 0
    };
    return i.length > 0 && o++, n.length > 0 && (a.uniforms = o, o++), a.outputs = o, `
${i.sort().map((u, l) => {
      const p = `@group(${a.inputs}) @binding(${l}) `, c = `in_${u}`;
      return e[u] === "texture" ? `${p} var ${c}: texture_2d<f32>;` : `${p} var<storage, read> ${c}: array<vec${t[u].channels}f>;`;
    }).join(`
`)}
${this._uniforms.map(
      (u, l) => `@group(${a.uniforms}) @binding(${l}) var<uniform> ${u.label}: ${u.type};`
    ).join(`
`)}

${this._outputs.map(
      (u, l) => `@group(${a.outputs}) @binding(${l}) var<storage, read_write> out_${u}: array<vec${this._outputBuffers[u].params.channels}f>;`
    ).join(`
`)}
${this._csDefine ?? ""}
@compute @workgroup_size(${he}, ${he}, 1)
fn main(@builtin(global_invocation_id) globalId: vec3u) {
${this._csMain}
}
`;
  }
  _updateBindGroups() {
    const t = [], e = this._device, i = this._groupOffsets;
    this._uniforms.length > 0 && (t[i.uniforms] = e.createBindGroup({
      label: this._label,
      layout: this._pipeline.getBindGroupLayout(i.uniforms),
      entries: this._uniforms.map(
        (n, o) => ({
          binding: o,
          resource: {
            buffer: this._uniformBuffers[n.label]
          }
        })
      )
    })), this._bindGroups = t;
  }
  createPass(t, e) {
    this._needsResizeBuffer && this.autoUpdateOutputBuffer && (this._resizeOutputBuffers(), this._needsResizeBuffer = !1);
    const i = this._inputs.reduce((a, s) => (a[s] = e[s].buffer ? "buffer" : "texture", a), {});
    this._updatePipeline(e, i);
    const n = this._groupOffsets;
    this._inputs.length > 0 && (this._bindGroups[n.inputs] = this._device.createBindGroup({
      label: this._label,
      layout: this._pipeline.getBindGroupLayout(n.inputs),
      entries: this._inputs.map((a, s) => ({
        binding: s,
        // TODO
        resource: e[a].buffer ? {
          buffer: e[a].buffer
        } : e[a].texture.createView()
      }))
    })), this._bindGroups[n.outputs] = this._device.createBindGroup({
      label: this._label,
      layout: this._pipeline.getBindGroupLayout(n.outputs),
      entries: this._outputs.map((a, s) => ({
        binding: s,
        resource: {
          buffer: this._outputBuffers[a].buffer
        }
      }))
    });
    const o = t.beginComputePass();
    o.setPipeline(this._pipeline), this._bindGroups.forEach((a, s) => {
      o.setBindGroup(s, a);
    }), o.dispatchWorkgroups(
      Math.ceil((this._execWidth ?? this._width) / he),
      Math.ceil((this._execHeight ?? this._height) / he),
      1
    ), o.end();
  }
}
const ne = 34028234663852886e22;
function oe(r, t, e) {
  return Number.isNaN(r) ? 0 : Math.min(Math.max(r, t), e);
}
const _i = (
  /* wgsl */
  `
const FLT_MAX = 3.4028234663852886e38;
fn sanitize(v: vec3f, lo: f32, hi: f32) -> vec3f {
  let bits = bitcast<vec3u>(v);
  let magnitude = bits & vec3u(0x7fffffffu);
  let negative = (bits & vec3u(0x80000000u)) != vec3u(0u);
  let infinite = select(vec3f(hi), vec3f(lo), negative);
  let clamped = select(clamp(v, vec3f(lo), vec3f(hi)), infinite, magnitude == vec3u(0x7f800000u));
  return select(clamped, vec3f(0.0), magnitude > vec3u(0x7f800000u));
}
`
), Je = 1412.83765, ti = 1.64593172, ei = 0.431384981, ii = -0.00294139609, ri = 0.192653254, ni = 0.00626026094, oi = 0.998620152, qi = 15794576e-13, Qi = 0.0322087631, Zi = 0.00223151711, Ji = 0.370974749, tr = 65504, er = or(tr), ir = 1 / er, rr = er, Me = Math.log(tr + 1), nr = 1 / Me;
function or(r) {
  return r <= qi ? Je * r : r <= Qi ? ti * Math.pow(r, ei) + ii : ri * Math.log(r + ni) + oi;
}
function tn(r) {
  return r <= Zi ? r / Je : r <= Ji ? Math.pow((r - ii) / ti, 1 / ei) : Math.exp((r - oi) / ri) - ni;
}
function en(r, t) {
  return t === "log" ? Math.log(r + 1) * nr : or(r) * ir;
}
function rn(r, t) {
  return t === "log" ? Math.exp(r * Me) - 1 : tn(r * rr);
}
function nn({
  data: r,
  channels: t,
  inputScale: e,
  transfer: i = "pu"
}) {
  const n = new Float32Array(r);
  for (let o = 0; o < n.length; o += t)
    for (let a = 0; a < 3; a++)
      n[o + a] = en(
        oe(n[o + a] * e, 0, ne),
        i
      );
  return n;
}
function on({
  data: r,
  channels: t,
  inputScale: e,
  transfer: i = "pu"
}) {
  const n = new Float32Array(r), o = 1 / e;
  for (let a = 0; a < n.length; a += t)
    for (let s = 0; s < 3; s++)
      n[a + s] = rn(oe(n[a + s], 0, ne), i) * o;
  return n;
}
function an({
  data: r,
  channels: t
}) {
  let e = 0;
  for (let a = 0; a < r.length; a += t) {
    const s = oe(r[a], 0, ne), u = oe(r[a + 1], 0, ne), l = oe(r[a + 2], 0, ne), p = 0.212671 * s + 0.71516 * u + 0.072169 * l;
    e += Math.log2(p + 1e-4);
  }
  const i = r.length / t, n = e / i;
  return 0.18 / Math.pow(2, n);
}
const ar = `
const a = ${Je};
const b = ${ti};
const c = ${ei};
const d = ${ii};
const e = ${ri};
const f = ${ni};
const g = ${oi};
const y0 =${qi};
const y1 =${Qi};
const x0 =${Zi};
const x1 =${Ji};

const normScale = ${ir};
const rcpNormScale = ${rr};
`;
function un(r) {
  return (
    /* wgsl */
    `
${ar}
const logXMax = ${Me};
const logNormScale = ${nr};
${r === "log" ? "fn HDRForward(y: f32) -> f32 { return log(y + 1.0) * logNormScale; }" : `fn HDRForward(y: f32) -> f32 {
  if (y <= y0) { return a * y * normScale; }
  else if (y <= y1) { return (b * pow(y, c) + d) * normScale; }
  else { return (e * log(y + f) + g) * normScale; }
}`}`
  );
}
function sn(r) {
  return (
    /* wgsl */
    `
${ar}
const logXMax = ${Me};
${r === "log" ? "fn HDRInverse(x: f32) -> f32 { return exp(x * logXMax) - 1.0; }" : `fn HDRInverse(x: f32) -> f32 {
  let y = x * rcpNormScale;
  if (y <= x0) { return y / a; }
  else if (y <= x1) { return pow((y - d) / b, 1 / c); }
  else { return exp((y - g) / e) - f; }
}`}
`
  );
}
class Ue {
  constructor(t, e, i, n) {
    this.x = t, this.y = e, this.width = i, this.height = n;
  }
}
class cn {
  constructor(t, e, i = "pu") {
    y(this, "_inputPassAux");
    y(this, "_inputPassColor");
    y(this, "_outputPass");
    y(this, "_copyPass");
    y(this, "_isInputTexture");
    this._device = t, this._isHDR = e, this._hdrTransfer = i;
    const n = [
      {
        label: "inputScale",
        type: "f32",
        data: new Float32Array([1])
      },
      {
        label: "inputSize",
        type: "vec2i",
        data: new Int32Array(2)
      },
      {
        label: "outputSize",
        type: "vec2i",
        data: new Int32Array(2)
      },
      {
        label: "inputOffset",
        type: "vec2i",
        data: new Int32Array(2)
      }
    ];
    this._inputPassAux = new fe("inputPassAux", this._device, {
      inputs: ["color", "albedo", "normal"],
      outputs: ["color", "albedo", "normal"],
      uniforms: n,
      csDefine: "",
      csMain: ""
    }), this._inputPassColor = new fe("inputPassColor", this._device, {
      inputs: ["color"],
      outputs: ["color"],
      uniforms: n,
      csDefine: "",
      csMain: ""
    }), this._outputPass = new fe("outputPass", this._device, {
      inputs: ["color", "raw"],
      outputs: ["color"],
      uniforms: [
        {
          label: "inputScale",
          type: "f32",
          data: new Float32Array([1])
        },
        {
          label: "inputSize",
          type: "vec2i",
          data: new Int32Array(2)
        },
        {
          label: "outputSize",
          type: "vec2i",
          data: new Int32Array(2)
        },
        {
          label: "imageSize",
          type: "vec2i",
          data: new Int32Array(2)
        },
        {
          label: "inputOffset",
          type: "vec2i",
          data: new Int32Array(2)
        },
        {
          label: "outputOffset",
          type: "vec2i",
          data: new Int32Array(2)
        }
      ],
      csDefine: "",
      csMain: ""
    }), this._copyPass = new fe("copyPass", this._device, {
      inputs: ["color"],
      outputs: ["color"],
      autoUpdateOutputBuffer: !1,
      uniforms: [
        {
          label: "size",
          type: "vec2i",
          data: new Int32Array(2)
        }
      ],
      csMain: (
        /*wgsl*/
        `
let outIdx = i32(globalId.x + globalId.y * u32(size.x));
out_color[outIdx] = textureLoad(in_color, globalId.xy, 0);
`
      )
    }), this._inputPassAux.setOutputParams({
      color: { channels: 3 },
      albedo: { channels: 3 },
      normal: { channels: 3 }
    }), this._inputPassColor.setOutputParams({
      color: { channels: 3 }
    }), this._outputPass.setOutputParams({
      color: { channels: 4 }
    });
  }
  _updatePasses(t, e = !1) {
    if (this._isInputTexture != null && this._isInputTexture === t)
      return;
    this._isInputTexture = t;
    const i = this._isHDR, n = un(this._hdrTransfer) + _i;
    function o(s) {
      return t ? `textureLoad(in_${s}, vec2u(inputPosition), 0)` : `in_${s}[inIdx]`;
    }
    const a = (
      /* wgsl */
      `
let x = i32(globalId.x);
let y = i32(globalId.y);
let inputPosition = clamp(
  vec2i(x, y) + inputOffset,
  vec2i(0),
  inputSize - vec2i(1)
);
let inIdx = inputPosition.y * inputSize.x + inputPosition.x;
let col = ${o("color")};

let outIdx = y * outputSize.x + x;

if (${e}) {
  // Denoise the inversed alpha. Or the anti aliased edge will be too dark after denoised
  out_color[outIdx] = sanitize(vec3f(1.0 - col.a), 0.0, 1.0);
}
else if (${i}) {
  let scaled = sanitize(col.rgb * inputScale, 0.0, FLT_MAX);
  out_color[outIdx] = vec3f(HDRForward(scaled.r), HDRForward(scaled.g), HDRForward(scaled.b));
}
else {
  out_color[outIdx] = sanitize(col.rgb, 0.0, 1.0);
}
`
    );
    this._inputPassAux.setCSCode({
      csDefine: n,
      csMain: (
        /* wgsl */
        `
${a}
let alb = ${o("albedo")};
let nor = ${o("normal")};
// Normals arrive [0, 1]-encoded, so OIDN's [-1, 1] clamp becomes [0, 1].
out_normal[outIdx] = sanitize(nor.rgb, 0.0, 1.0);
out_albedo[outIdx] = sanitize(alb.rgb, 0.0, 1.0);
  `
      )
    }), this._inputPassColor.setCSCode({
      csDefine: n,
      csMain: (
        /* wgsl */
        `
${a}
`
      )
    }), this._outputPass.setCSCode({
      csDefine: sn(this._hdrTransfer) + _i,
      csMain: (
        /* wgsl */
        `
let x = i32(globalId.x);
let y = i32(globalId.y);
if (x >= outputSize.x || y >= outputSize.y) {
  return;
}
let inIdx = (y + inputOffset.y) * inputSize.x + x + inputOffset.x;
let outIdx = (y + outputOffset.y) * imageSize.x + x + outputOffset.x;
let col = in_color[inIdx];
let raw = ${t ? "textureLoad(in_raw, globalId.xy + vec2u(outputOffset), 0)" : "in_raw[outIdx]"};

// The network output may contain negative values or NaNs.
let denoised = sanitize(col.rgb, 0.0, ${i && !e ? "FLT_MAX" : "1.0"});
if (${e}) {
  out_color[outIdx] = vec4f(raw.rgb, 1.0 - denoised.r);
}
else if (${i}) {
  out_color[outIdx] = vec4f(
    vec3f(HDRInverse(denoised.r), HDRInverse(denoised.g), HDRInverse(denoised.b)) / inputScale,
    // Pick the alpha
    raw.a
  );
}
else {
  out_color[outIdx] = vec4f(denoised, raw.a);
}
`
      )
    });
  }
  setImageSize(t, e) {
    this._inputPassAux.setUniform("inputSize", new Int32Array([t, e])), this._inputPassColor.setUniform("inputSize", new Int32Array([t, e])), this._outputPass.setUniform("imageSize", new Int32Array([t, e])), this._outputPass.setSize(t, e), this._copyPass.setSize(t, e), this._copyPass.setUniform("size", new Int32Array([t, e]));
  }
  setInputTile(t) {
    const e = new Int32Array([t.width, t.height]);
    [this._inputPassAux, this._inputPassColor].forEach((i) => {
      i.setUniform("inputOffset", new Int32Array([t.x, t.y])), i.setUniform("outputSize", e), i.setSize(e[0], e[1]);
    }), this._outputPass.setUniform("inputSize", e);
  }
  setOutputTile(t, e) {
    const i = this._outputPass, n = new Int32Array([t.width, t.height]), o = t.x - e.x, a = t.y - e.y;
    i.setUniform("outputSize", n), i.setUniform("inputOffset", new Int32Array([o, a])), i.setUniform(
      "outputOffset",
      new Int32Array([t.x, t.y])
    ), i.setExecuteSize(n[0], n[1]);
  }
  forward(t, e, i, n) {
    const o = t instanceof GPUTexture;
    this._updatePasses(o, n);
    const a = this._inputPassAux, s = this._inputPassColor, u = this._device.createCommandEncoder();
    function l(p) {
      return p instanceof GPUTexture ? {
        texture: p,
        channels: 4
      } : {
        buffer: p,
        channels: 4
      };
    }
    return e && i ? a.createPass(u, {
      color: l(t),
      albedo: l(e),
      normal: l(i)
    }) : s.createPass(u, {
      color: l(t)
    }), this._device.queue.submit([u.finish()]), e && i ? {
      color: a.getOutput("color"),
      albedo: a.getOutput("albedo"),
      normal: a.getOutput("normal")
    } : {
      color: s.getOutput("color")
    };
  }
  inverse(t, e) {
    const n = this._device.createCommandEncoder(), o = this._outputPass;
    return o.createPass(n, {
      color: { buffer: t, channels: 4 },
      raw: e instanceof GPUBuffer ? { buffer: e, channels: 4 } : { texture: e, channels: 4 }
    }), this._device.queue.submit([n.finish()]), o.getOutput("color");
  }
  copyInputDataToOutput(t) {
    const e = this._device.createCommandEncoder(), n = this._outputPass.getOutput("color"), o = this._copyPass;
    t instanceof GPUTexture ? (o.setOutputBuffers({
      color: n
    }), o.createPass(e, {
      color: { texture: t, channels: 4 }
    })) : e.copyBufferToBuffer(
      t,
      0,
      n,
      0,
      n.size
    ), this._device.queue.submit([e.finish()]);
  }
  dispose() {
    this._outputPass.dispose(), this._inputPassAux.dispose(), this._inputPassColor.dispose(), this._copyPass.dispose(!1);
  }
}
const ln = 256, pn = 432, dn = 16, hn = 16, Z = 16;
function Xt(r, t) {
  return Math.ceil(r / t) * t;
}
function ur(r, t) {
  return Math.floor(r / t) * t;
}
function wi(r, t, e) {
  const i = Math.round(e * r / t), n = Math.round((e + 1) * r / t);
  return { start: i, end: n };
}
function ge(r, t, e, i = 0) {
  const n = t - r, o = Math.max(
    i,
    Xt(n, Z)
  );
  let a = r - Math.floor((o - n) / 2);
  return a = xe(a, 0, Math.max(0, e - o)), { start: a, end: a + o };
}
function vi(r, t, e, i) {
  if (!Number.isInteger(r) || r <= 0 || !Number.isInteger(t) || t <= 0)
    throw new Error("Tile grid dimensions must be positive integers");
  if (!Number.isFinite(e) || e <= 0)
    throw new Error("Maximum tile size must be positive");
  if (!Number.isFinite(i) || i < 0)
    throw new Error("Tile overlap must be non-negative");
  const n = Math.max(
    Z,
    ur(e, Z)
  ), o = Xt(i, Z), a = Math.max(1, Math.ceil(r / n)), s = Math.max(1, Math.ceil(t / n)), u = [];
  let l = 0, p = 0;
  for (let g = 0; g < s; g++) {
    const f = wi(t, s, g);
    for (let _ = 0; _ < a; _++) {
      const m = wi(r, a, _), v = {
        start: _ === 0 ? m.start : Math.max(0, m.start - o),
        end: _ === a - 1 ? m.end : Math.min(r, m.end + o)
      }, w = {
        start: g === 0 ? f.start : Math.max(0, f.start - o),
        end: g === s - 1 ? f.end : Math.min(t, f.end + o)
      }, T = ge(
        v.start,
        v.end,
        r
      ), $ = ge(
        w.start,
        w.end,
        t
      ), b = {
        x: m.start,
        y: f.start,
        width: m.end - m.start,
        height: f.end - f.start
      }, x = {
        x: T.start,
        y: $.start,
        width: T.end - T.start,
        height: $.end - $.start
      };
      l = Math.max(l, b.width), p = Math.max(p, b.height), u.push({ column: _, row: g, input: x, output: b });
    }
  }
  const c = () => new Set(
    u.map(({ input: g }) => `${g.width}x${g.height}`)
  );
  if (c().size > 2) {
    const g = Math.max(
      ...u.map(({ input: v }) => v.width)
    ), f = Math.max(
      ...u.map(({ input: v }) => v.height)
    ), _ = u.reduce(
      (v, { input: w }) => v + g * w.height,
      0
    ), m = u.reduce(
      (v, { input: w }) => v + w.width * f,
      0
    );
    if (_ <= m)
      for (const v of u) {
        const w = ge(
          v.input.x,
          v.input.x + v.input.width,
          r,
          g
        );
        v.input.x = w.start, v.input.width = w.end - w.start;
      }
    else
      for (const v of u) {
        const w = ge(
          v.input.y,
          v.input.y + v.input.height,
          t,
          f
        );
        v.input.y = w.start, v.input.height = w.end - w.start;
      }
  }
  const d = c(), h = u.reduce(
    (g, { input: f }) => g + f.width * f.height,
    0
  );
  return {
    columns: a,
    rows: s,
    overlap: o,
    maxOutputWidth: l,
    maxOutputHeight: p,
    inputPixelCount: h,
    inputShapeCount: d.size,
    tiles: u
  };
}
function xe(r, t, e) {
  return Math.min(Math.max(r, t), e);
}
function fn(r, t) {
  const e = [...r].sort((i, n) => i - n);
  return e[Math.ceil(e.length * t) - 1];
}
class gn {
  constructor(t, e = !0) {
    y(this, "enabled");
    y(this, "maxTileSize");
    y(this, "minTileSize");
    y(this, "targetTileTimeMs");
    y(this, "_tileSize");
    y(this, "_adjustmentStep");
    y(this, "_smoothedTileTimeMs");
    y(this, "_completeExecutionsSinceChange", 0);
    const i = typeof e == "object" ? e : {};
    this.enabled = e !== !1, this.maxTileSize = Math.max(
      Z,
      ur(t, Z)
    ), this.minTileSize = xe(
      Xt(i.minTileSize ?? ln, Z),
      Z,
      this.maxTileSize
    ), this.targetTileTimeMs = Math.max(
      1,
      i.targetTileTimeMs ?? dn
    ), this._adjustmentStep = Math.max(
      Z,
      Xt(
        i.adjustmentStep ?? hn,
        Z
      )
    ), this._tileSize = this.enabled ? xe(
      Xt(
        i.initialTileSize ?? pn,
        Z
      ),
      this.minTileSize,
      this.maxTileSize
    ) : this.maxTileSize;
  }
  get tileSize() {
    return this._tileSize;
  }
  /** Returns true when the next execution should use a different tile size. */
  observe(t) {
    if (!this.enabled || t.length === 0) return !1;
    const e = t.filter(
      (a) => Number.isFinite(a) && a >= 0
    );
    if (e.length === 0) return !1;
    const i = e.length >= 3 ? e.slice(1) : e, n = fn(i, 0.75);
    if (this._smoothedTileTimeMs = this._smoothedTileTimeMs === void 0 ? n : this._smoothedTileTimeMs * 0.65 + n * 0.35, this._completeExecutionsSinceChange++, this._completeExecutionsSinceChange < 2) return !1;
    let o = this._tileSize;
    return this._smoothedTileTimeMs > this.targetTileTimeMs * 1.25 ? o -= this._adjustmentStep : this._smoothedTileTimeMs < this.targetTileTimeMs * 0.65 && (o += this._adjustmentStep), o = xe(
      Xt(o, Z),
      this.minTileSize,
      this.maxTileSize
    ), o === this._tileSize ? !1 : (this._tileSize = o, this._smoothedTileTimeMs = void 0, this._completeExecutionsSinceChange = 0, !0);
  }
}
function S(r, t) {
  return {
    op: "conv2d",
    id: r,
    input: t,
    weight: `${r}.weight`,
    bias: `${r}.bias`,
    activation: "relu",
    padding: "same"
  };
}
function Bt(r, t) {
  return {
    op: "maxPool2d",
    id: r,
    input: t,
    size: 2,
    stride: 2,
    padding: "same"
  };
}
function It(r, t) {
  return {
    op: "upsample2d",
    id: r,
    input: t,
    scale: 2,
    mode: "nearest"
  };
}
function Pt(r, t, e) {
  return {
    op: "concat",
    id: r,
    inputs: [t, e],
    axis: "channels"
  };
}
const mn = {
  schemaVersion: 1,
  id: "oidn-unet-small-v1",
  family: "oidn-unet-small",
  input: "input",
  output: "dec_conv0",
  receptiveField: 174,
  nodes: [
    S("enc_conv0", "input"),
    S("enc_conv1", "enc_conv0"),
    Bt("pool1", "enc_conv1"),
    S("enc_conv2", "pool1"),
    Bt("pool2", "enc_conv2"),
    S("enc_conv3", "pool2"),
    Bt("pool3", "enc_conv3"),
    S("enc_conv4", "pool3"),
    Bt("pool4", "enc_conv4"),
    S("enc_conv5a", "pool4"),
    S("enc_conv5b", "enc_conv5a"),
    It("up4", "enc_conv5b"),
    Pt("concat4", "up4", "pool3"),
    S("dec_conv4a", "concat4"),
    S("dec_conv4b", "dec_conv4a"),
    It("up3", "dec_conv4b"),
    Pt("concat3", "up3", "pool2"),
    S("dec_conv3a", "concat3"),
    S("dec_conv3b", "dec_conv3a"),
    It("up2", "dec_conv3b"),
    Pt("concat2", "up2", "pool1"),
    S("dec_conv2a", "concat2"),
    S("dec_conv2b", "dec_conv2a"),
    It("up1", "dec_conv2b"),
    Pt("concat1", "up1", "input"),
    S("dec_conv1a", "concat1"),
    S("dec_conv1b", "dec_conv1a"),
    S("dec_conv0", "dec_conv1b")
  ]
}, $n = {
  schemaVersion: 1,
  id: "oidn-unet-large-v1",
  family: "oidn-unet-large",
  input: "input",
  output: "dec_conv1c",
  receptiveField: 202,
  nodes: [
    S("enc_conv1a", "input"),
    S("enc_conv1b", "enc_conv1a"),
    Bt("pool1", "enc_conv1b"),
    S("enc_conv2a", "pool1"),
    S("enc_conv2b", "enc_conv2a"),
    Bt("pool2", "enc_conv2b"),
    S("enc_conv3a", "pool2"),
    S("enc_conv3b", "enc_conv3a"),
    Bt("pool3", "enc_conv3b"),
    S("enc_conv4a", "pool3"),
    S("enc_conv4b", "enc_conv4a"),
    Bt("pool4", "enc_conv4b"),
    S("enc_conv5a", "pool4"),
    S("enc_conv5b", "enc_conv5a"),
    It("up4", "enc_conv5b"),
    Pt("concat4", "up4", "pool3"),
    S("dec_conv4a", "concat4"),
    S("dec_conv4b", "dec_conv4a"),
    It("up3", "dec_conv4b"),
    Pt("concat3", "up3", "pool2"),
    S("dec_conv3a", "concat3"),
    S("dec_conv3b", "dec_conv3a"),
    It("up2", "dec_conv3b"),
    Pt("concat2", "up2", "pool1"),
    S("dec_conv2a", "concat2"),
    S("dec_conv2b", "dec_conv2a"),
    It("up1", "dec_conv2b"),
    Pt("concat1", "up1", "input"),
    S("dec_conv1a", "concat1"),
    S("dec_conv1b", "dec_conv1a"),
    S("dec_conv1c", "dec_conv1b")
  ]
}, yn = [
  mn,
  $n
];
function sr(r) {
  const t = /* @__PURE__ */ new Set();
  for (const e of r.nodes)
    e.op === "conv2d" && (t.add(e.weight), t.add(e.bias));
  return t;
}
function xi(r) {
  return r.desc.getByteSize();
}
function cr(r) {
  return [...r].sort().join(", ");
}
function lr(r, t = yn) {
  const e = t.filter((i) => {
    const n = sr(i);
    return [...n].some((o) => !r.has(o)) ? !1 : i.allowAdditionalTensors === !0 || [...r.keys()].every((o) => n.has(o));
  });
  if (e.length === 1) return e[0];
  throw e.length > 1 ? new Error(
    `Ambiguous OIDN model topology: ${e.map((i) => i.id).join(", ")}`
  ) : new Error(
    `Unsupported OIDN model topology. TZA tensors: ${cr([...r.keys()])}`
  );
}
function bi(r, t, e) {
  const i = r.get(t);
  if (!i)
    throw new Error(`Model ${e} is missing tensor ${t}`);
  if (i.data.byteLength !== xi(i))
    throw new Error(
      `Tensor ${t} has ${i.data.byteLength} bytes, expected ${xi(i)}`
    );
  return i;
}
function _n(r, t = lr(r)) {
  if (t.schemaVersion !== 1)
    throw new Error(`Unsupported model descriptor schema ${t.schemaVersion}`);
  const e = sr(t);
  if (!t.allowAdditionalTensors) {
    const c = [...r.keys()].filter((d) => !e.has(d));
    if (c.length > 0)
      throw new Error(
        `Model ${t.id} has unexpected tensors: ${cr(c)}`
      );
  }
  const i = /* @__PURE__ */ new Map(), n = /* @__PURE__ */ new Map(), o = /* @__PURE__ */ new Map(), a = /* @__PURE__ */ new Set([t.input]);
  let s, u;
  const l = (c, d) => {
    const h = i.get(c);
    if (h === void 0)
      throw new Error(
        `Model ${t.id} node ${d} reads unknown or forward value ${c}`
      );
    return h;
  };
  for (const c of t.nodes) {
    if (a.has(c.id))
      throw new Error(`Model ${t.id} produces duplicate value ${c.id}`);
    if (c.op === "conv2d") {
      const d = bi(r, c.weight, t.id), h = bi(r, c.bias, t.id), g = d.desc.dims;
      if (d.desc.layout !== "oihw" || g.length !== 4)
        throw new Error(`Tensor ${c.weight} must use OIHW layout`);
      if (g[2] !== 3 || g[3] !== 3)
        throw new Error(`Tensor ${c.weight} must use a 3x3 kernel`);
      if (h.desc.layout !== "x" || h.desc.dims.length !== 1)
        throw new Error(`Tensor ${c.bias} must be a one-dimensional bias`);
      if (h.desc.dims[0] !== g[0])
        throw new Error(
          `Tensor ${c.bias} has ${h.desc.dims[0]} channels, expected ${g[0]}`
        );
      if (d.desc.dataType !== h.desc.dataType)
        throw new Error(`Weight and bias dtype differ for ${c.id}`);
      if (u && u !== d.desc.dataType)
        throw new Error(`Mixed tensor dtypes are not supported by model ${t.id}`);
      u = d.desc.dataType, c.input === t.input && s === void 0 && (s = g[1], i.set(t.input, s));
      const f = l(c.input, c.id);
      if (f !== g[1])
        throw new Error(
          `Tensor ${c.weight} expects ${g[1]} input channels, but ${c.input} provides ${f}`
        );
      i.set(c.id, g[0]), n.set(c.id, {
        weight: d,
        bias: h,
        inputChannels: g[1],
        outputChannels: g[0],
        kernelHeight: g[2],
        kernelWidth: g[3]
      }), o.set(c.id, {
        inputChannels: g[1],
        outputChannels: g[0]
      });
    } else if (c.op === "concat") {
      if (c.inputs.length < 2)
        throw new Error(`Concat ${c.id} requires at least two inputs`);
      const d = c.inputs.reduce(
        (h, g) => h + l(g, c.id),
        0
      );
      i.set(c.id, d);
    } else
      i.set(c.id, l(c.input, c.id));
    a.add(c.id);
  }
  if (s === void 0 || u === void 0)
    throw new Error(`Model ${t.id} has no convolution reading its input`);
  const p = i.get(t.output);
  if (p === void 0)
    throw new Error(`Model ${t.id} output ${t.output} is not produced`);
  if (p !== 3)
    throw new Error(`Model ${t.id} must produce 3 channels, got ${p}`);
  return {
    spec: t,
    inputChannels: s,
    outputChannels: p,
    tensorDataType: u,
    channelsByValue: i,
    convChannels: o,
    convTensors: n
  };
}
function wn(r) {
  return r.op === "concat" ? r.inputs : [r.input];
}
function vn(r) {
  const t = /* @__PURE__ */ new Map();
  for (const e of r.nodes)
    for (const i of wn(e)) {
      const n = t.get(i) ?? [];
      n.push(e), t.set(i, n);
    }
  return t;
}
function Le(r, t, e) {
  const i = r.get(t);
  if (!(i === void 0 || i.length !== 1 || i[0].op !== e))
    return i[0];
}
function ai(r, t = {}) {
  var p;
  const e = r.spec, i = vn(e), n = new Map(e.nodes.map((c) => [c.id, c])), o = /* @__PURE__ */ new Set(), a = /* @__PURE__ */ new Map();
  let s = 0, u = 0;
  if (t.fuseConvPool !== !1)
    for (const c of e.nodes) {
      if (c.op !== "conv2d" || c.activation !== "relu") continue;
      const d = Le(i, c.id, "maxPool2d");
      !d || d.op !== "maxPool2d" || d.size !== 2 || d.stride !== 2 || (o.add(c.id), a.set(d.id, {
        op: "fusedConvReluMaxPool2d",
        id: d.id,
        input: c.input,
        conv: c,
        pool: d
      }), s++);
    }
  if (t.fuseUpsampleConcatConv !== !1)
    for (const c of e.nodes) {
      if (c.op !== "conv2d") continue;
      const d = n.get(c.input);
      if ((d == null ? void 0 : d.op) !== "concat" || d.inputs.length !== 2 || ((p = Le(i, d.id, "conv2d")) == null ? void 0 : p.id) !== c.id) continue;
      const h = r.channelsByValue.get(d.inputs[0]);
      if (h === void 0 || h % 4 !== 0)
        continue;
      const g = d.inputs.map((_) => {
        var v;
        const m = n.get(_);
        return (m == null ? void 0 : m.op) === "upsample2d" && m.scale === 2 && m.mode === "nearest" && ((v = Le(i, m.id, "concat")) == null ? void 0 : v.id) === d.id ? { value: m.input, upsample: m } : { value: _ };
      });
      if (g.filter((_) => _.upsample).length === 1) {
        o.add(d.id);
        for (const _ of d.inputs) {
          const m = n.get(_);
          (m == null ? void 0 : m.op) === "upsample2d" && o.add(m.id);
        }
        a.set(c.id, {
          op: "fusedUpsampleConcatConv2d",
          id: c.id,
          inputs: g,
          conv: c
        }), u++;
      }
    }
  const l = [];
  for (const c of e.nodes) {
    const d = a.get(c.id);
    d ? l.push(d) : o.has(c.id) || l.push(c);
  }
  return {
    spec: e,
    nodes: l,
    fusions: { convPool: s, upsampleConcatConv: u }
  };
}
function xn(r) {
  return r.op === "concat" ? r.inputs : r.op === "fusedUpsampleConcatConv2d" ? r.inputs.map((t) => t.value) : [r.input];
}
function ki(r, t) {
  return r.width === t.width && r.height === t.height;
}
function bn(r, t, e, i) {
  if (!Number.isInteger(t) || t <= 0 || !Number.isInteger(e) || e <= 0)
    throw new Error(`Invalid model input size ${t}x${e}`);
  const n = ai(r, i), o = { width: t, height: e, channels: r.inputChannels }, a = /* @__PURE__ */ new Map([
    [r.spec.input, o]
  ]), s = [], u = (c, d) => {
    const h = a.get(c);
    if (!h) throw new Error(`Planned node ${d} reads missing value ${c}`);
    return h;
  };
  for (const c of n.nodes) {
    let d;
    if (c.op === "conv2d") {
      const h = u(c.input, c.id);
      d = {
        width: h.width,
        height: h.height,
        channels: r.convChannels.get(c.id).outputChannels
      };
    } else if (c.op === "maxPool2d") {
      const h = u(c.input, c.id);
      d = {
        width: Math.ceil(h.width / 2),
        height: Math.ceil(h.height / 2),
        channels: h.channels
      };
    } else if (c.op === "upsample2d") {
      const h = u(c.input, c.id);
      d = {
        width: h.width * 2,
        height: h.height * 2,
        channels: h.channels
      };
    } else if (c.op === "concat") {
      const h = c.inputs.map((g) => u(g, c.id));
      if (h.some((g) => !ki(g, h[0])))
        throw new Error(`Concat ${c.id} has mismatched spatial shapes`);
      d = {
        width: h[0].width,
        height: h[0].height,
        channels: h.reduce((g, f) => g + f.channels, 0)
      };
    } else if (c.op === "fusedConvReluMaxPool2d") {
      const h = u(c.input, c.id);
      d = {
        width: Math.ceil(h.width / 2),
        height: Math.ceil(h.height / 2),
        channels: r.convChannels.get(c.conv.id).outputChannels
      };
    } else {
      const h = c.inputs.map((g) => {
        const f = u(g.value, c.id);
        return g.upsample ? { ...f, width: f.width * 2, height: f.height * 2 } : f;
      });
      if (h.some((g) => !ki(g, h[0])))
        throw new Error(`Fused decoder ${c.id} has mismatched spatial shapes`);
      d = {
        width: h[0].width,
        height: h[0].height,
        channels: r.convChannels.get(c.conv.id).outputChannels
      };
    }
    a.set(c.id, d), s.push(d);
  }
  const l = /* @__PURE__ */ new Map();
  n.nodes.forEach((c, d) => {
    for (const h of xn(c)) l.set(h, d);
  }), l.set(r.spec.output, n.nodes.length);
  const p = n.nodes.map((c, d) => ({
    node: c,
    outputShape: s[d],
    lastUse: l.get(c.id) ?? d
  }));
  return { ...n, inputShape: o, valueShapes: a, plannedNodes: p };
}
const ot = {
  MAP_READ: 1,
  MAP_WRITE: 2,
  COPY_SRC: 4,
  COPY_DST: 8,
  INDEX: 16,
  VERTEX: 32,
  UNIFORM: 64,
  STORAGE: 128,
  INDIRECT: 256,
  QUERY_RESOLVE: 512
}, Ti = {
  READ: 1,
  WRITE: 2
}, yt = 8, Ot = yt + 2, pr = 3 * 3;
function Bi(r, t, e = !1) {
  const i = r === "fp16" ? 2 : 4, n = Ot * Ot * t, o = e ? pr * t * 4 : 0;
  return (n + o) * 4 * i;
}
function kn(r) {
  return r === "fp16" ? "vec4<f16>" : "vec4<f32>";
}
function Tn(r) {
  return r === "fp16" ? `enable f16;
` : "";
}
function Bn(r, t, e) {
  const i = (n) => `${e}[weightBase + ${n}u]`;
  return r === "fp16" ? (
    /* wgsl */
    `
        let inputValue = vec4<f16>(${t});
        var partial = vec4<f16>(0.0h);
        partial = fma(${i(0)}, vec4<f16>(inputValue.x), partial);
        partial = fma(${i(1)}, vec4<f16>(inputValue.y), partial);
        partial = fma(${i(2)}, vec4<f16>(inputValue.z), partial);
        partial = fma(${i(3)}, vec4<f16>(inputValue.w), partial);
        acc += vec4<f32>(partial);
`
  ) : (
    /* wgsl */
    `
        let inputValue = vec4<f32>(${t});
        acc = fma(vec4<f32>(${i(0)}), vec4<f32>(inputValue.x), acc);
        acc = fma(vec4<f32>(${i(1)}), vec4<f32>(inputValue.y), acc);
        acc = fma(vec4<f32>(${i(2)}), vec4<f32>(inputValue.z), acc);
        acc = fma(vec4<f32>(${i(3)}), vec4<f32>(inputValue.w), acc);
`
  );
}
function In(r, t, e, i = !1) {
  if (!Number.isInteger(e) || e < 1)
    throw new Error(`Final RGB shader requires positive input blocks, got ${e}`);
  const n = kn(r), o = "vec4<f32>", a = Ot * Ot * e, s = pr * e * 4, u = t === "relu" ? "max(acc, vec4<f32>(0.0))" : "acc", l = Bn(
    r,
    "inputTile[patchBase + inputBlock]",
    i ? "weightTile" : "weights"
  );
  return (
    /* wgsl */
    `${Tn(r)}
struct Params {
  inputWidth: u32,
  inputHeight: u32,
  outputWidth: u32,
  outputHeight: u32,
  inputBlocks: u32,
  outputBlocks: u32,
}

@group(0) @binding(0) var<storage, read> inputData: array<${n}>;
@group(0) @binding(1) var<storage, read> weights: array<${n}>;
@group(0) @binding(2) var<storage, read> bias: array<vec4<f32>>;
@group(0) @binding(3) var<storage, read_write> outputData: array<${o}>;
@group(0) @binding(4) var<uniform> params: Params;

var<workgroup> inputTile: array<${n}, ${a}>;
${i ? `var<workgroup> weightTile: array<${n}, ${s}>;` : ""}

@compute @workgroup_size(${yt}, ${yt}, 1)
fn main(
  @builtin(local_invocation_id) localId: vec3<u32>,
  @builtin(global_invocation_id) gid: vec3<u32>,
  @builtin(workgroup_id) workgroupId: vec3<u32>
) {
  let localLinear = localId.y * ${yt}u + localId.x;
  for (
    var loadIndex = localLinear;
    loadIndex < ${a}u;
    loadIndex += ${yt * yt}u
  ) {
    let tilePixel = loadIndex / ${e}u;
    let inputBlock = loadIndex % ${e}u;
    let tileX = tilePixel % ${Ot}u;
    let tileY = tilePixel / ${Ot}u;
    let inputX = i32(workgroupId.x * ${yt}u + tileX) - 1;
    let inputY = i32(workgroupId.y * ${yt}u + tileY) - 1;
    var value = ${n}(0.0);
    if (
      inputX >= 0 && inputX < i32(params.inputWidth) &&
      inputY >= 0 && inputY < i32(params.inputHeight)
    ) {
      let inputIndex =
        (u32(inputY) * params.inputWidth + u32(inputX)) *
        ${e}u + inputBlock;
      value = inputData[inputIndex];
    }
    inputTile[loadIndex] = value;
  }

${i ? `  for (
    var loadIndex = localLinear;
    loadIndex < ${s}u;
    loadIndex += ${yt * yt}u
  ) {
    weightTile[loadIndex] = weights[loadIndex];
  }

` : ""}  // Out-of-range invocations must reach this barrier before returning.
  workgroupBarrier();

  let outputInBounds =
    gid.x < params.outputWidth &&
    gid.y < params.outputHeight &&
    gid.z < params.outputBlocks;
  if (!outputInBounds) {
    return;
  }

  var acc = bias[gid.z];
  for (var ky = 0u; ky < 3u; ky++) {
    let inputY = i32(gid.y) + i32(ky) - 1;
    if (inputY < 0 || inputY >= i32(params.inputHeight)) {
      continue;
    }
    for (var kx = 0u; kx < 3u; kx++) {
      let inputX = i32(gid.x) + i32(kx) - 1;
      if (inputX < 0 || inputX >= i32(params.inputWidth)) {
        continue;
      }
      let patchBase =
        ((localId.y + ky) * ${Ot}u + localId.x + kx) *
        ${e}u;
      for (var inputBlock = 0u; inputBlock < ${e}u; inputBlock++) {
        let weightBase =
          ((ky * 3u + kx) * ${e}u + inputBlock) * 4u;
        ${l}
      }
    }
  }

  let outputIndex =
    (gid.y * params.outputWidth + gid.x) * 1u + gid.z;
  outputData[outputIndex] = ${u};
}
`
  );
}
class dr {
  constructor() {
    y(this, "_stats", /* @__PURE__ */ new Map());
  }
  track(t, e) {
    let i = this._stats.get(t);
    return i || (i = {
      created: 0,
      destroyed: 0,
      live: 0,
      peakLive: 0,
      resources: /* @__PURE__ */ new Set()
    }, this._stats.set(t, i)), i.resources.has(e) || (i.resources.add(e), i.created++, i.live++, i.peakLive = Math.max(i.peakLive, i.live)), e;
  }
  release(t, e, i) {
    if (!e) return !1;
    const n = this._stats.get(t);
    if (!(n != null && n.resources.delete(e))) return !1;
    try {
      i();
    } finally {
      n.destroyed++, n.live--;
    }
    return !0;
  }
  snapshot(t = 0) {
    let e = 0, i = 0, n = 0, o = 0;
    const a = {};
    for (const [s, u] of this._stats) {
      const l = {
        created: u.created,
        destroyed: u.destroyed,
        live: u.live,
        peakLive: u.peakLive
      };
      a[s] = l, e += l.live, i += l.created, n += l.destroyed, o += l.peakLive;
    }
    return { live: e, created: i, destroyed: n, peakLive: o, pending: t, byKind: a };
  }
}
const Ii = /* @__PURE__ */ new WeakMap();
function Pn(r) {
  let t = Ii.get(r);
  return t || (t = /* @__PURE__ */ new Map(), Ii.set(r, t)), t;
}
const J = 8, hr = 8, j = 8, ct = j + 2;
function lt(r) {
  const [t, e] = r.workgroupSize;
  return {
    workgroupX: t,
    workgroupY: e,
    rowsPerThread: r.rowsPerThread,
    tileM: e * r.rowsPerThread,
    tileNBlocks: t,
    // Keep the half partial accumulation grouping independent of tile tuning.
    tileKBlocks: hr
  };
}
function bt(r) {
  const t = lt(r), e = r.sharedLayout === "padded" || r.sharedLayout === "padded-input", i = r.sharedLayout === "padded" || r.sharedLayout === "padded-weights";
  return {
    input: t.tileM * t.tileKBlocks + (e ? t.workgroupY : 0),
    weights: t.tileKBlocks * t.tileNBlocks * (i ? 5 : 4)
  };
}
function qt(r, t) {
  return t.sharedLayout === "padded" || t.sharedLayout === "padded-input" ? `(${r}) + (${r}) / ${t.rowsPerThread * hr}u` : r;
}
function ui(r, t) {
  return t.sharedLayout === "padded" || t.sharedLayout === "padded-weights" ? `(${r}) + (${r}) / 4u` : r;
}
function si(r, t) {
  return Math.ceil(r / t) * t;
}
function A(r) {
  return Math.ceil(r / 4);
}
function Cn(r, t) {
  return r.width * r.height * A(r.channels) * 4 * t;
}
let ee;
function fr(r) {
  const t = r & 32768 ? -1 : 1, e = r >>> 10 & 31, i = r & 1023;
  return e === 0 ? t * i * 2 ** -24 : e === 31 ? i === 0 ? t * (1 / 0) : NaN : t * (1 + i / 1024) * 2 ** (e - 15);
}
const Pi = new DataView(new ArrayBuffer(4));
function gr(r) {
  Pi.setFloat32(0, r);
  const t = Pi.getUint32(0), e = t >>> 16 & 32768, i = t >>> 23 & 255, n = t & 8388607;
  if (i === 255)
    return e | 31744 | (n ? 512 : 0);
  const o = i - 127 + 15;
  if (o >= 31) return e | 31744;
  if (o <= 0) {
    if (o < -10) return e;
    const u = n | 8388608, l = 14 - o, p = u >>> l, c = u & (1 << l) - 1, d = 1 << l - 1, h = c > d || c === d && p & 1 ? p + 1 : p;
    return e | h;
  }
  const a = o << 10 | n >>> 13, s = n & 8191;
  return e | (s > 4096 || s === 4096 && a & 1 ? a + 1 : a);
}
function Sn() {
  if (!ee) {
    ee = new Float32Array(65536);
    for (let r = 0; r < ee.length; r++)
      ee[r] = fr(r);
  }
  return ee;
}
function ke(r) {
  if (r.desc.dataType === "Float32")
    return new Float32Array(
      r.data.buffer,
      r.data.byteOffset,
      r.data.byteLength / 4
    );
  const t = new Uint16Array(
    r.data.buffer,
    r.data.byteOffset,
    r.data.byteLength / 2
  ), e = new Float32Array(t.length);
  if (t.length < 4096)
    for (let i = 0; i < t.length; i++)
      e[i] = fr(t[i]);
  else {
    const i = Sn();
    for (let n = 0; n < t.length; n++)
      e[n] = i[t[n]];
  }
  return e;
}
function ce(r, t, e, i) {
  const n = si(e.byteLength, 4), o = r.createBuffer({
    label: t,
    size: n,
    usage: i,
    mappedAtCreation: !0
  });
  return new Uint8Array(o.getMappedRange()).set(e), o.unmap(), o;
}
function Ci(r, t, e) {
  const i = new Uint32Array(si(e.length, 4));
  return i.set(e), ce(
    r,
    t,
    new Uint8Array(i.buffer, i.byteOffset, i.byteLength),
    ot.UNIFORM
  );
}
function Mn(r, t, e, i, n) {
  const o = A(e.inputChannels), a = A(e.outputChannels), s = a * e.kernelHeight * e.kernelWidth * o * 4 * 4, u = i === "fp16" && e.weight.desc.dataType === "Float16", l = i === "fp16" ? new Uint16Array(s) : void 0, p = l ? void 0 : new Float32Array(s), c = u ? new Uint16Array(
    e.weight.data.buffer,
    e.weight.data.byteOffset,
    e.weight.data.byteLength / 2
  ) : void 0, d = c ? void 0 : ke(e.weight);
  for (let f = 0; f < a; f++)
    for (let _ = 0; _ < e.kernelHeight; _++)
      for (let m = 0; m < e.kernelWidth; m++)
        for (let v = 0; v < o; v++)
          for (let w = 0; w < 4; w++) {
            const T = f * 4 + w;
            for (let $ = 0; $ < 4; $++) {
              const b = v * 4 + $, B = (n === "k-major" ? (((_ * e.kernelWidth + m) * o + v) * a + f) * 16 : (((f * e.kernelHeight + _) * e.kernelWidth + m) * o + v) * 16) + $ * 4 + w;
              if (T < e.outputChannels && b < e.inputChannels) {
                const k = ((T * e.inputChannels + b) * e.kernelHeight + _) * e.kernelWidth + m;
                l ? l[B] = c ? c[k] : gr(d[k]) : p[B] = d[k];
              }
            }
          }
  const h = new Float32Array(a * 4);
  h.set(ke(e.bias));
  const g = ce(
    r,
    `oidn/${t}/weights/${i}/${n}`,
    l ? new Uint8Array(l.buffer) : new Uint8Array(p.buffer),
    ot.STORAGE
  );
  try {
    return {
      weights: g,
      weightLayout: n,
      bias: ce(
        r,
        `oidn/${t}/bias`,
        new Uint8Array(h.buffer),
        ot.STORAGE
      )
    };
  } catch (f) {
    throw g.destroy(), f;
  }
}
function R(r) {
  return r === "fp16" ? "vec4<f16>" : "vec4<f32>";
}
function gt(r) {
  return r === "fp16" ? `enable f16;
` : "";
}
function ut(r, t) {
  return t === "fp16" ? `vec4<f16>(${r})` : r;
}
function pt(r, t) {
  return t === "relu" ? `max(${r}, vec4<f32>(0.0))` : r;
}
function le(r, t, e) {
  return e === "fp32" ? (
    /* wgsl */
    `
let inputValue = vec4<f32>(${r});
let weightBase = ${t};
acc = fma(vec4<f32>(weights[weightBase]), vec4<f32>(inputValue.x), acc);
acc = fma(vec4<f32>(weights[weightBase + 1u]), vec4<f32>(inputValue.y), acc);
acc = fma(vec4<f32>(weights[weightBase + 2u]), vec4<f32>(inputValue.z), acc);
acc = fma(vec4<f32>(weights[weightBase + 3u]), vec4<f32>(inputValue.w), acc);
`
  ) : (
    /* wgsl */
    `
let inputValue = vec4<f16>(${r});
let weightBase = ${t};
var partial = vec4<f16>(0.0h);
partial = fma(weights[weightBase], vec4<f16>(inputValue.x), partial);
partial = fma(weights[weightBase + 1u], vec4<f16>(inputValue.y), partial);
partial = fma(weights[weightBase + 2u], vec4<f16>(inputValue.z), partial);
partial = fma(weights[weightBase + 3u], vec4<f16>(inputValue.w), partial);
acc += vec4<f32>(partial);
`
  );
}
function En(r, t, e) {
  const i = e === "fp16" ? "vec4<f16>" : "vec4<f32>", n = e === "fp16" ? `var partial = vec4<f16>(0.0h);
partial = fma(subgroupBroadcast(weights[weightBase], 0u), ${i}(inputValue.x), partial);
partial = fma(subgroupBroadcast(weights[weightBase + 1u], 0u), ${i}(inputValue.y), partial);
partial = fma(subgroupBroadcast(weights[weightBase + 2u], 0u), ${i}(inputValue.z), partial);
partial = fma(subgroupBroadcast(weights[weightBase + 3u], 0u), ${i}(inputValue.w), partial);
acc += vec4<f32>(partial);` : `acc = fma(subgroupBroadcast(weights[weightBase], 0u), vec4<f32>(inputValue.x), acc);
acc = fma(subgroupBroadcast(weights[weightBase + 1u], 0u), vec4<f32>(inputValue.y), acc);
acc = fma(subgroupBroadcast(weights[weightBase + 2u], 0u), vec4<f32>(inputValue.z), acc);
acc = fma(subgroupBroadcast(weights[weightBase + 3u], 0u), vec4<f32>(inputValue.w), acc);`;
  return (
    /* wgsl */
    `
let inputValue = ${i}(${r});
let weightBase = ${t};
${n}
`
  );
}
function Ft(r, t, e) {
  return r === "fp16" && (t.loadMode === "packed-all" || e && t.loadMode === "packed-weights");
}
function Nt(r, t) {
  return t ? `bitcast<vec4<f16>>(${r})` : r;
}
function ci(r, t) {
  const { rowsPerThread: e, tileNBlocks: i, tileKBlocks: n } = lt(t);
  if (!t.unrollRows || t.accumulationOrder === "row-major")
    return An(r, t);
  const o = `(tileK * ${i}u + localId.x) * ${t.sharedLayout === "padded" || t.sharedLayout === "padded-weights" ? 5 : 4}u`, a = Array.from({ length: e }, (u, l) => l), s = R(r);
  return (
    /* wgsl */
    `
    ${r === "fp16" ? a.map((u) => `var partial${u} = vec4<f16>(0.0h);`).join(`
    `) : ""}
    for (var tileK = 0u; tileK < ${n}u; tileK++) {
      let weightBase = ${o};
      let w0 = weightTile[weightBase];
      let w1 = weightTile[weightBase + 1u];
      let w2 = weightTile[weightBase + 2u];
      let w3 = weightTile[weightBase + 3u];
      ${a.map((u) => {
      const l = qt(`(localId.y * ${e}u + ${u}u) * ${n}u + tileK`, t), p = r === "fp16" ? `partial${u}` : `acc[${u}]`;
      return `let v${u} = inputTile[${l}];
      ` + ["x", "y", "z", "w"].map((c, d) => `${p} = fma(w${d}, ${s}(v${u}.${c}), ${p});`).join(`
      `);
    }).join(`
      `)}
    }
    ${r === "fp16" ? a.map((u) => `acc[${u}] += vec4<f32>(partial${u});`).join(`
    `) : ""}
`
  );
}
function An(r, t) {
  const { rowsPerThread: e, tileNBlocks: i, tileKBlocks: n } = lt(t), o = qt(`tileSpatial * ${n}u + tileK`, t), a = `(tileK * ${i}u + localId.x) * ${t.sharedLayout === "padded" || t.sharedLayout === "padded-weights" ? 5 : 4}u`;
  if (t.accumulationOrder === "row-major") {
    const s = R(r), u = r === "fp16" ? "partial" : "acc[row]";
    return (
      /* wgsl */
      `
    for (var row = 0u; row < ${e}u; row++) {
      ${r === "fp16" ? "var partial = vec4<f16>(0.0h);" : ""}
      let tileSpatial = localId.y * ${e}u + row;
      for (var tileK = 0u; tileK < ${n}u; tileK++) {
        let weightBase = ${a};
        let inputValue = inputTile[${o}];
        ${u} = fma(weightTile[weightBase], ${s}(inputValue.x), ${u});
        ${u} = fma(weightTile[weightBase + 1u], ${s}(inputValue.y), ${u});
        ${u} = fma(weightTile[weightBase + 2u], ${s}(inputValue.z), ${u});
        ${u} = fma(weightTile[weightBase + 3u], ${s}(inputValue.w), ${u});
      }
      ${r === "fp16" ? "acc[row] += vec4<f32>(partial);" : ""}
    }
`
    );
  }
  return r === "fp16" ? (
    /* wgsl */
    `
    var partial: array<vec4<f16>, ${e}>;
    for (var row = 0u; row < ${e}u; row++) {
      partial[row] = vec4<f16>(0.0h);
    }
    for (var tileK = 0u; tileK < ${n}u; tileK++) {
      let weightBase =
        ${a};
      for (var row = 0u; row < ${e}u; row++) {
        let tileSpatial =
          localId.y * ${e}u + row;
        let inputValue =
          inputTile[${o}];
        partial[row] = fma(
          weightTile[weightBase],
          vec4<f16>(inputValue.x),
          partial[row]
        );
        partial[row] = fma(
          weightTile[weightBase + 1u],
          vec4<f16>(inputValue.y),
          partial[row]
        );
        partial[row] = fma(
          weightTile[weightBase + 2u],
          vec4<f16>(inputValue.z),
          partial[row]
        );
        partial[row] = fma(
          weightTile[weightBase + 3u],
          vec4<f16>(inputValue.w),
          partial[row]
        );
      }
    }
    for (var row = 0u; row < ${e}u; row++) {
      acc[row] += vec4<f32>(partial[row]);
    }
`
  ) : (
    /* wgsl */
    `
    for (var tileK = 0u; tileK < ${n}u; tileK++) {
      let weightBase =
        ${a};
      for (var row = 0u; row < ${e}u; row++) {
        let tileSpatial =
          localId.y * ${e}u + row;
        let inputValue =
          inputTile[${o}];
        acc[row] = fma(
          weightTile[weightBase],
          vec4<f32>(inputValue.x),
          acc[row]
        );
        acc[row] = fma(
          weightTile[weightBase + 1u],
          vec4<f32>(inputValue.y),
          acc[row]
        );
        acc[row] = fma(
          weightTile[weightBase + 2u],
          vec4<f32>(inputValue.z),
          acc[row]
        );
        acc[row] = fma(
          weightTile[weightBase + 3u],
          vec4<f32>(inputValue.w),
          acc[row]
        );
      }
    }
`
  );
}
function On(r, t, e, i, n) {
  const o = R(r), a = R(r), s = R(t), u = ut(
    pt("acc", e),
    t
  );
  return (
    /* wgsl */
    `${gt(r)}
struct Params {
  inputWidth: u32,
  inputHeight: u32,
  outputWidth: u32,
  outputHeight: u32,
  inputBlocks: u32,
  outputBlocks: u32,
}

@group(0) @binding(0) var<storage, read> inputData: array<${o}>;
@group(0) @binding(1) var<storage, read> weights: array<${a}>;
@group(0) @binding(2) var<storage, read> bias: array<vec4<f32>>;
@group(0) @binding(3) var<storage, read_write> outputData: array<${s}>;
@group(0) @binding(4) var<uniform> params: Params;

@compute @workgroup_size(${J}, ${J}, 1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
  if (gid.x >= params.outputWidth || gid.y >= params.outputHeight || gid.z >= ${n}u) {
    return;
  }
  var acc = bias[gid.z];
  for (var ky = 0u; ky < 3u; ky++) {
    let inputY = i32(gid.y) + i32(ky) - 1;
    if (inputY < 0 || inputY >= i32(params.inputHeight)) { continue; }
    for (var kx = 0u; kx < 3u; kx++) {
      let inputX = i32(gid.x) + i32(kx) - 1;
      if (inputX < 0 || inputX >= i32(params.inputWidth)) { continue; }
      let pixelBase = (u32(inputY) * params.inputWidth + u32(inputX)) * ${i}u;
      for (var inputBlock = 0u; inputBlock < ${i}u; inputBlock++) {
        ${le(
      "inputData[pixelBase + inputBlock]",
      `((((gid.z * 3u + ky) * 3u + kx) * ${i}u + inputBlock) * 4u)`,
      r
    )}
      }
    }
  }
  let outputIndex = (gid.y * params.outputWidth + gid.x) * ${n}u + gid.z;
  outputData[outputIndex] = ${u};
}
`
  );
}
function Nn(r, t, e, i, n) {
  const o = R(r), a = R(r), s = R(t), u = ut(
    pt("acc", e),
    t
  );
  return (
    /* wgsl */
    `${gt(r)}
enable subgroups;
struct Params {
  inputWidth: u32,
  inputHeight: u32,
  outputWidth: u32,
  outputHeight: u32,
  inputBlocks: u32,
  outputBlocks: u32,
}
@group(0) @binding(0) var<storage, read> inputData: array<${o}>;
@group(0) @binding(1) var<storage, read> weights: array<${a}>;
@group(0) @binding(2) var<storage, read> bias: array<vec4<f32>>;
@group(0) @binding(3) var<storage, read_write> outputData: array<${s}>;
@group(0) @binding(4) var<uniform> params: Params;

@compute @workgroup_size(${J}, ${J}, 1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
  let outputInBounds =
    gid.x < params.outputWidth && gid.y < params.outputHeight;
  var acc = bias[gid.z];
  for (var ky = 0u; ky < 3u; ky++) {
    let inputY = i32(gid.y) + i32(ky) - 1;
    let clampedY = u32(clamp(inputY, 0, i32(params.inputHeight) - 1));
    for (var kx = 0u; kx < 3u; kx++) {
      let inputX = i32(gid.x) + i32(kx) - 1;
      let clampedX = u32(clamp(inputX, 0, i32(params.inputWidth) - 1));
      let inputInBounds =
        outputInBounds && inputX >= 0 && inputY >= 0 &&
        inputX < i32(params.inputWidth) && inputY < i32(params.inputHeight);
      let pixelBase =
        (clampedY * params.inputWidth + clampedX) * ${i}u;
      for (var inputBlock = 0u; inputBlock < ${i}u; inputBlock++) {
        ${En(
      `select(${o}(0.0), inputData[pixelBase + inputBlock], inputInBounds)`,
      `((((gid.z * 3u + ky) * 3u + kx) * ${i}u + inputBlock) * 4u)`,
      r
    )}
      }
    }
  }
  if (outputInBounds) {
    let outputIndex =
      (gid.y * params.outputWidth + gid.x) * ${n}u + gid.z;
    outputData[outputIndex] = ${u};
  }
}
`
  );
}
function zn(r, t, e, i, n) {
  const o = R(r), a = R(t), s = ut(
    pt("acc", e),
    t
  ), u = ct * ct * i, l = j * j;
  return (
    /* wgsl */
    `${gt(r)}
struct Params {
  inputWidth: u32,
  inputHeight: u32,
  outputWidth: u32,
  outputHeight: u32,
  inputBlocks: u32,
  outputBlocks: u32,
}
@group(0) @binding(0) var<storage, read> inputData: array<${o}>;
@group(0) @binding(1) var<storage, read> weights: array<${o}>;
@group(0) @binding(2) var<storage, read> bias: array<vec4<f32>>;
@group(0) @binding(3) var<storage, read_write> outputData: array<${a}>;
@group(0) @binding(4) var<uniform> params: Params;

var<workgroup> inputPatch: array<${o}, ${u}>;

@compute @workgroup_size(${j}, ${j}, 1)
fn main(
  @builtin(local_invocation_id) localId: vec3<u32>,
  @builtin(workgroup_id) workgroupId: vec3<u32>
) {
  let localLinear =
    localId.y * ${j}u + localId.x;
  for (
    var loadIndex = localLinear;
    loadIndex < ${u}u;
    loadIndex += ${l}u
  ) {
    let patchPixel = loadIndex / ${i}u;
    let inputBlock = loadIndex % ${i}u;
    let patchX = patchPixel % ${ct}u;
    let patchY = patchPixel / ${ct}u;
    let inputX =
      i32(workgroupId.x * ${j}u + patchX) - 1;
    let inputY =
      i32(workgroupId.y * ${j}u + patchY) - 1;
    var value = ${o}(0.0);
    if (
      inputX >= 0 && inputX < i32(params.inputWidth) &&
      inputY >= 0 && inputY < i32(params.inputHeight)
    ) {
      let inputIndex =
        (u32(inputY) * params.inputWidth + u32(inputX)) *
        ${i}u + inputBlock;
      value = inputData[inputIndex];
    }
    inputPatch[loadIndex] = value;
  }
  workgroupBarrier();

  let outputX =
    workgroupId.x * ${j}u + localId.x;
  let outputY =
    workgroupId.y * ${j}u + localId.y;
  let outputBlock = workgroupId.z;
  if (
    outputX >= params.outputWidth || outputY >= params.outputHeight ||
    outputBlock >= ${n}u
  ) {
    return;
  }

  var acc = bias[outputBlock];
  for (var ky = 0u; ky < 3u; ky++) {
    for (var kx = 0u; kx < 3u; kx++) {
      let patchBase =
        ((localId.y + ky) * ${ct}u + localId.x + kx) *
        ${i}u;
      for (var inputBlock = 0u; inputBlock < ${i}u; inputBlock++) {
        ${le(
      "inputPatch[patchBase + inputBlock]",
      `((((outputBlock * 3u + ky) * 3u + kx) * ${i}u + inputBlock) * 4u)`,
      r
    )}
      }
    }
  }
  let outputIndex =
    (outputY * params.outputWidth + outputX) * ${n}u + outputBlock;
  outputData[outputIndex] = ${s};
}
`
  );
}
function mr(r, t, e, i) {
  const { workgroupX: n, workgroupY: o, tileM: a, tileKBlocks: s } = lt(e), u = t ? "params.outputWidth" : "params.inputWidth", l = t ? "params.outputHeight" : "params.inputHeight", p = e.addressMode === "base-offset", c = i ? i.blocks.map(
    (h, g) => `cachedBase${g}[load] = (${g === i.upsampled ? "y / 2u" : "y"} * params.source${g}Width + ${g === i.upsampled ? "x / 2u" : "x"}) * ${h}u;`
  ).join(`
    `) : `cachedBase0[load] = (y * params.inputWidth + x) * ${r}u;`, d = a * s / (n * o);
  return (
    /* wgsl */
    `
  ${p ? `var cachedBase0: array<u32, ${d}>;
  ${i ? `var cachedBase1: array<u32, ${d}>;` : ""}` : `var cachedX: array<u32, ${d}>;
  var cachedY: array<u32, ${d}>;`}
  var cachedMask: array<u32, ${d}>;
  for (var load = 0u; load < ${d}u; load++) {
    let loadIndex = localLinear + load * ${n * o}u;
    let spatial = workgroupId.x * ${a}u + loadIndex / ${s}u;
    let x = spatial % params.outputWidth;
    let y = spatial / params.outputWidth;
    ${p ? c : `cachedX[load] = x;
    cachedY[load] = y;`}
    let columns =
      select(0u, 0x049u, x > 0u && x - 1u < ${u}) |
      select(0u, 0x092u, x < ${u}) |
      select(0u, 0x124u, x + 1u < ${u});
    let rows =
      select(0u, 0x007u, y > 0u && y - 1u < ${l}) |
      select(0u, 0x038u, y < ${l}) |
      select(0u, 0x1c0u, y + 1u < ${l});
    cachedMask[load] = select(0u, columns & rows, spatial < spatialCount)${p && i ? " | ((x & 1u) << 9u) | ((y & 1u) << 10u)" : ""};
  }
  var channelBlock = (localLinear % ${s}u) % ${r}u;
  var filterPosition = (localLinear % ${s}u) / ${r}u;
`
  );
}
function $r(r, t) {
  const { tileKBlocks: e } = lt(t);
  return (
    /* wgsl */
    `
    channelBlock += ${e % r}u;
    let carry = channelBlock >= ${r}u;
    channelBlock -= select(0u, ${r}u, carry);
    filterPosition += ${Math.floor(e / r)}u + select(0u, 1u, carry);
`
  );
}
function be(r, t, e) {
  const { workgroupX: i, workgroupY: n, tileM: o, tileKBlocks: a } = lt(e);
  return (
    /* wgsl */
    `
    let inputBlock = channelBlock;
    let kernelY = filterPosition / 3u;
    let kernelX = filterPosition % 3u;
    for (var load = 0u; load < ${o * a / (i * n)}u; load++) {
      let loadIndex = localLinear + load * ${i * n}u;
      var value = ${r}(0.0);
      if (filterPosition < 9u && (cachedMask[load] & (1u << filterPosition)) != 0u) {
        ${e.addressMode === "base-offset" ? "" : `let inputX = i32(cachedX[load]) + i32(kernelX) - 1;
        let inputY = i32(cachedY[load]) + i32(kernelY) - 1;`}
        ${t}
      }
      inputTile[${qt("loadIndex", e)}] = value;
    }
`
  );
}
function Wn(r, t, e, i, n, o) {
  const { workgroupX: a, workgroupY: s, rowsPerThread: u, tileM: l, tileNBlocks: p, tileKBlocks: c } = lt(o), { addressMode: d, weightLayout: h } = o, g = R(r), f = R(t), _ = Ft(r, o, !1), m = Ft(r, o, !0), v = _ ? "vec2<u32>" : g, w = m ? "vec2<u32>" : g, T = ut(
    pt("acc", e),
    t
  ), $ = a * s, b = l * c, x = c * p * 4;
  return (
    /* wgsl */
    `${gt(r)}
struct Params {
  inputWidth: u32,
  inputHeight: u32,
  outputWidth: u32,
  outputHeight: u32,
  inputBlocks: u32,
  outputBlocks: u32,
}
@group(0) @binding(0) var<storage, read> inputData: array<${v}>;
@group(0) @binding(1) var<storage, read> weights: array<${w}>;
@group(0) @binding(2) var<storage, read> bias: array<vec4<f32>>;
@group(0) @binding(3) var<storage, read_write> outputData: array<${f}>;
@group(0) @binding(4) var<uniform> params: Params;

var<workgroup> inputTile: array<${g}, ${bt(o).input}>;
var<workgroup> weightTile: array<${g}, ${bt(o).weights}>;

@compute @workgroup_size(${a}, ${s}, 1)
fn main(
  @builtin(local_invocation_id) localId: vec3<u32>,
  @builtin(workgroup_id) workgroupId: vec3<u32>
) {
  let spatialBase =
    workgroupId.x * ${l}u +
    localId.y * ${u}u;
  let outputBlock =
    workgroupId.y * ${p}u + localId.x;
  let spatialCount = params.outputWidth * params.outputHeight;
  var acc: array<vec4<f32>, ${u}>;
  if (outputBlock < ${n}u) {
    for (var row = 0u; row < ${u}u; row++) {
      acc[row] = bias[outputBlock];
    }
  }

  let localLinear =
    localId.y * ${a}u + localId.x;
  ${d !== "analytic" ? mr(i, !1, o) : ""}
  let totalK = ${i * 9}u;
  for (var kBase = 0u; kBase < totalK; kBase += ${c}u) {
    ${d !== "analytic" ? be(g, `
        ${d === "base-offset" ? `let offset = (i32(kernelY) - 1) * i32(params.inputWidth) + i32(kernelX) - 1;
        let inputIndex = cachedBase0[load] + u32(offset * ${i}) + inputBlock;` : `let inputIndex = (u32(inputY) * params.inputWidth + u32(inputX)) *
          ${i}u + inputBlock;`}
        value = ${Nt("inputData[inputIndex]", _)};
    `, o) : (
      /* wgsl */
      `
    for (
      var loadIndex = localLinear;
      loadIndex < ${b}u;
      loadIndex += ${$}u
    ) {
      let tileSpatial = loadIndex / ${c}u;
      let tileK = loadIndex % ${c}u;
      let inputSpatialIndex =
        workgroupId.x * ${l}u + tileSpatial;
      let kIndex = kBase + tileK;
      var value = ${g}(0.0);
      if (inputSpatialIndex < spatialCount && kIndex < totalK) {
        let outputY = inputSpatialIndex / params.outputWidth;
        let outputX = inputSpatialIndex % params.outputWidth;
        let inputBlock = kIndex % ${i}u;
        let kernelIndex = kIndex / ${i}u;
        let kernelY = kernelIndex / 3u;
        let kernelX = kernelIndex % 3u;
        let inputY = i32(outputY) + i32(kernelY) - 1;
        let inputX = i32(outputX) + i32(kernelX) - 1;
        if (
          inputY >= 0 && inputY < i32(params.inputHeight) &&
          inputX >= 0 && inputX < i32(params.inputWidth)
        ) {
          let inputIndex =
            (u32(inputY) * params.inputWidth + u32(inputX)) *
            ${i}u + inputBlock;
          value = ${Nt("inputData[inputIndex]", _)};
        }
      }
      inputTile[${qt("loadIndex", o)}] = value;
    }
    `
    )}

    for (
      var loadIndex = localLinear;
      loadIndex < ${x}u;
      loadIndex += ${$}u
    ) {
      let tileK = loadIndex / ${p * 4}u;
      let outputRemainder = loadIndex % ${p * 4}u;
      let tileOutputBlock = outputRemainder / 4u;
      let outputLane = outputRemainder % 4u;
      let loadedOutputBlock =
        workgroupId.y * ${p}u + tileOutputBlock;
      let kIndex = kBase + tileK;
      var value = ${g}(0.0);
      if (loadedOutputBlock < ${n}u && kIndex < totalK) {
        ${h === "k-major" ? (
      /* wgsl */
      `
        let weightIndex = (kIndex * ${n}u + loadedOutputBlock) * 4u + outputLane;
        `
    ) : d !== "analytic" ? (
      /* wgsl */
      `
        let weightIndex = (loadedOutputBlock * ${i * 9}u + kIndex) * 4u + outputLane;
        `
    ) : (
      /* wgsl */
      `
        let inputBlock = kIndex % ${i}u;
        let kernelIndex = kIndex / ${i}u;
        let kernelY = kernelIndex / 3u;
        let kernelX = kernelIndex % 3u;
        let weightIndex =
          ((((loadedOutputBlock * 3u + kernelY) * 3u + kernelX) *
            ${i}u + inputBlock) * 4u + outputLane);
        `
    )}
        value = ${Nt("weights[weightIndex]", m)};
      }
      weightTile[${ui("loadIndex", o)}] = value;
    }

    workgroupBarrier();
    ${ci(r, o)}
    workgroupBarrier();
    ${d !== "analytic" ? $r(i, o) : ""}
  }

  if (outputBlock < ${n}u) {
    for (var row = 0u; row < ${u}u; row++) {
      let spatialIndex = spatialBase + row;
      if (spatialIndex < spatialCount) {
        let outputIndex = spatialIndex * ${n}u + outputBlock;
        outputData[outputIndex] = ${T.replaceAll("acc", "acc[row]")};
      }
    }
  }
}
`
  );
}
function Un(r, t, e, i) {
  const n = R(r), o = pt("acc", t);
  return (
    /* wgsl */
    `${gt(r)}
struct Params {
  inputWidth: u32,
  inputHeight: u32,
  outputWidth: u32,
  outputHeight: u32,
  inputBlocks: u32,
  outputBlocks: u32,
}
@group(0) @binding(0) var<storage, read> inputData: array<${n}>;
@group(0) @binding(1) var<storage, read> weights: array<${n}>;
@group(0) @binding(2) var<storage, read> bias: array<vec4<f32>>;
@group(0) @binding(3) var<storage, read_write> outputData: array<${n}>;
@group(0) @binding(4) var<uniform> params: Params;

@compute @workgroup_size(${J}, ${J}, 1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
  if (gid.x >= params.outputWidth || gid.y >= params.outputHeight || gid.z >= ${i}u) {
    return;
  }
  var pooled = vec4<f32>(-3.402823466e+38);
  for (var py = 0u; py < 2u; py++) {
    let centerY = gid.y * 2u + py;
    if (centerY >= params.inputHeight) { continue; }
    for (var px = 0u; px < 2u; px++) {
      let centerX = gid.x * 2u + px;
      if (centerX >= params.inputWidth) { continue; }
      var acc = bias[gid.z];
      for (var ky = 0u; ky < 3u; ky++) {
        let inputY = i32(centerY) + i32(ky) - 1;
        if (inputY < 0 || inputY >= i32(params.inputHeight)) { continue; }
        for (var kx = 0u; kx < 3u; kx++) {
          let inputX = i32(centerX) + i32(kx) - 1;
          if (inputX < 0 || inputX >= i32(params.inputWidth)) { continue; }
          let pixelBase = (u32(inputY) * params.inputWidth + u32(inputX)) * ${e}u;
          for (var inputBlock = 0u; inputBlock < ${e}u; inputBlock++) {
            ${le(
      "inputData[pixelBase + inputBlock]",
      `((((gid.z * 3u + ky) * 3u + kx) * ${e}u + inputBlock) * 4u)`,
      r
    )}
          }
        }
      }
      pooled = max(pooled, ${o});
    }
  }
  let outputIndex = (gid.y * params.outputWidth + gid.x) * ${i}u + gid.z;
  outputData[outputIndex] = ${ut("pooled", r)};
}
`
  );
}
function Ln(r, t, e) {
  const i = R(r);
  return (
    /* wgsl */
    `${gt(r)}
struct Params {
  inputWidth: u32,
  inputHeight: u32,
  outputWidth: u32,
  outputHeight: u32,
  outputBlocks: u32,
}
@group(0) @binding(0) var<storage, read> inputData: array<${i}>;
@group(0) @binding(1) var<storage, read_write> outputData: array<${i}>;
@group(0) @binding(2) var<uniform> params: Params;

@compute @workgroup_size(${J}, ${J}, 1)
fn main(
  @builtin(global_invocation_id) gid: vec3<u32>${e ? `,
  @builtin(num_workgroups) groupCount: vec3<u32>` : ""}
) {
  ${e ? `let linearX = gid.x + gid.z * groupCount.x * ${J}u;
  let outputBlock = linearX % ${t}u;
  let outputX = linearX / ${t}u;` : `let outputBlock = gid.z;
  let outputX = gid.x;`}
  if (
    outputX >= params.outputWidth ||
    gid.y >= params.outputHeight ||
    outputBlock >= ${t}u
  ) {
    return;
  }
  var pooled = vec4<f32>(-3.402823466e+38);
  for (var py = 0u; py < 2u; py++) {
    let inputY = gid.y * 2u + py;
    if (inputY >= params.inputHeight) { continue; }
    for (var px = 0u; px < 2u; px++) {
      let inputX = outputX * 2u + px;
      if (inputX >= params.inputWidth) { continue; }
      let inputIndex =
        (inputY * params.inputWidth + inputX) * ${t}u + outputBlock;
      pooled = max(pooled, vec4<f32>(inputData[inputIndex]));
    }
  }
  let outputIndex =
    (gid.y * params.outputWidth + outputX) * ${t}u + outputBlock;
  outputData[outputIndex] = ${ut("pooled", r)};
}
`
  );
}
function Dn(r, t, e, i, n, o, a) {
  const { workgroupX: s, workgroupY: u, rowsPerThread: l, tileM: p, tileNBlocks: c, tileKBlocks: d } = lt(a), { addressMode: h, weightLayout: g } = a, f = R(r), _ = R(t), m = Ft(r, a, !1), v = Ft(r, a, !0), w = m ? "vec2<u32>" : f, T = v ? "vec2<u32>" : f, $ = i[0] + i[1], b = ut(
    pt("acc[row]", e),
    t
  ), x = s * u, B = p * d, k = d * c * 4, I = (P, O) => (
    /* wgsl */
    `
          {
            let sourceBlock = ${O};
            ${h === "base-offset" ? `let dx = ${P === n ? "(i32((cachedMask[load] >> 9u) & 1u) + i32(kernelX) - 1) >> 1u" : "i32(kernelX) - 1"};
            let dy = ${P === n ? "(i32((cachedMask[load] >> 10u) & 1u) + i32(kernelY) - 1) >> 1u" : "i32(kernelY) - 1"};
            let offset = (dy * i32(params.source${P}Width) + dx) * ${i[P]};
            let sourceIndex = cachedBase${P}[load] + u32(offset) + sourceBlock;` : `let sourceX = ${P === n ? "u32(inputX) / 2u" : "u32(inputX)"};
            let sourceY = ${P === n ? "u32(inputY) / 2u" : "u32(inputY)"};
            let sourceIndex =
              (sourceY * params.source${P}Width + sourceX) *
              ${i[P]}u + sourceBlock;`}
            value = ${Nt(`input${P}[sourceIndex]`, m)};
          }`
  );
  return (
    /* wgsl */
    `${gt(r)}
struct Params {
  outputWidth: u32,
  outputHeight: u32,
  outputBlocks: u32,
  inputBlocks: u32,
  source0Width: u32,
  source0Height: u32,
  source1Width: u32,
  source1Height: u32,
}
@group(0) @binding(0) var<storage, read> input0: array<${w}>;
@group(0) @binding(1) var<storage, read> input1: array<${w}>;
@group(0) @binding(2) var<storage, read> weights: array<${T}>;
@group(0) @binding(3) var<storage, read> bias: array<vec4<f32>>;
@group(0) @binding(4) var<storage, read_write> outputData: array<${_}>;
@group(0) @binding(5) var<uniform> params: Params;

var<workgroup> inputTile: array<${f}, ${bt(a).input}>;
var<workgroup> weightTile: array<${f}, ${bt(a).weights}>;

@compute @workgroup_size(${s}, ${u}, 1)
fn main(
  @builtin(local_invocation_id) localId: vec3<u32>,
  @builtin(workgroup_id) workgroupId: vec3<u32>
) {
  let spatialBase =
    workgroupId.x * ${p}u +
    localId.y * ${l}u;
  let outputBlock =
    workgroupId.y * ${c}u + localId.x;
  let spatialCount = params.outputWidth * params.outputHeight;
  var acc: array<vec4<f32>, ${l}>;
  if (outputBlock < ${o}u) {
    for (var row = 0u; row < ${l}u; row++) {
      acc[row] = bias[outputBlock];
    }
  }

  let localLinear =
    localId.y * ${s}u + localId.x;
  ${h !== "analytic" ? mr($, !0, a, { blocks: i, upsampled: n }) : ""}
  let totalK = ${$ * 9}u;
  for (var kBase = 0u; kBase < totalK; kBase += ${d}u) {
    ${h !== "analytic" ? a.decoderLoad === "source-first" ? `
    if (channelBlock < ${i[0]}u) {
      ${be(f, I(0, "inputBlock"), a)}
    } else {
      ${be(f, I(1, `inputBlock - ${i[0]}u`), a)}
    }
    ` : be(f, `
        if (inputBlock < ${i[0]}u) {
          ${I(0, "inputBlock")}
        } else {
          ${I(1, `inputBlock - ${i[0]}u`)}
        }
    `, a) : (
      /* wgsl */
      `
    for (
      var loadIndex = localLinear;
      loadIndex < ${B}u;
      loadIndex += ${x}u
    ) {
      let tileSpatial = loadIndex / ${d}u;
      let tileK = loadIndex % ${d}u;
      let outputSpatialIndex =
        workgroupId.x * ${p}u + tileSpatial;
      let kIndex = kBase + tileK;
      var value = ${f}(0.0);
      if (outputSpatialIndex < spatialCount && kIndex < totalK) {
        let outputY = outputSpatialIndex / params.outputWidth;
        let outputX = outputSpatialIndex % params.outputWidth;
        let inputBlock = kIndex % ${$}u;
        let kernelIndex = kIndex / ${$}u;
        let kernelY = kernelIndex / 3u;
        let kernelX = kernelIndex % 3u;
        let inputY = i32(outputY) + i32(kernelY) - 1;
        let inputX = i32(outputX) + i32(kernelX) - 1;
        if (
          inputY >= 0 && inputY < i32(params.outputHeight) &&
          inputX >= 0 && inputX < i32(params.outputWidth)
        ) {
          if (inputBlock < ${i[0]}u) {
            ${I(0, "inputBlock")}
          } else {
            ${I(1, `inputBlock - ${i[0]}u`)}
          }
        }
      }
      inputTile[${qt("loadIndex", a)}] = value;
    }
    `
    )}

    for (
      var loadIndex = localLinear;
      loadIndex < ${k}u;
      loadIndex += ${x}u
    ) {
      let tileK = loadIndex / ${c * 4}u;
      let outputRemainder = loadIndex % ${c * 4}u;
      let tileOutputBlock = outputRemainder / 4u;
      let outputLane = outputRemainder % 4u;
      let loadedOutputBlock =
        workgroupId.y * ${c}u + tileOutputBlock;
      let kIndex = kBase + tileK;
      var value = ${f}(0.0);
      if (loadedOutputBlock < ${o}u && kIndex < totalK) {
        ${g === "k-major" ? (
      /* wgsl */
      `
        let weightIndex = (kIndex * ${o}u + loadedOutputBlock) * 4u + outputLane;
        `
    ) : h !== "analytic" ? (
      /* wgsl */
      `
        let weightIndex = (loadedOutputBlock * ${$ * 9}u + kIndex) * 4u + outputLane;
        `
    ) : (
      /* wgsl */
      `
        let inputBlock = kIndex % ${$}u;
        let kernelIndex = kIndex / ${$}u;
        let kernelY = kernelIndex / 3u;
        let kernelX = kernelIndex % 3u;
        let weightIndex =
          ((((loadedOutputBlock * 3u + kernelY) * 3u + kernelX) *
            ${$}u + inputBlock) * 4u + outputLane);
        `
    )}
        value = ${Nt("weights[weightIndex]", v)};
      }
      weightTile[${ui("loadIndex", a)}] = value;
    }

    workgroupBarrier();
    ${ci(r, a)}
    workgroupBarrier();
    ${h !== "analytic" ? $r($, a) : ""}
  }

  if (outputBlock < ${o}u) {
    for (var row = 0u; row < ${l}u; row++) {
      let spatialIndex = spatialBase + row;
      if (spatialIndex < spatialCount) {
        let outputIndex = spatialIndex * ${o}u + outputBlock;
        outputData[outputIndex] = ${b};
      }
    }
  }
}
`
  );
}
function Rn(r, t, e, i, n, o, a) {
  const s = lt(a), { workgroupX: u, workgroupY: l, rowsPerThread: p, tileM: c, tileNBlocks: d, tileKBlocks: h } = s, g = u * l, f = c * h / g, _ = R(r), m = Ft(r, a, !1), v = Ft(r, a, !0), w = i[n], T = 1 - n;
  i[T];
  const $ = (x, B) => {
    const k = i[x], I = B ? 4 : 9, P = B ? `phase * ${4 * w * o * 4}u` : `${ae * w * o * 4}u`, O = B ? `(i32(tap / 2u) + i32(phaseY) - 1) * i32(params.source${x}Width) + i32(tap % 2u) + i32(phaseX) - 1` : `(i32(tap / 3u) - 1) * i32(params.source${x}Width) + i32(tap % 3u) - 1`;
    return (
      /* wgsl */
      `
  for (var kBase = 0u; kBase < ${I * k}u; kBase += ${h}u) {
    let inputK = kBase + localLinear % ${h}u;
    let tap = inputK / ${k}u;
    let channel = inputK % ${k}u;
    for (var load = 0u; load < ${f}u; load++) {
      let i = localLinear + load * ${g}u;
      var value = ${_}(0.0);
      if (tap < ${I}u && (mask${x}[load] & (1u << tap)) != 0u) {
        let offset = ${O};
        let index = u32(base${x}[load] + offset) * ${k}u + channel;
        value = ${Nt(`input${x}[index]`, m)};
      }
      inputTile[${qt("i", a)}] = value;
    }
    for (var i = localLinear; i < ${h * d * 4}u; i += ${g}u) {
      let k = kBase + i / ${d * 4}u;
      let outputBlock = workgroupId.y * ${d}u + (i % ${d * 4}u) / 4u;
      var value = ${_}(0.0);
      if (k < ${I * k}u && outputBlock < ${o}u) {
        let index = ${P} + (k * ${o}u + outputBlock) * 4u + i % 4u;
        value = ${Nt("weights[index]", v)};
      }
      weightTile[${ui("i", a)}] = value;
    }
    workgroupBarrier();
    ${ci(r, a)}
    workgroupBarrier();
  }`
    );
  }, b = (x, B) => {
    const k = B ? 4 : 9, I = B ? 2 : 3;
    return (
      /* wgsl */
      `
    let x${x} = i32(${B ? "x" : "x * 2u + phaseX"});
    let y${x} = i32(${B ? "y" : "y * 2u + phaseY"});
    base${x}[load] = y${x} * i32(params.source${x}Width) + x${x};
    var bits${x} = 0u;
    for (var tap = 0u; tap < ${k}u; tap++) {
      let sx = x${x} + i32(tap % ${I}u) ${B ? "+ i32(phaseX)" : ""} - 1;
      let sy = y${x} + i32(tap / ${I}u) ${B ? "+ i32(phaseY)" : ""} - 1;
      if (valid && sx >= 0 && sy >= 0 && sx < i32(params.source${x}Width) && sy < i32(params.source${x}Height)) {
        bits${x} |= 1u << tap;
      }
    }
    mask${x}[load] = bits${x};`
    );
  };
  return (
    /* wgsl */
    `${gt(r)}
struct Params {
  outputWidth: u32, outputHeight: u32, outputBlocks: u32, inputBlocks: u32,
  source0Width: u32, source0Height: u32, source1Width: u32, source1Height: u32,
}
@group(0) @binding(0) var<storage, read> input0: array<${m ? "vec2<u32>" : _}>;
@group(0) @binding(1) var<storage, read> input1: array<${m ? "vec2<u32>" : _}>;
@group(0) @binding(2) var<storage, read> weights: array<${v ? "vec2<u32>" : _}>;
@group(0) @binding(3) var<storage, read> bias: array<vec4<f32>>;
@group(0) @binding(4) var<storage, read_write> outputData: array<${R(t)}>;
@group(0) @binding(5) var<uniform> params: Params;
var<workgroup> inputTile: array<${_}, ${bt(a).input}>;
var<workgroup> weightTile: array<${_}, ${bt(a).weights}>;

@compute @workgroup_size(${u}, ${l}, 1)
fn main(@builtin(local_invocation_id) localId: vec3<u32>, @builtin(workgroup_id) workgroupId: vec3<u32>) {
  let phase = workgroupId.z;
  let phaseX = phase & 1u;
  let phaseY = phase >> 1u;
  let phaseWidth = (params.outputWidth + 1u) / 2u;
  let localLinear = localId.y * ${u}u + localId.x;
  let outputBlock = workgroupId.y * ${d}u + localId.x;
  var acc: array<vec4<f32>, ${p}>;
  if (outputBlock < ${o}u) {
    for (var row = 0u; row < ${p}u; row++) { acc[row] = bias[outputBlock]; }
  }
  var base0: array<i32, ${f}>;
  var base1: array<i32, ${f}>;
  var mask0: array<u32, ${f}>;
  var mask1: array<u32, ${f}>;
  for (var load = 0u; load < ${f}u; load++) {
    let spatial = workgroupId.x * ${c}u + (localLinear + load * ${g}u) / ${h}u;
    let x = spatial % phaseWidth;
    let y = spatial / phaseWidth;
    let valid = x * 2u + phaseX < params.outputWidth && y * 2u + phaseY < params.outputHeight;
    ${b(n, !0)}
    ${b(T, !1)}
  }
  ${$(n, !0)}
  ${$(T, !1)}
  if (outputBlock < ${o}u) {
    for (var row = 0u; row < ${p}u; row++) {
      let spatial = workgroupId.x * ${c}u + localId.y * ${p}u + row;
      let x = (spatial % phaseWidth) * 2u + phaseX;
      let y = (spatial / phaseWidth) * 2u + phaseY;
      if (x < params.outputWidth && y < params.outputHeight) {
        outputData[(y * params.outputWidth + x) * ${o}u + outputBlock] = ${ut(pt("acc[row]", e), t)};
      }
    }
  }
}
`
  );
}
function Xn(r, t, e, i, n, o) {
  const a = R(r), s = R(t), u = i[0] + i[1], l = (c, d) => {
    const h = c === n;
    return (
      /* wgsl */
      `
      {
        let sourceX = ${h ? "u32(inputX) / 2u" : "u32(inputX)"};
        let sourceY = ${h ? "u32(inputY) / 2u" : "u32(inputY)"};
        let sourcePixelBase = (sourceY * params.source${c}Width + sourceX) * ${i[c]}u;
        for (var sourceBlock = 0u; sourceBlock < ${i[c]}u; sourceBlock++) {
          let inputBlock = ${d}u + sourceBlock;
          ${le(
        `input${c}[sourcePixelBase + sourceBlock]`,
        `((((gid.z * 3u + ky) * 3u + kx) * ${u}u + inputBlock) * 4u)`,
        r
      )}
        }
      }
`
    );
  }, p = ut(
    pt("acc", e),
    t
  );
  return (
    /* wgsl */
    `${gt(r)}
struct Params {
  outputWidth: u32,
  outputHeight: u32,
  outputBlocks: u32,
  inputBlocks: u32,
  source0Width: u32,
  source0Height: u32,
  source1Width: u32,
  source1Height: u32,
}
@group(0) @binding(0) var<storage, read> input0: array<${a}>;
@group(0) @binding(1) var<storage, read> input1: array<${a}>;
@group(0) @binding(2) var<storage, read> weights: array<${a}>;
@group(0) @binding(3) var<storage, read> bias: array<vec4<f32>>;
@group(0) @binding(4) var<storage, read_write> outputData: array<${s}>;
@group(0) @binding(5) var<uniform> params: Params;

@compute @workgroup_size(${J}, ${J}, 1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
  if (gid.x >= params.outputWidth || gid.y >= params.outputHeight || gid.z >= ${o}u) {
    return;
  }
  var acc = bias[gid.z];
  for (var ky = 0u; ky < 3u; ky++) {
    let inputY = i32(gid.y) + i32(ky) - 1;
    if (inputY < 0 || inputY >= i32(params.outputHeight)) { continue; }
    for (var kx = 0u; kx < 3u; kx++) {
      let inputX = i32(gid.x) + i32(kx) - 1;
      if (inputX < 0 || inputX >= i32(params.outputWidth)) { continue; }
      ${l(0, 0)}
      ${l(1, i[0])}
    }
  }
  let outputIndex = (gid.y * params.outputWidth + gid.x) * ${o}u + gid.z;
  outputData[outputIndex] = ${p};
}
`
  );
}
function Yn(r, t, e, i, n, o) {
  const a = R(r), s = R(t), u = i[0] + i[1], l = ct * ct * u, p = j * j, c = ut(
    pt("acc", e),
    t
  ), d = (h, g) => (
    /* wgsl */
    `
      let sourceBlock = ${g};
      let sourceIndex =
        (${h === n ? "u32(inputY) / 2u" : "u32(inputY)"} * params.source${h}Width + ${h === n ? "u32(inputX) / 2u" : "u32(inputX)"}) *
        ${i[h]}u + sourceBlock;
      value = input${h}[sourceIndex];`
  );
  return (
    /* wgsl */
    `${gt(r)}
struct Params {
  outputWidth: u32,
  outputHeight: u32,
  outputBlocks: u32,
  inputBlocks: u32,
  source0Width: u32,
  source0Height: u32,
  source1Width: u32,
  source1Height: u32,
}
@group(0) @binding(0) var<storage, read> input0: array<${a}>;
@group(0) @binding(1) var<storage, read> input1: array<${a}>;
@group(0) @binding(2) var<storage, read> weights: array<${a}>;
@group(0) @binding(3) var<storage, read> bias: array<vec4<f32>>;
@group(0) @binding(4) var<storage, read_write> outputData: array<${s}>;
@group(0) @binding(5) var<uniform> params: Params;

var<workgroup> inputPatch: array<${a}, ${l}>;

@compute @workgroup_size(${j}, ${j}, 1)
fn main(
  @builtin(local_invocation_id) localId: vec3<u32>,
  @builtin(workgroup_id) workgroupId: vec3<u32>
) {
  let localLinear =
    localId.y * ${j}u + localId.x;
  for (
    var loadIndex = localLinear;
    loadIndex < ${l}u;
    loadIndex += ${p}u
  ) {
    let patchPixel = loadIndex / ${u}u;
    let inputBlock = loadIndex % ${u}u;
    let patchX = patchPixel % ${ct}u;
    let patchY = patchPixel / ${ct}u;
    let inputX =
      i32(workgroupId.x * ${j}u + patchX) - 1;
    let inputY =
      i32(workgroupId.y * ${j}u + patchY) - 1;
    var value = ${a}(0.0);
    if (
      inputX >= 0 && inputX < i32(params.outputWidth) &&
      inputY >= 0 && inputY < i32(params.outputHeight)
    ) {
      if (inputBlock < ${i[0]}u) {
        ${d(0, "inputBlock")}
      } else {
        ${d(1, `inputBlock - ${i[0]}u`)}
      }
    }
    inputPatch[loadIndex] = value;
  }
  workgroupBarrier();

  let outputX =
    workgroupId.x * ${j}u + localId.x;
  let outputY =
    workgroupId.y * ${j}u + localId.y;
  let outputBlock = workgroupId.z;
  if (
    outputX >= params.outputWidth || outputY >= params.outputHeight ||
    outputBlock >= ${o}u
  ) {
    return;
  }

  var acc = bias[outputBlock];
  for (var ky = 0u; ky < 3u; ky++) {
    for (var kx = 0u; kx < 3u; kx++) {
      let patchBase =
        ((localId.y + ky) * ${ct}u + localId.x + kx) *
        ${u}u;
      for (var inputBlock = 0u; inputBlock < ${u}u; inputBlock++) {
        ${le(
      "inputPatch[patchBase + inputBlock]",
      `((((outputBlock * 3u + ky) * 3u + kx) * ${u}u + inputBlock) * 4u)`,
      r
    )}
      }
    }
  }
  let outputIndex =
    (outputY * params.outputWidth + outputX) * ${o}u + outputBlock;
  outputData[outputIndex] = ${c};
}
`
  );
}
function yr(r, t) {
  const e = R(r), i = Array.from(
    { length: t },
    (s, u) => `@group(0) @binding(${u}) var<storage, read> input${u}: array<vec4<f32>>;`
  ).join(`
`), n = Array.from({ length: t }, (s, u) => {
    const l = u * 3;
    return `if (channel < ${l + 3}u) { return input${u}[pixel][channel - ${l}u]; }`;
  }).join(`
  `), o = t, a = t + 1;
  return (
    /* wgsl */
    `${gt(r)}
struct Params {
  width: u32,
  height: u32,
  outputBlocks: u32,
  inputChannels: u32,
}
${i}
@group(0) @binding(${o}) var<storage, read_write> outputData: array<${e}>;
@group(0) @binding(${a}) var<uniform> params: Params;

fn readChannel(pixel: u32, channel: u32) -> f32 {
  ${n}
  return 0.0;
}

@compute @workgroup_size(${J}, ${J}, 1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
  if (gid.x >= params.width || gid.y >= params.height || gid.z >= params.outputBlocks) {
    return;
  }
  let pixel = gid.y * params.width + gid.x;
  let firstChannel = gid.z * 4u;
  let value = vec4<f32>(
    readChannel(pixel, firstChannel),
    readChannel(pixel, firstChannel + 1u),
    readChannel(pixel, firstChannel + 2u),
    readChannel(pixel, firstChannel + 3u)
  );
  outputData[pixel * params.outputBlocks + gid.z] = ${ut("value", r)};
}
`
  );
}
const Fn = "chromium-experimental-subgroup-matrix", Gn = "wgpu-cooperative-matrix", Gt = 32, Ht = 8, zt = 4, Hn = 1024;
function _r(r, t = "f32") {
  var s;
  const e = r.features.has(Fn) ? "tint" : r.features.has(Gn) ? "naga" : void 0;
  if (!e || !r.features.has("subgroups")) return;
  const i = r.adapterInfo, n = (i == null ? void 0 : i.subgroupMaxSize) ?? 0;
  if (!i || !n || i.subgroupMinSize !== n) return;
  const o = t === "f16" ? [["f16", "f16"], ["f16", "f32"], ["f32", "f32"]] : [["f16", "f32"], ["f32", "f32"], ["f16", "f16"]], a = r.features.has("shader-f16");
  for (const [u, l] of o)
    if (!(!a && (u === "f16" || l === "f16"))) {
      for (const p of [8, 16])
        if ((s = i.subgroupMatrixConfigs) == null ? void 0 : s.find((d) => d.componentType === u && d.resultComponentType === l && d.M === p && d.N === p && d.K === p))
          return {
            componentType: u,
            resultType: l,
            size: p,
            subgroupSize: n,
            dialect: e
          };
    }
}
function Vn(r, t) {
  return t.outputFragments ?? 2;
}
function jn(r, t, e, i) {
  const n = r.componentType === "f16" ? 2 : 4, o = r.resultType === "f16" ? 2 : 4, a = r.size, s = (Ht + 2) * (Gt + 2) * a, u = e ? 16 : 9;
  return n * (i ? 2 : 1) * (s + u * a * a * t) + o * zt * Gt * a;
}
function wr(r) {
  const t = r.size, e = r.dialect === "naga", i = (o, a, s) => e ? `coop_mat${t}x${t}<${o}, ${s}>` : `subgroup_matrix_${a}<${o}, ${t}, ${t}>`, n = i(r.resultType, "result", "C");
  return {
    left: i(r.componentType, "left", "A"),
    right: i(r.componentType, "right", "B"),
    result: n,
    load: (o, a, s, u) => e ? `coopLoadT<${o}>(&${a}[${s}], ${u})` : `subgroupMatrixLoad<${o}, row_major>(&${a}, ${s}, ${u})`,
    store: (o, a, s, u) => e ? `coopStoreT(${s}, &${o}[${a}], ${u});` : `subgroupMatrixStore<row_major>(&${o}, ${a}, ${s}, ${u});`,
    multiplyAdd: e ? "coopMultiplyAdd" : "subgroupMatrixMultiplyAccumulate",
    declare: (o) => e ? `var ${o}: ${n};` : `var ${o} = ${n}();`,
    // Naga provides subgroups without (and rejects) `enable subgroups`.
    extensions: e ? "enable wgpu_cooperative_matrix;" : `enable subgroups;
enable chromium_experimental_subgroup_matrix;
diagnostic(off, chromium_experimental.subgroup_matrix_uniformity);`
  };
}
function Ge(r, t, e, i, n, o, a) {
  const s = (g, f, _) => `{
        let staged = vec4<${o}>(${f});
        let base = ${_} + i * 4u;
        ${g.array}[base] = staged.x;
        ${g.array}[base + 1u] = staged.y;
        ${g.array}[base + 2u] = staged.z;
        ${g.array}[base + 3u] = staged.w;
      }`, u = (g) => t.map((f) => `
    for (var batch = 0u; batch < ${Math.ceil(f.vectors / i)}u; batch++) {
      let i = localIndex + batch * ${i}u;
      if (i < ${f.vectors}u) {
        var value = ${n}(0.0);
        ${f.read}
        ${s(f, "value", g(f))}
      }
    }`).join(""), l = (g) => t.map((f) => `let ${f.array}Base = ${g} * ${(f.stride ?? f.vectors) * 4}u;`).join(`
    `);
  if (!a || r < 2)
    return (
      /* wgsl */
      `
  for (var chunk = 0u; chunk < ${r}u; chunk++) {
    let stageChunk = chunk;
    ${l("0u")}
    ${u((g) => `${g.array}Base`)}
    workgroupBarrier();
    ${e}
    workgroupBarrier();
  }`
    );
  const p = t.map((g) => Math.ceil(g.vectors / i)), c = t.map((g, f) => Array.from({ length: p[f] }, (_, m) => `
      {
        let i = localIndex + ${m * i}u;
        var value = ${n}(0.0);
        if (i < ${g.vectors}u) {
          ${g.read}
        }
        prefetch${f}_${m} = value;
      }`).join("")).join(""), d = t.map((g, f) => Array.from({ length: p[f] }, (_, m) => `var prefetch${f}_${m} = ${n}(0.0);`).join(`
    `)).join(`
    `), h = t.map((g, f) => Array.from({ length: p[f] }, (_, m) => `
      {
        let i = localIndex + ${m * i}u;
        if (i < ${g.vectors}u) ${s(g, `prefetch${f}_${m}`, `next * ${(g.stride ?? g.vectors) * 4}u`)}
      }`).join("")).join("");
  return (
    /* wgsl */
    `
  {
    let stageChunk = 0u;
    ${u(() => "0u")}
  }
  workgroupBarrier();
  for (var chunk = 0u; chunk < ${r}u; chunk++) {
    let buffer = chunk & 1u;
    let next = 1u - buffer;
    ${l("buffer")}
    ${d}
    if (chunk + 1u < ${r}u) {
      let stageChunk = chunk + 1u;
      ${c}
    }
    ${e}
    if (chunk + 1u < ${r}u) {
      ${h}
    }
    workgroupBarrier();
  }`
  );
}
function vr(r, t) {
  return r.dialect === "naga" ? Array.from({ length: 9 }, (e, i) => `{ let tap = ${i}u; ${t} }`).join(`
`) : `for (var tap = 0u; tap < 9u; tap++) { ${t} }`;
}
function Kn(r, t, e, i, n, o, a, s, u, l = !1) {
  const p = o.size, c = o.componentType, d = o.resultType, h = Gt, g = h + 2, f = Ht + 2, _ = s ? 2 : 1, m = p / 4, v = Math.ceil(i / m), w = a * p, T = h / p, $ = zt * o.subgroupSize, b = R(r), x = R(t), { left: B, right: k, load: I, store: P, multiplyAdd: O, declare: F, extensions: tt } = wr(o), nt = [];
  for (let N = 0; N < 2; N++)
    for (let G = 0; G < T; G++)
      for (let z = 0; z < a; z++)
        nt.push(`acc${N}_${G}_${z}`);
  const dt = nt.map(F).join(`
  `), q = Array.from(
    { length: a },
    (N, G) => `let b${G} = ${I(k, "weightTile", `weightTileBase + tap * ${p * w}u + ${G * p}u`, `${w}u`)};`
  ).join(`
      `), rt = [];
  for (let N = 0; N < 2; N++)
    for (let G = 0; G < T; G++)
      rt.push(
        `{
        let a = ${I(B, "inputTile", `inputTileBase + ((rowBase + ${N}u + ky) * ${g}u + ${G * p}u + kx) * ${p}u`, `${p}u`)};
        ${Array.from(
          { length: a },
          (z, V) => `acc${N}_${G}_${V} = ${O}(a, b${V}, acc${N}_${G}_${V});`
        ).join(`
        `)}
      }`
      );
  const kt = `let ky = tap / 3u;
      let kx = tap % 3u;
      ${q}
      ${rt.join(`
      `)}`, Ut = vr(o, kt), X = [];
  if (l)
    for (let N = 0; N < a; N++)
      X.push(`{
    var pooled = vec4<f32>(-3.402823e38);
    let pixel = lane / ${m}u;
    let outputBlock = blockBase + ${N * m}u + lane % ${m}u;
    ${Array.from({ length: 2 }, (G, z) => `
    ${Array.from(
        { length: T },
        (V, Mt) => P("resultTile", `resultBase + ${Mt * p * p}u`, `acc${z}_${Mt}_${N}`, `${p}u`)
      ).join(`
    `)}
    workgroupBarrier();
    {
      let base = resultBase + pixel * ${p * 2}u + (lane % ${m}u) * 4u;
      let first = vec4<f32>(resultTile[base], resultTile[base + 1u], resultTile[base + 2u], resultTile[base + 3u]);
      let second = vec4<f32>(resultTile[base + ${p}u], resultTile[base + ${p + 1}u], resultTile[base + ${p + 2}u], resultTile[base + ${p + 3}u]);
      if (tileY + rowBase + ${z}u < params.inputHeight) {
        pooled = max(pooled, first);
        if (tileX + pixel * 2u + 1u < params.inputWidth) { pooled = max(pooled, second); }
      }
    }
    workgroupBarrier();`).join(`
    `)}
    let outputX = tileX / 2u + pixel;
    let outputY = (tileY + rowBase) / 2u;
    if (outputX < params.outputWidth && outputY < params.outputHeight && outputBlock < ${n}u) {
      let acc = pooled + bias[outputBlock];
      outputData[(outputY * params.outputWidth + outputX) * ${n}u + outputBlock] = ${ut(pt("acc", e), t)};
    }
  }`);
  else
    for (let N = 0; N < 2; N++)
      for (let G = 0; G < a; G++)
        X.push(`{
    ${Array.from(
          { length: T },
          (z, V) => P("resultTile", `resultBase + ${V * p * p}u`, `acc${N}_${V}_${G}`, `${p}u`)
        ).join(`
    `)}
    workgroupBarrier();
    let outputY = tileY + rowBase + ${N}u;
    for (var e = lane; e < ${h * m}u; e += ${o.subgroupSize}u) {
      let pixel = e / ${m}u;
      let outputX = tileX + pixel;
      let outputBlock = blockBase + ${G * m}u + e % ${m}u;
      if (outputX < params.outputWidth && outputY < params.outputHeight && outputBlock < ${n}u) {
        let base = resultBase + pixel * ${p}u + (e % ${m}u) * 4u;
        let acc = vec4<f32>(f32(resultTile[base]), f32(resultTile[base + 1u]),
          f32(resultTile[base + 2u]), f32(resultTile[base + 3u])) + bias[outputBlock];
        outputData[(outputY * params.outputWidth + outputX) * ${n}u + outputBlock] = ${ut(pt("acc", e), t)};
      }
    }
    workgroupBarrier();
  }`);
  const K = u ? (() => {
    const [N, G] = u.sourceBlocks, z = (V) => V === u.upsampledSource ? `min(u32(inputY) / 2u, params.source${V}Height - 1u) * params.source${V}Width + min(u32(inputX) / 2u, params.source${V}Width - 1u)` : `u32(inputY) * params.source${V}Width + u32(inputX)`;
    return (
      /* wgsl */
      `if (block < ${N}u) {
          value = input0[(${z(0)}) * ${N}u + block];
        } else {
          value = input1[(${z(1)}) * ${G}u + block - ${N}u];
        }`
    );
  })() : `value = inputData[(u32(inputY) * params.inputWidth + u32(inputX)) * ${i}u + block];`, St = u ? `struct Params {
  outputWidth: u32,
  outputHeight: u32,
  outputBlocks: u32,
  inputBlocks: u32,
  source0Width: u32,
  source0Height: u32,
  source1Width: u32,
  source1Height: u32,
}
@group(0) @binding(0) var<storage, read> input0: array<${b}>;
@group(0) @binding(1) var<storage, read> input1: array<${b}>;
@group(0) @binding(2) var<storage, read> weights: array<${b}>;
@group(0) @binding(3) var<storage, read> bias: array<vec4<f32>>;
@group(0) @binding(4) var<storage, read_write> outputData: array<${x}>;
@group(0) @binding(5) var<uniform> params: Params;` : `struct Params {
  inputWidth: u32,
  inputHeight: u32,
  outputWidth: u32,
  outputHeight: u32,
  inputBlocks: u32,
  outputBlocks: u32,
}
@group(0) @binding(0) var<storage, read> inputData: array<${b}>;
@group(0) @binding(1) var<storage, read> weights: array<${b}>;
@group(0) @binding(2) var<storage, read> bias: array<vec4<f32>>;
@group(0) @binding(3) var<storage, read_write> outputData: array<${x}>;
@group(0) @binding(4) var<uniform> params: Params;`, Lt = u ? "params.outputWidth" : "params.inputWidth", Dt = u ? "params.outputHeight" : "params.inputHeight";
  return (
    /* wgsl */
    `${r === "fp16" || t === "fp16" || c === "f16" || d === "f16" ? `enable f16;
` : ""}${tt}
${St}

var<workgroup> inputTile: array<${c}, ${_ * f * g * p}>;
var<workgroup> weightTile: array<${c}, ${_ * 9 * p * w}>;
var<workgroup> resultTile: array<${d}, ${zt * h * p}>;

@compute @workgroup_size(${$})
fn main(
  @builtin(local_invocation_index) localIndex: u32,
  @builtin(workgroup_id) workgroupId: vec3<u32>,
  @builtin(subgroup_id) subgroup: u32,
  @builtin(subgroup_invocation_id) lane: u32
) {
  let tileX = workgroupId.x * ${h}u;
  let tileY = workgroupId.y * ${Ht}u;
  let blockBase = workgroupId.z * ${w / 4}u;
  let rowBase = subgroup * 2u;
  let resultBase = subgroup * ${h * p}u;
  ${dt}

  ${Ge(v, [
      {
        array: "inputTile",
        vectors: f * g * m,
        read: (
          /* wgsl */
          `let pixel = i / ${m}u;
      let block = stageChunk * ${m}u + i % ${m}u;
      let inputX = i32(tileX + pixel % ${g}u) - 1;
      let inputY = i32(tileY + pixel / ${g}u) - 1;
      if (block < ${i}u && inputX >= 0 && inputY >= 0 &&
          inputX < i32(${Lt}) && inputY < i32(${Dt})) {
        ${K}
      }`
        )
      },
      {
        array: "weightTile",
        vectors: 9 * p * w / 4,
        read: (
          /* wgsl */
          `let quad = i % ${w / 4}u;
      let row = i / ${w / 4}u;
      let tap = row / ${p}u;
      let inputChannel = stageChunk * ${p}u + row % ${p}u;
      let outputBlock = blockBase + quad;
      if (inputChannel < ${i * 4}u && outputBlock < ${n}u) {
        value = weights[((tap * ${i}u + inputChannel / 4u) * ${n}u + outputBlock) * 4u + inputChannel % 4u];
      }`
        )
      }
    ], Ut, $, b, c, s)}

  ${X.join(`
  `)}
}
`
  );
}
const me = [[-1, 0], [0, 1]], Si = [[[0], [1, 2]], [[0, 1], [2]]], ae = 16;
function qn(r, t, e, i, n, o) {
  const a = A(e.outputChannels), s = n[o], u = n[1 - o], l = o === 0 ? 0 : n[0] * 4, p = o === 0 ? n[0] * 4 : 0, c = ae * s * a * 16, d = new Float32Array(c + 9 * u * a * 16), h = ke(e.weight), g = (v, w, T, $) => v < e.outputChannels && w < e.inputChannels ? h[((v * e.inputChannels + w) * 3 + T) * 3 + $] : 0;
  for (let v = 0; v < a * 4; v++) {
    const w = v >> 2, T = v & 3;
    for (let $ = 0; $ < s * 4; $++)
      for (let b = 0; b < 4; b++)
        for (let x = 0; x < 4; x++) {
          let B = 0;
          for (const I of Si[b >> 1][x >> 1])
            for (const P of Si[b & 1][x & 1])
              B += g(v, l + $, I, P);
          const k = ((b * 4 + x) * s + ($ >> 2)) * a + w;
          d[k * 16 + ($ & 3) * 4 + T] = B;
        }
    for (let $ = 0; $ < u * 4; $++)
      for (let b = 0; b < 9; b++) {
        const x = (b * u + ($ >> 2)) * a + w;
        d[c + x * 16 + ($ & 3) * 4 + T] = g(v, p + $, b / 3 | 0, b % 3);
      }
  }
  let f;
  if (i === "fp16") {
    const v = new Uint16Array(d.length);
    for (let w = 0; w < d.length; w++) v[w] = gr(d[w]);
    f = new Uint8Array(v.buffer);
  } else
    f = new Uint8Array(d.buffer);
  const _ = new Float32Array(a * 4);
  _.set(ke(e.bias));
  const m = ce(r, `oidn/${t}/weights/${i}/subpixel`, f, ot.STORAGE);
  try {
    return {
      weights: m,
      weightLayout: "k-major",
      bias: ce(r, `oidn/${t}/bias`, new Uint8Array(_.buffer), ot.STORAGE)
    };
  } catch (v) {
    throw m.destroy(), v;
  }
}
function Qn(r, t, e, i, n, o, a, s, u) {
  const l = n.size, p = u ? 2 : 1, c = n.componentType, d = n.resultType, h = Gt, g = Ht, f = h + 2, _ = g + 2, m = h / 2 + 2, v = g / 2 + 2, w = l / 4, T = a[s], $ = a[1 - s], b = Math.ceil(T / w), x = Math.ceil($ / w), B = o * l, k = g / 2, I = h / 2 / l, P = zt * n.subgroupSize, O = R(r), F = R(t), { left: tt, right: nt, load: dt, store: q, multiplyAdd: rt, declare: kt, extensions: Ut } = wr(n), X = (W, U, Q) => `acc${W}_${U}_${Q}`, K = [];
  for (let W = 0; W < k; W++)
    for (let U = 0; U < I; U++)
      for (let Q = 0; Q < o; Q++)
        K.push(kt(X(W, U, Q)));
  const St = (W, U) => {
    const Q = [];
    for (let $t = 0; $t < k; $t++)
      for (let te = 0; te < I; te++)
        Q.push(`{
        let a = ${dt(tt, "inputTile", `inputTileBase + ${W($t, te)}`, U)};
        ${Array.from(
          { length: o },
          (ua, We) => `${X($t, te, We)} = ${rt}(a, b${We}, ${X($t, te, We)});`
        ).join(`
        `)}
      }`);
    return Q.join(`
      `);
  }, Lt = (W) => Array.from(
    { length: o },
    (U, Q) => `let b${Q} = ${dt(nt, "weightTile", `weightTileBase + ${W} * ${l * B}u + ${Q * l}u`, `${B}u`)};`
  ).join(`
      `), Dt = (W, U, Q) => `(${Q} * params.source${W}Width + ${U}) * ${a[W]}u`, Rt = Math.max(_ * f, v * m) * w, N = ae * l * B / 4, G = (W, U, Q) => ({
    array: "weightTile",
    vectors: W * l * B / 4,
    stride: N,
    read: (
      /* wgsl */
      `let quad = i % ${B / 4}u;
      let row = i / ${B / 4}u;
      let matrix = row / ${l}u;
      let inputChannel = stageChunk * ${l}u + row % ${l}u;
      let outputBlock = blockBase + quad;
      if (inputChannel < ${U * 4}u && outputBlock < ${i}u) {
        value = weights[${Q}u + ((matrix * ${U}u + inputChannel / 4u) * ${i}u + outputBlock) * 4u + inputChannel % 4u];
      }`
    )
  }), z = (W, U, Q) => ({
    array: "inputTile",
    vectors: W * U * w,
    stride: Rt,
    read: (
      /* wgsl */
      `let pixel = i / ${w}u;
      let block = stageChunk * ${w}u + i % ${w}u;
      ${Q}`
    )
  }), V = (
    /* wgsl */
    `let inputX = i32(tileX / 2u + pixel % ${m}u) - 1;
      let inputY = i32(tileY / 2u + pixel / ${m}u) - 1;
      if (block < ${T}u && inputX >= 0 && inputY >= 0 &&
          inputX < i32(params.source${s}Width) && inputY < i32(params.source${s}Height)) {
        value = input${s}[${Dt(s, "u32(inputX)", "u32(inputY)")} + block];
      }`
  ), Mt = 1 - s, Tt = (
    /* wgsl */
    `let inputX = i32(tileX + pixel % ${f}u) - 1;
      let inputY = i32(tileY + pixel / ${f}u) - 1;
      if (block < ${$}u && inputX >= 0 && inputY >= 0 &&
          inputX < i32(params.outputWidth) && inputY < i32(params.outputHeight)) {
        value = input${Mt}[${Dt(Mt, "u32(inputX)", "u32(inputY)")} + block];
      }`
  ), mt = [];
  for (let W = 0; W < 2; W++)
    for (let U = 0; U < 2; U++)
      mt.push(`{
      let matrix = phase * 4u + ${W * 2 + U}u;
      let offsetY = select(${me[0][W] + 1}u, ${me[1][W] + 1}u, phaseY == 1u);
      let offsetX = select(${me[0][U] + 1}u, ${me[1][U] + 1}u, phaseX == 1u);
      ${Lt("matrix")}
      ${St((Q, $t) => `((${Q}u + offsetY) * ${m}u + ${$t * l}u + offsetX) * ${l}u`, `${l}u`)}
    }`);
  const Jt = `let ky = tap / 3u;
      let kx = tap % 3u;
      ${Lt("tap")}
      ${St((W, U) => `((phaseY + ${W * 2}u + ky) * ${f}u + phaseX + kx + ${U * l * 2}u) * ${l}u`, `${l * 2}u`)}`, Hr = vr(n, Jt), yi = [];
  for (let W = 0; W < k; W++)
    for (let U = 0; U < o; U++)
      yi.push(`{
    ${Array.from(
        { length: I },
        (Q, $t) => q("resultTile", `resultBase + ${$t * l * l}u`, X(W, $t, U), `${l}u`)
      ).join(`
    `)}
    workgroupBarrier();
    let outputY = tileY + phaseY + ${W * 2}u;
    for (var e = lane; e < ${h / 2 * w}u; e += ${n.subgroupSize}u) {
      let pixel = e / ${w}u;
      let outputX = tileX + phaseX + pixel * 2u;
      let outputBlock = blockBase + ${U * w}u + e % ${w}u;
      if (outputX < params.outputWidth && outputY < params.outputHeight && outputBlock < ${i}u) {
        let base = resultBase + pixel * ${l}u + (e % ${w}u) * 4u;
        let acc = vec4<f32>(f32(resultTile[base]), f32(resultTile[base + 1u]),
          f32(resultTile[base + 2u]), f32(resultTile[base + 3u])) + bias[outputBlock];
        outputData[(outputY * params.outputWidth + outputX) * ${i}u + outputBlock] = ${ut(pt("acc", e), t)};
      }
    }
    workgroupBarrier();
  }`);
  return (
    /* wgsl */
    `${r === "fp16" || t === "fp16" || c === "f16" || d === "f16" ? `enable f16;
` : ""}${Ut}
struct Params {
  outputWidth: u32,
  outputHeight: u32,
  outputBlocks: u32,
  inputBlocks: u32,
  source0Width: u32,
  source0Height: u32,
  source1Width: u32,
  source1Height: u32,
}
@group(0) @binding(0) var<storage, read> input0: array<${O}>;
@group(0) @binding(1) var<storage, read> input1: array<${O}>;
@group(0) @binding(2) var<storage, read> weights: array<${O}>;
@group(0) @binding(3) var<storage, read> bias: array<vec4<f32>>;
@group(0) @binding(4) var<storage, read_write> outputData: array<${F}>;
@group(0) @binding(5) var<uniform> params: Params;

var<workgroup> inputTile: array<${c}, ${p * Rt * 4}>;
var<workgroup> weightTile: array<${c}, ${p * N * 4}>;
var<workgroup> resultTile: array<${d}, ${zt * (h / 2) * l}>;

@compute @workgroup_size(${P})
fn main(
  @builtin(local_invocation_index) localIndex: u32,
  @builtin(workgroup_id) workgroupId: vec3<u32>,
  @builtin(subgroup_id) subgroup: u32,
  @builtin(subgroup_invocation_id) lane: u32
) {
  let tileX = workgroupId.x * ${h}u;
  let tileY = workgroupId.y * ${g}u;
  let blockBase = workgroupId.z * ${B / 4}u;
  let phase = subgroup;
  let phaseY = subgroup >> 1u;
  let phaseX = subgroup & 1u;
  let resultBase = subgroup * ${h / 2 * l}u;
  ${K.join(`
  `)}

  ${Ge(
      b,
      [z(v, m, V), G(ae, T, 0)],
      mt.join(`
    `),
      P,
      O,
      c,
      u
    )}

  ${Ge(x, [
      z(_, f, Tt),
      G(9, $, ae * T * i * 4)
    ], Hr, P, O, c, u)}

  ${yi.join(`
  `)}
}
`
  );
}
function Zn(r) {
  return r.op === "concat" ? r.inputs : r.op === "fusedUpsampleConcatConv2d" ? r.inputs.map((t) => t.value) : [r.input];
}
function xr(r, t = "auto") {
  const e = r.features.has("shader-f16");
  if (t === "fp16" && !e)
    throw new Error(
      "OIDN FP16 was requested but the OIDNGPUDevice does not have shader-f16 enabled"
    );
  return t === "auto" ? e ? "fp16" : "fp32" : t;
}
class Jn {
  constructor(t, e, i = {}) {
    y(this, "usedKernels", /* @__PURE__ */ new Set());
    y(this, "precision");
    y(this, "kernelSetting");
    y(this, "gemm");
    y(this, "maxSpatialInputBlocks");
    y(this, "subgroupsAvailable");
    /** Selected configuration when the `subgroup-matrix` kernel can run. */
    y(this, "subgroupMatrix");
    y(this, "subgroupMatrixOutputFragments");
    y(this, "subgroupMatrixSubPixel");
    y(this, "subgroupMatrixDoubleBuffer");
    y(this, "_subgroupMatrixFixedFragments");
    y(this, "_model");
    y(this, "_gemmByOutputBlocks", /* @__PURE__ */ new Map());
    y(this, "_packedConvs", /* @__PURE__ */ new Map());
    y(this, "_pipelineCache");
    y(this, "_executionCache", /* @__PURE__ */ new Map());
    y(this, "_retiredExecutions", /* @__PURE__ */ new Set());
    y(this, "_clock", 0);
    y(this, "_shapeCacheSize");
    y(this, "_profileNextExecution", !1);
    y(this, "_profileOperations", 0);
    y(this, "_resources", new dr());
    y(this, "_disposed", !1);
    var c, d, h, g, f, _, m, v, w, T, $, b, x, B;
    this._device = t, this._pipelineCache = Pn(t), this.precision = xr(
      t,
      i.precision ?? "auto"
    ), this.kernelSetting = i.kernel ?? "auto";
    const n = ((c = i.gemm) == null ? void 0 : c.workgroupSize) ?? [8, 8];
    if (this.gemm = Object.freeze({
      tilePolicy: ((d = i.gemm) == null ? void 0 : d.tilePolicy) ?? ((h = i.gemm) != null && h.workgroupSize ? "fixed" : "output-aligned"),
      decoderLoad: ((g = i.gemm) == null ? void 0 : g.decoderLoad) ?? "per-load",
      decoderSubPixel: ((f = i.gemm) == null ? void 0 : f.decoderSubPixel) ?? this.precision === "fp16",
      unrollRows: ((_ = i.gemm) == null ? void 0 : _.unrollRows) ?? !1,
      poolLayout: ((m = i.gemm) == null ? void 0 : m.poolLayout) ?? "channels",
      sharedLayout: ((v = i.gemm) == null ? void 0 : v.sharedLayout) ?? (this.precision === "fp16" ? "padded-input" : "padded"),
      accumulationOrder: ((w = i.gemm) == null ? void 0 : w.accumulationOrder) ?? "k-major",
      finalLayer: ((T = i.gemm) == null ? void 0 : T.finalLayer) ?? "shared-auto",
      loadMode: (($ = i.gemm) == null ? void 0 : $.loadMode) ?? "native",
      addressMode: ((b = i.gemm) == null ? void 0 : b.addressMode) ?? "incremental",
      weightLayout: ((x = i.gemm) == null ? void 0 : x.weightLayout) ?? "k-major",
      rowsPerThread: ((B = i.gemm) == null ? void 0 : B.rowsPerThread) ?? 8,
      workgroupSize: Object.freeze([n[0], n[1]])
    }), !["fixed", "output-aligned"].includes(this.gemm.tilePolicy))
      throw new Error(`Unsupported GEMM tile policy: ${this.gemm.tilePolicy}`);
    if (!["per-load", "source-first"].includes(this.gemm.decoderLoad))
      throw new Error(`Unsupported GEMM decoder load: ${this.gemm.decoderLoad}`);
    if (!["spatial", "channels"].includes(this.gemm.poolLayout))
      throw new Error(`Unsupported GEMM pool layout: ${this.gemm.poolLayout}`);
    if (!["linear", "padded", "padded-input", "padded-weights"].includes(this.gemm.sharedLayout))
      throw new Error(`Unsupported GEMM shared layout: ${this.gemm.sharedLayout}`);
    if (!["k-major", "row-major"].includes(this.gemm.accumulationOrder))
      throw new Error(`Unsupported GEMM accumulation order: ${this.gemm.accumulationOrder}`);
    if (!["direct", "shared-input", "shared-input-weights", "shared-auto"].includes(this.gemm.finalLayer))
      throw new Error(`Unsupported GEMM final layer: ${this.gemm.finalLayer}`);
    if (!["native", "packed-weights", "packed-all"].includes(this.gemm.loadMode))
      throw new Error(`Unsupported GEMM load mode: ${this.gemm.loadMode}`);
    if (!["analytic", "incremental", "base-offset"].includes(this.gemm.addressMode))
      throw new Error(`Unsupported GEMM address mode: ${this.gemm.addressMode}`);
    if (!["output-major", "k-major"].includes(this.gemm.weightLayout))
      throw new Error(`Unsupported GEMM weight layout: ${this.gemm.weightLayout}`);
    const o = lt(this.gemm), a = (bt(this.gemm).input + bt(this.gemm).weights) * 4 * (this.precision === "fp16" ? 2 : 4);
    if (![2, 4, 8].includes(o.rowsPerThread) || ![4, 8, 16].includes(o.workgroupX) || ![4, 8].includes(o.workgroupY) || n.length !== 2 || o.workgroupX > t.limits.maxComputeWorkgroupSizeX || o.workgroupY > t.limits.maxComputeWorkgroupSizeY || o.workgroupX * o.workgroupY > t.limits.maxComputeInvocationsPerWorkgroup || a > t.limits.maxComputeWorkgroupStorageSize)
      throw new Error("Unsupported GEMM tile configuration for this OIDNGPUDevice");
    this.subgroupsAvailable = t.features.has(
      "subgroups"
    );
    const s = i.subgroupMatrix ?? {}, u = this.kernelSetting === "subgroup-matrix" ? _r(t, s.accumulation) : void 0;
    this.subgroupMatrixOutputFragments = u ? Vn(u, s) : 0, this.subgroupMatrixSubPixel = s.subPixel ?? !0, this._subgroupMatrixFixedFragments = s.outputFragments !== void 0;
    const l = (k) => u !== void 0 && jn(
      u,
      this.subgroupMatrixOutputFragments,
      this.subgroupMatrixSubPixel,
      k
    ) <= t.limits.maxComputeWorkgroupStorageSize;
    if (u && s.outputFragments === void 0 && !l(!1) && (this.subgroupMatrixOutputFragments = 1), this.subgroupMatrixDoubleBuffer = (s.doubleBuffer ?? !1) && l(!0), u && zt * u.subgroupSize <= t.limits.maxComputeInvocationsPerWorkgroup && zt * u.subgroupSize <= t.limits.maxComputeWorkgroupSizeX && l(this.subgroupMatrixDoubleBuffer) && (this.subgroupMatrix = u), this.maxSpatialInputBlocks = this.precision === "fp16" && t.limits.maxComputeInvocationsPerWorkgroup >= j * j && t.limits.maxComputeWorkgroupSizeX >= j && t.limits.maxComputeWorkgroupSizeY >= j ? Math.floor(
      t.limits.maxComputeWorkgroupStorageSize / (ct * ct * 4 * 2)
    ) : 0, this._shapeCacheSize = Math.max(1, i.shapeCacheSize ?? 2), e.inputChannels % 3 !== 0 || e.inputChannels < 3 || e.inputChannels > 9)
      throw new Error(
        `Native OIDN expects 3, 6, or 9 input channels, got ${e.inputChannels}`
      );
    this._model = {
      spec: e.spec,
      inputChannels: e.inputChannels,
      outputChannels: e.outputChannels,
      channelsByValue: new Map(e.channelsByValue),
      convChannels: new Map(e.convChannels)
    };
    const p = /* @__PURE__ */ new Map();
    for (const k of ai(this._model, {
      fuseConvPool: this._matrixPool()
    }).nodes) {
      if (k.op === "fusedUpsampleConcatConv2d") {
        const I = this._subPixelDecoder(k);
        I && p.set(k.conv.id, I);
      }
      if (k.op !== "conv2d" && k.op !== "maxPool2d" && k.op !== "fusedConvReluMaxPool2d" && k.op !== "fusedUpsampleConcatConv2d")
        throw new Error(
          `Native OIDN descriptor ${e.spec.id} leaves unsupported ${k.op} node ${k.id} after graph optimization`
        );
    }
    try {
      for (const [k, I] of e.convTensors) {
        const P = this._selectConvKernel(
          A(I.inputChannels),
          k === e.spec.output
        ), O = P === "implicit-gemm" ? this.gemm.weightLayout : P === "subgroup-matrix" ? "k-major" : "output-major", F = p.get(k), tt = F ? qn(
          t,
          k,
          I,
          this.precision,
          F.sourceBlocks,
          F.upsampledSource
        ) : Mn(
          t,
          k,
          I,
          this.precision,
          O
        );
        this._resources.track("gpu-buffer", tt.weights), this._resources.track("gpu-buffer", tt.bias), this._packedConvs.set(k, tt);
      }
    } catch (k) {
      for (const I of this._packedConvs.values())
        this._releaseBuffer(I.weights), this._releaseBuffer(I.bias);
      throw this._packedConvs.clear(), k;
    }
  }
  /** Immutable packed weights and biases, for native resource sharing. */
  get weightBuffers() {
    return [...this._packedConvs.values()].flatMap((t) => [t.weights, t.bias]);
  }
  _pipeline(t, e) {
    let i = this._pipelineCache.get(t);
    return i || (i = this._device.createComputePipeline({
      label: `oidn/${t}`,
      layout: "auto",
      compute: {
        module: this._device.createShaderModule({
          label: `oidn/${t}`,
          code: e
        }),
        entryPoint: "main"
      }
    }), this._pipelineCache.set(t, i)), i;
  }
  _nodePipelineSpec(t, e, i = this.subgroupMatrixOutputFragments) {
    if (t.op === "conv2d") {
      const n = e ? "fp32" : this.precision, o = A(
        this._model.convChannels.get(t.id).inputChannels
      ), a = A(
        this._model.convChannels.get(t.id).outputChannels
      ), s = this._gemmForOutput(a), u = this._selectConvKernel(o, e), l = s.finalLayer === "shared-input-weights" || s.finalLayer === "shared-auto" && Bi(this.precision, o, !0) <= this._device.limits.maxComputeWorkgroupStorageSize;
      return u !== "subgroup-matrix" && e && this._model.convChannels.get(t.id).outputChannels === 3 && (this.kernelSetting === "auto" || this.kernelSetting === "implicit-gemm" || this.kernelSetting === "subgroup-matrix") && s.finalLayer !== "direct" && this._device.limits.maxComputeWorkgroupSizeX >= 8 && this._device.limits.maxComputeWorkgroupSizeY >= 8 && this._device.limits.maxComputeInvocationsPerWorkgroup >= 64 && Bi(this.precision, o, l) <= this._device.limits.maxComputeWorkgroupStorageSize ? {
        key: `conv-final-rgb/${this.precision}/${t.activation}/in${o}/weights-${l ? "shared" : "storage"}`,
        kernel: "direct",
        code: In(this.precision, t.activation, o, l)
      } : u === "subgroup-matrix" ? this._subgroupMatrixSpec(
        `conv/${t.activation}/in${o}/out${a}`,
        n,
        t.activation,
        o,
        a,
        i
      ) : {
        key: `conv-${u}/${this.precision}/${n}/${t.activation}/in${o}/out${a}` + (u === "implicit-gemm" ? `/address-${s.addressMode}/weights-${s.weightLayout}-v1/tile-${s.workgroupSize.join("x")}-r${s.rowsPerThread}/loads-${s.loadMode}/shared-${s.sharedLayout}/acc-${s.accumulationOrder}/unroll-${Number(s.unrollRows)}` : ""),
        kernel: u,
        code: u === "implicit-gemm" ? Wn(
          this.precision,
          n,
          t.activation,
          o,
          a,
          s
        ) : u === "spatial" ? zn(
          this.precision,
          n,
          t.activation,
          o,
          a
        ) : u === "subgroup" ? Nn(
          this.precision,
          n,
          t.activation,
          o,
          a
        ) : On(
          this.precision,
          n,
          t.activation,
          o,
          a
        )
      };
    }
    if (t.op === "maxPool2d") {
      const n = A(
        this._model.channelsByValue.get(t.id)
      ), o = this._coalescedPool();
      return {
        key: `max-pool/${this.precision}/out${n}/${o ? "channels" : "spatial"}`,
        kernel: "direct",
        code: Ln(this.precision, n, o)
      };
    }
    if (t.op === "fusedConvReluMaxPool2d") {
      const n = A(
        this._model.convChannels.get(t.conv.id).inputChannels
      ), o = A(
        this._model.convChannels.get(t.conv.id).outputChannels
      );
      return this._matrixPool() ? this._subgroupMatrixSpec(
        `conv-pool/${t.conv.activation}/in${n}/out${o}`,
        this.precision,
        t.conv.activation,
        n,
        o,
        i,
        void 0,
        !0
      ) : {
        key: `conv-pool/${this.precision}/${t.conv.activation}/in${n}/out${o}`,
        kernel: "direct",
        code: Un(
          this.precision,
          t.conv.activation,
          n,
          o
        )
      };
    }
    if (t.op === "fusedUpsampleConcatConv2d") {
      if (t.inputs.length !== 2)
        throw new Error(`Native fused decoder ${t.id} requires two inputs`);
      const n = t.inputs.map(
        (h) => A(this._model.channelsByValue.get(h.value))
      ), o = t.inputs.findIndex((h) => h.upsample);
      if (o !== 0 && o !== 1)
        throw new Error(`Native fused decoder ${t.id} has no upsample input`);
      const a = n[0] + n[1], s = e ? "fp32" : this.precision, u = this._selectConvKernel(a, e), l = u === "subgroup" ? "direct" : u, p = A(
        this._model.convChannels.get(t.conv.id).outputChannels
      );
      if (l === "subgroup-matrix" && this._subPixelDecoder(t)) {
        const h = this.subgroupMatrix;
        return {
          key: `subgroup-matrix-subpixel/${this.precision}/${s}/${h.dialect}/${h.componentType}-${h.resultType}-${h.size}-sg${h.subgroupSize}/n${i}${this.subgroupMatrixDoubleBuffer ? "-db" : ""}/${t.conv.activation}/${n.join("+")}/out${p}/up${o}`,
          kernel: "subgroup-matrix",
          code: Qn(
            this.precision,
            s,
            t.conv.activation,
            p,
            h,
            i,
            n,
            o,
            this.subgroupMatrixDoubleBuffer
          )
        };
      }
      if (l === "subgroup-matrix")
        return this._subgroupMatrixSpec(
          `decoder/${t.conv.activation}/${n.join("+")}/out${p}/up${o}`,
          s,
          t.conv.activation,
          a,
          p,
          i,
          { sourceBlocks: n, upsampledSource: o }
        );
      const c = this._gemmForOutput(p);
      return {
        key: `decoder-${l}${l === "implicit-gemm" && this._subPixelDecoder(t) ? "-subpixel" : ""}/${this.precision}/${s}/${t.conv.activation}/${n.join("+")}/out${p}/up${o}` + (l === "implicit-gemm" ? `/address-${c.addressMode}/weights-${c.weightLayout}-v1/tile-${c.workgroupSize.join("x")}-r${c.rowsPerThread}/loads-${c.loadMode}/shared-${c.sharedLayout}/acc-${c.accumulationOrder}/unroll-${Number(c.unrollRows)}/decoder-${c.decoderLoad}` : ""),
        kernel: l,
        code: l === "implicit-gemm" ? (this._subPixelDecoder(t) ? Rn : Dn)(
          this.precision,
          s,
          t.conv.activation,
          n,
          o,
          p,
          c
        ) : l === "spatial" ? Yn(
          this.precision,
          s,
          t.conv.activation,
          n,
          o,
          p
        ) : Xn(
          this.precision,
          s,
          t.conv.activation,
          n,
          o,
          p
        )
      };
    }
    throw new Error(
      `Native OIDN does not implement unfused ${t.op} node ${t.id}`
    );
  }
  /** The decoder's sources when it runs with the sub-pixel decomposition. */
  _subPixelDecoder(t) {
    if (t.id === this._model.spec.output || t.inputs.length !== 2)
      return;
    const e = this._selectConvKernel(
      A(this._model.convChannels.get(t.conv.id).inputChannels),
      !1
    );
    if (!(e === "subgroup-matrix" && this.subgroupMatrixSubPixel) && !(e === "implicit-gemm" && this.gemm.decoderSubPixel && this.gemm.weightLayout === "k-major"))
      return;
    const i = t.inputs[0].upsample ? 0 : t.inputs[1].upsample ? 1 : -1;
    if (i !== -1)
      return {
        sourceBlocks: t.inputs.map(
          (n) => A(this._model.channelsByValue.get(n.value))
        ),
        upsampledSource: i
      };
  }
  _subgroupMatrixSpec(t, e, i, n, o, a, s, u = !1) {
    const l = this.subgroupMatrix;
    return {
      key: `subgroup-matrix/${this.precision}/${e}/${l.dialect}/${l.componentType}-${l.resultType}-${l.size}-sg${l.subgroupSize}/n${a}${this.subgroupMatrixDoubleBuffer ? "-db" : ""}/${t}`,
      kernel: "subgroup-matrix",
      code: Kn(
        this.precision,
        e,
        i,
        n,
        o,
        l,
        a,
        this.subgroupMatrixDoubleBuffer,
        s,
        u
      )
    };
  }
  /**
   * Output fragments per workgroup for a subgroup-matrix layer: the default
   * unless that leaves fewer than SUBGROUP_MATRIX_MIN_WORKGROUPS workgroups,
   * where one fragment keeps more of the GPU busy (low-resolution layers).
   * Never more fragments than the output channels fill (the final RGB layer).
   */
  _subgroupMatrixFragmentsFor(t) {
    const e = Math.min(
      this.subgroupMatrixOutputFragments,
      Math.ceil(A(t.channels) * 4 / this.subgroupMatrix.size)
    );
    return this._subgroupMatrixFixedFragments || e === 1 ? e : Math.ceil(t.width / Gt) * Math.ceil(t.height / Ht) * Math.ceil(A(t.channels) * 4 / (e * this.subgroupMatrix.size)) < Hn ? 1 : e;
  }
  // Wider output tiles reuse each input load across twice as many channels.
  // Require full output blocks and keep the validated base tile as fallback.
  _gemmForOutput(t) {
    if (this.gemm.tilePolicy !== "output-aligned" || this.gemm.workgroupSize[0] !== 8 || t <= 0 || t % 16 !== 0)
      return this.gemm;
    const e = this._gemmByOutputBlocks.get(t);
    if (e) return e;
    const i = Object.freeze({
      ...this.gemm,
      workgroupSize: Object.freeze([16, this.gemm.workgroupSize[1]])
    }), n = lt(i), o = bt(i), a = this._device.limits, u = n.workgroupX <= a.maxComputeWorkgroupSizeX && n.workgroupY <= a.maxComputeWorkgroupSizeY && n.workgroupX * n.workgroupY <= a.maxComputeInvocationsPerWorkgroup && (o.input + o.weights) * 4 * (this.precision === "fp16" ? 2 : 4) <= a.maxComputeWorkgroupStorageSize ? i : this.gemm;
    return this._gemmByOutputBlocks.set(t, u), u;
  }
  // Keep fusion on the Large native 8x8 mixed-precision configuration measured
  // on Metal. Other matrix shapes and browser kernels retain separate pooling.
  _matrixPool() {
    const t = this.subgroupMatrix;
    return this._model.spec.family === "oidn-unet-large" && this.precision === "fp16" && (t == null ? void 0 : t.dialect) === "naga" && t.size === 8 && t.subgroupSize === 32 && t.componentType === "f16" && t.resultType === "f32";
  }
  _coalescedPool() {
    return this.gemm.poolLayout === "channels" && (this.kernelSetting === "auto" || this.kernelSetting === "implicit-gemm" || this.kernelSetting === "subgroup-matrix");
  }
  _selectConvKernel(t, e) {
    var n;
    const i = this.precision === "fp16" && t <= this.maxSpatialInputBlocks;
    return this.kernelSetting === "direct" ? "direct" : this.kernelSetting === "spatial" ? i ? "spatial" : "direct" : this.kernelSetting === "implicit-gemm" ? e ? "direct" : "implicit-gemm" : this.kernelSetting === "subgroup" ? this.subgroupsAvailable ? "subgroup" : "direct" : this.kernelSetting === "subgroup-matrix" ? e ? ((n = this.subgroupMatrix) == null ? void 0 : n.dialect) === "naga" ? "subgroup-matrix" : "direct" : this.subgroupMatrix ? "subgroup-matrix" : "implicit-gemm" : e ? "direct" : "implicit-gemm";
  }
  _nodePipeline(t, e) {
    const { key: i, code: n } = this._nodePipelineSpec(t, e);
    return this._pipeline(i, n);
  }
  _createExecution(t, e) {
    const i = this._matrixPool(), n = bn(this._model, t, e, { fuseConvPool: i }), o = /* @__PURE__ */ new Map(), a = [], s = [], u = /* @__PURE__ */ new Map();
    n.nodes.forEach((c, d) => {
      for (const h of Zn(c)) u.set(h, d);
    }), u.set(n.spec.output, n.nodes.length);
    const l = [], p = (c) => (l.push(c), this._resources.track("gpu-buffer", c));
    try {
      const c = ($, b, x, B) => {
        const k = Cn(b, x), I = s.filter((O) => O.lastUse < B);
        let P = I.filter((O) => O.capacity >= k).sort((O, F) => O.capacity - F.capacity)[0];
        !P && i && (P = I.sort((O, F) => F.capacity - O.capacity)[0]), P || (P = { capacity: 0, lastUse: -2, values: [] }, s.push(P)), P.capacity = Math.max(P.capacity, k), P.lastUse = u.get($) ?? -1, P.values.push($);
      };
      c(
        n.spec.input,
        n.inputShape,
        this.precision === "fp16" ? 2 : 4,
        -1
      ), n.plannedNodes.forEach(({ node: $, outputShape: b }, x) => {
        const B = $.id === n.spec.output;
        c(
          $.id,
          b,
          B || this.precision === "fp32" ? 4 : 2,
          x
        );
      }), s.forEach(($, b) => {
        const x = p(this._device.createBuffer({
          label: `oidn/activation/${t}x${e}/${b}`,
          size: si($.capacity, 4),
          usage: ot.STORAGE | ot.COPY_SRC | ot.COPY_DST
        }));
        a.push({ buffer: x, capacity: $.capacity });
        for (const B of $.values) o.set(B, x);
      });
      const d = this._model.inputChannels / 3, h = `pack/${this.precision}/${d}`, g = this._pipeline(
        h,
        yr(this.precision, d)
      ), f = [], _ = [], m = [], v = [], w = [];
      n.plannedNodes.forEach(({ node: $, outputShape: b }, x) => {
        const B = $.id === n.spec.output, k = this.subgroupMatrix ? this._subgroupMatrixFragmentsFor(b) : 0, I = this._nodePipelineSpec($, B, k);
        I.kernel && this.usedKernels.add(I.kernel);
        const P = this._pipeline(I.key, I.code);
        f.push(P), _.push(I.kernel ?? "direct"), m.push(I.kernel === "subgroup-matrix" ? k : 0);
        const O = o.get($.id);
        let F, tt, nt;
        if ($.op === "conv2d") {
          const q = n.valueShapes.get($.input);
          nt = $.id, tt = [
            q.width,
            q.height,
            b.width,
            b.height,
            A(q.channels),
            A(b.channels)
          ];
          const rt = this._packedConvs.get(nt);
          F = [
            { binding: 0, resource: { buffer: o.get($.input) } },
            { binding: 1, resource: { buffer: rt.weights } },
            { binding: 2, resource: { buffer: rt.bias } },
            { binding: 3, resource: { buffer: O } }
          ];
        } else if ($.op === "maxPool2d") {
          const q = n.valueShapes.get($.input);
          tt = [
            q.width,
            q.height,
            b.width,
            b.height,
            A(b.channels)
          ], F = [
            { binding: 0, resource: { buffer: o.get($.input) } },
            { binding: 1, resource: { buffer: O } }
          ];
        } else if ($.op === "fusedConvReluMaxPool2d") {
          const q = n.valueShapes.get($.input);
          nt = $.conv.id, tt = [
            q.width,
            q.height,
            b.width,
            b.height,
            A(q.channels),
            A(b.channels)
          ];
          const rt = this._packedConvs.get(nt);
          F = [
            { binding: 0, resource: { buffer: o.get($.input) } },
            { binding: 1, resource: { buffer: rt.weights } },
            { binding: 2, resource: { buffer: rt.bias } },
            { binding: 3, resource: { buffer: O } }
          ];
        } else if ($.op === "fusedUpsampleConcatConv2d") {
          nt = $.conv.id;
          const q = n.valueShapes.get($.inputs[0].value), rt = n.valueShapes.get($.inputs[1].value);
          tt = [
            b.width,
            b.height,
            A(b.channels),
            A(this._model.convChannels.get(nt).inputChannels),
            q.width,
            q.height,
            rt.width,
            rt.height
          ];
          const kt = this._packedConvs.get(nt);
          F = [
            {
              binding: 0,
              resource: { buffer: o.get($.inputs[0].value) }
            },
            {
              binding: 1,
              resource: { buffer: o.get($.inputs[1].value) }
            },
            { binding: 2, resource: { buffer: kt.weights } },
            { binding: 3, resource: { buffer: kt.bias } },
            { binding: 4, resource: { buffer: O } }
          ];
        } else
          throw new Error(`Unexpected native node ${$.op}`);
        const dt = p(
          Ci(
            this._device,
            `oidn/${$.id}/params/${t}x${e}`,
            tt
          )
        );
        w.push(dt), F.push({ binding: F.length, resource: { buffer: dt } }), v.push(
          this._device.createBindGroup({
            label: `oidn/${$.id}/bindings`,
            layout: P.getBindGroupLayout(0),
            entries: F
          })
        );
      });
      const T = p(
        Ci(
          this._device,
          `oidn/input/params/${t}x${e}`,
          [
            t,
            e,
            A(this._model.inputChannels),
            this._model.inputChannels
          ]
        )
      );
      return w.push(T), {
        plan: n,
        valueBuffers: o,
        slots: a,
        nodeBindings: v,
        nodePipelines: f,
        nodeKernels: _,
        nodeFragments: m,
        inputPipeline: g,
        inputUniform: T,
        ownedBuffers: w,
        lastUsed: ++this._clock
      };
    } catch (c) {
      for (const d of l) this._releaseBuffer(d);
      throw c;
    }
  }
  _execution(t, e) {
    if (this._disposed) throw new Error("Native OIDN executor is disposed");
    const i = `${t}x${e}`;
    let n = this._executionCache.get(i);
    if (!n && (n = this._createExecution(t, e), this._executionCache.set(i, n), this._executionCache.size > this._shapeCacheSize)) {
      const o = [...this._executionCache.entries()].filter(([a]) => a !== i).sort((a, s) => a[1].lastUsed - s[1].lastUsed)[0];
      o && (this._executionCache.delete(o[0]), this._retireExecution(o[1]));
    }
    return n.lastUsed = ++this._clock, n;
  }
  /**
   * Releases an evicted plan. The synchronous core destroys it at once; the
   * browser executor waits for the queue first.
   */
  _retireExecution(t) {
    this._destroyExecution(t);
  }
  /** Allocates shape-dependent execution resources before interactive use. */
  prewarm(t) {
    for (const e of t)
      this._execution(e.width, e.height);
  }
  /** Captures per-pass GPU timestamps for the next execute call when supported. */
  profileNextExecution() {
    return this._device.features.has("timestamp-query") ? (this._profileNextExecution = !0, !0) : !1;
  }
  /** Shape-owned packed input; external preprocessing must write all lanes. */
  packedInputBuffer(t, e) {
    const i = this._execution(t, e);
    return i.valueBuffers.get(i.plan.spec.input);
  }
  /** Executes after a preprocessing pass has filled packedInputBuffer. */
  executePacked(t, e) {
    return this._execute([], t, e, !0);
  }
  execute(t, e, i) {
    return this._execute(t, e, i, !1);
  }
  _execute(t, e, i, n) {
    const o = this._model.inputChannels / 3;
    if (!n && t.length !== o)
      throw new Error(
        `Native OIDN expected ${o} input buffers, got ${t.length}`
      );
    const a = this._execution(e, i), s = [
      ...n ? [] : ["input-pack"],
      ...a.plan.nodes.map((f) => f.id)
    ], u = this._profileNextExecution && this._device.features.has("timestamp-query");
    this._profileNextExecution = !1;
    const l = s.length * 2, p = u ? this._resources.track(
      "gpu-query-set",
      this._device.createQuerySet({ type: "timestamp", count: l })
    ) : void 0, c = l * 8;
    let d, h;
    try {
      d = u ? this._resources.track(
        "gpu-buffer",
        this._device.createBuffer({
          label: `oidn/profile/resolve/${e}x${i}`,
          size: c,
          usage: ot.QUERY_RESOLVE | ot.COPY_SRC
        })
      ) : void 0, h = u ? this._resources.track(
        "gpu-buffer",
        this._device.createBuffer({
          label: `oidn/profile/readback/${e}x${i}`,
          size: c,
          usage: ot.COPY_DST | ot.MAP_READ
        })
      ) : void 0;
    } catch (f) {
      throw this._releaseBuffer(h), this._releaseBuffer(d), this._releaseQuerySet(p), f;
    }
    const g = (f, _) => ({
      label: f,
      ...p ? {
        timestampWrites: {
          querySet: p,
          beginningOfPassWriteIndex: _ * 2,
          endOfPassWriteIndex: _ * 2 + 1
        }
      } : {}
    });
    try {
      const f = this._device.createCommandEncoder({
        label: `oidn/native/${e}x${i}`
      });
      if (!n) {
        const _ = t.map(
          (w, T) => ({ binding: T, resource: { buffer: w } })
        );
        _.push({
          binding: o,
          resource: { buffer: a.valueBuffers.get(a.plan.spec.input) }
        }), _.push({
          binding: o + 1,
          resource: { buffer: a.inputUniform }
        });
        const m = this._device.createBindGroup({
          label: "oidn/input/bindings",
          layout: a.inputPipeline.getBindGroupLayout(0),
          entries: _
        }), v = f.beginComputePass(
          g("oidn/input-pack", 0)
        );
        v.setPipeline(a.inputPipeline), v.setBindGroup(0, m), v.dispatchWorkgroups(
          Math.ceil(e / J),
          Math.ceil(i / J),
          A(this._model.inputChannels)
        ), v.end();
      }
      a.plan.plannedNodes.forEach(({ node: _, outputShape: m }, v) => {
        const w = f.beginComputePass(
          g(`oidn/${a.plan.nodes[v].id}`, v + (n ? 0 : 1))
        );
        if (w.setPipeline(a.nodePipelines[v]), w.setBindGroup(0, a.nodeBindings[v]), _.op === "maxPool2d" && this._coalescedPool()) {
          const T = Math.ceil(
            m.width * A(m.channels) / J
          ), $ = this._device.limits.maxComputeWorkgroupsPerDimension;
          w.dispatchWorkgroups(
            Math.min(T, $),
            Math.ceil(m.height / J),
            Math.ceil(T / $)
          );
        } else if (a.nodeKernels[v] === "subgroup-matrix")
          w.dispatchWorkgroups(
            Math.ceil(m.width * (_.op === "fusedConvReluMaxPool2d" ? 2 : 1) / Gt),
            Math.ceil(m.height * (_.op === "fusedConvReluMaxPool2d" ? 2 : 1) / Ht),
            Math.ceil(
              A(m.channels) / (a.nodeFragments[v] * this.subgroupMatrix.size / 4)
            )
          );
        else if (a.nodeKernels[v] === "implicit-gemm") {
          const T = lt(this._gemmForOutput(A(m.channels))), $ = _.op === "fusedUpsampleConcatConv2d" && this._subPixelDecoder(_) !== void 0;
          w.dispatchWorkgroups(
            Math.ceil(
              ($ ? Math.ceil(m.width / 2) * Math.ceil(m.height / 2) : m.width * m.height) / T.tileM
            ),
            Math.ceil(
              A(m.channels) / T.tileNBlocks
            ),
            $ ? 4 : 1
          );
        } else
          w.dispatchWorkgroups(
            Math.ceil(m.width / J),
            Math.ceil(m.height / J),
            A(m.channels)
          );
        w.end();
      }), p && (f.resolveQuerySet(
        p,
        0,
        l,
        d,
        0
      ), f.copyBufferToBuffer(
        d,
        0,
        h,
        0,
        c
      )), this._device.queue.submit([f.finish()]);
    } catch (f) {
      throw this._releaseBuffer(h), this._releaseBuffer(d), this._releaseQuerySet(p), f;
    }
    return p && (this._profileOperations++, this._readProfile(p, d, h, s)), a.valueBuffers.get(a.plan.spec.output);
  }
  /**
   * Receives the timestamp queries of a profiled execution. The synchronous
   * core cannot read them back, so it releases them.
   */
  _readProfile(t, e, i, n) {
    this._releaseQuerySet(t), this._releaseBuffer(e), this._releaseBuffer(i), this._profileOperations--;
  }
  _releaseBuffer(t) {
    this._resources.release("gpu-buffer", t, () => t.destroy());
  }
  _releaseQuerySet(t) {
    this._resources.release(
      "gpu-query-set",
      t,
      () => t.destroy()
    );
  }
  _destroyExecution(t) {
    t.slots.forEach((e) => this._releaseBuffer(e.buffer)), t.ownedBuffers.forEach((e) => this._releaseBuffer(e));
  }
  getResourceInfo() {
    return this._resources.snapshot(
      this._retiredExecutions.size + this._profileOperations
    );
  }
  dispose() {
    if (!this._disposed) {
      this._disposed = !0;
      for (const t of this._packedConvs.values())
        this._releaseBuffer(t.weights), this._releaseBuffer(t.bias);
      this._packedConvs.clear();
      for (const t of this._executionCache.values())
        this._destroyExecution(t);
      this._executionCache.clear();
      for (const t of this._retiredExecutions)
        this._destroyExecution(t);
      this._retiredExecutions.clear();
    }
  }
}
function ca(r, t) {
  return xr(r, t);
}
function la(r, t) {
  return _r(r, t);
}
const Mi = /* @__PURE__ */ new WeakMap();
class to extends Jn {
  constructor(e, i, n) {
    super(e, i, n);
    y(this, "_lastExecutionProfile");
  }
  // Inputs and outputs are buffers of the browser device, so they are GPUBuffers.
  execute(e, i, n) {
    return super.execute(e, i, n);
  }
  get _asyncDevice() {
    return this._device;
  }
  get _pipelinePromises() {
    let e = Mi.get(this._device);
    return e || (e = /* @__PURE__ */ new Map(), Mi.set(this._device, e)), e;
  }
  getLastExecutionProfile() {
    return this._lastExecutionProfile;
  }
  _pipelineAsync(e, i) {
    const n = this._pipelineCache.get(e);
    if (n) return Promise.resolve(n);
    const o = this._pipelinePromises.get(e);
    if (o) return o;
    const a = this._asyncDevice.createComputePipelineAsync({
      label: `oidn/${e}`,
      layout: "auto",
      compute: {
        module: this._device.createShaderModule({
          label: `oidn/${e}`,
          code: i
        }),
        entryPoint: "main"
      }
    }).then((s) => (this._pipelineCache.set(e, s), this._pipelinePromises.delete(e), s), (s) => {
      throw this._pipelinePromises.delete(e), s;
    });
    return this._pipelinePromises.set(e, a), a;
  }
  _retireExecution(e) {
    this._retiredExecutions.add(e), this._asyncDevice.queue.onSubmittedWorkDone().catch(() => {
    }).then(() => {
      this._retiredExecutions.delete(e), this._destroyExecution(e);
    });
  }
  /** Compiles all shape-independent kernels before the model reports ready. */
  async prepare() {
    if (this._disposed) throw new Error("Native OIDN executor is disposed");
    const e = ai(this._model, { fuseConvPool: !1 }), i = this._model.inputChannels / 3, n = [
      {
        key: `pack/${this.precision}/${i}`,
        code: yr(this.precision, i)
      },
      ...e.nodes.map(
        (o) => this._nodePipelineSpec(o, o.id === e.spec.output)
      )
    ];
    await Promise.all(
      n.map(({ key: o, code: a }) => this._pipelineAsync(o, a))
    );
  }
  _readProfile(e, i, n, o) {
    this._lastExecutionProfile = (async () => {
      try {
        await n.mapAsync(Ti.READ);
        const a = new Uint32Array(n.getMappedRange()), s = (l) => a[l * 2 + 1] * 4294967296 + a[l * 2], u = o.map((l, p) => ({
          id: l,
          durationMs: (s(p * 2 + 1) - s(p * 2)) / 1e6
        }));
        return {
          totalMs: u.reduce(
            (l, p) => l + p.durationMs,
            0
          ),
          layers: u
        };
      } finally {
        n.mapState === "mapped" && n.unmap(), this._releaseQuerySet(e), this._releaseBuffer(i), this._releaseBuffer(n), this._profileOperations--;
      }
    })();
  }
  /** Executes interleaved CPU image data through the native GPU runtime. */
  async executeCPU(e, i, n) {
    const o = i * n * this._model.inputChannels;
    if (e.length !== o)
      throw new Error(
        `Native OIDN CPU input has ${e.length} values, expected ${o}`
      );
    const a = this._execution(i, n), s = this._model.inputChannels / 3, u = i * n;
    a.cpuInputBuffers || (a.cpuInputBuffers = Array.from({ length: s }, (h, g) => {
      const f = this._device.createBuffer({
        label: `oidn/cpu-input/${i}x${n}/${g}`,
        size: u * 16,
        usage: ot.STORAGE | ot.COPY_DST
      });
      return this._resources.track("gpu-buffer", f), a.ownedBuffers.push(f), f;
    }), a.cpuReadbackBuffer = this._device.createBuffer({
      label: `oidn/cpu-readback/${i}x${n}`,
      size: u * 16,
      usage: ot.COPY_DST | ot.MAP_READ
    }), this._resources.track("gpu-buffer", a.cpuReadbackBuffer), a.ownedBuffers.push(a.cpuReadbackBuffer));
    for (let h = 0; h < s; h++) {
      const g = new Float32Array(u * 4);
      for (let f = 0; f < u; f++) {
        const _ = f * this._model.inputChannels + h * 3, m = f * 4;
        g[m] = e[_], g[m + 1] = e[_ + 1], g[m + 2] = e[_ + 2];
      }
      this._device.queue.writeBuffer(
        a.cpuInputBuffers[h],
        0,
        new Uint8Array(g.buffer, g.byteOffset, g.byteLength)
      );
    }
    const l = super.execute(a.cpuInputBuffers, i, n), p = this._device.createCommandEncoder({
      label: `oidn/cpu-readback/${i}x${n}`
    });
    p.copyBufferToBuffer(
      l,
      0,
      a.cpuReadbackBuffer,
      0,
      u * 16
    ), this._device.queue.submit([p.finish()]), await a.cpuReadbackBuffer.mapAsync(Ti.READ);
    const c = new Float32Array(
      a.cpuReadbackBuffer.getMappedRange()
    ), d = new Float32Array(u * 3);
    for (let h = 0; h < u; h++)
      d[h * 3] = c[h * 4], d[h * 3 + 1] = c[h * 4 + 1], d[h * 3 + 2] = c[h * 4 + 2];
    return a.cpuReadbackBuffer.unmap(), d;
  }
}
const eo = "This is not an object", io = "This is not a Float16Array object", Ei = "This constructor is not a subclass of Float16Array", br = "The constructor property value is not an object", ro = "Species constructor didn't return TypedArray object", no = "Derived constructor created TypedArray object which was too small length", ue = "Attempting to access detached ArrayBuffer", He = "Cannot convert undefined or null to object", Ve = "Cannot mix BigInt and other types, use explicit conversions", Ai = "@@iterator property is not callable", Oi = "Reduce of empty array with no initial value", oo = "The comparison function must be either a function or undefined", De = "Offset is out of bounds";
function Y(r) {
  return (t, ...e) => at(r, t, e);
}
function Qt(r, t) {
  return Y(
    Vt(
      r,
      t
    ).get
  );
}
const {
  apply: at,
  construct: re,
  defineProperty: Ni,
  get: Re,
  getOwnPropertyDescriptor: Vt,
  getPrototypeOf: pe,
  has: je,
  ownKeys: kr,
  set: zi,
  setPrototypeOf: Tr
} = Reflect, ao = Proxy, {
  EPSILON: uo,
  MAX_SAFE_INTEGER: Wi,
  isFinite: Br,
  isNaN: jt
} = Number, {
  iterator: wt,
  species: so,
  toStringTag: li,
  for: co
} = Symbol, Kt = Object, {
  create: Ee,
  defineProperty: de,
  freeze: lo,
  is: Ui
} = Kt, Ke = Kt.prototype, po = (
  /** @type {any} */
  Ke.__lookupGetter__ ? Y(
    /** @type {any} */
    Ke.__lookupGetter__
  ) : (r, t) => {
    if (r == null)
      throw H(
        He
      );
    let e = Kt(r);
    do {
      const i = Vt(e, t);
      if (i !== void 0)
        return xt(i, "get") ? i.get : void 0;
    } while ((e = pe(e)) !== null);
  }
), xt = (
  /** @type {any} */
  Kt.hasOwn || Y(Ke.hasOwnProperty)
), Ir = Array, Pr = Ir.isArray, Ae = Ir.prototype, ho = Y(Ae.join), fo = Y(Ae.push), go = Y(
  Ae.toLocaleString
), pi = Ae[wt], mo = Y(pi), {
  abs: $o,
  trunc: Cr
} = Math, Oe = ArrayBuffer, yo = Oe.isView, Sr = Oe.prototype, _o = Y(Sr.slice), wo = Qt(Sr, "byteLength"), qe = typeof SharedArrayBuffer < "u" ? SharedArrayBuffer : null, vo = qe && Qt(qe.prototype, "byteLength"), di = pe(Uint8Array), xo = di.from, it = di.prototype, bo = it[wt], ko = Y(it.keys), To = Y(
  it.values
), Bo = Y(
  it.entries
), Io = Y(it.set), Li = Y(
  it.reverse
), Po = Y(it.fill), Co = Y(
  it.copyWithin
), Di = Y(it.sort), ie = Y(it.slice), So = Y(
  it.subarray
), et = Qt(
  it,
  "buffer"
), Et = Qt(
  it,
  "byteOffset"
), E = Qt(
  it,
  "length"
), Mr = Qt(
  it,
  li
), Mo = Uint8Array, st = Uint16Array, Ri = (...r) => at(xo, st, r), hi = Uint32Array, Eo = Float32Array, Wt = pe([][wt]()), Ne = Y(Wt.next), Ao = Y(function* () {
}().next), Oo = pe(Wt), H = TypeError, Xe = RangeError, Er = WeakSet, Ar = Er.prototype, No = Y(Ar.add), zo = Y(Ar.has), ze = WeakMap, fi = ze.prototype, Te = Y(fi.get), Wo = Y(fi.has), gi = Y(fi.set), Or = new ze(), Uo = Ee(null, {
  next: {
    value: function() {
      const t = Te(Or, this);
      return Ne(t);
    }
  },
  [wt]: {
    value: function() {
      return this;
    }
  }
});
function $e(r) {
  if (r[wt] === pi && Wt.next === Ne)
    return r;
  const t = Ee(Uo);
  return gi(Or, t, mo(r)), t;
}
const Nr = new ze(), zr = Ee(Oo, {
  next: {
    value: function() {
      const t = Te(Nr, this);
      return Ao(t);
    },
    writable: !0,
    configurable: !0
  }
});
for (const r of kr(Wt))
  r !== "next" && de(zr, r, Vt(Wt, r));
function Xi(r) {
  const t = Ee(zr);
  return gi(Nr, t, r), t;
}
function Be(r) {
  return r !== null && typeof r == "object" || typeof r == "function";
}
function Yi(r) {
  return r !== null && typeof r == "object";
}
function Ie(r) {
  return Mr(r) !== void 0;
}
function Qe(r) {
  const t = Mr(r);
  return t === "BigInt64Array" || t === "BigUint64Array";
}
function Lo(r) {
  try {
    return Pr(r) ? !1 : (wo(
      /** @type {any} */
      r
    ), !0);
  } catch {
    return !1;
  }
}
function Wr(r) {
  if (qe === null)
    return !1;
  try {
    return vo(
      /** @type {any} */
      r
    ), !0;
  } catch {
    return !1;
  }
}
function Do(r) {
  return Lo(r) || Wr(r);
}
function Fi(r) {
  return Pr(r) ? r[wt] === pi && Wt.next === Ne : !1;
}
function Ro(r) {
  return Ie(r) ? r[wt] === bo && Wt.next === Ne : !1;
}
function ye(r) {
  if (typeof r != "string")
    return !1;
  const t = +r;
  return r !== t + "" || !Br(t) ? !1 : t === Cr(t);
}
const Pe = co("__Float16Array__");
function Xo(r) {
  if (!Yi(r))
    return !1;
  const t = pe(r);
  if (!Yi(t))
    return !1;
  const e = t.constructor;
  if (e === void 0)
    return !1;
  if (!Be(e))
    throw H(br);
  return je(e, Pe);
}
const Ze = 1 / uo;
function Yo(r) {
  return r + Ze - Ze;
}
const Ur = 6103515625e-14, Fo = 65504, Lr = 9765625e-10, Gi = Lr * Ur, Go = Lr * Ze;
function Ho(r) {
  const t = +r;
  if (!Br(t) || t === 0)
    return t;
  const e = t > 0 ? 1 : -1, i = $o(t);
  if (i < Ur)
    return e * Yo(i / Gi) * Gi;
  const n = (1 + Go) * i, o = n - (n - i);
  return o > Fo || jt(o) ? e * (1 / 0) : e * o;
}
const Dr = new Oe(4), Rr = new Eo(Dr), Xr = new hi(Dr), ht = new st(512), ft = new Mo(512);
for (let r = 0; r < 256; ++r) {
  const t = r - 127;
  t < -24 ? (ht[r] = 0, ht[r | 256] = 32768, ft[r] = 24, ft[r | 256] = 24) : t < -14 ? (ht[r] = 1024 >> -t - 14, ht[r | 256] = 1024 >> -t - 14 | 32768, ft[r] = -t - 1, ft[r | 256] = -t - 1) : t <= 15 ? (ht[r] = t + 15 << 10, ht[r | 256] = t + 15 << 10 | 32768, ft[r] = 13, ft[r | 256] = 13) : t < 128 ? (ht[r] = 31744, ht[r | 256] = 64512, ft[r] = 24, ft[r | 256] = 24) : (ht[r] = 31744, ht[r | 256] = 64512, ft[r] = 13, ft[r | 256] = 13);
}
function _t(r) {
  Rr[0] = Ho(r);
  const t = Xr[0], e = t >> 23 & 511;
  return ht[e] + ((t & 8388607) >> ft[e]);
}
const mi = new hi(2048);
for (let r = 1; r < 1024; ++r) {
  let t = r << 13, e = 0;
  for (; !(t & 8388608); )
    t <<= 1, e -= 8388608;
  t &= -8388609, e += 947912704, mi[r] = t | e;
}
for (let r = 1024; r < 2048; ++r)
  mi[r] = 939524096 + (r - 1024 << 13);
const Zt = new hi(64);
for (let r = 1; r < 31; ++r)
  Zt[r] = r << 23;
Zt[31] = 1199570944;
Zt[32] = 2147483648;
for (let r = 33; r < 63; ++r)
  Zt[r] = 2147483648 + (r - 32 << 23);
Zt[63] = 3347054592;
const Yr = new st(64);
for (let r = 1; r < 64; ++r)
  r !== 32 && (Yr[r] = 1024);
function L(r) {
  const t = r >> 10;
  return Xr[0] = mi[Yr[t] + (r & 1023)] + Zt[t], Rr[0];
}
function vt(r) {
  const t = +r;
  return jt(t) || t === 0 ? 0 : Cr(t);
}
function Ye(r) {
  const t = vt(r);
  return t < 0 ? 0 : t < Wi ? t : Wi;
}
function _e(r, t) {
  if (!Be(r))
    throw H(eo);
  const e = r.constructor;
  if (e === void 0)
    return t;
  if (!Be(e))
    throw H(br);
  const i = e[so];
  return i ?? t;
}
function se(r) {
  if (Wr(r))
    return !1;
  try {
    return _o(r, 0, 0), !1;
  } catch {
  }
  return !0;
}
function Hi(r, t) {
  const e = jt(r), i = jt(t);
  if (e && i)
    return 0;
  if (e)
    return 1;
  if (i || r < t)
    return -1;
  if (r > t)
    return 1;
  if (r === 0 && t === 0) {
    const n = Ui(r, 0), o = Ui(t, 0);
    if (!n && o)
      return -1;
    if (n && !o)
      return 1;
  }
  return 0;
}
const $i = 2, Ce = new ze();
function Yt(r) {
  return Wo(Ce, r) || !yo(r) && Xo(r);
}
function M(r) {
  if (!Yt(r))
    throw H(io);
}
function we(r, t) {
  const e = Yt(r), i = Ie(r);
  if (!e && !i)
    throw H(ro);
  if (typeof t == "number") {
    let n;
    if (e) {
      const o = C(r);
      n = E(o);
    } else
      n = E(r);
    if (n < t)
      throw H(
        no
      );
  }
  if (Qe(r))
    throw H(Ve);
}
function C(r) {
  const t = Te(Ce, r);
  if (t !== void 0) {
    const n = et(t);
    if (se(n))
      throw H(ue);
    return t;
  }
  const e = (
    /** @type {any} */
    r.buffer
  );
  if (se(e))
    throw H(ue);
  const i = re(D, [
    e,
    /** @type {any} */
    r.byteOffset,
    /** @type {any} */
    r.length
  ], r.constructor);
  return Te(Ce, i);
}
function Vi(r) {
  const t = E(r), e = [];
  for (let i = 0; i < t; ++i)
    e[i] = L(r[i]);
  return e;
}
const Fr = new Er();
for (const r of kr(it)) {
  if (r === li)
    continue;
  const t = Vt(it, r);
  xt(t, "get") && typeof t.get == "function" && No(Fr, t.get);
}
const Vo = lo(
  /** @type {ProxyHandler<Float16BitsArray>} */
  {
    get(r, t, e) {
      return ye(t) && xt(r, t) ? L(Re(r, t)) : zo(Fr, po(r, t)) ? Re(r, t) : Re(r, t, e);
    },
    set(r, t, e, i) {
      return ye(t) && xt(r, t) ? zi(r, t, _t(e)) : zi(r, t, e, i);
    },
    getOwnPropertyDescriptor(r, t) {
      if (ye(t) && xt(r, t)) {
        const e = Vt(r, t);
        return e.value = L(e.value), e;
      }
      return Vt(r, t);
    },
    defineProperty(r, t, e) {
      return ye(t) && xt(r, t) && xt(e, "value") && (e.value = _t(e.value)), Ni(r, t, e);
    }
  }
);
class D {
  /** @see https://tc39.es/ecma262/#sec-typedarray */
  constructor(t, e, i) {
    let n;
    if (Yt(t))
      n = re(st, [C(t)], new.target);
    else if (Be(t) && !Do(t)) {
      let a, s;
      if (Ie(t)) {
        a = t, s = E(t);
        const u = et(t);
        if (se(u))
          throw H(ue);
        if (Qe(t))
          throw H(Ve);
        const l = new Oe(
          s * $i
        );
        n = re(st, [l], new.target);
      } else {
        const u = t[wt];
        if (u != null && typeof u != "function")
          throw H(Ai);
        u != null ? Fi(t) ? (a = t, s = t.length) : (a = [.../** @type {Iterable<unknown>} */
        t], s = a.length) : (a = /** @type {ArrayLike<unknown>} */
        t, s = Ye(a.length)), n = re(st, [s], new.target);
      }
      for (let u = 0; u < s; ++u)
        n[u] = _t(a[u]);
    } else
      n = re(st, arguments, new.target);
    const o = (
      /** @type {any} */
      new ao(n, Vo)
    );
    return gi(Ce, o, n), o;
  }
  /**
   * limitation: `Object.getOwnPropertyNames(Float16Array)` or `Reflect.ownKeys(Float16Array)` include this key
   * @see https://tc39.es/ecma262/#sec-%typedarray%.from
   */
  static from(t, ...e) {
    const i = this;
    if (!je(i, Pe))
      throw H(
        Ei
      );
    if (i === D) {
      if (Yt(t) && e.length === 0) {
        const p = C(t), c = new st(
          et(p),
          Et(p),
          E(p)
        );
        return new D(
          et(ie(c))
        );
      }
      if (e.length === 0)
        return new D(
          et(
            Ri(t, _t)
          )
        );
      const u = e[0], l = e[1];
      return new D(
        et(
          Ri(t, function(p, ...c) {
            return _t(
              at(u, this, [p, ...$e(c)])
            );
          }, l)
        )
      );
    }
    let n, o;
    const a = t[wt];
    if (a != null && typeof a != "function")
      throw H(Ai);
    if (a != null)
      Fi(t) ? (n = t, o = t.length) : Ro(t) ? (n = t, o = E(t)) : (n = [...t], o = n.length);
    else {
      if (t == null)
        throw H(
          He
        );
      n = Kt(t), o = Ye(n.length);
    }
    const s = new i(o);
    if (e.length === 0)
      for (let u = 0; u < o; ++u)
        s[u] = /** @type {number} */
        n[u];
    else {
      const u = e[0], l = e[1];
      for (let p = 0; p < o; ++p)
        s[p] = at(u, l, [n[p], p]);
    }
    return s;
  }
  /**
   * limitation: `Object.getOwnPropertyNames(Float16Array)` or `Reflect.ownKeys(Float16Array)` include this key
   * @see https://tc39.es/ecma262/#sec-%typedarray%.of
   */
  static of(...t) {
    const e = this;
    if (!je(e, Pe))
      throw H(
        Ei
      );
    const i = t.length;
    if (e === D) {
      const o = new D(i), a = C(o);
      for (let s = 0; s < i; ++s)
        a[s] = _t(t[s]);
      return o;
    }
    const n = new e(i);
    for (let o = 0; o < i; ++o)
      n[o] = t[o];
    return n;
  }
  /** @see https://tc39.es/ecma262/#sec-%typedarray%.prototype.keys */
  keys() {
    M(this);
    const t = C(this);
    return ko(t);
  }
  /**
   * limitation: returns a object whose prototype is not `%ArrayIteratorPrototype%`
   * @see https://tc39.es/ecma262/#sec-%typedarray%.prototype.values
   */
  values() {
    M(this);
    const t = C(this);
    return Xi(function* () {
      for (const e of To(t))
        yield L(e);
    }());
  }
  /**
   * limitation: returns a object whose prototype is not `%ArrayIteratorPrototype%`
   * @see https://tc39.es/ecma262/#sec-%typedarray%.prototype.entries
   */
  entries() {
    M(this);
    const t = C(this);
    return Xi(function* () {
      for (const [e, i] of Bo(t))
        yield (
          /** @type {[number, number]} */
          [e, L(i)]
        );
    }());
  }
  /** @see https://tc39.es/ecma262/#sec-%typedarray%.prototype.at */
  at(t) {
    M(this);
    const e = C(this), i = E(e), n = vt(t), o = n >= 0 ? n : i + n;
    if (!(o < 0 || o >= i))
      return L(e[o]);
  }
  /** @see https://tc39.es/proposal-change-array-by-copy/#sec-%typedarray%.prototype.with */
  with(t, e) {
    M(this);
    const i = C(this), n = E(i), o = vt(t), a = o >= 0 ? o : n + o, s = +e;
    if (a < 0 || a >= n)
      throw Xe(De);
    const u = new st(
      et(i),
      Et(i),
      E(i)
    ), l = new D(
      et(
        ie(u)
      )
    ), p = C(l);
    return p[a] = _t(s), l;
  }
  /** @see https://tc39.es/ecma262/#sec-%typedarray%.prototype.map */
  map(t, ...e) {
    M(this);
    const i = C(this), n = E(i), o = e[0], a = _e(i, D);
    if (a === D) {
      const u = new D(n), l = C(u);
      for (let p = 0; p < n; ++p) {
        const c = L(i[p]);
        l[p] = _t(
          at(t, o, [c, p, this])
        );
      }
      return u;
    }
    const s = new a(n);
    we(s, n);
    for (let u = 0; u < n; ++u) {
      const l = L(i[u]);
      s[u] = at(t, o, [l, u, this]);
    }
    return (
      /** @type {any} */
      s
    );
  }
  /** @see https://tc39.es/ecma262/#sec-%typedarray%.prototype.filter */
  filter(t, ...e) {
    M(this);
    const i = C(this), n = E(i), o = e[0], a = [];
    for (let l = 0; l < n; ++l) {
      const p = L(i[l]);
      at(t, o, [p, l, this]) && fo(a, p);
    }
    const s = _e(i, D), u = new s(a);
    return we(u), /** @type {any} */
    u;
  }
  /** @see https://tc39.es/ecma262/#sec-%typedarray%.prototype.reduce */
  reduce(t, ...e) {
    M(this);
    const i = C(this), n = E(i);
    if (n === 0 && e.length === 0)
      throw H(Oi);
    let o, a;
    e.length === 0 ? (o = L(i[0]), a = 1) : (o = e[0], a = 0);
    for (let s = a; s < n; ++s)
      o = t(
        o,
        L(i[s]),
        s,
        this
      );
    return o;
  }
  /** @see https://tc39.es/ecma262/#sec-%typedarray%.prototype.reduceright */
  reduceRight(t, ...e) {
    M(this);
    const i = C(this), n = E(i);
    if (n === 0 && e.length === 0)
      throw H(Oi);
    let o, a;
    e.length === 0 ? (o = L(i[n - 1]), a = n - 2) : (o = e[0], a = n - 1);
    for (let s = a; s >= 0; --s)
      o = t(
        o,
        L(i[s]),
        s,
        this
      );
    return o;
  }
  /** @see https://tc39.es/ecma262/#sec-%typedarray%.prototype.foreach */
  forEach(t, ...e) {
    M(this);
    const i = C(this), n = E(i), o = e[0];
    for (let a = 0; a < n; ++a)
      at(t, o, [
        L(i[a]),
        a,
        this
      ]);
  }
  /** @see https://tc39.es/ecma262/#sec-%typedarray%.prototype.find */
  find(t, ...e) {
    M(this);
    const i = C(this), n = E(i), o = e[0];
    for (let a = 0; a < n; ++a) {
      const s = L(i[a]);
      if (at(t, o, [s, a, this]))
        return s;
    }
  }
  /** @see https://tc39.es/ecma262/#sec-%typedarray%.prototype.findindex */
  findIndex(t, ...e) {
    M(this);
    const i = C(this), n = E(i), o = e[0];
    for (let a = 0; a < n; ++a) {
      const s = L(i[a]);
      if (at(t, o, [s, a, this]))
        return a;
    }
    return -1;
  }
  /** @see https://tc39.es/proposal-array-find-from-last/index.html#sec-%typedarray%.prototype.findlast */
  findLast(t, ...e) {
    M(this);
    const i = C(this), n = E(i), o = e[0];
    for (let a = n - 1; a >= 0; --a) {
      const s = L(i[a]);
      if (at(t, o, [s, a, this]))
        return s;
    }
  }
  /** @see https://tc39.es/proposal-array-find-from-last/index.html#sec-%typedarray%.prototype.findlastindex */
  findLastIndex(t, ...e) {
    M(this);
    const i = C(this), n = E(i), o = e[0];
    for (let a = n - 1; a >= 0; --a) {
      const s = L(i[a]);
      if (at(t, o, [s, a, this]))
        return a;
    }
    return -1;
  }
  /** @see https://tc39.es/ecma262/#sec-%typedarray%.prototype.every */
  every(t, ...e) {
    M(this);
    const i = C(this), n = E(i), o = e[0];
    for (let a = 0; a < n; ++a)
      if (!at(t, o, [
        L(i[a]),
        a,
        this
      ]))
        return !1;
    return !0;
  }
  /** @see https://tc39.es/ecma262/#sec-%typedarray%.prototype.some */
  some(t, ...e) {
    M(this);
    const i = C(this), n = E(i), o = e[0];
    for (let a = 0; a < n; ++a)
      if (at(t, o, [
        L(i[a]),
        a,
        this
      ]))
        return !0;
    return !1;
  }
  /** @see https://tc39.es/ecma262/#sec-%typedarray%.prototype.set */
  set(t, ...e) {
    M(this);
    const i = C(this), n = vt(e[0]);
    if (n < 0)
      throw Xe(De);
    if (t == null)
      throw H(
        He
      );
    if (Qe(t))
      throw H(
        Ve
      );
    if (Yt(t))
      return Io(
        C(this),
        C(t),
        n
      );
    if (Ie(t)) {
      const u = et(t);
      if (se(u))
        throw H(ue);
    }
    const o = E(i), a = Kt(t), s = Ye(a.length);
    if (n === 1 / 0 || s + n > o)
      throw Xe(De);
    for (let u = 0; u < s; ++u)
      i[u + n] = _t(a[u]);
  }
  /** @see https://tc39.es/ecma262/#sec-%typedarray%.prototype.reverse */
  reverse() {
    M(this);
    const t = C(this);
    return Li(t), this;
  }
  /** @see https://tc39.es/proposal-change-array-by-copy/#sec-%typedarray%.prototype.toReversed */
  toReversed() {
    M(this);
    const t = C(this), e = new st(
      et(t),
      Et(t),
      E(t)
    ), i = new D(
      et(
        ie(e)
      )
    ), n = C(i);
    return Li(n), i;
  }
  /** @see https://tc39.es/ecma262/#sec-%typedarray%.prototype.fill */
  fill(t, ...e) {
    M(this);
    const i = C(this);
    return Po(
      i,
      _t(t),
      ...$e(e)
    ), this;
  }
  /** @see https://tc39.es/ecma262/#sec-%typedarray%.prototype.copywithin */
  copyWithin(t, e, ...i) {
    M(this);
    const n = C(this);
    return Co(n, t, e, ...$e(i)), this;
  }
  /** @see https://tc39.es/ecma262/#sec-%typedarray%.prototype.sort */
  sort(t) {
    M(this);
    const e = C(this), i = t !== void 0 ? t : Hi;
    return Di(e, (n, o) => i(L(n), L(o))), this;
  }
  /** @see https://tc39.es/proposal-change-array-by-copy/#sec-%typedarray%.prototype.toSorted */
  toSorted(t) {
    M(this);
    const e = C(this);
    if (t !== void 0 && typeof t != "function")
      throw new H(oo);
    const i = t !== void 0 ? t : Hi, n = new st(
      et(e),
      Et(e),
      E(e)
    ), o = new D(
      et(
        ie(n)
      )
    ), a = C(o);
    return Di(a, (s, u) => i(L(s), L(u))), o;
  }
  /** @see https://tc39.es/ecma262/#sec-%typedarray%.prototype.slice */
  slice(t, e) {
    M(this);
    const i = C(this), n = _e(i, D);
    if (n === D) {
      const g = new st(
        et(i),
        Et(i),
        E(i)
      );
      return new D(
        et(
          ie(g, t, e)
        )
      );
    }
    const o = E(i), a = vt(t), s = e === void 0 ? o : vt(e);
    let u;
    a === -1 / 0 ? u = 0 : a < 0 ? u = o + a > 0 ? o + a : 0 : u = o < a ? o : a;
    let l;
    s === -1 / 0 ? l = 0 : s < 0 ? l = o + s > 0 ? o + s : 0 : l = o < s ? o : s;
    const p = l - u > 0 ? l - u : 0, c = new n(p);
    if (we(c, p), p === 0)
      return c;
    const d = et(i);
    if (se(d))
      throw H(ue);
    let h = 0;
    for (; u < l; )
      c[h] = L(i[u]), ++u, ++h;
    return (
      /** @type {any} */
      c
    );
  }
  /** @see https://tc39.es/ecma262/#sec-%typedarray%.prototype.subarray */
  subarray(t, e) {
    M(this);
    const i = C(this), n = _e(i, D), o = new st(
      et(i),
      Et(i),
      E(i)
    ), a = So(o, t, e), s = new n(
      et(a),
      Et(a),
      E(a)
    );
    return we(s), /** @type {any} */
    s;
  }
  /** @see https://tc39.es/ecma262/#sec-%typedarray%.prototype.indexof */
  indexOf(t, ...e) {
    M(this);
    const i = C(this), n = E(i);
    let o = vt(e[0]);
    if (o === 1 / 0)
      return -1;
    o < 0 && (o += n, o < 0 && (o = 0));
    for (let a = o; a < n; ++a)
      if (xt(i, a) && L(i[a]) === t)
        return a;
    return -1;
  }
  /** @see https://tc39.es/ecma262/#sec-%typedarray%.prototype.lastindexof */
  lastIndexOf(t, ...e) {
    M(this);
    const i = C(this), n = E(i);
    let o = e.length >= 1 ? vt(e[0]) : n - 1;
    if (o === -1 / 0)
      return -1;
    o >= 0 ? o = o < n - 1 ? o : n - 1 : o += n;
    for (let a = o; a >= 0; --a)
      if (xt(i, a) && L(i[a]) === t)
        return a;
    return -1;
  }
  /** @see https://tc39.es/ecma262/#sec-%typedarray%.prototype.includes */
  includes(t, ...e) {
    M(this);
    const i = C(this), n = E(i);
    let o = vt(e[0]);
    if (o === 1 / 0)
      return !1;
    o < 0 && (o += n, o < 0 && (o = 0));
    const a = jt(t);
    for (let s = o; s < n; ++s) {
      const u = L(i[s]);
      if (a && jt(u) || u === t)
        return !0;
    }
    return !1;
  }
  /** @see https://tc39.es/ecma262/#sec-%typedarray%.prototype.join */
  join(t) {
    M(this);
    const e = C(this), i = Vi(e);
    return ho(i, t);
  }
  /** @see https://tc39.es/ecma262/#sec-%typedarray%.prototype.tolocalestring */
  toLocaleString(...t) {
    M(this);
    const e = C(this), i = Vi(e);
    return go(i, ...$e(t));
  }
  /** @see https://tc39.es/ecma262/#sec-get-%typedarray%.prototype-@@tostringtag */
  get [li]() {
    if (Yt(this))
      return (
        /** @type {any} */
        "Float16Array"
      );
  }
}
de(D, "BYTES_PER_ELEMENT", {
  value: $i
});
de(D, Pe, {});
Tr(D, di);
const Se = D.prototype;
de(Se, "BYTES_PER_ELEMENT", {
  value: $i
});
de(Se, wt, {
  value: Se.values,
  writable: !0,
  configurable: !0
});
Tr(Se, it);
const ji = /* @__PURE__ */ new WeakMap();
function jo(r, t) {
  let e = ji.get(r);
  e || (e = /* @__PURE__ */ new Map(), ji.set(r, e));
  let i = e.get(t);
  if (!i) {
    const n = r.createShaderModule({
      label: `oidn/webnn/input-pack/${t}`,
      code: Qo(t)
    }), o = r.createShaderModule({
      label: "oidn/webnn/output-unpack",
      code: Zo()
    });
    i = {
      input: r.createComputePipeline({
        label: `oidn/webnn/input-pack/${t}`,
        layout: "auto",
        compute: { module: n, entryPoint: "main" }
      }),
      output: r.createComputePipeline({
        label: "oidn/webnn/output-unpack",
        layout: "auto",
        compute: { module: o, entryPoint: "main" }
      })
    }, e.set(t, i);
  }
  return i;
}
const Ct = 8;
function Gr(r, t) {
  return Math.ceil(r / t) * t;
}
function Ko(r, t, e, i) {
  const n = r.createBuffer({
    label: t,
    size: Gr(e.byteLength, 4),
    usage: i,
    mappedAtCreation: !0
  });
  return new Uint8Array(n.getMappedRange()).set(
    new Uint8Array(e.buffer, e.byteOffset, e.byteLength)
  ), n.unmap(), n;
}
function Ki(r, t, e) {
  const i = new Uint32Array(Gr(e.length, 4));
  return i.set(e), Ko(r, t, i, GPUBufferUsage.UNIFORM);
}
function Fe(r, t, e, i) {
  var n, o, a, s;
  return !!((s = (a = (o = (n = r == null ? void 0 : r[t]) == null ? void 0 : n[e]) == null ? void 0 : o.dataTypes) == null ? void 0 : a.includes) != null && s.call(a, i));
}
function qo(r, t) {
  if (t === "fp16" && r.desc.dataType === "Float16")
    return new Uint8Array(
      r.data.buffer,
      r.data.byteOffset,
      r.data.byteLength
    );
  if (t === "fp32" && r.desc.dataType === "Float32")
    return new Uint8Array(
      r.data.buffer,
      r.data.byteOffset,
      r.data.byteLength
    );
  const e = r.desc.dataType === "Float32" ? new Float32Array(
    r.data.buffer,
    r.data.byteOffset,
    r.data.byteLength / 4
  ) : new D(
    r.data.buffer,
    r.data.byteOffset,
    r.data.byteLength / 2
  ), i = t === "fp16" ? new D(e) : new Float32Array(e);
  return new Uint8Array(
    i.buffer,
    i.byteOffset,
    i.byteLength
  );
}
function Qo(r) {
  const t = Array.from(
    { length: r },
    (i, n) => `@group(0) @binding(${n}) var<storage, read> input${n}: array<vec4<f32>>;`
  ).join(`
`), e = Array.from({ length: r }, (i, n) => {
    const o = n * 3;
    return `if (channel < ${o + 3}u) {
      return input${n}[pixel][channel - ${o}u];
    }`;
  }).join(`
  `);
  return (
    /* wgsl */
    `enable f16;
struct Params { width: u32, height: u32, channels: u32, padding: u32 }
${t}
@group(0) @binding(${r}) var<storage, read_write> outputData: array<f16>;
@group(0) @binding(${r + 1}) var<uniform> params: Params;

fn readChannel(pixel: u32, channel: u32) -> f32 {
  ${e}
  return 0.0;
}

@compute @workgroup_size(${Ct}, ${Ct}, 1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
  if (gid.x >= params.width || gid.y >= params.height || gid.z >= params.channels) {
    return;
  }
  let pixel = gid.y * params.width + gid.x;
  let outputIndex = (gid.z * params.height + gid.y) * params.width + gid.x;
  outputData[outputIndex] = f16(readChannel(pixel, gid.z));
}
`
  );
}
function Zo() {
  return (
    /* wgsl */
    `enable f16;
struct Params { width: u32, height: u32, padding0: u32, padding1: u32 }
@group(0) @binding(0) var<storage, read> inputData: array<f16>;
@group(0) @binding(1) var<storage, read_write> outputData: array<vec4<f32>>;
@group(0) @binding(2) var<uniform> params: Params;

@compute @workgroup_size(${Ct}, ${Ct}, 1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
  if (gid.x >= params.width || gid.y >= params.height) { return; }
  let pixel = gid.y * params.width + gid.x;
  let plane = params.width * params.height;
  outputData[pixel] = vec4<f32>(
    f32(inputData[pixel]),
    f32(inputData[plane + pixel]),
    f32(inputData[plane * 2u + pixel]),
    0.0
  );
}
`
  );
}
function Jo(r, t) {
  if (t === "fp32")
    throw new Error(
      "OIDN WebNN GPU interop currently requires FP16 exportable tensors"
    );
  if (!r.features.has("shader-f16"))
    throw new Error("OIDN WebNN requires shader-f16 on the shared GPUDevice");
  return "fp16";
}
function ta(r, t, e) {
  return e === "relu" ? r.relu(t) : t;
}
class ea {
  constructor(t, e, i = {}) {
    y(this, "precision");
    y(this, "support");
    y(this, "_context");
    y(this, "_builderConstructor");
    y(this, "_shapeCache", /* @__PURE__ */ new Map());
    y(this, "_shapePromises", /* @__PURE__ */ new Map());
    y(this, "_retiredExecutions", /* @__PURE__ */ new Set());
    y(this, "_pendingCreationCount", 0);
    y(this, "_shapeCacheSize");
    y(this, "_clock", 0);
    y(this, "_inputPipeline");
    y(this, "_outputPipeline");
    y(this, "_resources", new dr());
    y(this, "_disposed", !1);
    this._device = t, this._model = e, this.precision = Jo(
      t,
      i.precision ?? "auto"
    ), this._shapeCacheSize = Math.max(1, i.shapeCacheSize ?? 2), this.support = {
      available: !1,
      fp16Conv: !1,
      gpuInterop: !1
    };
    const n = jo(
      t,
      e.inputChannels / 3
    );
    this._inputPipeline = n.input, this._outputPipeline = n.output;
  }
  async prepare() {
    var i, n, o;
    if (this._disposed) throw new Error("OIDN WebNN executor is disposed");
    const t = (i = globalThis.navigator) == null ? void 0 : i.ml, e = globalThis.MLGraphBuilder;
    if (!(t != null && t.createContext) || typeof e != "function")
      throw this.support.reason = "WebNN is not exposed by this browser", new Error(this.support.reason);
    this._builderConstructor = e;
    try {
      try {
        this._context = await t.createContext({
          deviceType: "gpu",
          powerPreference: "high-performance"
        });
      } catch {
        this._context = await t.createContext({ deviceType: "gpu" });
      }
      if (this._resources.track("ml-context", this._context), this._disposed) throw new Error("OIDN WebNN executor is disposed");
      if (typeof this._context.createExportableTensor != "function" || typeof this._context.exportToGPU != "function")
        throw this.support.reason = "WebNN WebGPU tensor interop is unavailable", new Error(this.support.reason);
      const a = ((o = (n = this._context).opSupportLimits) == null ? void 0 : o.call(n)) ?? {};
      if (this.support.fp16Conv = Fe(a, "conv2d", "input", "float16") && Fe(a, "conv2d", "filter", "float16") && Fe(a, "conv2d", "output", "float16"), !this.support.fp16Conv)
        throw this.support.reason = "WebNN does not support FP16 conv2d", new Error(this.support.reason);
      let s, u;
      try {
        s = this._resources.track(
          "ml-tensor",
          await this._context.createExportableTensor(
            { dataType: "float16", shape: [4] },
            this._device
          )
        ), u = this._resources.track(
          "gpu-buffer",
          await this._context.exportToGPU(s)
        ), this.support.gpuInterop = !0;
      } catch (l) {
        throw this.support.reason = `WebNN FP16 WebGPU interop failed: ${String(l)}`, new Error(this.support.reason);
      } finally {
        this._releaseBuffer(u), this._releaseTensor(s);
      }
      this.support.available = !0;
    } catch (a) {
      throw this._releaseContext(), a;
    }
  }
  _constant(t, e) {
    return t.constant(
      {
        dataType: "float16",
        shape: [...e.desc.dims]
      },
      qo(e, this.precision)
    );
  }
  async _createExecution(t, e) {
    if (this._disposed) throw new Error("OIDN WebNN executor is disposed");
    const i = new this._builderConstructor(this._context), n = /* @__PURE__ */ new Map(), o = /* @__PURE__ */ new Map();
    n.set(
      this._model.spec.input,
      i.input("input", {
        dataType: "float16",
        shape: [1, this._model.inputChannels, e, t]
      })
    ), o.set(this._model.spec.input, [this._model.inputChannels, e, t]);
    for (const d of this._model.spec.nodes) {
      let h, g;
      if (d.op === "conv2d") {
        const f = o.get(d.input), _ = this._model.convTensors.get(d.id), m = i.conv2d(
          n.get(d.input),
          this._constant(i, _.weight),
          {
            bias: this._constant(i, _.bias),
            padding: [1, 1, 1, 1],
            inputLayout: "nchw",
            filterLayout: "oihw"
          }
        );
        h = ta(i, m, d.activation), g = [_.outputChannels, f[1], f[2]];
      } else if (d.op === "maxPool2d") {
        const f = o.get(d.input);
        h = i.maxPool2d(n.get(d.input), {
          windowDimensions: [2, 2],
          strides: [2, 2],
          padding: [0, f[1] % 2, 0, f[2] % 2],
          layout: "nchw"
        }), g = [
          f[0],
          Math.ceil(f[1] / 2),
          Math.ceil(f[2] / 2)
        ];
      } else if (d.op === "upsample2d") {
        const f = o.get(d.input);
        h = i.resample2d(n.get(d.input), {
          mode: "nearest-neighbor",
          axes: [2, 3],
          scales: [2, 2]
        }), g = [f[0], f[1] * 2, f[2] * 2];
      } else {
        const f = d.inputs.map((_) => o.get(_));
        if (f.some(
          (_) => _[1] !== f[0][1] || _[2] !== f[0][2]
        ))
          throw new Error(
            `WebNN concat ${d.id} has mismatched spatial shapes`
          );
        h = i.concat(
          d.inputs.map((_) => n.get(_)),
          1
        ), g = [
          f.reduce((_, m) => _ + m[0], 0),
          f[0][1],
          f[0][2]
        ];
      }
      n.set(d.id, h), o.set(d.id, g);
    }
    let a, s, u, l, p, c;
    try {
      return a = this._resources.track(
        "ml-graph",
        await i.build({
          output: n.get(this._model.spec.output)
        })
      ), s = this._resources.track(
        "ml-tensor",
        await this._context.createExportableTensor(
          {
            dataType: "float16",
            shape: [1, this._model.inputChannels, e, t],
            writable: !0
          },
          this._device
        )
      ), u = this._resources.track(
        "ml-tensor",
        await this._context.createExportableTensor(
          {
            dataType: "float16",
            shape: [1, this._model.outputChannels, e, t],
            readable: !0
          },
          this._device
        )
      ), l = this._resources.track(
        "gpu-buffer",
        this._device.createBuffer({
          label: `oidn/webnn/output/${t}x${e}`,
          size: t * e * 4 * 4,
          usage: GPUBufferUsage.STORAGE | GPUBufferUsage.COPY_SRC | GPUBufferUsage.COPY_DST
        })
      ), p = this._resources.track(
        "gpu-buffer",
        Ki(
          this._device,
          `oidn/webnn/input/${t}x${e}`,
          [t, e, this._model.inputChannels]
        )
      ), c = this._resources.track(
        "gpu-buffer",
        Ki(
          this._device,
          `oidn/webnn/output/${t}x${e}`,
          [t, e]
        )
      ), {
        graph: a,
        inputTensor: s,
        outputTensor: u,
        outputBuffer: l,
        inputUniform: p,
        outputUniform: c,
        width: t,
        height: e,
        lastUsed: ++this._clock
      };
    } catch (d) {
      throw this._releaseBuffer(c), this._releaseBuffer(p), this._releaseBuffer(l), this._releaseTensor(u), this._releaseTensor(s), this._releaseGraph(a), d;
    }
  }
  async _execution(t, e) {
    const i = `${t}x${e}`;
    let n = this._shapeCache.get(i);
    if (!n) {
      let o = this._shapePromises.get(i);
      o || (o = (async () => {
        this._pendingCreationCount++;
        try {
          return await this._createExecution(t, e);
        } finally {
          this._pendingCreationCount--;
        }
      })(), this._shapePromises.set(i, o));
      try {
        if (n = await o, this._disposed)
          throw this._destroyExecution(n), new Error("OIDN WebNN executor is disposed");
        this._shapeCache.set(i, n);
      } finally {
        this._shapePromises.get(i) === o && this._shapePromises.delete(i);
      }
      if (this._shapeCache.size > this._shapeCacheSize) {
        const a = [...this._shapeCache.entries()].filter(([s]) => s !== i).sort((s, u) => s[1].lastUsed - u[1].lastUsed)[0];
        a && (this._shapeCache.delete(a[0]), this._retireExecution(a[1]));
      }
    }
    return n.lastUsed = ++this._clock, n;
  }
  /** Compiles common tile shapes while the host still reports model loading. */
  async prewarm(t) {
    for (const e of t)
      await this._execution(e.width, e.height);
  }
  async execute(t, e, i) {
    const n = this._model.inputChannels / 3;
    if (t.length !== n)
      throw new Error(
        `OIDN WebNN expected ${n} input buffers, got ${t.length}`
      );
    const o = await this._execution(e, i), a = this._resources.track(
      "gpu-buffer",
      await this._context.exportToGPU(o.inputTensor)
    );
    try {
      const u = t.map(
        (d, h) => ({ binding: h, resource: { buffer: d } })
      );
      u.push({
        binding: n,
        resource: { buffer: a }
      }), u.push({
        binding: n + 1,
        resource: { buffer: o.inputUniform }
      });
      const l = this._device.createBindGroup({
        label: "oidn/webnn/input-bindings",
        layout: this._inputPipeline.getBindGroupLayout(0),
        entries: u
      }), p = this._device.createCommandEncoder({
        label: "oidn/webnn/input-pack"
      }), c = p.beginComputePass();
      c.setPipeline(this._inputPipeline), c.setBindGroup(0, l), c.dispatchWorkgroups(
        Math.ceil(e / Ct),
        Math.ceil(i / Ct),
        this._model.inputChannels
      ), c.end(), this._device.queue.submit([p.finish()]);
    } finally {
      this._releaseBuffer(a);
    }
    this._context.dispatch(
      o.graph,
      { input: o.inputTensor },
      { output: o.outputTensor }
    );
    const s = this._resources.track(
      "gpu-buffer",
      await this._context.exportToGPU(o.outputTensor)
    );
    try {
      const u = this._device.createBindGroup({
        label: "oidn/webnn/output-bindings",
        layout: this._outputPipeline.getBindGroupLayout(0),
        entries: [
          { binding: 0, resource: { buffer: s } },
          { binding: 1, resource: { buffer: o.outputBuffer } },
          { binding: 2, resource: { buffer: o.outputUniform } }
        ]
      }), l = this._device.createCommandEncoder({
        label: "oidn/webnn/output-unpack"
      }), p = l.beginComputePass();
      p.setPipeline(this._outputPipeline), p.setBindGroup(0, u), p.dispatchWorkgroups(
        Math.ceil(e / Ct),
        Math.ceil(i / Ct)
      ), p.end(), this._device.queue.submit([l.finish()]);
    } finally {
      this._releaseBuffer(s);
    }
    return o.outputBuffer;
  }
  async executeCPU(t, e, i) {
    const n = await this._execution(e, i), o = e * i, a = new D(o * this._model.inputChannels);
    for (let l = 0; l < o; l++)
      for (let p = 0; p < this._model.inputChannels; p++)
        a[p * o + l] = t[l * this._model.inputChannels + p];
    this._context.writeTensor(n.inputTensor, a), this._context.dispatch(
      n.graph,
      { input: n.inputTensor },
      { output: n.outputTensor }
    );
    const s = new D(
      await this._context.readTensor(n.outputTensor)
    ), u = new Float32Array(o * this._model.outputChannels);
    for (let l = 0; l < o; l++)
      for (let p = 0; p < this._model.outputChannels; p++)
        u[l * this._model.outputChannels + p] = s[p * o + l];
    return u;
  }
  _destroyExecution(t) {
    this._releaseGraph(t.graph), this._releaseTensor(t.inputTensor), this._releaseTensor(t.outputTensor), this._releaseBuffer(t.outputBuffer), this._releaseBuffer(t.inputUniform), this._releaseBuffer(t.outputUniform);
  }
  _retireExecution(t) {
    this._retiredExecutions.add(t), this._device.queue.onSubmittedWorkDone().catch(() => {
    }).then(() => {
      this._retiredExecutions.delete(t), this._destroyExecution(t);
    });
  }
  _releaseBuffer(t) {
    this._resources.release("gpu-buffer", t, () => t.destroy());
  }
  _releaseTensor(t) {
    this._resources.release("ml-tensor", t, () => t.destroy());
  }
  _releaseGraph(t) {
    this._resources.release("ml-graph", t, () => {
      var e;
      return (e = t.destroy) == null ? void 0 : e.call(t);
    });
  }
  _releaseContext() {
    this._resources.release(
      "ml-context",
      this._context,
      () => {
        var t, e;
        return (e = (t = this._context).destroy) == null ? void 0 : e.call(t);
      }
    );
  }
  getResourceInfo() {
    return this._resources.snapshot(
      this._pendingCreationCount + this._retiredExecutions.size
    );
  }
  dispose() {
    if (!this._disposed) {
      this._disposed = !0;
      for (const t of this._shapeCache.values())
        this._destroyExecution(t);
      this._shapeCache.clear();
      for (const t of this._retiredExecutions)
        this._destroyExecution(t);
      this._retiredExecutions.clear(), this._shapePromises.clear(), this._releaseContext();
    }
  }
}
const ia = 100;
function At(r, t) {
  return Math.ceil(r / t) * t;
}
function ve(r) {
  return r.data instanceof GPUBuffer || r.data instanceof GPUTexture;
}
class ra {
  constructor(t, e, i = {}) {
    y(this, "_device");
    y(this, "_aux");
    y(this, "_hdr");
    y(this, "_hdrTransfer");
    y(this, "_dataProcessGPU");
    y(this, "_nativeExecutor");
    y(this, "_webNNExecutor");
    y(this, "_modelSpec");
    y(this, "_inputChannels");
    y(this, "_engine");
    y(this, "_dynamicTileController");
    y(this, "_lastExecution");
    y(this, "_profileNextImage", !1);
    y(this, "_lastImageProfile");
    y(this, "_activeExecutionFailures", /* @__PURE__ */ new Set());
    y(this, "_deviceLostObserved", !1);
    y(this, "_deviceLostSettled", !1);
    y(this, "_deviceLostReason");
    this._aux = i.aux || !1, this._hdr = i.hdr || !1, this._hdrTransfer = i.hdrTransfer ?? "pu", this._engine = i.engine ?? "auto";
    const n = i.modelSpec ?? lr(t), o = _n(t, n);
    this._modelSpec = o.spec, this._inputChannels = o.inputChannels;
    const a = this._aux ? 9 : 3;
    if (o.inputChannels !== a)
      throw new Error(
        `OIDN model expects ${o.inputChannels} input channels, but aux=${this._aux} provides ${a}`
      );
    this._dynamicTileController = new gn(
      i.maxTileSize ?? 512,
      i.dynamicTile
    ), this._device = e, this._observeDeviceLoss(), this._engine === "webnn" ? this._webNNExecutor = new ea(
      this._device,
      o,
      { precision: i.precision }
    ) : this._nativeExecutor = new to(
      this._device,
      o,
      {
        precision: i.precision,
        kernel: i.kernel,
        gemm: i.gemm,
        subgroupMatrix: i.subgroupMatrix
      }
    );
  }
  getDevice() {
    return this._device;
  }
  /** Completes backend compilation before first interactive use. */
  async prepare() {
    if (this._webNNExecutor) {
      await this._webNNExecutor.prepare();
      const t = At(
        this._modelSpec.receptiveField / 2,
        Z
      ), e = [
        this._dynamicTileController.tileSize,
        this._dynamicTileController.minTileSize
      ];
      await this._webNNExecutor.prewarm(
        [...new Set(e)].map((i) => ({
          width: i + 2 * t,
          height: i + 2 * t
        }))
      );
      return;
    }
    await this._nativeExecutor.prepare();
  }
  /**
   * Prepares the input shapes selected for an image before its first denoise.
   * Hosts can call this while they still display their model-loading state.
   */
  async prepareForImage(t, e, i = {}) {
    var u, l;
    const n = At(
      this._modelSpec.receptiveField / 2,
      Z
    ), o = i.tileOverlap === void 0 ? n : At(Math.max(0, i.tileOverlap), Z), a = vi(
      t,
      e,
      i.wholeImage ? At(Math.max(t, e), Z) : this._dynamicTileController.tileSize,
      o
    ), s = [...new Map(
      a.tiles.map(({ input: p }) => [
        `${p.width}x${p.height}`,
        { width: p.width, height: p.height }
      ])
    ).values()];
    await ((u = this._webNNExecutor) == null ? void 0 : u.prewarm(s)), (l = this._nativeExecutor) == null || l.prewarm(s);
  }
  getRuntimeInfo() {
    var t;
    return {
      configuredEngine: this._engine,
      gpuEngine: this._webNNExecutor ? "webnn" : "wgsl",
      precision: (this._webNNExecutor ?? this._nativeExecutor).precision,
      kernel: this._nativeExecutor ? {
        configured: this._nativeExecutor.kernelSetting,
        gemm: this._nativeExecutor.gemm,
        maxSpatialInputBlocks: this._nativeExecutor.maxSpatialInputBlocks,
        subgroupsAvailable: this._nativeExecutor.subgroupsAvailable,
        subgroupMatrix: this._nativeExecutor.subgroupMatrix
      } : void 0,
      webnn: (t = this._webNNExecutor) == null ? void 0 : t.support,
      resources: (this._webNNExecutor ?? this._nativeExecutor).getResourceInfo(),
      model: this._modelSpec.id,
      modelFamily: this._modelSpec.family,
      inputChannels: this._inputChannels,
      hdrTransfer: this._hdrTransfer,
      dynamicTile: {
        enabled: this._dynamicTileController.enabled,
        currentTileSize: this._dynamicTileController.tileSize,
        minTileSize: this._dynamicTileController.minTileSize,
        maxTileSize: this._dynamicTileController.maxTileSize,
        targetTileTimeMs: this._dynamicTileController.targetTileTimeMs
      },
      lastExecution: this._lastExecution,
      activeExecutionCount: this._activeExecutionFailures.size
    };
  }
  _observeDeviceLoss() {
    if (this._activeExecutionFailures ?? (this._activeExecutionFailures = /* @__PURE__ */ new Set()), this._deviceLostObserved) return;
    this._deviceLostObserved = !0;
    const t = this._device.lost;
    if (!t) return;
    const e = (i) => {
      if (this._deviceLostSettled) return;
      this._deviceLostSettled = !0, this._deviceLostReason = i;
      const n = [...this._activeExecutionFailures];
      this._activeExecutionFailures.clear();
      for (const o of n) o(i);
    };
    t.then(
      (i) => e(new Error(`WebGPU device lost: ${i.message}`)),
      e
    );
  }
  _registerExecutionFailure(t) {
    return this._observeDeviceLoss(), this._activeExecutionFailures.add(t), this._deviceLostSettled && (this._activeExecutionFailures.delete(t), queueMicrotask(() => t(this._deviceLostReason))), () => this._activeExecutionFailures.delete(t);
  }
  /** Captures network GPU timestamps across every tile of the next image. */
  profileNextExecution() {
    return !this._nativeExecutor || !this._device.features.has("timestamp-query") ? !1 : (this._profileNextImage = !0, !0);
  }
  getLastExecutionProfile() {
    return this._lastImageProfile ? Promise.resolve(this._lastImageProfile) : void 0;
  }
  _processImageData(t, e, i, n) {
    const o = t.data, a = o.length / 4, s = this._aux ? 9 : 3, u = new Float32Array(a * s);
    if (e && !i || i && !e)
      throw new Error("Normal map and albedo map are both required");
    if (e && i && (e.width !== i.width || e.height !== i.height || t.width !== e.width || t.height !== e.height))
      throw new Error("Image size mismatch");
    const l = e == null ? void 0 : e.data, p = i == null ? void 0 : i.data, c = l instanceof Float32Array ? 1 : 255, d = p instanceof Float32Array ? 1 : 255;
    for (let h = 0; h < o.length; h += 4) {
      const g = h / 4 * s;
      for (let f = 0; f < 3; f++)
        n ? u[g + f] = o[h + f] : u[g + f] = o[h + f] / 255, l && (u[g + f + 3] = l[h + f] / c), p && (u[g + f + 6] = p[h + f] / d);
    }
    return u;
  }
  _readTile(t, e, i, n) {
    const o = new Float32Array(
      i.width * i.height * e
    ), a = t.length / (n * e);
    for (let s = 0; s < i.height; s++)
      for (let u = 0; u < i.width; u++) {
        const l = Math.min(n - 1, u + i.x), c = (Math.min(a - 1, s + i.y) * n + l) * e, d = (s * i.width + u) * e;
        for (let h = 0; h < e; h++)
          o[d + h] = t[c + h];
      }
    return o;
  }
  _writeTile(t, e, i, n, o, a) {
    const { data: s, width: u } = t, l = i.x - e.x, p = i.y - e.y;
    for (let c = 0; c < i.height; c++)
      for (let d = 0; d < i.width; d++) {
        const h = ((c + p) * o + d + l) * 3, g = ((c + i.y) * u + (d + i.x)) * 4;
        for (let f = 0; f < 3; f++)
          a ? s[g + f] = n[h + f] : s[g + f] = Math.min(
            Math.max(n[h + f] * 255, 0),
            255
          );
        t.data[g + 3] = a ? 1 : 255;
      }
  }
  async _executeTile(t, e, i, n, o, a, s, u, l) {
    const p = this._aux ? 9 : 3, c = new Ue(
      n.input.x,
      n.input.y,
      n.input.width,
      n.input.height
    ), d = new Ue(
      n.output.x,
      n.output.y,
      n.output.width,
      n.output.height
    ), h = c.width, g = c.height;
    let f, _, m = 1;
    const v = this._device;
    let w = this._dataProcessGPU;
    if (t instanceof Float32Array) {
      let $ = this._readTile(t, p, c, a);
      u && (m = an({
        data: $,
        channels: p
      }), $ = nn({
        data: $,
        channels: p,
        inputScale: m,
        transfer: this._hdrTransfer
      })), _ = await (this._webNNExecutor ?? this._nativeExecutor).executeCPU(
        $,
        h,
        g
      );
    } else {
      w || (w = this._dataProcessGPU = new cn(
        v,
        u,
        this._hdrTransfer
      )), w.setImageSize(a, s), w.setInputTile(c), o && w.copyInputDataToOutput(t.color);
      const { color: $, albedo: b, normal: x } = w.forward(
        t.color,
        this._aux ? t.albedo : void 0,
        this._aux ? t.normal : void 0,
        l
      );
      f = await (this._webNNExecutor ?? this._nativeExecutor).execute(
        this._aux ? [$, b, x] : [$],
        h,
        g
      );
    }
    let T;
    if (t instanceof Float32Array) {
      u && (_ = on({
        data: _,
        channels: 3,
        inputScale: m,
        transfer: this._hdrTransfer
      })), this._writeTile(
        i,
        c,
        d,
        _,
        c.width,
        u
      );
      for (let $ = 0; $ < d.height; $++)
        for (let b = 0; b < d.width; b++) {
          const x = ($ * d.width + b) * 4, B = (($ + d.y) * a + (b + d.x)) * 4;
          for (let k = 0; k < 4; k++)
            e.data[x + k] = i.data[B + k];
        }
    } else
      w.setOutputTile(d, c), T = w.inverse(
        f,
        t.color
      );
    return T;
  }
  tileExecute({
    color: t,
    albedo: e,
    normal: i,
    done: n,
    progress: o,
    denoiseAlpha: a,
    tileOverlap: s,
    wholeImage: u,
    scheduling: l = "event-loop",
    error: p
  }) {
    if (this._aux && (!e || !i))
      throw new Error("Normal map and albedo map are both required");
    if (!this._aux && (e || i))
      throw new Error("Normal map and albedo map are not required");
    const c = t.width, d = t.height, h = this._dynamicTileController.tileSize, g = u ? At(Math.max(c, d), Z) : h, f = At(
      this._modelSpec.receptiveField / 2,
      Z
    ), _ = s === void 0 ? f : At(Math.max(0, s), Z), m = vi(
      c,
      d,
      g,
      _
    ), v = m.tiles.length > 1, w = this._hdr || !1;
    let T;
    ve(t) || (T = this._processImageData(
      t,
      e,
      i,
      w
    ));
    function $(X, K) {
      return w ? {
        data: new Float32Array(X * K * 4),
        width: X,
        height: K
      } : new ImageData(X, K);
    }
    const b = ve(t) ? void 0 : $(c, d);
    let x = "active", B, k, I = () => !1;
    const P = () => typeof performance > "u" ? Date.now() : performance.now(), O = P(), F = [], tt = this._profileNextImage === !0;
    this._profileNextImage = !1;
    const nt = /* @__PURE__ */ new Map(), dt = () => {
      B !== void 0 && (clearTimeout(B), B = void 0), k !== void 0 && typeof cancelAnimationFrame < "u" && (cancelAnimationFrame(k), k = void 0);
    }, q = (X) => {
      console.error("OIDN error callback failed", X);
    }, rt = (X) => {
      if (x === "active")
        if (x = "settled", dt(), I(), p)
          try {
            Promise.resolve(p(X)).catch(q);
          } catch (K) {
            q(K);
          }
        else
          console.error("OIDN execution failed", X);
    }, kt = (X) => {
      if (l === "event-loop" || typeof requestAnimationFrame > "u")
        B = setTimeout(() => {
          B = void 0, X();
        }, 0);
      else {
        const K = () => {
          dt(), X();
        };
        k = requestAnimationFrame(K), B = setTimeout(K, ia);
      }
    }, Ut = async (X) => {
      if (x !== "active")
        return;
      const K = m.tiles[X], St = ve(t) ? void 0 : $(K.output.width, K.output.height), Lt = P();
      tt && this._nativeExecutor.profileNextExecution();
      const Dt = await this._executeTile(
        ve(t) ? {
          color: t.data,
          albedo: e == null ? void 0 : e.data,
          normal: i == null ? void 0 : i.data
        } : T,
        St,
        b,
        K,
        X === 0,
        c,
        d,
        w,
        a
      );
      if (tt) {
        const z = await this._nativeExecutor.getLastExecutionProfile();
        for (const V of (z == null ? void 0 : z.layers) ?? [])
          nt.set(V.id, (nt.get(V.id) ?? 0) + V.durationMs);
      }
      if (x !== "active") return;
      const Rt = b || {
        data: Dt,
        width: c,
        height: d
      };
      if (o && await o(
        Rt,
        // Is undefined if using webgpu buffer
        St,
        new Ue(
          K.output.x,
          K.output.y,
          K.output.width,
          K.output.height
        ),
        X,
        m.tiles.length
      ), x !== "active") return;
      const N = X + 1 < m.tiles.length;
      if (await this._device.queue.onSubmittedWorkDone(), x !== "active") return;
      await (async () => {
        if (F.push(P() - Lt), x === "active")
          if (N)
            kt(() => {
              x === "active" && Ut(X + 1).catch(rt);
            });
          else {
            const z = [...F].sort((Tt, mt) => Tt - mt), V = Math.floor(z.length / 2), Mt = z.length % 2 ? z[V] : (z[V - 1] + z[V]) / 2;
            if (this._lastExecution = {
              width: c,
              height: d,
              tileCount: m.tiles.length,
              tileColumns: m.columns,
              tileRows: m.rows,
              tileOverlap: m.overlap,
              inputPixelCount: m.inputPixelCount,
              inputShapeCount: m.inputShapeCount,
              durationMs: P() - O,
              tileTimeMs: {
                min: z[0],
                median: Mt,
                mean: z.reduce((Tt, mt) => Tt + mt, 0) / z.length,
                max: z[z.length - 1]
              }
            }, v && this._dynamicTileController.observe(F), I(), tt) {
              const Tt = [...nt].map(([mt, Jt]) => ({ id: mt, durationMs: Jt }));
              this._lastImageProfile = { totalMs: Tt.reduce((mt, Jt) => mt + Jt.durationMs, 0), layers: Tt };
            }
            await n(Rt), x === "active" && (x = "settled");
          }
      })();
    };
    return I = this._registerExecutionFailure(rt), Ut(0).catch(rt), () => {
      x === "active" && (x = "aborted", dt(), I());
    };
  }
  dispose() {
    var t, e, i;
    (t = this._dataProcessGPU) == null || t.dispose(), (e = this._nativeExecutor) == null || e.dispose(), (i = this._webNNExecutor) == null || i.dispose();
  }
}
async function na() {
  if (!navigator.gpu) throw new Error("WebGPU is not available");
  const r = {
    powerPreference: "high-performance"
  }, t = await navigator.gpu.requestAdapter(r);
  if (!t) throw new Error("No WebGPU adapter is available");
  const e = {}, i = [];
  t.features.has("timestamp-query") && i.push("timestamp-query"), t.features.has("bgra8unorm-storage") && i.push("bgra8unorm-storage"), t.features.has("shader-f16") && i.push("shader-f16");
  for (const o of ["subgroups", "chromium-experimental-subgroup-matrix"])
    t.features.has(o) && i.push(o);
  e.requiredFeatures = i;
  const n = t.limits;
  return e.requiredLimits = {
    maxComputeWorkgroupStorageSize: n.maxComputeWorkgroupStorageSize,
    maxComputeWorkgroupsPerDimension: n.maxComputeWorkgroupsPerDimension,
    maxStorageBufferBindingSize: n.maxStorageBufferBindingSize,
    maxBufferSize: n.maxBufferSize,
    maxComputeWorkgroupSizeX: n.maxComputeWorkgroupSizeX,
    maxComputeInvocationsPerWorkgroup: n.maxComputeInvocationsPerWorkgroup
  }, t.requestDevice(e);
}
async function oa(r, t, e) {
  const i = (t == null ? void 0 : t.device) ?? await na(), n = Zr(r), o = new ra(n, i, e);
  return await o.prepare(), o;
}
async function pa(r, t, e) {
  return fetch(r).then((i) => i.arrayBuffer()).then((i) => oa(i, t, e));
}
export {
  to as NativeUNetExecutor,
  $n as OIDN_UNET_LARGE_SPEC,
  mn as OIDN_UNET_SMALL_SPEC,
  ra as UNet,
  ea as WebNNUNetExecutor,
  lr as detectUNetModelSpec,
  oa as initUNetFromBuffer,
  pa as initUNetFromURL,
  ai as optimizeModelGraph,
  Zr as parseTZA,
  bn as planModelExecution,
  vi as planTileGrid,
  ca as resolveNativeUNetPrecision,
  la as selectSubgroupMatrixConfig,
  _n as validateUNetModel
};
