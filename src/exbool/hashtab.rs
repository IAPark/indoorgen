//! Emulation of Blender's `blender::Map` / `blender::Set` (open addressing, python probing, load factor 1/2)
//! so that *iteration order* matches Blender's, plus an emulation of libstdc++'s `std::sort`.

pub static LINEAR_STEPS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
fn linear_steps() -> u64 {
    LINEAR_STEPS.load(std::sync::atomic::Ordering::Relaxed)
}

pub static TOTAL_FACTOR_X10: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(20);
fn total_factor() -> f64 {
    TOTAL_FACTOR_X10.load(std::sync::atomic::Ordering::Relaxed) as f64 / 10.0
}

pub trait BHash {
    fn bhash(&self) -> u64;
}
impl BHash for i32 {
    fn bhash(&self) -> u64 {
        *self as i64 as u64
    }
}
impl BHash for (i32, i32) {
    fn bhash(&self) -> u64 {
        // get_default_hash(a, b) = hash(a) ^ (hash(b) * 33)
        self.0.bhash() ^ self.1.bhash().wrapping_mul(33)
    }
}

enum Slot<K, V> {
    Empty,
    Occupied(K, V),
    Removed,
}

fn pow2_ceil(x: i64) -> i64 {
    if x <= 0 {
        return 0;
    }
    let mut p = 1i64;
    while p < x {
        p <<= 1;
    }
    p
}

pub struct BMap<K, V> {
    slots: Vec<Slot<K, V>>,
    removed: i64,
    occupied_and_removed: i64,
    usable: i64,
    mask: u64,
    /// inline capacity (in slots) of the slot array: 8 when the element is small (<100 bytes), else 0.
    inline_slots: i64,
    pub forced_total: i64,
}

impl<K: BHash + PartialEq, V> BMap<K, V> {
    pub fn new(inline_slots: i64) -> Self {
        let mut slots = Vec::new();
        slots.push(Slot::Empty);
        BMap { slots, removed: 0, occupied_and_removed: 0, usable: 0, mask: 0, inline_slots, forced_total: 0 }
    }
    pub fn len(&self) -> i64 {
        self.occupied_and_removed - self.removed
    }
    fn realloc_and_reinsert(&mut self, min_usable: i64) {
        let mut total = pow2_ceil((min_usable as f64 * total_factor()).ceil() as i64);
        if total < self.inline_slots {
            total = self.inline_slots;
        }
        if self.forced_total > 0 {
            total = self.forced_total;
        }
        let usable = (total as f64 / total_factor()).floor() as i64;
        let new_mask = (total as u64).wrapping_sub(1);
        let mut new_slots: Vec<Slot<K, V>> = Vec::with_capacity(total as usize);
        for _ in 0..total {
            new_slots.push(Slot::Empty);
        }
        let old = std::mem::take(&mut self.slots);
        for s in old {
            if let Slot::Occupied(k, v) = s {
                let h = k.bhash();
                let mut hash = h;
                let mut perturb = h;
                let mut placed = None;
                'probe: loop {
                    for lo in 0..linear_steps() {
                        let idx = (hash.wrapping_add(lo) & new_mask) as usize;
                        if let Slot::Empty = new_slots[idx] {
                            placed = Some(idx);
                            break 'probe;
                        }
                    }
                    perturb >>= 5;
                    hash = hash.wrapping_mul(5).wrapping_add(1).wrapping_add(perturb);
                }
                new_slots[placed.unwrap()] = Slot::Occupied(k, v);
            }
        }
        self.slots = new_slots;
        self.occupied_and_removed -= self.removed;
        self.removed = 0;
        self.usable = usable;
        self.mask = new_mask;
    }
    fn ensure_can_add(&mut self) {
        if self.occupied_and_removed >= self.usable {
            let n = self.len() + 1;
            self.realloc_and_reinsert(n);
        }
    }
    pub fn reserve(&mut self, n: i64) {
        let min_usable = self.len() + n;
        if self.usable < min_usable {
            self.realloc_and_reinsert(min_usable);
        }
    }
    fn find(&self, k: &K) -> Option<usize> {
        let h = k.bhash();
        let mut hash = h;
        let mut perturb = h;
        loop {
            for lo in 0..linear_steps() {
                let idx = (hash.wrapping_add(lo) & self.mask) as usize;
                match &self.slots[idx] {
                    Slot::Empty => return None,
                    Slot::Occupied(kk, _) => {
                        if kk == k {
                            return Some(idx);
                        }
                    }
                    Slot::Removed => {}
                }
            }
            perturb >>= 5;
            hash = hash.wrapping_mul(5).wrapping_add(1).wrapping_add(perturb);
        }
    }
    fn insert_new(&mut self, k: K, v: V) {
        let h = k.bhash();
        let mut hash = h;
        let mut perturb = h;
        loop {
            for lo in 0..linear_steps() {
                let idx = (hash.wrapping_add(lo) & self.mask) as usize;
                if let Slot::Empty = self.slots[idx] {
                    self.slots[idx] = Slot::Occupied(k, v);
                    self.occupied_and_removed += 1;
                    return;
                }
            }
            perturb >>= 5;
            hash = hash.wrapping_mul(5).wrapping_add(1).wrapping_add(perturb);
        }
    }
    /// Map::add (returns false and keeps the old value if present)
    pub fn add(&mut self, k: K, v: V) -> bool {
        self.ensure_can_add();
        if self.find(&k).is_some() {
            return false;
        }
        self.insert_new(k, v);
        true
    }
    pub fn add_new(&mut self, k: K, v: V) {
        self.ensure_can_add();
        self.insert_new(k, v);
    }
    pub fn add_overwrite(&mut self, k: K, v: V) -> bool {
        self.ensure_can_add();
        if let Some(i) = self.find(&k) {
            if let Slot::Occupied(_, vv) = &mut self.slots[i] {
                *vv = v;
            }
            return false;
        }
        self.insert_new(k, v);
        true
    }
    pub fn lookup(&self, k: &K) -> Option<&V> {
        self.find(k).map(|i| match &self.slots[i] {
            Slot::Occupied(_, v) => v,
            _ => unreachable!(),
        })
    }
    pub fn lookup_mut(&mut self, k: &K) -> Option<&mut V> {
        match self.find(k) {
            Some(i) => match &mut self.slots[i] {
                Slot::Occupied(_, v) => Some(v),
                _ => unreachable!(),
            },
            None => None,
        }
    }
    pub fn contains(&self, k: &K) -> bool {
        self.find(k).is_some()
    }
    pub fn remove(&mut self, k: &K) -> bool {
        if let Some(i) = self.find(k) {
            self.slots[i] = Slot::Removed;
            self.removed += 1;
            true
        } else {
            false
        }
    }
    pub fn items(&self) -> impl Iterator<Item = (&K, &V)> {
        self.slots.iter().filter_map(|s| match s {
            Slot::Occupied(k, v) => Some((k, v)),
            _ => None,
        })
    }
}

