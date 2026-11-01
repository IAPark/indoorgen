//! numpy.random.RandomState-compatible MT19937 (legacy seeding), so a seeded run can be
//! diffed against the Python reference.

const N: usize = 624;
const M: usize = 397;

thread_local! {
    pub static TRACE_ON: std::cell::Cell<bool> = std::cell::Cell::new(false);
    static DEPTH: std::cell::Cell<u32> = std::cell::Cell::new(0);
}
thread_local! {
    pub static CUR: std::cell::Cell<(i64, i64)> = std::cell::Cell::new((-1, -1));
}
pub fn set_cur(stage: i64, it: i64) {
    CUR.with(|c| c.set((stage, it)));
}
pub fn set_trace(on: bool) {
    TRACE_ON.with(|t| t.set(on));
}
struct Guard;
impl Drop for Guard {
    fn drop(&mut self) {
        DEPTH.with(|d| d.set(d.get() - 1));
    }
}
fn enter(name: &str, peek: u32, loc: &std::panic::Location) -> Guard {
    let d = DEPTH.with(|d| { let v = d.get(); d.set(v + 1); v });
    if d == 0 && TRACE_ON.with(|t| t.get()) {
        eprintln!("RT   CALL {name} @{}:{} peek_before {peek}", loc.file().rsplit('/').next().unwrap(), loc.line());
    }
    Guard
}

#[derive(Clone)]
pub struct Rng {
    mt: [u32; N],
    pos: usize,
    has_gauss: bool,
    gauss: f64,
}

impl Rng {
    /// `np.random.seed(seed)` for 0 <= seed < 2^32.
    pub fn seeded(seed: u64) -> Self {
        let mut mt = [0u32; N];
        let mut s = (seed & 0xffff_ffff) as u32;
        for (i, slot) in mt.iter_mut().enumerate() {
            *slot = s;
            s = 1812433253u32.wrapping_mul(s ^ (s >> 30)).wrapping_add(i as u32 + 1);
        }
        Rng { mt, pos: N, has_gauss: false, gauss: 0.0 }
    }

    fn regen(&mut self) {
        let mt = &mut self.mt;
        let mag = |y: u32| if y & 1 == 1 { 0x9908b0dfu32 } else { 0 };
        for k in 0..N - M {
            let y = (mt[k] & 0x8000_0000) | (mt[k + 1] & 0x7fff_ffff);
            mt[k] = mt[k + M] ^ (y >> 1) ^ mag(y);
        }
        for k in N - M..N - 1 {
            let y = (mt[k] & 0x8000_0000) | (mt[k + 1] & 0x7fff_ffff);
            mt[k] = mt[k + M - N] ^ (y >> 1) ^ mag(y);
        }
        let y = (mt[N - 1] & 0x8000_0000) | (mt[0] & 0x7fff_ffff);
        mt[N - 1] = mt[M - 1] ^ (y >> 1) ^ mag(y);
        self.pos = 0;
    }

    #[inline]
    pub fn next_u32(&mut self) -> u32 {
        if self.pos >= N {
            self.regen();
        }
        let mut y = self.mt[self.pos];
        self.pos += 1;
        y ^= y >> 11;
        y ^= (y << 7) & 0x9d2c5680;
        y ^= (y << 15) & 0xefc60000;
        y ^= y >> 18;
        y
    }

    /// next raw 32-bit word without consuming it (tempers the next state word in place; the 2.5 KB state is cloned only when a
    /// refill is due -- every f64 / uniform call peeks for the trace guard, and the clone was most of their cost)
    #[inline]
    pub fn peek(&self) -> u32 {
        if self.pos >= N { return self.clone().next_u32() }
        let mut y = self.mt[self.pos];
        y ^= y >> 11;
        y ^= (y << 7) & 0x9d2c5680;
        y ^= (y << 15) & 0xefc60000;
        y ^= y >> 18;
        y
    }

    #[inline]
    pub fn next_u64(&mut self) -> u64 {
        ((self.next_u32() as u64) << 32) | self.next_u32() as u64
    }

    /// `np.random.uniform()` / `random_sample()`
    #[inline]
    #[track_caller]
    pub fn f64(&mut self) -> f64 {
        let _g = enter("f64", self.peek(), std::panic::Location::caller());
        let a = (self.next_u32() >> 5) as f64;
        let b = (self.next_u32() >> 6) as f64;
        (a * 67108864.0 + b) / 9007199254740992.0
    }

