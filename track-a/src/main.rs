use std::fs::File;
use std::io::Write;
use std::os::unix::io::AsRawFd;
use std::sync::atomic::{AtomicU16, AtomicU32, Ordering};
use std::sync::{Arc, Barrier};

extern "C" {
    fn mmap(addr: *mut u8, length: usize, prot: i32, flags: i32, fd: i32, offset: i64) -> *mut u8;
}

const NUM_SLOTS: usize = 72;
const MAX_ENDPOINTS: usize = 2048;
const TOTAL_GROUPS: usize = NUM_SLOTS * MAX_ENDPOINTS;
const TABLE_CAP: usize = 4096;
const TABLE_MASK: usize = TABLE_CAP - 1;

struct AtomicSlot {
    hash: AtomicU32,
    offset: AtomicU32,
    len: AtomicU16,
    id: AtomicU16,
}

#[derive(Clone, Copy)]
struct FastSlot {
    hash: u32,
    len: u16,
    id: u16,
}

#[derive(Clone, Copy)]
struct LocalCacheEntry {
    hash: u32,
    offset: u32,
    len: u16,
    id: u16,
}

#[inline(always)]
unsafe fn find_space_and_hash(mut p: *const u8) -> (usize, u32) {
    let p_start = p;
    let mut h: u64 = 0xcbf29ce484222325;

    loop {
        let w = (p as *const u64).read_unaligned();
        let v = w ^ 0x2020202020202020;
        let zero_mask = v.wrapping_sub(0x0101010101010101) & (!v) & 0x8080808080808080;
        if zero_mask != 0 {
            let add_len = (zero_mask.trailing_zeros() >> 3) as usize;
            let valid_mask = if add_len == 8 { u64::MAX } else { (1u64 << (add_len * 8)) - 1 };
            h = (h ^ (w & valid_mask)).wrapping_mul(0x100000001b3);
            let total_len = p.offset_from(p_start) as usize + add_len;
            return (total_len, (h ^ (h >> 32)) as u32);
        }
        h = (h ^ w).wrapping_mul(0x100000001b3);
        p = p.add(8);
    }
}

#[inline(always)]
fn intern_endpoint_fast(
    h: u32,
    ep_bytes: &[u8],
    ep_offset: u32,
    base: *const u8,
    table: &[AtomicSlot],
    next_id: &AtomicU16,
    local_cache: &mut [LocalCacheEntry; 2048],
) -> u16 {
    let cache_idx = (h as usize) & 2047;
    let cached = local_cache[cache_idx];
    if cached.hash == h && cached.len as usize == ep_bytes.len() {
        let existing = unsafe { std::slice::from_raw_parts(base.add(cached.offset as usize), cached.len as usize) };
        if existing == ep_bytes {
            return cached.id;
        }
    }

    let mut s = (h as usize) & TABLE_MASK;
    loop {
        let slot = &table[s];
        let cur_h = slot.hash.load(Ordering::Relaxed);
        if cur_h == 0 {
            if slot.hash.compare_exchange(0, h, Ordering::AcqRel, Ordering::Relaxed).is_ok() {
                slot.offset.store(ep_offset, Ordering::Relaxed);
                slot.len.store(ep_bytes.len() as u16, Ordering::Relaxed);
                let id = next_id.fetch_add(1, Ordering::Relaxed);
                slot.id.store(id + 1, Ordering::Release);
                local_cache[cache_idx] = LocalCacheEntry {
                    hash: h,
                    offset: ep_offset,
                    len: ep_bytes.len() as u16,
                    id,
                };
                return id;
            }
        }
        if slot.hash.load(Ordering::Relaxed) == h {
            let mut id = slot.id.load(Ordering::Acquire);
            while id == 0 {
                std::hint::spin_loop();
                id = slot.id.load(Ordering::Acquire);
            }
            let off = slot.offset.load(Ordering::Relaxed) as usize;
            let len = slot.len.load(Ordering::Relaxed) as usize;
            if len == ep_bytes.len() {
                let existing = unsafe { std::slice::from_raw_parts(base.add(off), len) };
                if existing == ep_bytes {
                    let real_id = id - 1;
                    local_cache[cache_idx] = LocalCacheEntry {
                        hash: h,
                        offset: ep_offset,
                        len: ep_bytes.len() as u16,
                        id: real_id,
                    };
                    return real_id;
                }
            }
        }
        s = (s + 1) & TABLE_MASK;
    }
}