/// blender::Set<K>
pub struct BSet<K> {
    m: BMap<K, ()>,
}
impl<K: BHash + PartialEq> BSet<K> {
    /// `inline_slots`: 8 for small keys (e.g. int)
    pub fn new(inline_slots: i64) -> Self {
        BSet { m: BMap::new(inline_slots) }
    }
    pub fn int_set() -> Self {
        Self::new(8)
    }
    pub fn len(&self) -> usize {
        self.m.len() as usize
    }
    pub fn is_empty(&self) -> bool {
        self.m.len() == 0
    }
    pub fn add(&mut self, k: K) -> bool {
        self.m.add(k, ())
    }
    pub fn add_new(&mut self, k: K) {
        self.m.add_new(k, ())
    }
    pub fn contains(&self, k: &K) -> bool {
        self.m.contains(k)
    }
    pub fn iter(&self) -> impl Iterator<Item = &K> {
        self.m.items().map(|(k, _)| k)
    }
    pub fn reserve(&mut self, n: i64) {
        self.m.reserve(n)
    }
}
impl<K: BHash + PartialEq> Clone for BSet<K>
where
    K: Clone,
{
    fn clone(&self) -> Self {
        // A copy of a blender Set copies the slot array verbatim.
        let mut slots = Vec::with_capacity(self.m.slots.len());
        for s in &self.m.slots {
            slots.push(match s {
                Slot::Empty => Slot::Empty,
                Slot::Removed => Slot::Removed,
                Slot::Occupied(k, _) => Slot::Occupied(k.clone(), ()),
            });
        }
        BSet {
            m: BMap {
                slots,
                removed: self.m.removed,
                occupied_and_removed: self.m.occupied_and_removed,
                usable: self.m.usable,
                mask: self.m.mask,
                inline_slots: self.m.inline_slots,
                forced_total: self.m.forced_total,
            },
        }
    }
}

// ------------------------------------------------------------------------------------------------
// libstdc++ std::sort
// ------------------------------------------------------------------------------------------------

fn lg(mut n: usize) -> usize {
    let mut k = 0;
    while n > 1 {
        n >>= 1;
        k += 1;
    }
    k
}

fn insertion_sort<T: Clone, F: Fn(&T, &T) -> bool>(a: &mut [T], first: usize, last: usize, less: &F) {
    if first == last {
        return;
    }
    for i in first + 1..last {
        if less(&a[i], &a[first]) {
            let val = a[i].clone();
            let mut j = i;
            while j > first {
                a[j] = a[j - 1].clone();
                j -= 1;
            }
            a[first] = val;
        } else {
            unguarded_linear_insert(a, i, less);
        }
    }
}

