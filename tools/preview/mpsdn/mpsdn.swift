// Open Image Denoise's network on the GPU through MPSGraph, for a GPU OIDN's own Metal device turns down (a virtual machine's: Apple5, no Metal 3).
// Same weights (the published .tza), same steps: autoexposure, the PU transfer, the U-Net, back again. C entry points, loaded by oidn.rs.
import Foundation
import Metal
import MetalPerformanceShadersGraph

private struct PU {
  static let a: Float = 1.41283765e+03, b: Float = 1.64593172e+00, c: Float = 4.31384981e-01, d: Float = -2.94139609e-03
  static let e: Float = 1.92653254e-01, f: Float = 6.26026094e-03, g: Float = 9.98620152e-01
  static let y0: Float = 1.57945760e-06, y1: Float = 3.22087631e-02, x0: Float = 2.23151711e-03, x1: Float = 3.70974749e-01
  static func forward(_ y: Float) -> Float { y <= y0 ? a * y : (y <= y1 ? b * powf(y, c) + d : e * logf(y + f) + g) }
}

private final class Net {
  let dev: MTLDevice, q: MTLCommandQueue, gdev: MPSGraphDevice
  var w: [String: (dims: [Int], data: [Float])] = [:]
  var built: (w: Int, h: Int, ex: MPSGraphExecutable)?
  init?(path: String) {
    guard let dev = MTLCreateSystemDefaultDevice(), let q = dev.makeCommandQueue(), let b = FileManager.default.contents(atPath: path) else { return nil }
    self.dev = dev; self.q = q; self.gdev = MPSGraphDevice(mtlDevice: dev)
    let rd = { (o: Int, n: Int) -> UInt64 in var v: UInt64 = 0; for i in 0..<n { v |= UInt64(b[o + i]) << (8 * UInt64(i)) }; return v }
    guard b.count > 12, rd(0, 2) == 0x41D7 else { return nil }
    var p = Int(rd(4, 8)); let n = Int(rd(p, 4)); p += 4
    for _ in 0..<n {
      let l = Int(rd(p, 2)); p += 2; let name = String(decoding: b[p..<p + l], as: UTF8.self); p += l
      let nd = Int(b[p]); p += 1; var dims: [Int] = []; for _ in 0..<nd { dims.append(Int(rd(p, 4))); p += 4 }
      p += nd; let dt = b[p]; p += 1; let off = Int(rd(p, 8)); p += 8
      let cnt = dims.reduce(1, *); var data = [Float](repeating: 0, count: cnt)
      if dt == UInt8(ascii: "h") { for i in 0..<cnt { data[i] = Float(Float16(bitPattern: UInt16(rd(off + 2 * i, 2)))) } }
      else { for i in 0..<cnt { data[i] = Float(bitPattern: UInt32(rd(off + 4 * i, 4))) } }
      w[name] = (dims, data)
    }
  }
  func build(_ W: Int, _ H: Int) {
    let g = MPSGraph(); let f16 = MPSDataType.float16
    let PH = (H + 15) / 16 * 16, PW = (W + 15) / 16 * 16
    let col = g.placeholder(shape: [1, H as NSNumber, W as NSNumber, 3], dataType: .float32, name: "c")
    let nrm = g.placeholder(shape: [1, H as NSNumber, W as NSNumber, 3], dataType: .float32, name: "n")
    let sc = g.placeholder(shape: [1], dataType: .float32, name: "s")
    let k = { (v: Double) in g.constant(v, dataType: .float32) }
    let nan0 = { (x: MPSGraphTensor) in g.select(predicate: g.isNaN(with: x, name: nil), trueTensor: k(0), falseTensor: x, name: nil) }
    // colour: scaled, sanitised, the PU curve, normalised
    var c = g.clamp(nan0(g.multiplication(col, sc, name: nil)), min: k(0), max: k(65504), name: nil)
    let lo = g.multiplication(c, k(Double(PU.a)), name: nil)
    let mid = g.addition(g.multiplication(g.power(g.maximum(c, k(1e-30), name: nil), k(Double(PU.c)), name: nil), k(Double(PU.b)), name: nil), k(Double(PU.d)), name: nil)
    let hi = g.addition(g.multiplication(g.logarithm(with: g.addition(c, k(Double(PU.f)), name: nil), name: nil), k(Double(PU.e)), name: nil), k(Double(PU.g)), name: nil)
    c = g.select(predicate: g.lessThanOrEqualTo(c, k(Double(PU.y0)), name: nil), trueTensor: lo, falseTensor: g.select(predicate: g.lessThanOrEqualTo(c, k(Double(PU.y1)), name: nil), trueTensor: mid, falseTensor: hi, name: nil), name: nil)
    let xMax = PU.forward(65504); c = g.multiplication(c, k(Double(1 / xMax)), name: nil)
    let alb = g.constant(0.5, shape: [1, H as NSNumber, W as NSNumber, 3], dataType: .float32) // (flat: the light has no surface colours in it, as oidn.rs gives it)
    let nn = g.addition(g.multiplication(g.clamp(nan0(nrm), min: k(-1), max: k(1), name: nil), k(0.5), name: nil), k(0.5), name: nil)
    var x = g.concatTensors([c, alb, nn], dimension: 3, name: nil)
    x = g.padTensor(x, with: .zero, leftPadding: [0, 0, 0, 0], rightPadding: [0, (PH - H) as NSNumber, (PW - W) as NSNumber, 0], constantValue: 0, name: nil)
    let inp = g.cast(x, to: f16, name: "in")
    let desc = MPSGraphConvolution2DOpDescriptor(strideInX: 1, strideInY: 1, dilationRateInX: 1, dilationRateInY: 1, groups: 1, paddingStyle: .TF_SAME, dataLayout: .NHWC, weightsLayout: .HWIO)!
    let pd = MPSGraphPooling2DOpDescriptor(kernelWidth: 2, kernelHeight: 2, strideInX: 2, strideInY: 2, paddingStyle: .TF_VALID, dataLayout: .NHWC)!
    func conv(_ name: String, _ x: MPSGraphTensor) -> MPSGraphTensor {
      let (d, v) = w[name + ".weight"]!; let (o, i) = (d[0], d[1])
      var hwio = [Float16](repeating: 0, count: 9 * i * o)
      for oo in 0..<o { for ii in 0..<i { for ky in 0..<3 { for kx in 0..<3 { hwio[((ky * 3 + kx) * i + ii) * o + oo] = Float16(v[((oo * i + ii) * 3 + ky) * 3 + kx]) } } } }
      let wt = g.constant(hwio.withUnsafeBufferPointer { Data(buffer: $0) }, shape: [3, 3, i as NSNumber, o as NSNumber], dataType: f16)
      let bv = w[name + ".bias"]!.data.map { Float16($0) }
      let bt = g.constant(bv.withUnsafeBufferPointer { Data(buffer: $0) }, shape: [o as NSNumber], dataType: f16)
      return g.reLU(with: g.addition(g.convolution2D(x, weights: wt, descriptor: desc, name: nil), bt, name: nil), name: nil)
    }
    let pool = { (x: MPSGraphTensor) in g.maxPooling2D(withSourceTensor: x, descriptor: pd, name: nil) }
    let up = { (x: MPSGraphTensor, h: Int, w: Int) in g.resize(x, size: [h as NSNumber, w as NSNumber], mode: .nearest, centerResult: true, alignCorners: false, layout: .NHWC, name: nil) }
    let cat = { (a: MPSGraphTensor, b: MPSGraphTensor) in g.concatTensors([a, b], dimension: 3, name: nil) }
    let large = w["enc_conv1b.weight"] != nil
    var y: MPSGraphTensor; var p1, p2, p3, p4: MPSGraphTensor
    if large {
      y = conv("enc_conv1a", inp); p1 = pool(conv("enc_conv1b", y))
      y = conv("enc_conv2a", p1); p2 = pool(conv("enc_conv2b", y))
      y = conv("enc_conv3a", p2); p3 = pool(conv("enc_conv3b", y))
      y = conv("enc_conv4a", p3); p4 = pool(conv("enc_conv4b", y))
    } else {
      y = conv("enc_conv0", inp); p1 = pool(conv("enc_conv1", y)); p2 = pool(conv("enc_conv2", p1)); p3 = pool(conv("enc_conv3", p2)); p4 = pool(conv("enc_conv4", p3))
    }
    y = up(conv("enc_conv5b", conv("enc_conv5a", p4)), PH / 8, PW / 8)
    y = up(conv("dec_conv4b", conv("dec_conv4a", cat(y, p3))), PH / 4, PW / 4)
    y = up(conv("dec_conv3b", conv("dec_conv3a", cat(y, p2))), PH / 2, PW / 2)
    y = up(conv("dec_conv2b", conv("dec_conv2a", cat(y, p1))), PH, PW)
    y = conv("dec_conv1b", conv("dec_conv1a", cat(y, inp)))
    y = conv("dec_conv0", y)
    // back: the PU curve undone, the exposure taken off
    var o = g.sliceTensor(g.cast(y, to: .float32, name: nil), starts: [0, 0, 0, 0], ends: [1, H as NSNumber, W as NSNumber, 3], strides: [1, 1, 1, 1], name: nil)
    o = g.multiplication(g.clamp(nan0(o), min: k(0), max: k(1e30), name: nil), k(Double(xMax)), name: nil)
    let ilo = g.division(o, k(Double(PU.a)), name: nil)
    let imid = g.power(g.maximum(g.division(g.subtraction(o, k(Double(PU.d)), name: nil), k(Double(PU.b)), name: nil), k(0), name: nil), k(Double(1 / PU.c)), name: nil)
    let ihi = g.subtraction(g.exponent(with: g.division(g.subtraction(o, k(Double(PU.g)), name: nil), k(Double(PU.e)), name: nil), name: nil), k(Double(PU.f)), name: nil)
    o = g.select(predicate: g.lessThanOrEqualTo(o, k(Double(PU.x0)), name: nil), trueTensor: ilo, falseTensor: g.select(predicate: g.lessThanOrEqualTo(o, k(Double(PU.x1)), name: nil), trueTensor: imid, falseTensor: ihi, name: nil), name: nil)
    o = g.division(o, sc, name: nil)
    let sh: [NSNumber] = [1, H as NSNumber, W as NSNumber, 3]
    let ex = g.compile(with: gdev, feeds: [col: MPSGraphShapedType(shape: sh, dataType: .float32), nrm: MPSGraphShapedType(shape: sh, dataType: .float32), sc: MPSGraphShapedType(shape: [1], dataType: .float32)], targetTensors: [o], targetOperations: nil, compilationDescriptor: { let cd = MPSGraphCompilationDescriptor(); cd.optimizationLevel = .level0; return cd }()) // (the default level spent 170-420 ms of a first run specialising kernels; level 0 ~120 ms, later runs as fast)
    built = (W, H, ex)
  }
  func run(_ cp: UnsafePointer<Float>, _ np: UnsafePointer<Float>, _ W: Int, _ H: Int, _ op: UnsafeMutablePointer<Float>) -> Bool {
    if built == nil || built!.w != W || built!.h != H { build(W, H) }
    let b = built!; let n = W * H * 3
    // autoexposure as OIDN's: luminance averaged in 16x16 bins, the log-mean of the bins that are lit, to 0.18
    let nbh = (H + 15) / 16, nbw = (W + 15) / 16; var sum: Double = 0; var cnt = 0
    for by in 0..<nbh { for bx in 0..<nbw {
      let h0 = by * H / nbh, h1 = (by + 1) * H / nbh, w0 = bx * W / nbw, w1 = (bx + 1) * W / nbw; var s: Float = 0
      for y in h0..<h1 { for x in w0..<w1 { let i = (y * W + x) * 3; var r = cp[i], gg = cp[i + 1], bb = cp[i + 2]
        if !r.isFinite || r < 0 { r = r.isNaN || r < 0 ? 0 : r }; if !gg.isFinite || gg < 0 { gg = gg.isNaN || gg < 0 ? 0 : gg }; if !bb.isFinite || bb < 0 { bb = bb.isNaN || bb < 0 ? 0 : bb }
        s += 0.212671 * r + 0.715160 * gg + 0.072169 * bb } }
      let L = s / Float((h1 - h0) * (w1 - w0)); if L > 1e-8 { sum += Double(log2f(L)); cnt += 1 }
    } }
    var scale: Float = cnt > 0 ? 0.18 / exp2f(Float(sum / Double(cnt))) : 1
    let td = { (p: UnsafeRawPointer, bytes: Int, shape: [NSNumber]) in MPSGraphTensorData(device: self.gdev, data: Data(bytes: p, count: bytes), shape: shape, dataType: .float32) }
    let sh: [NSNumber] = [1, H as NSNumber, W as NSNumber, 3]
    let r = b.ex.run(with: q, inputs: [td(cp, n * 4, sh), td(np, n * 4, sh), td(&scale, 4, [1])], results: nil, executionDescriptor: nil)
    guard let od = r.first else { return false }
    od.mpsndarray().readBytes(op, strideBytes: nil)
    return true
  }
}

private var net: Net?
private let lock = NSLock()
@_cdecl("mpsdn_init") public func mpsdn_init(_ path: UnsafePointer<CChar>) -> Int32 { lock.lock(); defer { lock.unlock() }; if net == nil { net = Net(path: String(cString: path)) }; return net == nil ? 0 : 1 }
/// a first run, on a thread of its own, while the scene loads: the first frame then finds the kernels ready
@_cdecl("mpsdn_warm") public func mpsdn_warm(_ w: Int32, _ h: Int32) {
  Thread.detachNewThread {
    let n = Int(w) * Int(h) * 3; let z = [Float](repeating: 0.1, count: n); var o = [Float](repeating: 0, count: n)
    _ = mpsdn_run(z, z, w, h, &o)
  }
}
@_cdecl("mpsdn_run") public func mpsdn_run(_ c: UnsafePointer<Float>, _ n: UnsafePointer<Float>, _ w: Int32, _ h: Int32, _ o: UnsafeMutablePointer<Float>) -> Int32 {
  lock.lock(); defer { lock.unlock() }
  guard let net else { return 0 }; return net.run(c, n, Int(w), Int(h), o) ? 1 : 0
}
