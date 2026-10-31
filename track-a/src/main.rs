use std::fs::File;
use std::io::Write;
use std::os::unix::io::AsRawFd;

extern "C" {
    fn mmap(addr: *mut u8, length: usize, prot: i32, flags: i32, fd: i32, offset: i64) -> *mut u8;
}

const NUM_SLOTS: usize = 72;
const MAX_ENDPOINTS: usize = 2048;
const TABLE_SIZE: usize = 4096;
const TABLE_MASK: usize = TABLE_SIZE - 1;

#[derive(Clone, Copy)]
struct HashSlot {
    hash: u32,
    offset: u32,
    len: u16,
    id: u16,
}

#[inline(always)]
fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811c9dc5;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x01000193);
    }
    h
}

struct ThreadParseResult {
    endpoints: Vec<Vec<u8>>,
    records: Vec<(u32, u32)>, // (local_group, latency)
    errors: Vec<u32>,         // size: NUM_SLOTS * MAX_ENDPOINTS
}

fn parse_chunk(
    data: &[u8],
    chunk_start: usize,
    chunk_end: usize,
) -> ThreadParseResult {
    let mut table = [HashSlot { hash: 0, offset: 0, len: 0, id: 0 }; TABLE_SIZE];
    let mut endpoints: Vec<Vec<u8>> = Vec::with_capacity(2048);
    let mut records: Vec<(u32, u32)> = Vec::with_capacity((chunk_end - chunk_start) / 40);
    let mut errors = vec![0u32; NUM_SLOTS * MAX_ENDPOINTS];

    let mut idx = chunk_start;
    while idx < chunk_end {
        let line_start = idx;
        let day = (data[line_start + 8] - b'0') as u32 * 10 + (data[line_start + 9] - b'0') as u32;
        let hour = (data[line_start + 11] - b'0') as u32 * 10 + (data[line_start + 12] - b'0') as u32;
        let slot = if data[line_start + 6] == b'0' {
            (day - 30) * 24 + hour - 18
        } else {
            30 + (day - 1) * 24 + hour
        };

        idx += 28; // Skip timestamp + space
        while idx < chunk_end && data[idx] != b' ' {
            idx += 1;
        }
        idx += 1; // Skip space after method

        let ep_start = idx;
        while idx < chunk_end && data[idx] != b' ' {
            idx += 1;
        }
        let ep_len = idx - ep_start;
        let ep_bytes = &data[ep_start..idx];
        idx += 1; // Skip space after endpoint

        let is_5xx = data[idx] == b'5';
        idx += 4; // Skip status (3 chars) + space (1 char)

        let mut lat: u32 = 0;
        while idx < chunk_end && data[idx] >= b'0' && data[idx] <= b'9' {
            lat = lat * 10 + (data[idx] - b'0') as u32;
            idx += 1;
        }
        if idx < chunk_end && data[idx] == b'\r' {
            idx += 1;
        }
        if idx < chunk_end && data[idx] == b'\n' {
            idx += 1;
        }

        // Endpoint lookup
        let h = fnv1a(ep_bytes);
        let mut slot_idx = (h as usize) & TABLE_MASK;
        let ep_id: u16;
        loop {
            let entry = &mut table[slot_idx];
            if entry.len == 0 {
                // New endpoint
                let new_id = endpoints.len() as u16;
                entry.hash = h;
                entry.offset = ep_start as u32;
                entry.len = ep_len as u16;
                entry.id = new_id;
                endpoints.push(ep_bytes.to_vec());
                ep_id = new_id;
                break;
            } else if entry.len as usize == ep_len && entry.hash == h {
                let off = entry.offset as usize;
                if &data[off..off + ep_len] == ep_bytes {
                    ep_id = entry.id;
                    break;
                }
            }
            slot_idx = (slot_idx + 1) & TABLE_MASK;
        }

        let group = (slot << 11) | (ep_id as u32);
        records.push((group, lat));
        if is_5xx {
            errors[slot as usize * MAX_ENDPOINTS + ep_id as usize] += 1;
        }
    }

    ThreadParseResult {
        endpoints,
        records,
        errors,
    }
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
    boundaries[0] = 0;
    boundaries[num_threads] = total_size;

    for i in 1..num_threads {
        let approx = (total_size * i) / num_threads;
        let mut p = approx;
        while p < total_size && data[p] != b'\n' {
            p += 1;
        }
        if p < total_size {
            p += 1;
        }
        boundaries[i] = p;
    }

    let raw_ptr = ptr as usize;
    let mut handles = Vec::with_capacity(num_threads);

    for i in 0..num_threads {
        let start = boundaries[i];
        let end = boundaries[i + 1];
        let handle = std::thread::spawn(move || {
            let slice = unsafe { std::slice::from_raw_parts(raw_ptr as *const u8, total_size) };
            parse_chunk(slice, start, end)
        });
        handles.push(handle);
    }

    let mut results = Vec::with_capacity(num_threads);
    for h in handles {
        results.push(h.join().unwrap());
    }

    // Unify endpoints across all threads
    let mut all_endpoint_names: Vec<Vec<u8>> = Vec::with_capacity(2048);
    for r in &results {
        for ep in &r.endpoints {
            if !all_endpoint_names.contains(ep) {
                all_endpoint_names.push(ep.clone());
            }
        }
    }
    all_endpoint_names.sort_unstable();
    let num_endpoints = all_endpoint_names.len();

    // Mapping per thread: local_ep_id -> global_ep_id
    let mut thread_maps: Vec<Vec<usize>> = Vec::with_capacity(num_threads);
    for r in &results {
        let mut map = Vec::with_capacity(r.endpoints.len());
        for ep in &r.endpoints {
            let global_id = all_endpoint_names.binary_search(ep).unwrap();
            map.push(global_id);
        }
        thread_maps.push(map);
    }

    let total_groups = NUM_SLOTS * num_endpoints;
    let mut group_counts = vec![0u32; total_groups];
    let mut global_errors = vec![0u32; total_groups];

    // Compute counts & errors
    for (t, r) in results.iter().enumerate() {
        let map = &thread_maps[t];
        for &(packed_group, _) in &r.records {
            let slot = (packed_group >> 11) as usize;
            let local_ep = (packed_group & 0x7FF) as usize;
            let global_ep = map[local_ep];
            group_counts[slot * num_endpoints + global_ep] += 1;
        }
        for slot in 0..NUM_SLOTS {
            for (local_ep, &err) in r.errors[slot * MAX_ENDPOINTS..slot * MAX_ENDPOINTS + r.endpoints.len()].iter().enumerate() {
                if err > 0 {
                    let global_ep = map[local_ep];
                    global_errors[slot * num_endpoints + global_ep] += err;
                }
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
        for &(packed_group, lat) in &r.records {
            let slot = (packed_group >> 11) as usize;
            let local_ep = (packed_group & 0x7FF) as usize;
            let global_ep = map[local_ep];
            let g = slot * num_endpoints + global_ep;
            let pos = write_cursors[g];
            write_cursors[g] += 1;
            all_latencies[pos] = lat;
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

    // Output formatting
    let mut out_buffer = Vec::with_capacity(4 * 1024 * 1024);

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
        let hour_str = format!("2026-{:02}-{:02}T{:02}:00:00Z", month, day, clock_hour);

        for ep_id in 0..num_endpoints {
            let g = slot * num_endpoints + ep_id;
            let count = group_counts[g] as usize;
            if count == 0 {
                continue;
            }
            let start = offsets[g];
            let end = offsets[g + 1];
            let values = &all_latencies[start..end];

            let p50 = values[(50 * count + 99) / 100 - 1];
            let p95 = values[(95 * count + 99) / 100 - 1];
            let p99 = values[(99 * count + 99) / 100 - 1];

            let errors = global_errors[g] as u64;
            let error_rate = errors * 1_000_000 / count as u64;

            out_buffer.extend_from_slice(hour_str.as_bytes());
            out_buffer.push(b' ');
            out_buffer.extend_from_slice(&all_endpoint_names[ep_id]);
            out_buffer.push(b' ');
            let _ = write!(
                out_buffer,
                "{} {} {} {} {} {}\n",
                count, p50, p95, p99, errors, error_rate
            );
        }
    }

    out_file.write_all(&out_buffer).expect("write output");
}
