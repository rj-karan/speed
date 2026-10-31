use std::fs::File;
use std::io::Write;
use std::os::unix::io::AsRawFd;

extern "C" {
    fn mmap(addr: *mut u8, len: usize, prot: i32, flags: i32, fd: i32, offset: i64) -> *mut u8;
}

#[derive(Clone, Copy)]
struct Aggregate {
    count: u64,
    id_sum: u64,
    amount_sum: i64,
    amount_min: i64,
    amount_max: i64,
    names_distinct: u64,
    name_bytes: u64,
    note_bytes: u64,
}

impl Aggregate {
    const fn new() -> Self {
        Self {
            count: 0,
            id_sum: 0,
            amount_sum: 0,
            amount_min: i64::MAX,
            amount_max: i64::MIN,
            names_distinct: 0,
            name_bytes: 0,
            note_bytes: 0,
        }
    }
}

const TABLE_CAP: usize = 131072;
const TABLE_MASK: usize = TABLE_CAP - 1;

#[derive(Clone, Copy)]
struct NameEntry {
    hash: u64,
    offset: u32,
    len_plus_one: u32,
    countries: [u64; 4],
}

#[inline(always)]
fn fnv1a_64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in bytes {
        h = (h ^ (b as u64)).wrapping_mul(0x100000001b3);
    }
    if h == 0 { 1 } else { h }
}

#[inline(always)]
unsafe fn read_uint(mut p: *const u8) -> (u32, *const u8) {
    let mut val: u32 = 0;
    while *p >= b'0' && *p <= b'9' {
        val = val * 10 + (*p - b'0') as u32;
        p = p.add(1);
    }
    (val, p)
}

fn map_file(path: &str) -> Option<(&'static [u8], usize, *mut u8)> {
    let file = File::open(path).ok()?;
    let metadata = file.metadata().ok()?;
    let sz = metadata.len() as usize;
    if sz == 0 {
        return Some((&[], 0, std::ptr::null_mut()));
    }
    let ptr = unsafe { mmap(std::ptr::null_mut(), sz, 1, 2, file.as_raw_fd(), 0) };
    if ptr == usize::MAX as *mut u8 {
        return None;
    }
    let slice = unsafe { std::slice::from_raw_parts(ptr, sz) };
    Some((slice, sz, ptr))
}

fn format_money(cents: i64) -> String {
    let sign = if cents < 0 { "-" } else { "" };
    let abs_c = cents.unsigned_abs();
    format!("{}{}.{:02}", sign, abs_c / 100, abs_c % 100)
}

