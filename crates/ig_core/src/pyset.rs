//! Emulation of CPython's `set` of small non-negative ints, so that iteration order (which the
//! reference's room-assignment search depends on) matches bit for bit.

const LINEAR_PROBES: usize = 9;
const PERTURB_SHIFT: u32 = 5;
const DUMMY: i64 = -1; // hash == -1 marks a dummy slot (key irrelevant)

#[derive(Clone, Copy)]
struct Entry {
    key: Option<u64>, // None => unused
    hash: i64,        // -1 => dummy
}

#[derive(Clone)]
pub struct PySet {
    table: Vec<Entry>,
    fill: usize,
    used: usize,
}

impl PySet {
    pub fn new() -> Self {
        PySet { table: vec![Entry { key: None, hash: 0 }; 8], fill: 0, used: 0 }
    }
    fn mask(&self) -> usize {
        self.table.len() - 1
    }
    pub fn len(&self) -> usize {
        self.used
    }

    fn insert_clean(table: &mut [Entry], key: u64) {
        let mask = table.len() - 1;
        let mut perturb = key as usize;
        let mut i = key as usize & mask;
        loop {
            let probes = if i + LINEAR_PROBES <= mask { LINEAR_PROBES } else { 0 };
            for j in 0..=probes {
                let e = &mut table[i + j];
                if e.key.is_none() {
                    e.key = Some(key);
                    e.hash = key as i64;
                    return;
                }
            }
            perturb >>= PERTURB_SHIFT;
            i = (i * 5 + 1 + perturb) & mask;
        }
    }

    fn resize(&mut self, minused: usize) {
        let mut newsize = 8;
        while newsize <= minused {
            newsize <<= 1;
        }
        let old = std::mem::replace(&mut self.table, vec![Entry { key: None, hash: 0 }; newsize]);
        for e in old {
            if let Some(k) = e.key {
                if e.hash != DUMMY {
                    Self::insert_clean(&mut self.table, k);
                }
            }
        }
        self.fill = self.used;
    }

    pub fn contains(&self, key: u64) -> bool {
        self.find(key).is_some()
    }

    fn find(&self, key: u64) -> Option<usize> {
        let mask = self.mask();
        let mut perturb = key as usize;
        let mut i = key as usize & mask;
        loop {
            let probes = if i + LINEAR_PROBES <= mask { LINEAR_PROBES } else { 0 };
            for j in 0..=probes {
                let e = &self.table[i + j];
                if e.key.is_none() {
                    return None;
                }
                if e.hash == key as i64 && e.key == Some(key) {
                    return Some(i + j);
                }
            }
            perturb >>= PERTURB_SHIFT;
            i = (i * 5 + 1 + perturb) & mask;
        }
    }

    pub fn add(&mut self, key: u64) {
        let mask = self.mask();
        let mut perturb = key as usize;
        let mut i = key as usize & mask;
        let mut freeslot: Option<usize> = None;
        loop {
            let probes = if i + LINEAR_PROBES <= mask { LINEAR_PROBES } else { 0 };
            for j in 0..=probes {
                let idx = i + j;
                let e = self.table[idx];
                if e.key.is_none() {
                    // found unused (or a dummy seen earlier)
                    match freeslot {
                        None => {
                            self.table[idx] = Entry { key: Some(key), hash: key as i64 };
                            self.fill += 1;
                            self.used += 1;
                            if self.fill * 5 >= mask * 3 {
                                let n = self.used * 4;
                                self.resize(n);
                            }
                        }
                        Some(f) => {
                            self.table[f] = Entry { key: Some(key), hash: key as i64 };
                            self.used += 1;
                        }
                    }
                    return;
                }
                if e.hash == key as i64 && e.key == Some(key) {
                    return;
                }
                if e.hash == DUMMY && freeslot.is_none() {
                    freeslot = Some(idx);
                }
            }
            perturb >>= PERTURB_SHIFT;
            i = (i * 5 + 1 + perturb) & mask;
        }
    }

    pub fn remove(&mut self, key: u64) {
        if let Some(i) = self.find(key) {
            self.table[i] = Entry { key: Some(u64::MAX), hash: DUMMY };
            self.used -= 1;
        }
    }

    /// `set(iterable)`: incremental adds
    pub fn from_iter(it: impl IntoIterator<Item = u64>) -> Self {
        let mut s = PySet::new();
        for k in it {
            s.add(k);
        }
        s
    }

    /// iteration order
    pub fn iter(&self) -> Vec<u64> {
        self.table.iter().filter(|e| e.hash != DUMMY).filter_map(|e| e.key).collect()
    }

    /// `set.copy()` (set_merge into an empty set)
    pub fn copy(&self) -> Self {
        let mut s = PySet::new();
        if self.used == 0 {
            return s;
        }
        if (s.fill + self.used) * 5 >= s.mask() * 3 {
            s.resize((s.used + self.used) * 2);
        }
        if s.fill == 0 && s.mask() == self.mask() && self.fill == self.used {
            s.table = self.table.clone();
            s.fill = self.fill;
            s.used = self.used;
            return s;
        }
        for k in self.iter() {
            Self::insert_clean(&mut s.table, k);
        }
        s.fill = self.used;
        s.used = self.used;
        s
    }

    /// `self.intersection(list)`: iterate the list in order, add those contained
    pub fn intersection_list(&self, list: &[u64]) -> Self {
        let mut r = PySet::new();
        for &k in list {
            if self.contains(k) {
                r.add(k);
            }
        }
        r
    }
}
