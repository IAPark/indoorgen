//! Loop (corner) custom data for the BMesh port: a flat per-loop-slot array of numeric components with Blender's
//! `CustomData_bmesh_interp` / `copy_block` semantics for the float / int / bool / float3 / color layer types.

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum LKind {
    Float,
    Int,
    Bool,
    Vec3,
    Color,
    /// no interpolation (copied only)
    Other,
}

impl LKind {
    pub fn ncomp(self) -> usize {
        match self {
            LKind::Float | LKind::Int | LKind::Bool | LKind::Other => 1,
            LKind::Vec3 => 3,
            LKind::Color => 4,
        }
    }
    pub fn has_math(self) -> bool {
        self != LKind::Other
    }
    fn default_value(self, comp: usize) -> f64 {
        match self {
            LKind::Color => 1.0,
            _ => {
                let _ = comp;
                0.0
            }
        }
    }
}

#[derive(Default)]
pub struct LoopData {
    /// (kind, component offset)
    pub layers: Vec<(LKind, usize)>,
    pub stride: usize,
    pub data: Vec<f64>,
    defaults: Vec<f64>,
}

impl LoopData {
    pub fn new(kinds: &[LKind]) -> LoopData {
        let mut layers = vec![];
        let mut stride = 0;
        let mut defaults = vec![];
        for &k in kinds {
            layers.push((k, stride));
            for c in 0..k.ncomp() {
                defaults.push(k.default_value(c));
            }
            stride += k.ncomp();
        }
        LoopData { layers, stride, data: vec![], defaults }
    }
    pub fn is_empty(&self) -> bool {
        self.stride == 0
    }
    /// CustomData_has_math
    pub fn has_math(&self) -> bool {
        self.layers.iter().any(|l| l.0.has_math())
    }
    fn ensure(&mut self, slot: usize) {
        let need = (slot + 1) * self.stride;
        if self.data.len() < need {
            self.data.resize(need, 0.0);
        }
    }
    /// CustomData_bmesh_set_default
    pub fn reset(&mut self, slot: usize) {
        if self.stride == 0 {
            return;
        }
        self.ensure(slot);
        let o = slot * self.stride;
        for i in 0..self.stride {
            self.data[o + i] = self.defaults[i];
        }
    }
    pub fn block(&self, slot: usize) -> &[f64] {
        &self.data[slot * self.stride..(slot + 1) * self.stride]
    }
    pub fn set_block(&mut self, slot: usize, vals: &[f64]) {
        if self.stride == 0 {
            return;
        }
        self.ensure(slot);
        let o = slot * self.stride;
        self.data[o..o + self.stride].copy_from_slice(vals);
    }
    /// CustomData_bmesh_copy_block
    pub fn copy(&mut self, src: usize, dst: usize) {
        if self.stride == 0 || src == dst {
            return;
        }
        self.ensure(src.max(dst));
        let s = self.stride;
        let (a, b) = (src * s, dst * s);
        for i in 0..s {
            self.data[b + i] = self.data[a + i];
        }
    }
    /// CustomData_bmesh_interp: `dst` = sum of the `srcs` blocks weighted by `w` (layers without interpolation are untouched)
    pub fn interp(&mut self, srcs: &[usize], w: &[f32], dst: usize) {
        if self.stride == 0 || srcs.is_empty() {
            return;
        }
        self.ensure(dst);
        for &(kind, off) in &self.layers.clone() {
            match kind {
                LKind::Float => {
                    let mut r = 0.0f32;
                    for (i, &s) in srcs.iter().enumerate() {
                        r += (self.data[s * self.stride + off] as f32) * w[i];
                    }
                    self.data[dst * self.stride + off] = r as f64;
                }
                LKind::Int => {
                    let mut r = 0.0f32;
                    for (i, &s) in srcs.iter().enumerate() {
                        r += (self.data[s * self.stride + off] as f32) * w[i];
                    }
                    self.data[dst * self.stride + off] = (r.round()) as f64;
                }
                LKind::Bool => {
                    let mut r = false;
                    for (i, &s) in srcs.iter().enumerate() {
                        r |= self.data[s * self.stride + off] != 0.0 && w[i] > 0.0;
                    }
                    self.data[dst * self.stride + off] = if r { 1.0 } else { 0.0 };
                }
                LKind::Vec3 | LKind::Color => {
                    let n = kind.ncomp();
                    let mut r = [0.0f32; 4];
                    for (i, &s) in srcs.iter().enumerate() {
                        for c in 0..n {
                            r[c] += (self.data[s * self.stride + off + c] as f32) * w[i];
                        }
                    }
                    for c in 0..n {
                        self.data[dst * self.stride + off + c] = r[c] as f64;
                    }
                }
                LKind::Other => {}
            }
        }
    }
}