fn main() {
    let regions_path = if std::path::Path::new("/data/regions.csv").exists() {
        "/data/regions.csv"
    } else {
        "data/c/regions.csv"
    };

    let records_path = if std::path::Path::new("/data/records.csv").exists() {
        "/data/records.csv"
    } else {
        "data/c/records.csv"
    };

    let (regions_data, regions_size, _) = map_file(regions_path).expect("open regions");
    let (records_data, records_size, rec_ptr) = map_file(records_path).expect("open records");

    let output_path = if std::path::Path::new("/out").is_dir() {
        "/out/aggregates.txt"
    } else {
        "aggregates.txt"
    };

    let mut out_file = File::create(output_path).expect("create output");

    if records_size == 0 {
        return;
    }

    const NO_COUNTRY: u16 = 65535;
    let mut regions_table = vec![NO_COUNTRY; 1_000_000];
    let mut code_to_index = [NO_COUNTRY; 26 * 26 * 26];
    let mut country_codes = Vec::with_capacity(300);

    let mut p = 0;
    while p < regions_size && regions_data[p] != b'\n' { p += 1; }
    if p < regions_size { p += 1; }

    while p < regions_size {
        let (id, next_p) = unsafe { read_uint(regions_data.as_ptr().add(p)) };
        p = (unsafe { next_p.offset_from(regions_data.as_ptr()) } as usize) + 1; // Skip comma
        let c0 = (regions_data[p] - b'A') as u16;
        let c1 = (regions_data[p + 1] - b'A') as u16;
        let c2 = (regions_data[p + 2] - b'A') as u16;
        let code = c0 * 676 + c1 * 26 + c2;

        let mut idx = code_to_index[code as usize];
        if idx == NO_COUNTRY {
            idx = country_codes.len() as u16;
            country_codes.push(code);
            code_to_index[code as usize] = idx;
        }
        if (id as usize) < regions_table.len() {
            regions_table[id as usize] = idx;
        }
        p += 3;
        while p < regions_size && regions_data[p] != b'\n' { p += 1; }
        if p < regions_size { p += 1; }
    }

    let unmatched_idx = country_codes.len();
    let num_groups = unmatched_idx + 1;

    let nthreads = std::thread::available_parallelism().map_or(12, |n| n.get()).min(12);
    let mut bounds = vec![0; nthreads + 1];
    p = 0;
    if records_size >= 3 && records_data[0] == 0xef && records_data[1] == 0xbb && records_data[2] == 0xbf { p = 3; }
    while p < records_size && records_data[p] != b'\n' { p += 1; }
    bounds[0] = p + 1;
    bounds[nthreads] = records_size;

    for i in 1..nthreads {
        let approx = (records_size * i) / nthreads;
        let mut q = approx;
        while q < approx + 100000 {
            if records_data[q] == b'\n' {
                let q_next = q + 1;
                if q_next < records_size && records_data[q_next] >= b'0' && records_data[q_next] <= b'9' {
                    if q >= 3 && records_data[q - 3] == b'.' && records_data[q - 2] >= b'0' && records_data[q - 2] <= b'9' && records_data[q - 1] >= b'0' && records_data[q - 1] <= b'9' {
                        bounds[i] = q_next;
                        break;
                    }
                }
            }
            q += 1;
        }
    }

    let regions_ptr = regions_table.as_ptr() as usize;
    let raw_ptr = rec_ptr as usize;

    let handles: Vec<_> = (0..nthreads).map(|t| {
        let start = bounds[t];
        let end = bounds[t + 1];
        std::thread::spawn(move || {
            let base = raw_ptr as *const u8;
            let reg_tab = regions_ptr as *const u16;
            let mut local_groups = vec![Aggregate::new(); num_groups];
            let mut local_table = vec![NameEntry { hash: 0, offset: 0, len_plus_one: 0, countries: [0; 4] }; TABLE_CAP];

            let mut p = unsafe { base.add(start) };
            let p_end = unsafe { base.add(end) };

            unsafe {
                while p < p_end {
                    if p.add(3) <= p_end && *p == 0xef && *p.add(1) == 0xbb && *p.add(2) == 0xbf {
                        p = p.add(3);
                    }
                    let (id, next_p) = read_uint(p);
                    p = next_p.add(1); // comma

                    let name_offset: u32;
                    let name_raw_len: u32;
                    let name_unescaped: u32;
                    if *p != b'"' {
                        let n_start = p;
                        while *p != b',' { p = p.add(1); }
                        name_offset = n_start.offset_from(base) as u32;
                        name_raw_len = p.offset_from(n_start) as u32;
                        name_unescaped = name_raw_len;
                    } else {
                        p = p.add(1); // skip opening quote
                        let n_start = p;
                        name_offset = n_start.offset_from(base) as u32;
                        let mut escapes = 0u32;
                        loop {
                            if *p != b'"' { p = p.add(1); continue; }
                            if *p.add(1) != b'"' { break; }
                            p = p.add(2);
                            escapes += 1;
                        }
                        name_raw_len = p.offset_from(n_start) as u32;
                        name_unescaped = name_raw_len - escapes;
                        p = p.add(1); // closing quote
                    }
                    let name_bytes = std::slice::from_raw_parts(base.add(name_offset as usize), name_raw_len as usize);
                    p = p.add(1); // comma

                    let note_unescaped: u32;
                    if *p != b'"' {
                        let n_start = p;
                        while *p != b',' { p = p.add(1); }
                        note_unescaped = p.offset_from(n_start) as u32;
                    } else {
                        p = p.add(1);
                        let n_start = p;
                        let mut escapes = 0u32;
                        loop {
                            if *p != b'"' { p = p.add(1); continue; }
                            if *p.add(1) != b'"' { break; }
                            p = p.add(2);
                            escapes += 1;
                        }
                        note_unescaped = p.offset_from(n_start) as u32 - escapes;
                        p = p.add(1);
                    }
                    p = p.add(1); // comma

                    let (region_id, next_p) = read_uint(p);
                    p = next_p.add(1); // comma

                    let negative = *p == b'-';
                    if negative { p = p.add(1); }
                    let (dollars, next_p) = read_uint(p);
                    p = next_p.add(1); // '.'
                    let tens = (*p - b'0') as i32;
                    let units = (*p.add(1) - b'0') as i32;
                    p = p.add(2);
                    let mut cents = (dollars as i32) * 100 + tens * 10 + units;
                    if negative { cents = -cents; }
                    let cents_i64 = cents as i64;

                    if p < p_end && *p == b'\r' { p = p.add(1); }
                    if p < p_end && *p == b'\n' { p = p.add(1); }

                    let key = if (region_id as usize) < 1_000_000 {
                        let k = *reg_tab.add(region_id as usize);
                        if k == NO_COUNTRY { unmatched_idx } else { k as usize }
                    } else {
                        unmatched_idx
                    };

                    let g = &mut local_groups[key];
                    if g.count == 0 || cents_i64 < g.amount_min { g.amount_min = cents_i64; }
                    if g.count == 0 || cents_i64 > g.amount_max { g.amount_max = cents_i64; }
                    g.count += 1;
                    g.id_sum += id as u64;
                    g.amount_sum += cents_i64;
                    g.name_bytes += name_unescaped as u64;
                    g.note_bytes += note_unescaped as u64;

                    // Name distinct table
                    let h = fnv1a_64(name_bytes);
                    let mut s = (h as usize) & TABLE_MASK;
                    loop {
                        let entry = &mut local_table[s];
                        if entry.len_plus_one == 0 {
                            entry.hash = h;
                            entry.offset = name_offset;
                            entry.len_plus_one = name_raw_len + 1;
                            entry.countries[key >> 6] |= 1u64 << (key & 63);
                            break;
                        } else if entry.len_plus_one == name_raw_len + 1 && entry.hash == h {
                            let existing = std::slice::from_raw_parts(base.add(entry.offset as usize), name_raw_len as usize);
                            if existing == name_bytes {
                                entry.countries[key >> 6] |= 1u64 << (key & 63);
                                break;
                            }
                        }
                        s = (s + 1) & TABLE_MASK;
                    }
                }
            }
            (local_groups, local_table)
        })
    }).collect();

    let mut all_groups = vec![Aggregate::new(); num_groups];
    let mut global_name_table = vec![NameEntry { hash: 0, offset: 0, len_plus_one: 0, countries: [0; 4] }; 262144];
    const GLOBAL_MASK: usize = 262144 - 1;

    for h in handles {
        let (lg, lt) = h.join().unwrap();
        for k in 0..num_groups {
            let g = &mut all_groups[k];
            let src = &lg[k];
            if src.count > 0 {
                g.count += src.count;
                g.id_sum += src.id_sum;
                g.amount_sum += src.amount_sum;
                g.name_bytes += src.name_bytes;
                g.note_bytes += src.note_bytes;
                if g.amount_min == i64::MAX || src.amount_min < g.amount_min { g.amount_min = src.amount_min; }
                if g.amount_max == i64::MIN || src.amount_max > g.amount_max { g.amount_max = src.amount_max; }
            }
        }
        for entry in lt {
            if entry.len_plus_one > 0 {
                let name_len = (entry.len_plus_one - 1) as usize;
                let name_bytes = &records_data[entry.offset as usize..entry.offset as usize + name_len];
                let h = entry.hash;
                let mut s = (h as usize) & GLOBAL_MASK;
                loop {
                    let g_entry = &mut global_name_table[s];
                    if g_entry.len_plus_one == 0 {
                        g_entry.hash = h;
                        g_entry.offset = entry.offset;
                        g_entry.len_plus_one = entry.len_plus_one;
                        for i in 0..4 { g_entry.countries[i] |= entry.countries[i]; }
                        break;
                    } else if g_entry.len_plus_one == entry.len_plus_one && g_entry.hash == h {
                        let existing = &records_data[g_entry.offset as usize..g_entry.offset as usize + name_len];
                        if existing == name_bytes {
                            for i in 0..4 { g_entry.countries[i] |= entry.countries[i]; }
                            break;
                        }
                    }
                    s = (s + 1) & GLOBAL_MASK;
                }
            }
        }
    }

    // Count distinct names per country
    for entry in &global_name_table {
        if entry.len_plus_one > 0 {
            for word_idx in 0..4 {
                let mut w = entry.countries[word_idx];
                while w != 0 {
                    let bit = w.trailing_zeros() as usize;
                    let k = (word_idx << 6) | bit;
                    if k < num_groups {
                        all_groups[k].names_distinct += 1;
                    }
                    w &= w - 1;
                }
            }
        }
    }

    let mut order: Vec<usize> = (0..country_codes.len()).collect();
    order.sort_unstable_by_key(|&i| country_codes[i]);
    order.push(unmatched_idx);

    let mut out_buffer = Vec::with_capacity(64 * 1024);
    for &key in &order {
        let g = &all_groups[key];
        if g.count == 0 { continue; }
        if key == unmatched_idx {
            out_buffer.extend_from_slice(b"_UNMATCHED");
        } else {
            let code = country_codes[key];
            let c0 = b'A' + (code / 676) as u8;
            let c1 = b'A' + ((code / 26) % 26) as u8;
            let c2 = b'A' + (code % 26) as u8;
            out_buffer.extend_from_slice(&[c0, c1, c2]);
        }
        let _ = write!(
            out_buffer,
            " {} {} {} {} {} {} {} {}\n",
            g.count,
            g.id_sum,
            format_money(g.amount_sum),
            format_money(g.amount_min),
            format_money(g.amount_max),
            g.names_distinct,
            g.name_bytes,
            g.note_bytes
        );
    }

    out_file.write_all(&out_buffer).expect("write output");
}