    #[track_caller]
    pub fn uniform(&mut self, lo: f64, hi: f64) -> f64 {
        let _g = enter("uniform", self.peek(), std::panic::Location::caller());
        lo + (hi - lo) * self.f64()
    }

    /// `np.random.randint(lo, hi)` (masked rejection, int64 path)
    #[track_caller]
    pub fn randint(&mut self, lo: i64, hi: i64) -> i64 {
        let _g = enter("randint", self.peek(), std::panic::Location::caller());
        assert!(hi > lo, "randint: empty range {lo}..{hi}");
        let rng = (hi - 1 - lo) as u64;
        if rng == 0 {
            return lo;
        }
        let mut mask = rng;
        mask |= mask >> 1;
        mask |= mask >> 2;
        mask |= mask >> 4;
        mask |= mask >> 8;
        mask |= mask >> 16;
        mask |= mask >> 32;
        if rng <= 0xffff_ffff {
            loop {
                let v = (self.next_u32() as u64) & mask;
                if v <= rng {
                    return lo + v as i64;
                }
            }
        } else {
            loop {
                let v = self.next_u64() & mask;
                if v <= rng {
                    return lo + v as i64;
                }
            }
        }
    }

    /// `np.random.choice(n)` -> index in 0..n
    #[track_caller]
    pub fn choice_idx(&mut self, n: usize) -> usize {
        let _g = enter("choice_idx", self.peek(), std::panic::Location::caller());
        self.randint(0, n as i64) as usize
    }

    pub fn choice<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.choice_idx(items.len())]
    }

    /// `np.random.choice(n, p=p)`; p is normalised like numpy (cumsum / last).
    #[track_caller]
    pub fn choice_p(&mut self, p: &[f64]) -> usize {
        let _g = enter("choice_p", self.peek(), std::panic::Location::caller());
        let mut cdf = Vec::with_capacity(p.len());
        let mut acc = 0.0;
        for &x in p {
            acc += x;
            cdf.push(acc);
        }
        let last = *cdf.last().unwrap();
        for c in cdf.iter_mut() {
            *c /= last;
        }
        let u = self.f64();
        // searchsorted side='right'
        cdf.partition_point(|&c| c <= u).min(p.len() - 1)
    }

    /// `np.random.randn()` (legacy polar method with cache)
    #[track_caller]
    pub fn randn(&mut self) -> f64 {
        let _g = enter("randn", self.peek(), std::panic::Location::caller());
        if self.has_gauss {
            self.has_gauss = false;
            return self.gauss;
        }
        loop {
            let x1 = 2.0 * self.f64() - 1.0;
            let x2 = 2.0 * self.f64() - 1.0;
            let r2 = x1 * x1 + x2 * x2;
            if r2 < 1.0 && r2 != 0.0 {
                let f = (-2.0 * r2.ln() / r2).sqrt();
                self.gauss = f * x1;
                self.has_gauss = true;
                return f * x2;
            }
        }
    }

    #[track_caller]
    pub fn normal(&mut self, mean: f64, std: f64) -> f64 {
        let _g = enter("normal", self.peek(), std::panic::Location::caller());
        mean + std * self.randn()
    }

    pub fn bool(&mut self, p: f64) -> bool {
        self.f64() < p
    }

    pub fn log_uniform(&mut self, lo: f64, hi: f64) -> f64 {
        self.uniform(lo.ln(), hi.ln()).exp()
    }

    /// `np.random.shuffle` (legacy Fisher-Yates)
    #[track_caller]
    pub fn shuffle<T>(&mut self, v: &mut [T]) {
        let _g = enter("shuffle", self.peek(), std::panic::Location::caller());
        for i in (1..v.len()).rev() {
            let j = self.interval(i as u64) as usize;
            v.swap(i, j);
        }
    }

    fn interval(&mut self, max: u64) -> u64 {
        if max == 0 {
            return 0;
        }
        let mut mask = max;
        mask |= mask >> 1;
        mask |= mask >> 2;
        mask |= mask >> 4;
        mask |= mask >> 8;
        mask |= mask >> 16;
        mask |= mask >> 32;
        if max <= 0xffff_ffff {
            loop {
                let v = (self.next_u32() as u64) & mask;
                if v <= max {
                    return v;
                }
            }
        }
        loop {
            let v = self.next_u64() & mask;
            if v <= max {
                return v;
            }
        }
    }
}
