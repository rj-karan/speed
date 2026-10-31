use std::fs::File;
use std::io::Write;
use std::os::unix::io::AsRawFd;
use std::sync::atomic::{AtomicU16, AtomicU32, Ordering};
use std::sync::Arc;

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
fn intern_endpoint_fused(
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

    let ptr = unsafe { mmap(std::ptr::null_mut(), total_size, 1, 2, file.as_raw_fd(), 0) };
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

    let table_ref = Arc::new(atomic_table);
    let next_id_ref = Arc::new(next_ep_id);
    let counts_ref = Arc::new(global_counts);
    let errors_ref = Arc::new(global_errors);

    let mut parse_handles = Vec::with_capacity(num_threads);

    for i in 0..num_threads {
        let start = boundaries[i];
        let end = boundaries[i + 1];
        let t_table = Arc::clone(&table_ref);
        let t_next = Arc::clone(&next_id_ref);
        let t_counts = Arc::clone(&counts_ref);
        let t_errors = Arc::clone(&errors_ref);

        let handle = std::thread::spawn(move || {
            let base = raw_ptr as *const u8;
            let mut p = unsafe { base.add(start) };
            let p_end = unsafe { base.add(end) };

            let mut items = Vec::with_capacity((end - start) / 25);
            let mut local_cache = [LocalCacheEntry { hash: 0, offset: 0, len: 0, id: 0 }; 1024];

            unsafe {
                while p < p_end {
                    let month = *p.add(6);
                    let day = (*p.add(8) - b'0') as u32 * 10 + (*p.add(9) - b'0') as u32;
                    let hour = (*p.add(11) - b'0') as u32 * 10 + (*p.add(12) - b'0') as u32;
                    let slot = if month == b'0' {
                        (day - 30) * 24 + hour - 18
                    } else {
                        30 + (day - 1) * 24 + hour
                    };

                    p = p.add(28); // timestamp + space
                    while *p != b' ' { p = p.add(1); }
                    p = p.add(1); // method + space

                    let ep_start = p;
                    let mut h: u32 = 0x811c9dc5;
                    while *p != b' ' {
                        h = (h ^ (*p as u32)).wrapping_mul(0x01000193);
                        p = p.add(1);
                    }
                    let ep_len = p.offset_from(ep_start) as usize;
                    let ep_bytes = std::slice::from_raw_parts(ep_start, ep_len);
                    let ep_offset = ep_start.offset_from(base) as u32;
                    p = p.add(1); // endpoint + space

                    let is_5xx = *p == b'5';
                    p = p.add(4); // status + space

                    let mut lat: u32 = 0;
                    while p < p_end && *p >= b'0' && *p <= b'9' {
                        lat = lat * 10 + (*p - b'0') as u32;
                        p = p.add(1);
                    }
                    if p < p_end && *p == b'\r' { p = p.add(1); }
                    if p < p_end && *p == b'\n' { p = p.add(1); }

                    let ep_id = intern_endpoint_fused(h, ep_bytes, ep_offset, base, &t_table, &t_next, &mut local_cache);
                    let g = (slot as usize) * MAX_ENDPOINTS + (ep_id as usize);

                    t_counts[g].fetch_add(1, Ordering::Relaxed);
                    if is_5xx {
                        t_errors[g].fetch_add(1, Ordering::Relaxed);
                    }
                    items.push(((g as u64) << 32) | (lat as u64));
                }
            }

            items
        });
        parse_handles.push(handle);
    }

    let mut thread_items = Vec::with_capacity(num_threads);
    for h in parse_handles {
        thread_items.push(h.join().unwrap());
    }

    let num_endpoints = next_id_ref.load(Ordering::Relaxed) as usize;

    let base_ptr = raw_ptr as *const u8;
    let mut ep_slices: Vec<(usize, &[u8])> = Vec::with_capacity(num_endpoints);
    for slot in table_ref.iter() {
        let id_val = slot.id.load(Ordering::Relaxed);
        if id_val > 0 {
            let id = (id_val - 1) as usize;
            let off = slot.offset.load(Ordering::Relaxed) as usize;
            let len = slot.len.load(Ordering::Relaxed) as usize;
            let slice = unsafe { std::slice::from_raw_parts(base_ptr.add(off), len) };
            ep_slices.push((id, slice));
        }
    }
    ep_slices.sort_unstable_by(|a, b| a.1.cmp(b.1));

    // Prefix sum over TOTAL_GROUPS to get offsets
    let mut offsets = vec![0usize; TOTAL_GROUPS + 1];
    for g in 0..TOTAL_GROUPS {
        offsets[g + 1] = offsets[g] + counts_ref[g].load(Ordering::Relaxed) as usize;
    }
    let total_records = offsets[TOTAL_GROUPS];

    let mut all_latencies = vec![0u32; total_records];
    let lat_ptr = all_latencies.as_mut_ptr() as usize;

    // Initialize atomic write cursors from offsets
    let write_cursors: Vec<AtomicU32> = (0..TOTAL_GROUPS)
        .map(|g| AtomicU32::new(offsets[g] as u32))
        .collect();
    let cursors_ref = Arc::new(write_cursors);

    // Parallel scatter using atomic fetch_add
    let mut scatter_handles = Vec::with_capacity(num_threads);
    for items in thread_items {
        let c_ref = Arc::clone(&cursors_ref);
        let handle = std::thread::spawn(move || {
            let base = lat_ptr as *mut u32;
            for item in items {
                let g = (item >> 32) as usize;
                let lat = item as u32;
                let pos = c_ref[g].fetch_add(1, Ordering::Relaxed) as usize;
                unsafe {
                    *base.add(pos) = lat;
                }
            }
        });
        scatter_handles.push(handle);
    }
    for h in scatter_handles {
        h.join().unwrap();
    }
    drop(cursors_ref);

    // Parallel sort of group slices
    let groups_per_thread = (TOTAL_GROUPS + num_threads - 1) / num_threads;
    let mut sort_handles = Vec::with_capacity(num_threads);
    let offsets_arc = Arc::new(offsets);

    for t in 0..num_threads {
        let g_start = (t * groups_per_thread).min(TOTAL_GROUPS);
        let g_end = ((t + 1) * groups_per_thread).min(TOTAL_GROUPS);
        let off_ref = Arc::clone(&offsets_arc);
        let handle = std::thread::spawn(move || {
            let base = lat_ptr as *mut u32;
            for g in g_start..g_end {
                let start = off_ref[g];
                let end = off_ref[g + 1];
                if end > start + 1 {
                    let slice = unsafe { std::slice::from_raw_parts_mut(base.add(start), end - start) };
                    slice.sort_unstable();
                }
            }
        });
        sort_handles.push(handle);
    }
    for h in sort_handles {
        h.join().unwrap();
    }

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

    // Parallel formatting across NUM_SLOTS (72 slots)
    let slots_per_thread = (NUM_SLOTS + num_threads - 1) / num_threads;
    let ep_slices_arc = Arc::new(ep_slices);
    let hour_strings_arc = Arc::new(hour_strings);
    let all_latencies_arc = Arc::new(all_latencies);

    let mut format_handles = Vec::with_capacity(num_threads);
    for t in 0..num_threads {
        let s_start = (t * slots_per_thread).min(NUM_SLOTS);
        let s_end = ((t + 1) * slots_per_thread).min(NUM_SLOTS);
        let ep_s = Arc::clone(&ep_slices_arc);
        let h_s = Arc::clone(&hour_strings_arc);
        let lats = Arc::clone(&all_latencies_arc);
        let offs = Arc::clone(&offsets_arc);
        let cnts = Arc::clone(&counts_ref);
        let errs = Arc::clone(&errors_ref);

        let handle = std::thread::spawn(move || {
            let mut buf = Vec::with_capacity(512 * 1024);
            for slot in s_start..s_end {
                let hour_bytes = h_s[slot].as_bytes();
                for &(orig_id, ep_slice) in ep_s.iter() {
                    let g = slot * MAX_ENDPOINTS + orig_id;
                    let count = cnts[g].load(Ordering::Relaxed) as usize;
                    if count == 0 {
                        continue;
                    }
                    let start = offs[g];
                    let end = offs[g + 1];
                    let values = &lats[start..end];

                    let p50 = values[(50 * count + 99) / 100 - 1] as u64;
                    let p95 = values[(95 * count + 99) / 100 - 1] as u64;
                    let p99 = values[(99 * count + 99) / 100 - 1] as u64;

                    let errors = errs[g].load(Ordering::Relaxed) as u64;
                    let error_rate = errors * 1_000_000 / count as u64;

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
            buf
        });
        format_handles.push(handle);
    }

    let mut out_file = File::create(output_path).expect("create out");
    for h in format_handles {
        let chunk = h.join().unwrap();
        out_file.write_all(&chunk).expect("write chunk");
    }
}