#[inline(always)]
fn lookup_endpoint_fast(
    h: u32,
    ep_len: u16,
    table: &[FastSlot],
) -> u16 {
    let mut s = (h as usize) & TABLE_MASK;
    loop {
        let slot = unsafe { table.get_unchecked(s) };
        if slot.hash == h && slot.len == ep_len {
            return slot.id;
        }
        s = (s + 1) & TABLE_MASK;
    }
}

const DIGITS: &[u8; 200] = b"\
00010203040506070809\
10111213141516171819\
20212223242526272829\
30313233343536373839\
40414243444546474849\
50515253545556575859\
60616263646566676869\
70717273747576777879\
80818283848586878889\
90919293949596979899";

#[inline(always)]
unsafe fn write_u64_ptr(mut n: u64, ptr: *mut u8) -> *mut u8 {
    if n == 0 {
        *ptr = b'0';
        return ptr.add(1);
    }
    let mut tmp = [0u8; 20];
    let mut i = 20;
    while n >= 100 {
        let rem = (n % 100) as usize * 2;
        n /= 100;
        i -= 2;
        tmp[i] = DIGITS[rem];
        tmp[i + 1] = DIGITS[rem + 1];
    }
    if n < 10 {
        i -= 1;
        tmp[i] = b'0' + n as u8;
    } else {
        let rem = n as usize * 2;
        i -= 2;
        tmp[i] = DIGITS[rem];
        tmp[i + 1] = DIGITS[rem + 1];
    }
    let len = 20 - i;
    std::ptr::copy_nonoverlapping(tmp.as_ptr().add(i), ptr, len);
    ptr.add(len)
}

