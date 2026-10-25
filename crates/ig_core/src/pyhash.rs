//! CPython str hashing (PYTHONHASHSEED=0 => zero siphash key) and the iteration order of a `set` of such strings.

fn rotl(x: u64, b: u32) -> u64 {
    x.rotate_left(b)
}

pub fn siphash13(data: &[u8]) -> u64 {
    let (k0, k1) = (0u64, 0u64);
    let mut v0 = k0 ^ 0x736f6d6570736575;
    let mut v1 = k1 ^ 0x646f72616e646f6d;
    let mut v2 = k0 ^ 0x6c7967656e657261;
    let mut v3 = k1 ^ 0x7465646279746573;
    let mut round = |v0: &mut u64, v1: &mut u64, v2: &mut u64, v3: &mut u64| {
        *v0 = v0.wrapping_add(*v1);
        *v2 = v2.wrapping_add(*v3);
        *v1 = rotl(*v1, 13);
        *v3 = rotl(*v3, 16);
        *v1 ^= *v0;
        *v3 ^= *v2;
        *v0 = rotl(*v0, 32);
        *v2 = v2.wrapping_add(*v1);
        *v0 = v0.wrapping_add(*v3);
        *v1 = rotl(*v1, 17);
        *v3 = rotl(*v3, 21);
        *v1 ^= *v2;
        *v3 ^= *v0;
        *v2 = rotl(*v2, 32);
    };
    let n = data.len();
    let mut i = 0;
    while i + 8 <= n {
        let m = u64::from_le_bytes(data[i..i + 8].try_into().unwrap());
        v3 ^= m;
        round(&mut v0, &mut v1, &mut v2, &mut v3);
        v0 ^= m;
        i += 8;
    }
    let mut b = (n as u64) << 56;
    for (k, byte) in data[i..].iter().enumerate() {
        b |= (*byte as u64) << (8 * k);
    }
    v3 ^= b;
    round(&mut v0, &mut v1, &mut v2, &mut v3);
    v0 ^= b;
    v2 ^= 0xff;
    for _ in 0..3 {
        round(&mut v0, &mut v1, &mut v2, &mut v3);
    }
    v0 ^ v1 ^ v2 ^ v3
}

pub fn py_str_hash(s: &str) -> i64 {
    if s.is_empty() {
        return 0;
    }
    let h = siphash13(s.as_bytes()) as i64;
    if h == -1 {
        -2
    } else {
        h
    }
}

const LINEAR_PROBES: usize = 9;

fn insert_slot(table: &[Option<(usize, i64)>], hash: i64) -> usize {
    let mask = table.len() - 1;
    let mut perturb = hash as u64 as usize;
    let mut i = (hash as u64 as usize) & mask;
    loop {
        let probes = if i + LINEAR_PROBES <= mask { LINEAR_PROBES } else { 0 };
        for j in 0..=probes {
            if table[i + j].is_none() {
                return i + j;
            }
        }
        perturb >>= 5;
        i = (i * 5 + 1 + perturb) & mask;
    }
}

/// iteration order (indices into `keys`) of `set()` filled by sequential `add`s of the given strings
pub fn str_set_order(keys: &[String]) -> Vec<usize> {
    let mut table: Vec<Option<(usize, i64)>> = vec![None; 8];
    let mut fill = 0usize;
    for (ix, k) in keys.iter().enumerate() {
        let h = py_str_hash(k);
        let s = insert_slot(&table, h);
        table[s] = Some((ix, h));
        fill += 1;
        let mask = table.len() - 1;
        if fill * 5 >= mask * 3 {
            let minused = fill * 4;
            let mut newsize = 8;
            while newsize <= minused {
                newsize <<= 1;
            }
            let old = std::mem::replace(&mut table, vec![None; newsize]);
            for e in old.into_iter().flatten() {
                let s = insert_slot(&table, e.1);
                table[s] = Some(e);
            }
        }
    }
    table.into_iter().flatten().map(|e| e.0).collect()
}
