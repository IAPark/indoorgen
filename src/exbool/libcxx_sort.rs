//! Emulation of libc++'s `std::sort` (LLVM 16/17 introsort without the bitset partition, which is what
//! Apple clang's headers use for a lambda comparator).  Needed wherever Blender's output order depends on the
//! tie-breaking behaviour of an unstable sort (e.g. dissolving edges of equal length, vertices with equal `orig`).

fn sort3<T: Clone, F: Fn(&T, &T) -> bool>(a: &mut [T], x: usize, y: usize, z: usize, c: &F) {
    if !c(&a[y], &a[x]) {
        if !c(&a[z], &a[y]) {
            return;
        }
        a.swap(y, z);
        if c(&a[y], &a[x]) {
            a.swap(x, y);
        }
        return;
    }
    if c(&a[z], &a[y]) {
        a.swap(x, z);
        return;
    }
    a.swap(x, y);
    if c(&a[z], &a[y]) {
        a.swap(y, z);
    }
}

fn sort4<T: Clone, F: Fn(&T, &T) -> bool>(a: &mut [T], x1: usize, x2: usize, x3: usize, x4: usize, c: &F) {
    sort3(a, x1, x2, x3, c);
    if c(&a[x4], &a[x3]) {
        a.swap(x3, x4);
        if c(&a[x3], &a[x2]) {
            a.swap(x2, x3);
            if c(&a[x2], &a[x1]) {
                a.swap(x1, x2);
            }
        }
    }
}

fn sort5<T: Clone, F: Fn(&T, &T) -> bool>(a: &mut [T], x1: usize, x2: usize, x3: usize, x4: usize, x5: usize, c: &F) {
    sort4(a, x1, x2, x3, x4, c);
    if c(&a[x5], &a[x4]) {
        a.swap(x4, x5);
        if c(&a[x4], &a[x3]) {
            a.swap(x3, x4);
            if c(&a[x3], &a[x2]) {
                a.swap(x2, x3);
                if c(&a[x2], &a[x1]) {
                    a.swap(x1, x2);
                }
            }
        }
    }
}

fn insertion_sort<T: Clone, F: Fn(&T, &T) -> bool>(a: &mut [T], first: usize, last: usize, c: &F) {
    if first == last {
        return;
    }
    for i in first + 1..last {
        if c(&a[i], &a[i - 1]) {
            let t = a[i].clone();
            let mut j = i;
            let mut k = i - 1;
            loop {
                a[j] = a[k].clone();
                j = k;
                if j == first {
                    break;
                }
                k -= 1;
                if !c(&t, &a[k]) {
                    break;
                }
            }
            a[j] = t;
        }
    }
}

fn insertion_sort_incomplete<T: Clone, F: Fn(&T, &T) -> bool>(a: &mut [T], first: usize, last: usize, c: &F) -> bool {
    match last - first {
        0 | 1 => return true,
        2 => {
            if c(&a[last - 1], &a[first]) {
                a.swap(first, last - 1);
            }
            return true;
        }
        3 => {
            sort3(a, first, first + 1, last - 1, c);
            return true;
        }
        4 => {
            sort4(a, first, first + 1, first + 2, last - 1, c);
            return true;
        }
        5 => {
            sort5(a, first, first + 1, first + 2, first + 3, last - 1, c);
            return true;
        }
        _ => {}
    }
    let mut j = first + 2;
    sort3(a, first, first + 1, j, c);
    let limit = 8;
    let mut count = 0;
    let mut i = j + 1;
    while i != last {
        if c(&a[i], &a[j]) {
            let t = a[i].clone();
            let mut k = j;
            j = i;
            loop {
                a[j] = a[k].clone();
                j = k;
                if j == first {
                    break;
                }
                k -= 1;
                if !c(&t, &a[k]) {
                    break;
                }
            }
            a[j] = t;
            count += 1;
            if count == limit {
                i += 1;
                return i == last;
            }
        }
        j = i;
        i += 1;
    }
    true
}