fn main() {
    let input_path = if std::path::Path::new("/data/requests.log").exists() {
        "/data/requests.log"
    } else {
        "data/a/requests.log"
    };

    let file = match File::open(input_path) {
        Ok(f) => f,
        Err(_) => std::process::exit(1),
    };
    let metadata = file.metadata().expect("metadata");
    let total_size = metadata.len() as usize;

    let output_path = if std::path::Path::new("/out").is_dir() {
        "/out/summary.txt"
    } else {
        "summary.txt"
    };

    if total_size == 0 {
        let _ = File::create(output_path);
        return;
    }

    let ptr = unsafe { mmap(std::ptr::null_mut(), total_size, 1, 1, file.as_raw_fd(), 0) }; // MAP_SHARED
    if ptr == usize::MAX as *mut u8 {
        std::process::exit(1);
    }
    let data = unsafe { std::slice::from_raw_parts(ptr, total_size) };

    let num_threads = std::thread::available_parallelism().map_or(12, |n| n.get()).min(12);

    let mut boundaries = vec![0usize; num_threads + 1];
    boundaries[num_threads] = total_size;
    for i in 1..num_threads {
        let approx = (total_size * i) / num_threads;
        let mut p = approx;
        while p < total_size && data[p] != b'\n' { p += 1; }
        if p < total_size { p += 1; }
        boundaries[i] = p;
    }

    let raw_ptr = ptr as usize;

    let atomic_table: Vec<AtomicSlot> = (0..TABLE_CAP).map(|_| AtomicSlot {
        hash: AtomicU32::new(0),
        offset: AtomicU32::new(0),
        len: AtomicU16::new(0),
        id: AtomicU16::new(0),
    }).collect();
    let next_ep_id = AtomicU16::new(0);

    let table_ref = Arc::new(atomic_table);
    let next_id_ref = Arc::new(next_ep_id);

    let mut hour_strings: Vec<String> = Vec::with_capacity(NUM_SLOTS);
    for slot in 0..NUM_SLOTS {
        let (month, day, clock_hour) = if slot < 6 {
            (10, 30, slot + 18)
        } else if slot < 30 {
            (10, 31, slot - 6)
        } else if slot < 54 {
            (11, 1, slot - 30)
        } else {
            (11, 2, slot - 54)
        };
        hour_strings.push(format!("2026-{:02}-{:02}T{:02}:00:00Z", month, day, clock_hour));
    }
    let hour_strings_arc = Arc::new(hour_strings);

    let barrier = Arc::new(Barrier::new(num_threads));

    // Shared structures for the stages
    let mut thread_counts_vecs: Vec<Vec<u16>> = (0..num_threads).map(|_| vec![0u16; TOTAL_GROUPS]).collect();
    let mut thread_errors_vecs: Vec<Vec<u16>> = (0..num_threads).map(|_| vec![0u16; TOTAL_GROUPS]).collect();
    let mut counts_ptrs: Vec<*mut u16> = thread_counts_vecs.iter_mut().map(|v| v.as_mut_ptr()).collect();
    let mut errors_ptrs: Vec<*mut u16> = thread_errors_vecs.iter_mut().map(|v| v.as_mut_ptr()).collect();
    let all_thread_counts_ptr = counts_ptrs.as_mut_ptr() as usize;
    let all_thread_errors_ptr = errors_ptrs.as_mut_ptr() as usize;

    let mut global_counts: Vec<u32> = vec![0u32; TOTAL_GROUPS];
    let mut global_errors: Vec<u32> = vec![0u32; TOTAL_GROUPS];
    let global_counts_ptr = global_counts.as_mut_ptr() as usize;
    let global_errors_ptr = global_errors.as_mut_ptr() as usize;

    let mut slot_totals = vec![0usize; NUM_SLOTS];
    let slot_totals_ptr = slot_totals.as_mut_ptr() as usize;

    let mut slot_start_offsets = vec![0usize; NUM_SLOTS + 1];
    let slot_start_offsets_ptr = slot_start_offsets.as_mut_ptr() as usize;

    let mut offsets = vec![0usize; TOTAL_GROUPS + 1];
    let offsets_ptr = offsets.as_mut_ptr() as usize;

    let mut fast_table = vec![FastSlot { hash: 0, len: 0, id: 0 }; TABLE_CAP];
    let fast_table_ptr = fast_table.as_mut_ptr() as usize;

    let mut all_latencies: Vec<u32> = Vec::new();
    let all_latencies_ptr = &mut all_latencies as *mut Vec<u32> as usize;

    let mut ep_slices_store: Vec<(usize, usize, usize)> = Vec::with_capacity(MAX_ENDPOINTS);
    let ep_slices_ptr = &mut ep_slices_store as *mut Vec<(usize, usize, usize)> as usize;

    let mut out_chunks: Vec<Vec<u8>> = (0..num_threads).map(|_| Vec::new()).collect();
    let out_chunks_ptr = out_chunks.as_mut_ptr() as usize;

    let mut handles = Vec::with_capacity(num_threads);

    for thread_id in 0..num_threads {
        let start = boundaries[thread_id];
        let end = boundaries[thread_id + 1];
        let t_table = Arc::clone(&table_ref);
        let t_next = Arc::clone(&next_id_ref);
        let t_hours = Arc::clone(&hour_strings_arc);
        let bar = Arc::clone(&barrier);

        let handle = std::thread::spawn(move || {
            let base = raw_ptr as *const u8;
            let mut local_cache = [LocalCacheEntry { hash: 0, offset: 0, len: 0, id: 0 }; 2048];

            let my_counts_ptr = unsafe { *((all_thread_counts_ptr as *const *mut u16).add(thread_id)) };
            let my_errors_ptr = unsafe { *((all_thread_errors_ptr as *const *mut u16).add(thread_id)) };

            let mut t_min_slot = 72u32;
            let mut t_max_slot = 0u32;

            // PASS 1: Thread-local count & intern
            let mut p = unsafe { base.add(start) };
            let p_end = unsafe { base.add(end) };

            unsafe {
                while p < p_end {
                    let month = *p.add(6);
                    let hour = (*p.add(11) - b'0') as u32 * 10 + (*p.add(12) - b'0') as u32;
                    let slot = if month == b'0' {
                        if *p.add(9) == b'0' { hour - 18 } else { hour + 6 }
                    } else {
                        if *p.add(9) == b'1' { hour + 30 } else { hour + 54 }
                    };

                    p = p.add(28);
                    let method_len = if *p.add(3) == b' ' { 4 }
                    else if *p.add(4) == b' ' { 5 }
                    else if *p.add(5) == b' ' { 6 }
                    else if *p.add(6) == b' ' { 7 }
                    else { 8 };
                    p = p.add(method_len);

                    let (ep_len, h) = find_space_and_hash(p);
                    let ep_bytes = std::slice::from_raw_parts(p, ep_len);
                    let ep_offset = p.offset_from(base) as u32;
                    p = p.add(ep_len + 1);

                    let is_5xx = *p == b'5';
                    p = p.add(4); // skip status + space

                    // Fast SWAR skip to newline
                    let w = (p as *const u64).read_unaligned();
                    let v = w ^ 0x0a0a0a0a0a0a0a0a;
                    let m = v.wrapping_sub(0x0101010101010101) & (!v) & 0x8080808080808080;
                    if m != 0 {
                        let len = (m.trailing_zeros() >> 3) as usize;
                        p = p.add(len + 1);
                    } else {
                        p = p.add(8);
                        while *p != b'\n' { p = p.add(1); }
                        p = p.add(1);
                    }

                    let ep_id = intern_endpoint_fast(h, ep_bytes, ep_offset, base, &t_table, &t_next, &mut local_cache);
                    let g = (slot as usize) * MAX_ENDPOINTS + (ep_id as usize);

                    let c = *my_counts_ptr.add(g);
                    *my_counts_ptr.add(g) = c.wrapping_add(1);
                    if is_5xx {
                        let e = *my_errors_ptr.add(g);
                        *my_errors_ptr.add(g) = e.wrapping_add(1);
                    }

                    if slot < t_min_slot { t_min_slot = slot; }
                    if slot > t_max_slot { t_max_slot = slot; }
                }
            }

            bar.wait();

            // STEP 2: Parallel reduction of slots per thread
            let slots_per_thread = (NUM_SLOTS + num_threads - 1) / num_threads;
            let my_s_start = (thread_id * slots_per_thread).min(NUM_SLOTS);
            let my_s_end = ((thread_id + 1) * slots_per_thread).min(NUM_SLOTS);

            let all_counts = all_thread_counts_ptr as *const *const u16;
            let all_errors = all_thread_errors_ptr as *const *const u16;
            let g_cnt_ptr = global_counts_ptr as *mut u32;
            let g_err_ptr = global_errors_ptr as *mut u32;
            let s_tot_ptr = slot_totals_ptr as *mut usize;

            for s in my_s_start..my_s_end {
                let g_base = s * MAX_ENDPOINTS;
                let mut slot_cnt: usize = 0;
                unsafe {
                    std::ptr::write_bytes(g_cnt_ptr.add(g_base), 0, MAX_ENDPOINTS);
                    std::ptr::write_bytes(g_err_ptr.add(g_base), 0, MAX_ENDPOINTS);
                }
                for t in 0..num_threads {
                    unsafe {
                        let t_cnt_p = (*all_counts.add(t)).add(g_base);
                        let t_err_p = (*all_errors.add(t)).add(g_base);
                        for ep_id in 0..MAX_ENDPOINTS {
                            let c = *t_cnt_p.add(ep_id) as u32;
                            if c > 0 {
                                *g_cnt_ptr.add(g_base + ep_id) += c;
                                let e = *t_err_p.add(ep_id) as u32;
                                *g_err_ptr.add(g_base + ep_id) += e;
                            }
                        }
                    }
                }
                for ep_id in 0..MAX_ENDPOINTS {
                    slot_cnt += unsafe { *g_cnt_ptr.add(g_base + ep_id) } as usize;
                }
                unsafe {
                    *s_tot_ptr.add(s) = slot_cnt;
                }
            }

            bar.wait();

            // STEP 3: Thread 0 prefix sums slots, resizes latencies, builds fast_table & ep_store
            if thread_id == 0 {
                let s_tot = unsafe { std::slice::from_raw_parts(slot_totals_ptr as *const usize, NUM_SLOTS) };
                let s_starts = unsafe { std::slice::from_raw_parts_mut(slot_start_offsets_ptr as *mut usize, NUM_SLOTS + 1) };
                let mut cur = 0usize;
                for s in 0..NUM_SLOTS {
                    s_starts[s] = cur;
                    cur += s_tot[s];
                }
                s_starts[NUM_SLOTS] = cur;

                let lat_vec = unsafe { &mut *(all_latencies_ptr as *mut Vec<u32>) };
                if lat_vec.capacity() < cur {
                    lat_vec.reserve(cur - lat_vec.len());
                }
                unsafe { lat_vec.set_len(cur); }

                let ft = unsafe { std::slice::from_raw_parts_mut(fast_table_ptr as *mut FastSlot, TABLE_CAP) };
                for s in 0..TABLE_CAP {
                    let cur_h = t_table[s].hash.load(Ordering::Relaxed);
                    if cur_h != 0 {
                        ft[s] = FastSlot {
                            hash: cur_h,
                            len: t_table[s].len.load(Ordering::Relaxed),
                            id: t_table[s].id.load(Ordering::Relaxed) - 1,
                        };
                    }
                }

                let store = unsafe { &mut *(ep_slices_ptr as *mut Vec<(usize, usize, usize)>) };
                store.clear();
                for slot in t_table.iter() {
                    let id_val = slot.id.load(Ordering::Relaxed);
                    if id_val > 0 {
                        let id = (id_val - 1) as usize;
                        let off = slot.offset.load(Ordering::Relaxed) as usize;
                        let len = slot.len.load(Ordering::Relaxed) as usize;
                        store.push((id, off, len));
                    }
                }
                store.sort_unstable_by(|a, b| {
                    let sa = unsafe { std::slice::from_raw_parts(base.add(a.1), a.2) };
                    let sb = unsafe { std::slice::from_raw_parts(base.add(b.1), b.2) };
                    sa.cmp(sb)
                });
            }

            bar.wait();

            // STEP 4: Parallel group offsets computation
            let s_starts = unsafe { std::slice::from_raw_parts(slot_start_offsets_ptr as *const usize, NUM_SLOTS) };
            let off_ptr = offsets_ptr as *mut usize;
            for s in my_s_start..my_s_end {
                let mut cur_off = s_starts[s];
                for ep_id in 0..MAX_ENDPOINTS {
                    let g = s * MAX_ENDPOINTS + ep_id;
                    unsafe {
                        *off_ptr.add(g) = cur_off;
                        cur_off += *g_cnt_ptr.add(g) as usize;
                    }
                }
            }

            bar.wait();

            // STEP 5: Initialize thread-local cursors for Pass 2
            let local_slots = if t_min_slot <= t_max_slot { (t_max_slot - t_min_slot + 1) as usize } else { 0 };
            let mut my_cursors = vec![0u32; local_slots * MAX_ENDPOINTS];

            if t_min_slot <= t_max_slot {
                for s in t_min_slot..=t_max_slot {
                    let s_idx = s as usize;
                    let local_base = (s_idx - t_min_slot as usize) * MAX_ENDPOINTS;
                    for ep_id in 0..MAX_ENDPOINTS {
                        let g = s_idx * MAX_ENDPOINTS + ep_id;
                        let my_cnt = unsafe { *my_counts_ptr.add(g) };
                        if my_cnt > 0 {
                            let mut my_start = unsafe { *off_ptr.add(g) } as u32;
                            for k in 0..thread_id {
                                unsafe {
                                    let k_cnt_p = *all_counts.add(k);
                                    my_start += *k_cnt_p.add(g) as u32;
                                }
                            }
                            my_cursors[local_base + ep_id] = my_start;
                        }
                    }
                }
            }

            // PASS 2: Re-scan and direct lock-free thread-local scatter
            let lat_vec = unsafe { &mut *(all_latencies_ptr as *mut Vec<u32>) };
            let lat_base = lat_vec.as_mut_ptr();
            let ft = unsafe { std::slice::from_raw_parts(fast_table_ptr as *const FastSlot, TABLE_CAP) };

            p = unsafe { base.add(start) };

            unsafe {
                while p < p_end {
                    let month = *p.add(6);
                    let hour = (*p.add(11) - b'0') as u32 * 10 + (*p.add(12) - b'0') as u32;
                    let slot = if month == b'0' {
                        if *p.add(9) == b'0' { hour - 18 } else { hour + 6 }
                    } else {
                        if *p.add(9) == b'1' { hour + 30 } else { hour + 54 }
                    };

                    p = p.add(28);
                    let method_len = if *p.add(3) == b' ' { 4 }
                    else if *p.add(4) == b' ' { 5 }
                    else if *p.add(5) == b' ' { 6 }
                    else if *p.add(6) == b' ' { 7 }
                    else { 8 };
                    p = p.add(method_len);

                    let (ep_len, h) = find_space_and_hash(p);
                    p = p.add(ep_len + 5);

                    let mut lat = (*p - b'0') as u32;
                    p = p.add(1);
                    while *p != b'\n' {
                        lat = lat * 10 + (*p - b'0') as u32;
                        p = p.add(1);
                    }
                    p = p.add(1);

                    let ep_id = lookup_endpoint_fast(h, ep_len as u16, ft);
                    let local_idx = ((slot - t_min_slot) as usize) * MAX_ENDPOINTS + (ep_id as usize);

                    let pos = my_cursors[local_idx];
                    my_cursors[local_idx] = pos + 1;
                    *lat_base.add(pos as usize) = lat;
                }
            }

            bar.wait();

            // PASS 3: Partition percentiles and format output using pointer writing
            let off_slice = unsafe { std::slice::from_raw_parts(offsets_ptr as *const usize, TOTAL_GROUPS + 1) };
            let ep_store = unsafe { &*(ep_slices_ptr as *const Vec<(usize, usize, usize)>) };

            let mut buf: Vec<u8> = Vec::with_capacity(1024 * 1024);

            for slot in my_s_start..my_s_end {
                let hour_bytes = t_hours[slot].as_bytes();
                for &(orig_id, ep_off, ep_len) in ep_store.iter() {
                    let g = slot * MAX_ENDPOINTS + orig_id;
                    let count = unsafe { *g_cnt_ptr.add(g) } as usize;
                    if count == 0 {
                        continue;
                    }
                    let start = off_slice[g];
                    let group_slice = unsafe { std::slice::from_raw_parts_mut(lat_base.add(start), count) };

                    let (p50, p95, p99) = if count == 1 {
                        let v = group_slice[0] as u64;
                        (v, v, v)
                    } else if count == 2 {
                        if group_slice[0] > group_slice[1] {
                            group_slice.swap(0, 1);
                        }
                        (group_slice[0] as u64, group_slice[1] as u64, group_slice[1] as u64)
                    } else if count <= 16 {
                        for i in 1..count {
                            let key = group_slice[i];
                            let mut j = i;
                            while j > 0 && group_slice[j - 1] > key {
                                group_slice[j] = group_slice[j - 1];
                                j -= 1;
                            }
                            group_slice[j] = key;
                        }
                        (
                            group_slice[(50 * count + 99) / 100 - 1] as u64,
                            group_slice[(95 * count + 99) / 100 - 1] as u64,
                            group_slice[(99 * count + 99) / 100 - 1] as u64,
                        )
                    } else {
                        group_slice.sort_unstable();
                        (
                            group_slice[(50 * count + 99) / 100 - 1] as u64,
                            group_slice[(95 * count + 99) / 100 - 1] as u64,
                            group_slice[(99 * count + 99) / 100 - 1] as u64,
                        )
                    };

                    let errors = unsafe { *g_err_ptr.add(g) } as u64;
                    let error_rate = if errors == 0 { 0 } else { errors * 1_000_000 / count as u64 };
                    let ep_slice = unsafe { std::slice::from_raw_parts(base.add(ep_off), ep_len) };

                    buf.reserve(128 + ep_len);
                    unsafe {
                        let cur_len = buf.len();
                        let mut out_p = buf.as_mut_ptr().add(cur_len);

                        std::ptr::copy_nonoverlapping(hour_bytes.as_ptr(), out_p, hour_bytes.len());
                        out_p = out_p.add(hour_bytes.len());
                        *out_p = b' '; out_p = out_p.add(1);

                        std::ptr::copy_nonoverlapping(ep_slice.as_ptr(), out_p, ep_len);
                        out_p = out_p.add(ep_len);
                        *out_p = b' '; out_p = out_p.add(1);

                        out_p = write_u64_ptr(count as u64, out_p);
                        *out_p = b' '; out_p = out_p.add(1);

                        out_p = write_u64_ptr(p50, out_p);
                        *out_p = b' '; out_p = out_p.add(1);

                        out_p = write_u64_ptr(p95, out_p);
                        *out_p = b' '; out_p = out_p.add(1);

                        out_p = write_u64_ptr(p99, out_p);
                        *out_p = b' '; out_p = out_p.add(1);

                        if errors == 0 {
                            *out_p = b'0';
                            *(out_p.add(1)) = b' ';
                            *(out_p.add(2)) = b'0';
                            *(out_p.add(3)) = b'\n';
                            out_p = out_p.add(4);
                        } else {
                            out_p = write_u64_ptr(errors, out_p);
                            *out_p = b' '; out_p = out_p.add(1);

                            out_p = write_u64_ptr(error_rate, out_p);
                            *out_p = b'\n'; out_p = out_p.add(1);
                        }

                        buf.set_len(out_p.offset_from(buf.as_ptr()) as usize);
                    }
                }
            }

            unsafe {
                let out_ptr = (out_chunks_ptr as *mut Vec<u8>).add(thread_id);
                *out_ptr = buf;
            }
        });
        handles.push(handle);
    }

    for h in handles {
        h.join().unwrap();
    }

    let mut out_file = File::create(output_path).expect("create out");
    for chunk in out_chunks {
        out_file.write_all(&chunk).expect("write chunk");
    }
}
