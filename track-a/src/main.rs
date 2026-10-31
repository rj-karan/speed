use std::fs::File;
use std::io::Write;
use std::os::unix::io::AsRawFd;

extern "C" {
    fn mmap(addr: *mut u8, length: usize, prot: i32, flags: i32, fd: i32, offset: i64) -> *mut u8;
}

const NUM_SLOTS: usize = 72;
const TABLE_CAP: usize = 4096;
const TABLE_MASK: usize = TABLE_CAP - 1;

#[derive(Clone, Copy)]
struct Slot {
    hash: u32,
    offset: u32,
    len: u16,
    id: u16,
}

#[inline(always)]
fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811c9dc5;
    for &b in bytes {
        h = (h ^ (b as u32)).wrapping_mul(0x01000193);
    }
    h
}

struct ThreadParseResult {
    endpoints: Vec<(u32, u16)>, // (offset_from_base, len)
    records: Vec<u64>,
}

unsafe fn parse_chunk(
    base: *const u8,
    start: usize,
    end: usize,
) -> ThreadParseResult {
    let mut p = base.add(start);
    let p_end = base.add(end);

    let mut table = [Slot { hash: 0, offset: 0, len: 0, id: 0 }; TABLE_CAP];
    let mut endpoints: Vec<(u32, u16)> = Vec::with_capacity(2048);
    let mut records: Vec<u64> = Vec::with_capacity((end - start) / 25);

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
        while *p != b' ' { p = p.add(1); }
        let ep_len = p.offset_from(ep_start) as usize;
        let ep_bytes = std::slice::from_raw_parts(ep_start, ep_len);
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

        let h = fnv1a(ep_bytes);
        let mut s = (h as usize) & TABLE_MASK;
        let ep_id: u16;
        loop {
            let e = &mut table[s];
            if e.len == 0 {
                let new_id = endpoints.len() as u16;
                let off = ep_start.offset_from(base) as u32;
                e.hash = h;
                e.offset = off;
                e.len = ep_len as u16;
                e.id = new_id;
                endpoints.push((off, ep_len as u16));
                ep_id = new_id;
                break;
            } else if e.len as usize == ep_len && e.hash == h {
                let existing = std::slice::from_raw_parts(base.add(e.offset as usize), ep_len);
                if existing == ep_bytes {
                    ep_id = e.id;
                    break;
                }
            }
            s = (s + 1) & TABLE_MASK;
        }

        let err_bit = if is_5xx { 1u64 << 60 } else { 0 };
        records.push(((slot as u64) << 44) | ((ep_id as u64) << 32) | (lat as u64) | err_bit);
    }

    ThreadParseResult {
        endpoints,
        records,
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

    let mut out_file = match File::create(output_path) {
        Ok(f) => f,
        Err(_) => std::process::exit(1),
    };

    if total_size == 0 {
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
    let mut handles = Vec::with_capacity(num_threads);

    for i in 0..num_threads {
        let start = boundaries[i];
        let end = boundaries[i + 1];
        let handle = std::thread::spawn(move || {
            let base = raw_ptr as *const u8;
            unsafe { parse_chunk(base, start, end) }
        });
        handles.push(handle);
    }

    let mut results = Vec::with_capacity(num_threads);
    for h in handles {
        results.push(h.join().unwrap());
    }

    // Unify endpoints
    let mut all_endpoints: Vec<&[u8]> = Vec::with_capacity(2048);
    for r in &results {
        for &(off, len) in &r.endpoints {
            let ep = &data[off as usize..off as usize + len as usize];
            if !all_endpoints.contains(&ep) {
                all_endpoints.push(ep);
            }
        }
    }
    all_endpoints.sort_unstable();
    let num_endpoints = all_endpoints.len();

    // Map each thread's local endpoint IDs to global endpoint IDs
    let mut thread_maps: Vec<Vec<usize>> = Vec::with_capacity(num_threads);
    for r in &results {
        let mut map = Vec::with_capacity(r.endpoints.len());
        for &(off, len) in &r.endpoints {
            let ep = &data[off as usize..off as usize + len as usize];
            let global_id = all_endpoints.binary_search(&ep).unwrap();
            map.push(global_id);
        }
        thread_maps.push(map);
    }

    let total_groups = NUM_SLOTS * num_endpoints;
    let mut group_counts = vec![0u32; total_groups];
    let mut global_errors = vec![0u32; total_groups];

    // Compute counts and errors
    for (t, r) in results.iter().enumerate() {
        let map = &thread_maps[t];
        for &rec in &r.records {
            let slot = ((rec >> 44) & 0x7F) as usize;
            let local_ep = ((rec >> 32) & 0xFFF) as usize;
            let global_ep = map[local_ep];
            let g = slot * num_endpoints + global_ep;
            group_counts[g] += 1;
            if (rec & (1u64 << 60)) != 0 {
                global_errors[g] += 1;
            }
        }
    }

    // Prefix sums for global offsets
    let mut offsets = vec![0usize; total_groups + 1];
    for g in 0..total_groups {
        offsets[g + 1] = offsets[g] + group_counts[g] as usize;
    }
    let total_records = offsets[total_groups];

    let mut all_latencies = vec![0u32; total_records];
    let mut write_cursors = offsets.clone();

    // Scatter latencies into all_latencies
    for (t, r) in results.iter().enumerate() {
        let map = &thread_maps[t];
        for &rec in &r.records {
            let slot = ((rec >> 44) & 0x7F) as usize;
            let local_ep = ((rec >> 32) & 0xFFF) as usize;
            let global_ep = map[local_ep];
            let g = slot * num_endpoints + global_ep;
            let pos = write_cursors[g];
            write_cursors[g] += 1;
            all_latencies[pos] = rec as u32;
        }
    }
    drop(results);

    // Parallel sort of group slices
    let lat_ptr = all_latencies.as_mut_ptr() as usize;
    let groups_per_thread = (total_groups + num_threads - 1) / num_threads;
    let mut sort_handles = Vec::with_capacity(num_threads);

    for t in 0..num_threads {
        let g_start = (t * groups_per_thread).min(total_groups);
        let g_end = ((t + 1) * groups_per_thread).min(total_groups);
        let offsets_ref = offsets.clone();
        let handle = std::thread::spawn(move || {
            let base = lat_ptr as *mut u32;
            for g in g_start..g_end {
                let start = offsets_ref[g];
                let end = offsets_ref[g + 1];
                if end > start {
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

    // Precomputed hour strings
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

    // Output formatting
    let mut out_buffer = Vec::with_capacity(4 * 1024 * 1024);

    for slot in 0..NUM_SLOTS {
        let hour_bytes = hour_strings[slot].as_bytes();
        for ep_id in 0..num_endpoints {
            let g = slot * num_endpoints + ep_id;
            let count = group_counts[g] as usize;
            if count == 0 {
                continue;
            }
            let start = offsets[g];
            let end = offsets[g + 1];
            let values = &all_latencies[start..end];

            let p50 = values[(50 * count + 99) / 100 - 1] as u64;
            let p95 = values[(95 * count + 99) / 100 - 1] as u64;
            let p99 = values[(99 * count + 99) / 100 - 1] as u64;

            let errors = global_errors[g] as u64;
            let error_rate = errors * 1_000_000 / count as u64;

            out_buffer.extend_from_slice(hour_bytes);
            out_buffer.push(b' ');
            out_buffer.extend_from_slice(all_endpoints[ep_id]);
            out_buffer.push(b' ');
            write_u64_fast(count as u64, &mut out_buffer);
            out_buffer.push(b' ');
            write_u64_fast(p50, &mut out_buffer);
            out_buffer.push(b' ');
            write_u64_fast(p95, &mut out_buffer);
            out_buffer.push(b' ');
            write_u64_fast(p99, &mut out_buffer);
            out_buffer.push(b' ');
            write_u64_fast(errors, &mut out_buffer);
            out_buffer.push(b' ');
            write_u64_fast(error_rate, &mut out_buffer);
            out_buffer.push(b'\n');
        }
    }

    out_file.write_all(&out_buffer).expect("write output");
}