/// returns (pivot position, already partitioned)
fn partition_with_equals_on_right<T: Clone, F: Fn(&T, &T) -> bool>(a: &mut [T], first0: usize, last0: usize, c: &F) -> (usize, bool) {
    let begin = first0;
    let mut first = first0;
    let mut last = last0;
    let pivot = a[first].clone();
    loop {
        first += 1;
        if !c(&a[first], &pivot) {
            break;
        }
    }
    if begin == first - 1 {
        loop {
            if !(first < last) {
                break;
            }
            last -= 1;
            if c(&a[last], &pivot) {
                break;
            }
        }
    } else {
        loop {
            last -= 1;
            if c(&a[last], &pivot) {
                break;
            }
        }
    }
    let already = first >= last;
    while first < last {
        a.swap(first, last);
        loop {
            first += 1;
            if !c(&a[first], &pivot) {
                break;
            }
        }
        loop {
            last -= 1;
            if c(&a[last], &pivot) {
                break;
            }
        }
    }
    let pivot_pos = first - 1;
    if begin != pivot_pos {
        a[begin] = a[pivot_pos].clone();
    }
    a[pivot_pos] = pivot;
    (pivot_pos, already)
}

fn partition_with_equals_on_left<T: Clone, F: Fn(&T, &T) -> bool>(a: &mut [T], first0: usize, last0: usize, c: &F) -> usize {
    let begin = first0;
    let mut first = first0;
    let mut last = last0;
    let pivot = a[first].clone();
    if c(&pivot, &a[last - 1]) {
        loop {
            first += 1;
            if c(&pivot, &a[first]) {
                break;
            }
        }
    } else {
        loop {
            first += 1;
            if !(first < last && !c(&pivot, &a[first])) {
                break;
            }
        }
    }
    if first < last {
        loop {
            last -= 1;
            if !c(&pivot, &a[last]) {
                break;
            }
        }
    }
    while first < last {
        a.swap(first, last);
        loop {
            first += 1;
            if c(&pivot, &a[first]) {
                break;
            }
        }
        loop {
            last -= 1;
            if !c(&pivot, &a[last]) {
                break;
            }
        }
    }
    let pivot_pos = first - 1;
    if begin != pivot_pos {
        a[begin] = a[pivot_pos].clone();
    }
    a[pivot_pos] = pivot;
    first
}

fn sift_down<T: Clone, F: Fn(&T, &T) -> bool>(a: &mut [T], first: usize, c: &F, len: usize, start0: usize) {
    let mut start = start0;
    let mut child = start - first;
    if len < 2 || (len - 2) / 2 < child {
        return;
    }
    child = 2 * child + 1;
    let mut child_i = first + child;
    if child + 1 < len && c(&a[child_i], &a[child_i + 1]) {
        child_i += 1;
        child += 1;
    }
    if c(&a[child_i], &a[start]) {
        return;
    }
    let top = a[start].clone();
    loop {
        a[start] = a[child_i].clone();
        start = child_i;
        if (len - 2) / 2 < child {
            break;
        }
        child = 2 * child + 1;
        child_i = first + child;
        if child + 1 < len && c(&a[child_i], &a[child_i + 1]) {
            child_i += 1;
            child += 1;
        }
        if c(&a[child_i], &top) {
            break;
        }
    }
    a[start] = top;
}

fn heap_sort_all<T: Clone, F: Fn(&T, &T) -> bool>(a: &mut [T], first: usize, last: usize, c: &F) {
    let n = last - first;
    if n > 1 {
        let mut start = (n as isize - 2) / 2;
        while start >= 0 {
            sift_down(a, first, c, n, first + start as usize);
            start -= 1;
        }
    }
    let mut l = last;
    let mut n = n;
    while n > 1 {
        a.swap(first, l - 1);
        sift_down(a, first, c, n - 1, first);
        l -= 1;
        n -= 1;
    }
}

