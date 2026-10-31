use std::fs::File;
use std::io::Write;
use std::os::unix::io::AsRawFd;

extern "C" {
    fn mmap(addr: *mut u8, length: usize, prot: i32, flags: i32, fd: i32, offset: i64) -> *mut u8;
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
            amount_min: 0,
            amount_max: 0,
            names_distinct: 0,
            name_bytes: 0,
            note_bytes: 0,
        }
    }
}

const TABLE_CAP: usize = 262144;
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
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    if h == 0 { 1 } else { h }
}

#[inline(always)]
fn read_uint(data: &[u8], mut p: usize) -> (u32, usize) {
    let mut val: u32 = 0;
    while p < data.len() && data[p] >= b'0' && data[p] <= b'9' {
        val = val * 10 + (data[p] - b'0') as u32;
        p += 1;
    }
    (val, p)
}

struct Field {
    raw_offset: u32,
    raw_len: u32,
    unescaped_bytes: u32,
}

#[inline(always)]
fn read_text(data: &[u8], mut p: usize) -> (Field, usize) {
    if data[p] != b'"' {
        let start = p;
        while p < data.len() && data[p] != b',' {
            p += 1;
        }
        let len = (p - start) as u32;
        (
            Field {
                raw_offset: start as u32,
                raw_len: len,
                unescaped_bytes: len,
            },
            p,
        )
    } else {
        p += 1; // Skip opening quote
        let start = p;
        let mut escapes = 0u32;
        loop {
            if data[p] != b'"' {
                p += 1;
                continue;
            }
            if p + 1 < data.len() && data[p + 1] == b'"' {
                p += 2;
                escapes += 1;
            } else {
                break;
            }
        }
        let finish = p;
        p += 1; // Skip closing quote
        let raw_len = (finish - start) as u32;
        (
            Field {
                raw_offset: start as u32,
                raw_len,
                unescaped_bytes: raw_len - escapes,
            },
            p,
        )
    }
}

fn map_file(path: &str) -> Option<(&'static [u8], usize)> {
    let file = File::open(path).ok()?;
    let metadata = file.metadata().ok()?;
    let size = metadata.len() as usize;
    if size == 0 {
        return Some((&[], 0));
    }
    let ptr = unsafe { mmap(std::ptr::null_mut(), size, 1, 2, file.as_raw_fd(), 0) };
    if ptr == usize::MAX as *mut u8 {
        return None;
    }
    let slice = unsafe { std::slice::from_raw_parts(ptr, size) };
    Some((slice, size))
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

    let (regions_data, regions_size) = map_file(regions_path).expect("open regions");
    let (records_data, records_size) = map_file(records_path).expect("open records");

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
    while p < regions_size && regions_data[p] != b'\n' {
        p += 1;
    }
    if p < regions_size {
        p += 1;
    }

    while p < regions_size {
        let (id, next_p) = read_uint(regions_data, p);
        p = next_p + 1; // Skip comma
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
        while p < regions_size && regions_data[p] != b'\n' {
            p += 1;
        }
        if p < regions_size {
            p += 1;
        }
    }

    let unmatched_idx = country_codes.len();
    let num_groups = unmatched_idx + 1;
    let mut groups = vec![Aggregate::new(); num_groups];

    // Open addressing table for distinct names
    let mut name_table = vec![
        NameEntry {
            hash: 0,
            offset: 0,
            len_plus_one: 0,
            countries: [0; 4],
        };
        TABLE_CAP
    ];

    p = 0;
    // Skip UTF-8 BOM if present at start
    if records_size >= 3 && records_data[0] == 0xEF && records_data[1] == 0xBB && records_data[2] == 0xBF {
        p = 3;
    }
    // Skip header line
    while p < records_size && records_data[p] != b'\n' {
        p += 1;
    }
    if p < records_size {
        p += 1;
    }

    while p < records_size {
        if p + 3 <= records_size && records_data[p] == 0xEF && records_data[p + 1] == 0xBB && records_data[p + 2] == 0xBF {
            p += 3;
        }
        let (id, next_p) = read_uint(records_data, p);
        p = next_p + 1; // Skip comma

        let (name_field, next_p) = read_text(records_data, p);
        p = next_p + 1; // Skip comma

        let (note_field, next_p) = read_text(records_data, p);
        p = next_p + 1; // Skip comma

        let (region_id, next_p) = read_uint(records_data, p);
        p = next_p + 1; // Skip comma

        let negative = records_data[p] == b'-';
        if negative {
            p += 1;
        }
        let (dollars, next_p) = read_uint(records_data, p);
        p = next_p + 1; // Skip '.'
        let tens = (records_data[p] - b'0') as i32;
        let units = (records_data[p + 1] - b'0') as i32;
        p += 2;
        let mut cents = (dollars as i32) * 100 + tens * 10 + units;
        if negative {
            cents = -cents;
        }
        let cents_i64 = cents as i64;

        if p < records_size && records_data[p] == b'\r' {
            p += 1;
        }
        if p < records_size && records_data[p] == b'\n' {
            p += 1;
        }

        let key = if (region_id as usize) < regions_table.len() {
            let k = regions_table[region_id as usize];
            if k == NO_COUNTRY {
                unmatched_idx
            } else {
                k as usize
            }
        } else {
            unmatched_idx
        };

        let g = &mut groups[key];
        if g.count == 0 || cents_i64 < g.amount_min {
            g.amount_min = cents_i64;
        }
        if g.count == 0 || cents_i64 > g.amount_max {
            g.amount_max = cents_i64;
        }
        g.count += 1;
        g.id_sum += id as u64;
        g.amount_sum += cents_i64;
        g.name_bytes += name_field.unescaped_bytes as u64;
        g.note_bytes += note_field.unescaped_bytes as u64;

        // Distinct name check
        let name_bytes = &records_data[name_field.raw_offset as usize..name_field.raw_offset as usize + name_field.raw_len as usize];
        let h = fnv1a_64(name_bytes);
        let mut slot = (h as usize) & TABLE_MASK;
        loop {
            let entry = &mut name_table[slot];
            if entry.len_plus_one == 0 {
                entry.hash = h;
                entry.offset = name_field.raw_offset;
                entry.len_plus_one = name_field.raw_len + 1;
                entry.countries[key >> 6] |= 1u64 << (key & 63);
                g.names_distinct += 1;
                break;
            } else if entry.len_plus_one == name_field.raw_len + 1 && entry.hash == h {
                let existing = &records_data[entry.offset as usize..entry.offset as usize + name_field.raw_len as usize];
                if existing == name_bytes {
                    let word = &mut entry.countries[key >> 6];
                    let bit = 1u64 << (key & 63);
                    if (*word & bit) == 0 {
                        *word |= bit;
                        g.names_distinct += 1;
                    }
                    break;
                }
            }
            slot = (slot + 1) & TABLE_MASK;
        }
    }

    // Sort countries alphabetically
    let mut order: Vec<usize> = (0..country_codes.len()).collect();
    order.sort_unstable_by_key(|&i| country_codes[i]);
    order.push(unmatched_idx);

    let mut out_buffer = Vec::with_capacity(64 * 1024);
    for &key in &order {
        let g = &groups[key];
        if g.count == 0 {
            continue;
        }
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
