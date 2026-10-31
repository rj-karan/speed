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
    local_cache: &mut [LocalCacheEntry; 1024],
) -> u16 {
    let cache_idx = (h as usize) & 1023;
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
fn write_u64_fast(mut n: u64, buf: &mut Vec<u8>) {
    if n == 0 {
        buf.push(b'0');
        return;
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
    buf.extend_from_slice(&tmp[i..]);
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

    let ptr = unsafe { mmap(std::ptr::null_mut(), total_size, 1, 1, file.as_raw_fd(), 0) }; // MAP_SHARED (1)
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

    let global_counts: Vec<AtomicU32> = (0..TOTAL_GROUPS).map(|_| AtomicU32::new(0)).collect();
    let global_errors: Vec<AtomicU32> = (0..TOTAL_GROUPS).map(|_| AtomicU32::new(0)).collect();
    let write_cursors: Vec<AtomicU32> = (0..TOTAL_GROUPS).map(|_| AtomicU32::new(0)).collect();

    let table_ref = Arc::new(atomic_table);
    let next_id_ref = Arc::new(next_ep_id);
    let counts_ref = Arc::new(global_counts);
    let errors_ref = Arc::new(global_errors);
    let cursors_ref = Arc::new(write_cursors);

    // Precompute hour strings
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
    let mut offsets = vec![0usize; TOTAL_GROUPS + 1];
    let offsets_ptr = offsets.as_mut_ptr() as usize;

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
        let t_counts = Arc::clone(&counts_ref);
        let t_errors = Arc::clone(&errors_ref);
        let t_cursors = Arc::clone(&cursors_ref);
        let t_hours = Arc::clone(&hour_strings_arc);
        let bar = Arc::clone(&barrier);

        let handle = std::thread::spawn(move || {
            let base = raw_ptr as *const u8;
            let mut p = unsafe { base.add(start) };
            let p_end = unsafe { base.add(end) };

            let mut items = Vec::with_capacity((end - start) / 44 + 100);
            let mut local_cache = [LocalCacheEntry { hash: 0, offset: 0, len: 0, id: 0 }; 1024];

            unsafe {
                while p < p_end {
                    let month = *p.add(6);
                    let hour = (*p.add(11) - b'0') as u32 * 10 + (*p.add(12) - b'0') as u32;
                    let slot = if month == b'0' {
                        if *p.add(9) == b'0' { hour - 18 } else { hour + 6 }
                    } else {
                        if *p.add(9) == b'1' { hour + 30 } else { hour + 54 }
                    };

                    p = p.add(28); // timestamp + space
                    let method_len = if *p.add(3) == b' ' { 4 }
                    else if *p.add(4) == b' ' { 5 }
                    else if *p.add(5) == b' ' { 6 }
                    else if *p.add(6) == b' ' { 7 }
                    else { 8 };
                    p = p.add(method_len);

                    let (ep_len, h) = find_space_and_hash(p);
                    let ep_bytes = std::slice::from_raw_parts(p, ep_len);
                    let ep_offset = p.offset_from(base) as u32;
                    p = p.add(ep_len + 1); // endpoint + space

                    let is_5xx = *p == b'5';
                    p = p.add(4); // status + space

                    let mut lat = (*p - b'0') as u32;
                    p = p.add(1);
                    while *p != b'\n' {
                        lat = lat * 10 + (*p - b'0') as u32;
                        p = p.add(1);
                    }
                    p = p.add(1); // skip '\n'

                    let ep_id = intern_endpoint_fast(h, ep_bytes, ep_offset, base, &t_table, &t_next, &mut local_cache);
                    let g = (slot as usize) * MAX_ENDPOINTS + (ep_id as usize);

                    t_counts[g].fetch_add(1, Ordering::Relaxed);
                    if is_5xx {
                        t_errors[g].fetch_add(1, Ordering::Relaxed);
                    }
                    items.push(((g as u64) << 32) | (lat as u64));
                }
            }

            bar.wait();

            if thread_id == 0 {
                let off_slice = unsafe { std::slice::from_raw_parts_mut(offsets_ptr as *mut usize, TOTAL_GROUPS + 1) };
                for g in 0..TOTAL_GROUPS {
                    let cnt = t_counts[g].load(Ordering::Relaxed) as usize;
                    off_slice[g + 1] = off_slice[g] + cnt;
                    t_cursors[g].store(off_slice[g] as u32, Ordering::Relaxed);
                }

                let total_records = off_slice[TOTAL_GROUPS];
                let lat_vec = unsafe { &mut *(all_latencies_ptr as *mut Vec<u32>) };
                lat_vec.resize(total_records, 0);

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

            let lat_vec = unsafe { &*(all_latencies_ptr as *const Vec<u32>) };
            let lat_base = lat_vec.as_ptr() as *mut u32;

            for item in items {
                let g = (item >> 32) as usize;
                let lat = item as u32;
                let pos = t_cursors[g].fetch_add(1, Ordering::Relaxed) as usize;
                unsafe {
                    *lat_base.add(pos) = lat;
                }
            }

            bar.wait();

            let off_slice = unsafe { std::slice::from_raw_parts(offsets_ptr as *const usize, TOTAL_GROUPS + 1) };
            let groups_per_thread = (TOTAL_GROUPS + num_threads - 1) / num_threads;
            let g_start = (thread_id * groups_per_thread).min(TOTAL_GROUPS);
            let g_end = ((thread_id + 1) * groups_per_thread).min(TOTAL_GROUPS);

            for g in g_start..g_end {
                let start = off_slice[g];
                let end = off_slice[g + 1];
                if end > start + 1 {
                    let slice = unsafe { std::slice::from_raw_parts_mut(lat_base.add(start), end - start) };
                    slice.sort_unstable();
                }
            }

            bar.wait();

            let slots_per_thread = (NUM_SLOTS + num_threads - 1) / num_threads;
            let s_start = (thread_id * slots_per_thread).min(NUM_SLOTS);
            let s_end = ((thread_id + 1) * slots_per_thread).min(NUM_SLOTS);
            let ep_store = unsafe { &*(ep_slices_ptr as *const Vec<(usize, usize, usize)>) };

            let mut buf = Vec::with_capacity(512 * 1024);
            let all_lats = unsafe { std::slice::from_raw_parts(lat_base, off_slice[TOTAL_GROUPS]) };

            for slot in s_start..s_end {
                let hour_bytes = t_hours[slot].as_bytes();
                for &(orig_id, ep_off, ep_len) in ep_store.iter() {
                    let g = slot * MAX_ENDPOINTS + orig_id;
                    let count = t_counts[g].load(Ordering::Relaxed) as usize;
                    if count == 0 {
                        continue;
                    }
                    let start = off_slice[g];
                    let end = off_slice[g + 1];
                    let values = &all_lats[start..end];

                    let p50 = values[(50 * count + 99) / 100 - 1] as u64;
                    let p95 = values[(95 * count + 99) / 100 - 1] as u64;
                    let p99 = values[(99 * count + 99) / 100 - 1] as u64;

                    let errors = t_errors[g].load(Ordering::Relaxed) as u64;
                    let error_rate = errors * 1_000_000 / count as u64;
                    let ep_slice = unsafe { std::slice::from_raw_parts(base.add(ep_off), ep_len) };

                    buf.extend_from_slice(hour_bytes);
                    buf.push(b' ');
                    buf.extend_from_slice(ep_slice);
                    buf.push(b' ');
                    write_u64_fast(count as u64, &mut buf);
                    buf.push(b' ');
                    write_u64_fast(p50, &mut buf);
                    buf.push(b' ');
                    write_u64_fast(p95, &mut buf);
                    buf.push(b' ');
                    write_u64_fast(p99, &mut buf);
                    buf.push(b' ');
                    write_u64_fast(errors, &mut buf);
                    buf.push(b' ');
                    write_u64_fast(error_rate, &mut buf);
                    buf.push(b'\n');
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