fn introsort<T: Clone, F: Fn(&T, &T) -> bool>(a: &mut [T], mut first: usize, mut last: usize, c: &F, mut depth: i32, mut leftmost: bool) {
    let limit = 24;
    loop {
        let len = last - first;
        match len {
            0 | 1 => return,
            2 => {
                if c(&a[last - 1], &a[first]) {
                    a.swap(first, last - 1);
                }
                return;
            }
            3 => {
                sort3(a, first, first + 1, last - 1, c);
                return;
            }
            4 => {
                sort4(a, first, first + 1, first + 2, last - 1, c);
                return;
            }
            5 => {
                sort5(a, first, first + 1, first + 2, first + 3, last - 1, c);
                return;
            }
            _ => {}
        }
        if len < limit {
            // guarded and unguarded insertion sort behave identically for valid comparators
            insertion_sort(a, first, last, c);
            return;
        }
        if depth == 0 {
            heap_sort_all(a, first, last, c);
            return;
        }
        depth -= 1;
        {
            let half = len / 2;
            if len > 128 {
                sort3(a, first, first + half, last - 1, c);
                sort3(a, first + 1, first + (half - 1), last - 2, c);
                sort3(a, first + 2, first + (half + 1), last - 3, c);
                sort3(a, first + (half - 1), first + half, first + (half + 1), c);
                a.swap(first, first + half);
            } else {
                sort3(a, first + half, first, last - 1, c);
            }
        }
        if !leftmost && !c(&a[first - 1], &a[first]) {
            first = partition_with_equals_on_left(a, first, last, c);
            continue;
        }
        let (i, already) = partition_with_equals_on_right(a, first, last, c);
        if already {
            let fs = insertion_sort_incomplete(a, first, i, c);
            if insertion_sort_incomplete(a, i + 1, last, c) {
                if fs {
                    return;
                }
                last = i;
                continue;
            } else if fs {
                first = i + 1;
                continue;
            }
        }
        introsort(a, first, i, c, depth, leftmost);
        leftmost = false;
        first = i + 1;
    }
}

/// `std::sort(a.begin(), a.end(), less)` as implemented by libc++.
pub fn std_sort<T: Clone, F: Fn(&T, &T) -> bool>(a: &mut [T], less: F) {
    if std::env::var("BOOL_NEW_SORT").is_err() {
        std_sort_old(a, less);
    } else {
        std_sort_new(a, less);
    }
}

pub fn std_sort_new<T: Clone, F: Fn(&T, &T) -> bool>(a: &mut [T], less: F) {
    let n = a.len();
    if n < 2 {
        return;
    }
    let depth = 2 * (usize::BITS - 1 - n.leading_zeros()) as i32;
    introsort(a, 0, n, &less, depth, true);
}

// ---------------------------------------------------------------------------------------------------------------------
// Old (pre LLVM 15) libc++ `std::sort`
// ---------------------------------------------------------------------------------------------------------------------

fn insertion_sort_3<T: Clone, F: Fn(&T, &T) -> bool>(a: &mut [T], first: usize, last: usize, c: &F) {
    let mut j = first + 2;
    sort3(a, first, first + 1, j, c);
    let mut i = j + 1;
    while i != last {
        if c(&a[i], &a[j]) {
            let t = a[i].clone();
            let mut k = j;
            a[i] = a[k].clone();
            loop {
                if j == first {
                    break;
                }
                k -= 1;
                if !c(&t, &a[k]) {
                    break;
                }
                a[j] = a[k].clone();
                j = k;
            }
            a[j] = t;
        }
        j = i;
        i += 1;
    }
}

