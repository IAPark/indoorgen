//! Socket value types (float32 semantics like Blender) and conversions.
use std::rc::Rc;

pub type Res<T> = Result<T, String>;
pub type V3 = [f32; 3];

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Ty {
    F,
    I,
    B,
    V,
    C,
    Q,
    S,
    Ref,
    Geo,
    M,
}

impl Ty {
    pub fn from_name(s: &str) -> Ty {
        match s {
            "FLOAT" | "VALUE" => Ty::F,
            "INT" => Ty::I,
            "BOOLEAN" => Ty::B,
            "VECTOR" => Ty::V,
            "RGBA" => Ty::C,
            "ROTATION" => Ty::Q,
            "STRING" => Ty::S,
            "GEOMETRY" => Ty::Geo,
            "MATRIX" => Ty::M,
            _ => Ty::Ref,
        }
    }
    pub fn from_data_type(s: &str) -> Ty {
        match s {
            "FLOAT" => Ty::F,
            "INT" | "INT32" => Ty::I,
            "BOOLEAN" => Ty::B,
            "FLOAT_VECTOR" | "VECTOR" | "FLOAT3" => Ty::V,
            "FLOAT_COLOR" | "RGBA" | "BYTE_COLOR" => Ty::C,
            "QUATERNION" | "ROTATION" => Ty::Q,
            "FLOAT4X4" => Ty::M,
            _ => Ty::F,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Val {
    F(f32),
    I(i32),
    B(bool),
    V(V3),
    C([f32; 4]),
    /// rotation as quaternion (w, x, y, z)
    Q([f32; 4]),
    S(Rc<str>),
    /// object / material / collection reference by name
    Ref(Rc<str>),
    M(Box<[[f32; 4]; 4]>),
}

impl Val {
    pub fn ty(&self) -> Ty {
        match self {
            Val::F(_) => Ty::F,
            Val::I(_) => Ty::I,
            Val::B(_) => Ty::B,
            Val::V(_) => Ty::V,
            Val::C(_) => Ty::C,
            Val::Q(_) => Ty::Q,
            Val::S(_) => Ty::S,
            Val::Ref(_) => Ty::Ref,
            Val::M(_) => Ty::M,
        }
    }
    pub fn zero(t: Ty) -> Val {
        match t {
            Ty::F => Val::F(0.0),
            Ty::I => Val::I(0),
            Ty::B => Val::B(false),
            Ty::V => Val::V([0.0; 3]),
            Ty::C => Val::C([0.0, 0.0, 0.0, 1.0]),
            Ty::Q => Val::Q([1.0, 0.0, 0.0, 0.0]),
            Ty::S => Val::S("".into()),
            Ty::M => Val::M(Box::new([[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]])),
            _ => Val::Ref("".into()),
        }
    }
    pub fn f(&self) -> f32 {
        match self {
            Val::F(x) => *x,
            Val::I(x) => *x as f32,
            Val::B(x) => *x as i32 as f32,
            Val::V(v) => (v[0] + v[1] + v[2]) / 3.0,
            Val::C(c) => rgb_to_grayscale(c),
            _ => 0.0,
        }
    }
    pub fn i(&self) -> i32 {
        match self {
            Val::I(x) => *x,
            Val::F(x) => float_to_int(*x),
            Val::B(x) => *x as i32,
            Val::V(_) | Val::C(_) => float_to_int(self.f()),
            _ => 0,
        }
    }
    pub fn b(&self) -> bool {
        match self {
            Val::B(x) => *x,
            Val::I(x) => *x > 0,
            Val::F(x) => *x > 0.0,
            Val::V(v) => v[0] != 0.0 || v[1] != 0.0 || v[2] != 0.0,
            Val::C(c) => c[0] > 0.0 || c[1] > 0.0 || c[2] > 0.0,
            _ => false,
        }
    }
    pub fn m(&self) -> [[f32; 4]; 4] {
        match self {
            Val::M(m) => **m,
            _ => [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]],
        }
    }
    pub fn v(&self) -> V3 {
        match self {
            Val::V(v) => *v,
            Val::F(x) => [*x; 3],
            Val::I(x) => [*x as f32; 3],
            Val::B(x) => [*x as i32 as f32; 3],
            Val::C(c) => [c[0], c[1], c[2]],
            Val::Q(q) => crate::gn::math::quat_to_euler(*q),
            _ => [0.0; 3],
        }
    }
    pub fn c(&self) -> [f32; 4] {
        match self {
            Val::C(c) => *c,
            Val::V(v) => [v[0], v[1], v[2], 1.0],
            Val::F(x) => [*x, *x, *x, 1.0],
            Val::I(x) => [*x as f32, *x as f32, *x as f32, 1.0],
            Val::B(x) => {
                let f = *x as i32 as f32;
                [f, f, f, 1.0]
            }
            _ => [0.0, 0.0, 0.0, 1.0],
        }
    }
    pub fn q(&self) -> [f32; 4] {
        match self {
            Val::Q(q) => *q,
            Val::V(v) => crate::gn::math::euler_to_quat(*v),
            _ => [1.0, 0.0, 0.0, 0.0],
        }
    }
    pub fn s(&self) -> Rc<str> {
        match self {
            Val::S(s) | Val::Ref(s) => s.clone(),
            _ => "".into(),
        }
    }
    pub fn convert(&self, t: Ty) -> Val {
        if self.ty() == t {
            return self.clone();
        }
        match t {
            Ty::F => Val::F(self.f()),
            Ty::I => Val::I(self.i()),
            Ty::B => Val::B(self.b()),
            Ty::V => Val::V(self.v()),
            Ty::C => Val::C(self.c()),
            Ty::Q => Val::Q(self.q()),
            _ => self.clone(),
        }
    }
}

/// Blender's float -> int implicit conversion (round to nearest, ties away; saturating)
pub fn float_to_int(x: f32) -> i32 {
    if x.is_nan() {
        return 0;
    }
    x.round().clamp(i32::MIN as f32, i32::MAX as f32) as i32
}

pub fn rgb_to_grayscale(c: &[f32; 4]) -> f32 {
    c[0] * 0.2126 + c[1] * 0.7152 + c[2] * 0.0722
}

static ANON_COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
/// unique name for an anonymous attribute (Capture Attribute, Extrude Top/Side, Duplicate Index ...)
pub fn anon_name(kind: &str) -> String {
    format!("__{}_{}", kind, ANON_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed))
}