fn unguarded_linear_insert<T: Clone, F: Fn(&T, &T) -> bool>(a: &mut [T], last: usize, less: &F) {
    let val = a[last].clone();
    let mut l = last;
    while less(&val, &a[l - 1]) {
        a[l] = a[l - 1].clone();
        l -= 1;
    }
    a[l] = val;
}

fn move_median_to_first<T, F: Fn(&T, &T) -> bool>(a: &mut [T], result: usize, x: usize, y: usize, z: usize, less: &F) {
    if less(&a[x], &a[y]) {
        if less(&a[y], &a[z]) {
            a.swap(result, y);
        } else if less(&a[x], &a[z]) {
            a.swap(result, z);
        } else {
            a.swap(result, x);
        }
    } else if less(&a[x], &a[z]) {
        a.swap(result, x);
    } else if less(&a[y], &a[z]) {
        a.swap(result, z);
    } else {
        a.swap(result, y);
    }
}

fn unguarded_partition<T, F: Fn(&T, &T) -> bool>(a: &mut [T], mut first: usize, mut last: usize, pivot: usize, less: &F) -> usize {
    loop {
        while less(&a[first], &a[pivot]) {
            first += 1;
        }
        last -= 1;
        while less(&a[pivot], &a[last]) {
            last -= 1;
        }
        if !(first < last) {
            return first;
        }
        a.swap(first, last);
        first += 1;
    }
}

fn sift_down<T: Clone, F: Fn(&T, &T) -> bool>(a: &mut [T], first: usize, hole: usize, len: usize, value: T, less: &F) {
    // __adjust_heap
    let top = hole;
    let mut hole = hole;
    let mut second = hole;
    while second < (len.saturating_sub(1)) / 2 {
        second = 2 * (second + 1);
        if less(&a[first + second], &a[first + second - 1]) {
            second -= 1;
        }
        a[first + hole] = a[first + second].clone();
        hole = second;
    }
    if len & 1 == 0 && second == (len - 2) / 2 {
        second = 2 * (second + 1);
        a[first + hole] = a[first + second - 1].clone();
        hole = second - 1;
    }
    // __push_heap
    let mut parent = if hole > 0 { (hole - 1) / 2 } else { 0 };
    while hole > top && less(&a[first + parent], &value) {
        a[first + hole] = a[first + parent].clone();
        hole = parent;
        parent = if hole > 0 { (hole - 1) / 2 } else { 0 };
    }
    a[first + hole] = value;
}

fn make_heap<T: Clone, F: Fn(&T, &T) -> bool>(a: &mut [T], first: usize, last: usize, less: &F) {
    let len = last - first;
    if len < 2 {
        return;
    }
    let mut parent = (len - 2) / 2;
    loop {
        let value = a[first + parent].clone();
        sift_down(a, first, parent, len, value, less);
        if parent == 0 {
            return;
        }
        parent -= 1;
    }
}

fn partial_sort_all<T: Clone, F: Fn(&T, &T) -> bool>(a: &mut [T], first: usize, last: usize, less: &F) {
    // std::__partial_sort(first, last, last): heap_select (make_heap) then sort_heap
    make_heap(a, first, last, less);
    let mut l = last;
    while l - first > 1 {
        l -= 1;
        let value = a[l].clone();
        a[l] = a[first].clone();
        sift_down(a, first, 0, l - first, value, less);
    }
}

fn introsort_loop<T: Clone, F: Fn(&T, &T) -> bool>(a: &mut [T], mut first: usize, mut last: usize, mut depth: usize, less: &F) {
    while last - first > 16 {
        if depth == 0 {
            partial_sort_all(a, first, last, less);
            return;
        }
        depth -= 1;
        let mid = first + (last - first) / 2;
        move_median_to_first(a, first, first + 1, mid, last - 1, less);
        let cut = unguarded_partition(a, first + 1, last, first, less);
        introsort_loop(a, cut, last, depth, less);
        last = cut;
    }
}

/// libstdc++ `std::sort(first, last, less)` (introsort + final insertion sort).
pub fn std_sort<T: Clone, F: Fn(&T, &T) -> bool>(a: &mut [T], less: F) {
    let n = a.len();
    if n < 2 {
        return;
    }
    introsort_loop(a, 0, n, lg(n) * 2, &less);
    if n > 16 {
        insertion_sort(a, 0, 16, &less);
        for i in 16..n {
            unguarded_linear_insert(a, i, &less);
        }
    } else {
        insertion_sort(a, 0, n, &less);
    }
}