fn old_sort<T: Clone, F: Fn(&T, &T) -> bool>(a: &mut [T], mut first: usize, mut last: usize, c: &F) {
    let limit = 30;
    'restart: loop {
        let len = last - first;
        match len {
            0 | 1 => return,
            2 => {
                if c(&a[last - 1], &a[first]) {
                    a.swap(first, last - 1);
                }
                return;
            }
            3 => {
                sort3(a, first, first + 1, last - 1, c);
                return;
            }
            4 => {
                sort4(a, first, first + 1, first + 2, last - 1, c);
                return;
            }
            5 => {
                sort5(a, first, first + 1, first + 2, first + 3, last - 1, c);
                return;
            }
            _ => {}
        }
        if len <= limit {
            insertion_sort_3(a, first, last, c);
            return;
        }
        let mut m = first;
        let lm1 = last - 1;
        let mut n_swaps: u32;
        {
            let mut delta = len / 2;
            m += delta;
            if len >= 1000 {
                delta /= 2;
                // __sort5(first, first+delta, m, m+delta, lm1) with swap counting; approximate count via helper
                n_swaps = sort5_count(a, first, first + delta, m, m + delta, lm1, c);
            } else {
                n_swaps = sort3_count(a, first, m, lm1, c);
            }
        }
        let mut i = first;
        let mut j = lm1;
        if !c(&a[i], &a[m]) {
            loop {
                j -= 1;
                if i == j {
                    i += 1;
                    j = last;
                    j -= 1;
                    if !c(&a[first], &a[j]) {
                        loop {
                            if i == j {
                                return;
                            } else if c(&a[first], &a[i]) {
                                a.swap(i, j);
                                n_swaps += 1;
                                i += 1;
                                break;
                            }
                            i += 1;
                        }
                    }
                    if i == j {
                        return;
                    }
                    loop {
                        while !c(&a[first], &a[i]) {
                            i += 1;
                        }
                        loop {
                            j -= 1;
                            if !c(&a[first], &a[j]) {
                                break;
                            }
                        }
                        if i >= j {
                            break;
                        }
                        a.swap(i, j);
                        n_swaps += 1;
                        i += 1;
                    }
                    first = i;
                    continue 'restart;
                }
                if c(&a[j], &a[m]) {
                    a.swap(i, j);
                    n_swaps += 1;
                    break;
                }
            }
        }
        i += 1;
        if i < j {
            loop {
                while c(&a[i], &a[m]) {
                    i += 1;
                }
                loop {
                    j -= 1;
                    if c(&a[j], &a[m]) {
                        break;
                    }
                }
                if i > j {
                    break;
                }
                a.swap(i, j);
                n_swaps += 1;
                if m == i {
                    m = j;
                }
                i += 1;
            }
        }
        if i != m && c(&a[m], &a[i]) {
            a.swap(i, m);
            n_swaps += 1;
        }
        if n_swaps == 0 {
            let fs = insertion_sort_incomplete(a, first, i, c);
            if insertion_sort_incomplete(a, i + 1, last, c) {
                if fs {
                    return;
                }
                last = i;
                continue;
            } else if fs {
                i += 1;
                first = i;
                continue;
            }
        }
        if i - first < last - i {
            old_sort(a, first, i, c);
            i += 1;
            first = i;
        } else {
            old_sort(a, i + 1, last, c);
            last = i;
        }
    }
}

fn sort3_count<T: Clone, F: Fn(&T, &T) -> bool>(a: &mut [T], x: usize, y: usize, z: usize, c: &F) -> u32 {
    let mut r = 0;
    if !c(&a[y], &a[x]) {
        if !c(&a[z], &a[y]) {
            return r;
        }
        a.swap(y, z);
        r = 1;
        if c(&a[y], &a[x]) {
            a.swap(x, y);
            r = 2;
        }
        return r;
    }
    if c(&a[z], &a[y]) {
        a.swap(x, z);
        return 1;
    }
    a.swap(x, y);
    r = 1;
    if c(&a[z], &a[y]) {
        a.swap(y, z);
        r = 2;
    }
    r
}

