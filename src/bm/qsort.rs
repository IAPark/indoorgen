//! Port of the BSD / Apple libc `qsort` (Bentley-McIlroy, median-of-3), so that the order of equal keys matches what
//! Blender gets from the system `qsort` on macOS.
fn insertion<T>(a: &mut [T], cmp: &dyn Fn(&T, &T) -> i32) {
    for pm in 1..a.len() {
        let mut pl = pm;
        while pl > 0 && cmp(&a[pl - 1], &a[pl]) > 0 {
            a.swap(pl, pl - 1);
            pl -= 1;
        }
    }
}
fn med3<T>(a: &[T], x: usize, y: usize, z: usize, cmp: &dyn Fn(&T, &T) -> i32) -> usize {
    if cmp(&a[x], &a[y]) < 0 {
        if cmp(&a[y], &a[z]) < 0 {
            y
        } else if cmp(&a[x], &a[z]) < 0 {
            z
        } else {
            x
        }
    } else if cmp(&a[y], &a[z]) > 0 {
        y
    } else if cmp(&a[x], &a[z]) < 0 {
        x
    } else {
        z
    }
}
fn vecswap<T>(a: &mut [T], i: usize, j: usize, n: usize) {
    for k in 0..n {
        a.swap(i + k, j + k);
    }
}
pub fn qsort<T>(a: &mut [T], cmp: &dyn Fn(&T, &T) -> i32) {
    let mut a = a;
    loop {
        let n = a.len();
        if n < 7 {
            insertion(a, cmp);
            return;
        }
        let mut pm = n / 2;
        if n > 7 {
            let mut pl = 0;
            let mut pn = n - 1;
            if n > 40 {
                let d = n / 8;
                pl = med3(a, pl, pl + d, pl + 2 * d, cmp);
                pm = med3(a, pm - d, pm, pm + d, cmp);
                pn = med3(a, pn - 2 * d, pn - d, pn, cmp);
            }
            pm = med3(a, pl, pm, pn, cmp);
        }
        a.swap(0, pm);
        let mut pa = 1usize;
        let mut pb = 1usize;
        let mut pc = n as isize - 1;
        let mut pd = n as isize - 1;
        let mut swap_cnt = false;
        loop {
            while (pb as isize) <= pc {
                let r = cmp(&a[pb], &a[0]);
                if r > 0 {
                    break;
                }
                if r == 0 {
                    swap_cnt = true;
                    a.swap(pa, pb);
                    pa += 1;
                }
                pb += 1;
            }
            while (pb as isize) <= pc {
                let r = cmp(&a[pc as usize], &a[0]);
                if r < 0 {
                    break;
                }
                if r == 0 {
                    swap_cnt = true;
                    a.swap(pc as usize, pd as usize);
                    pd -= 1;
                }
                pc -= 1;
            }
            if (pb as isize) > pc {
                break;
            }
            a.swap(pb, pc as usize);
            swap_cnt = true;
            pb += 1;
            pc -= 1;
        }
        if !swap_cnt {
            insertion(a, cmp);
            return;
        }
        let pn = n;
        let d1 = pa.min(pb - pa);
        vecswap(a, 0, pb - d1, d1);
        let d1 = ((pd - pc) as usize).min(pn - pd as usize - 1);
        vecswap(a, pb, pn - d1, d1);
        let d1 = pb - pa;
        let d2 = (pd - pc) as usize;
        let (left, right) = (d1, d2);
        let whole = std::mem::take(&mut a);
        let (l, rest) = whole.split_at_mut(left.min(n));
        let rl = rest.len();
        let r = &mut rest[rl - right..];
        if left <= right {
            if left > 1 {
                qsort(l, cmp);
            }
            if right > 1 {
                a = r;
                continue;
            }
        } else {
            if right > 1 {
                qsort(r, cmp);
            }
            if left > 1 {
                a = l;
                continue;
            }
        }
        return;
    }
}

/// libc `qsort` with `BLI_sortutil_cmp_float_reverse` on elements whose first field is an `f32` key (descending order).
/// On macOS this calls the system qsort (the very function Blender uses, so the order of equal keys is identical).
#[cfg(target_os = "macos")]
pub fn qsort_f32_reverse<T>(a: &mut [T]) {
    use std::os::raw::{c_int, c_void};
    extern "C" {
        fn qsort(base: *mut c_void, n: usize, size: usize, cmp: extern "C" fn(*const c_void, *const c_void) -> c_int);
    }
    extern "C" fn cmp(x: *const c_void, y: *const c_void) -> c_int {
        let (x, y) = unsafe { (*(x as *const f32), *(y as *const f32)) };
        if x < y {
            1
        } else if x > y {
            -1
        } else {
            0
        }
    }
    unsafe { qsort(a.as_mut_ptr() as *mut c_void, a.len(), std::mem::size_of::<T>(), cmp) }
}
#[cfg(not(target_os = "macos"))]
pub fn qsort_f32_reverse<T: Copy>(a: &mut [T]) {
    let key = |t: &T| unsafe { *(t as *const T as *const f32) };
    qsort(a, &|x, y| {
        let (x, y) = (key(x), key(y));
        if x < y {
            1
        } else if x > y {
            -1
        } else {
            0
        }
    });
}
