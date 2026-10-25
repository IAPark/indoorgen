//! Exact rational helpers (stand-in for GMP's mpq_class).
use num_bigint::{BigInt, BigUint, Sign};
use num_rational::BigRational;
use num_traits::{One, Signed, ToPrimitive, Zero};

pub type Q = BigRational;
pub type Q3 = [Q; 3];
pub type Q2 = [Q; 2];

pub fn q_from_f64(x: f64) -> Q {
    BigRational::from_float(x).expect("finite")
}
pub fn q_int(x: i64) -> Q {
    BigRational::from_integer(BigInt::from(x))
}
pub fn q_zero() -> Q {
    Q::zero()
}
pub fn q_sgn(q: &Q) -> i32 {
    match q.numer().sign() {
        Sign::Plus => 1,
        Sign::Minus => -1,
        Sign::NoSign => 0,
    }
}
pub fn q_is_zero(q: &Q) -> bool {
    q.numer().is_zero()
}
fn ldexp(m: f64, e: i64) -> f64 {
    let mut m = m;
    let mut e = e;
    while e > 1000 {
        m *= 2f64.powi(1000);
        e -= 1000;
    }
    while e < -1000 {
        m *= 2f64.powi(-1000);
        e += 1000;
    }
    m * 2f64.powi(e as i32)
}

/// `mpq_class::get_d()`: GMP truncates toward zero.
pub fn q_get_d(q: &Q) -> f64 {
    if q.numer().is_zero() {
        return 0.0;
    }
    let neg = q.numer().is_negative();
    let n: &BigUint = q.numer().magnitude();
    let d: &BigUint = q.denom().magnitude();
    let nb = n.bits() as i64;
    let db = d.bits() as i64;
    let s = 66 - (nb - db);
    let (num, den): (BigUint, BigUint) = if s >= 0 { (n << (s as usize), d.clone()) } else { (n.clone(), d << ((-s) as usize)) };
    let quo = num / den;
    let qb = quo.bits() as i64;
    let shift = qb - 53;
    let mant: u64 = (&quo >> (shift as usize)).to_u64().unwrap();
    let e = shift - s;
    let r = ldexp(mant as f64, e);
    if neg {
        -r
    } else {
        r
    }
}

pub fn q3_from_f64(p: [f64; 3]) -> Q3 {
    [q_from_f64(p[0]), q_from_f64(p[1]), q_from_f64(p[2])]
}
pub fn q3_sub(a: &Q3, b: &Q3) -> Q3 {
    [&a[0] - &b[0], &a[1] - &b[1], &a[2] - &b[2]]
}
pub fn q3_dot(a: &Q3, b: &Q3) -> Q {
    &a[0] * &b[0] + &a[1] * &b[1] + &a[2] * &b[2]
}
pub fn q3_cross(a: &Q3, b: &Q3) -> Q3 {
    [&a[1] * &b[2] - &a[2] * &b[1], &a[2] * &b[0] - &a[0] * &b[2], &a[0] * &b[1] - &a[1] * &b[0]]
}
pub fn q3_get_d(a: &Q3) -> [f64; 3] {
    [q_get_d(&a[0]), q_get_d(&a[1]), q_get_d(&a[2])]
}

/// orient3d(a,b,c,d): +1 if d is below the plane of a,b,c (CCW seen from above), -1 above, 0 on it.
pub fn q_orient3d(a: &Q3, b: &Q3, c: &Q3, d: &Q3) -> i32 {
    let adx = &a[0] - &d[0];
    let bdx = &b[0] - &d[0];
    let cdx = &c[0] - &d[0];
    let ady = &a[1] - &d[1];
    let bdy = &b[1] - &d[1];
    let cdy = &c[1] - &d[1];
    let adz = &a[2] - &d[2];
    let bdz = &b[2] - &d[2];
    let cdz = &c[2] - &d[2];
    let det = &adx * (&bdy * &cdz - &bdz * &cdy) + &bdx * (&cdy * &adz - &cdz * &ady) + &cdx * (&ady * &bdz - &adz * &bdy);
    q_sgn(&det)
}
pub fn q_orient2d(a: &Q2, b: &Q2, c: &Q2) -> i32 {
    let det = (&a[0] - &c[0]) * (&b[1] - &c[1]) - (&a[1] - &c[1]) * (&b[0] - &c[0]);
    q_sgn(&det)
}
pub fn q_incircle(a: &Q2, b: &Q2, c: &Q2, d: &Q2) -> i32 {
    let adx = &a[0] - &d[0];
    let bdx = &b[0] - &d[0];
    let cdx = &c[0] - &d[0];
    let ady = &a[1] - &d[1];
    let bdy = &b[1] - &d[1];
    let cdy = &c[1] - &d[1];
    let alift = &adx * &adx + &ady * &ady;
    let blift = &bdx * &bdx + &bdy * &bdy;
    let clift = &cdx * &cdx + &cdy * &cdy;
    let det = &alift * (&bdx * &cdy - &cdx * &bdy) + &blift * (&cdx * &ady - &adx * &cdy) + &clift * (&adx * &bdy - &bdx * &ady);
    q_sgn(&det)
}
pub fn q_abs(q: &Q) -> Q {
    q.abs()
}
pub fn q_one() -> Q {
    Q::one()
}