fn sort5_count<T: Clone, F: Fn(&T, &T) -> bool>(a: &mut [T], x1: usize, x2: usize, x3: usize, x4: usize, x5: usize, c: &F) -> u32 {
    let mut r = sort3_count(a, x1, x2, x3, c);
    if c(&a[x4], &a[x3]) {
        a.swap(x3, x4);
        r += 1;
        if c(&a[x3], &a[x2]) {
            a.swap(x2, x3);
            r += 1;
            if c(&a[x2], &a[x1]) {
                a.swap(x1, x2);
                r += 1;
            }
        }
    }
    if c(&a[x5], &a[x4]) {
        a.swap(x4, x5);
        r += 1;
        if c(&a[x4], &a[x3]) {
            a.swap(x3, x4);
            r += 1;
            if c(&a[x3], &a[x2]) {
                a.swap(x2, x3);
                r += 1;
                if c(&a[x2], &a[x1]) {
                    a.swap(x1, x2);
                    r += 1;
                }
            }
        }
    }
    r
}

pub static USE_OLD: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// `std::sort` of the older libc++ (selected with USE_OLD).
pub fn std_sort_old<T: Clone, F: Fn(&T, &T) -> bool>(a: &mut [T], less: F) {
    let n = a.len();
    old_sort(a, 0, n, &less);
}

// ---------------------------------------------------------------------------------------------------------------------
// tbb::parallel_sort (what `blender::parallel_sort` uses for >= 1000 elements), run depth-first / sequentially.
// ---------------------------------------------------------------------------------------------------------------------

fn tbb_median_of_three<T, F: Fn(&T, &T) -> bool>(a: &[T], l: usize, m: usize, r: usize, comp: &F) -> usize {
    if comp(&a[l], &a[m]) {
        if comp(&a[m], &a[r]) {
            m
        } else if comp(&a[l], &a[r]) {
            r
        } else {
            l
        }
    } else if comp(&a[r], &a[m]) {
        m
    } else if comp(&a[r], &a[l]) {
        r
    } else {
        l
    }
}

fn tbb_pseudo_median_of_nine<T, F: Fn(&T, &T) -> bool>(a: &[T], size: usize, comp: &F) -> usize {
    let offset = size / 8;
    let m1 = tbb_median_of_three(a, 0, offset, offset * 2, comp);
    let m2 = tbb_median_of_three(a, offset * 3, offset * 4, offset * 5, comp);
    let m3 = tbb_median_of_three(a, offset * 6, offset * 7, size - 1, comp);
    tbb_median_of_three(a, m1, m2, m3, comp)
}

fn tbb_rec<T: Clone, F: Fn(&T, &T) -> bool>(a: &mut [T], comp: &F) {
    let size = a.len();
    const GRAIN: usize = 500;
    if size < GRAIN {
        std_sort(a, |x: &T, y: &T| comp(x, y));
        return;
    }
    let m = tbb_pseudo_median_of_nine(a, size, comp);
    if m != 0 {
        a.swap(0, m);
    }
    let mut i = 0usize;
    let mut j = size;
    'part: loop {
        loop {
            j -= 1;
            if !comp(&a[0], &a[j]) {
                break;
            }
        }
        loop {
            if i == j {
                break 'part;
            }
            i += 1;
            if !comp(&a[i], &a[0]) {
                break;
            }
        }
        if i == j {
            break 'part;
        }
        a.swap(i, j);
    }
    a.swap(j, 0);
    let (left, right) = a.split_at_mut(j);
    tbb_rec(left, comp);
    tbb_rec(&mut right[1..], comp);
}

pub fn tbb_parallel_sort<T: Clone, F: Fn(&T, &T) -> bool>(a: &mut [T], less: F) {
    if a.len() < 500 {
        std_sort(a, less);
    } else {
        tbb_rec(a, &less);
    }
}

/// `blender::parallel_sort`
pub fn blender_parallel_sort<T: Clone, F: Fn(&T, &T) -> bool>(a: &mut [T], less: F) {
    if a.len() >= 1000 {
        tbb_parallel_sort(a, less);
    } else {
        std_sort(a, less);
    }
}
