use std::arch::x86_64::*;
use std::cell::UnsafeCell;
use std::fmt::Write as FmtWrite;
use std::fs::File;
use std::io::{self, Write};
use std::os::fd::AsRawFd;
use std::ptr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;

const CAP: usize = 1 << 17;
#[repr(C, align(64))]
struct Slot {
    hash: AtomicU64,
    offset: UnsafeCell<u32>,
    len: UnsafeCell<u32>,
    prefix: UnsafeCell<[u8; 16]>,
    countries: [AtomicU64; 4],
}
// The hash publishes the immutable key with release/acquire ordering. Membership
// words are atomic; only the successful 0->1 claimant writes the key fields.
unsafe impl Sync for Slot {}
impl Slot {
    const fn new() -> Self {
        Self {
            hash: AtomicU64::new(0),
            offset: UnsafeCell::new(0),
            len: UnsafeCell::new(0),
            prefix: UnsafeCell::new([0; 16]),
            countries: [const { AtomicU64::new(0) }; 4],
        }
    }
}
static TABLE: [Slot; CAP] = [const { Slot::new() }; CAP];

unsafe extern "C" {
    fn mmap(
        addr: *mut std::ffi::c_void,
        len: usize,
        prot: i32,
        flags: i32,
        fd: i32,
        offset: i64,
    ) -> *mut std::ffi::c_void;
}
fn map_file(path: &str) -> io::Result<&'static [u8]> {
    let f = File::open(path)?;
    let size = f.metadata()?.len() as usize;
    if size == 0 {
        return Ok(&[]);
    }
    // Linux keeps the mapping valid after the file descriptor is closed. Both
    // input mappings live until process exit and are never mutated.
    unsafe {
        let p = mmap(ptr::null_mut(), size, 1, 1, f.as_raw_fd(), 0);
        if p as isize == -1 {
            return Err(io::Error::last_os_error());
        }
        Ok(std::slice::from_raw_parts(p.cast(), size))
    }
}
#[inline(always)]
unsafe fn decimal(data: &[u8], p: &mut usize) -> u32 {
    let mut n = 0u32;
    while (*data.get_unchecked(*p)).is_ascii_digit() {
        n = n * 10 + (*data.get_unchecked(*p) - b'0') as u32;
        *p += 1;
    }
    n
}
#[inline(always)]
unsafe fn packed_decimal(data: &[u8], p: &mut usize) -> u32 {
    if data.len() - *p < 8 {
        return decimal(data, p);
    }
    let bytes = ptr::read_unaligned(data.as_ptr().add(*p).cast::<u64>());
    let stop = (bytes.wrapping_add(0x4646464646464646) | bytes.wrapping_sub(0x3030303030303030))
        & 0x8080808080808080;
    let n = if stop == 0 {
        8
    } else {
        stop.trailing_zeros() as usize / 8
    };
    if n == 0 {
        return 0;
    }
    let mut value = (bytes & 0x0f0f0f0f0f0f0f0f) << ((8 - n) * 8);
    value = (value.wrapping_mul(10).wrapping_add(value >> 8)) & 0x00ff00ff00ff00ff;
    value = (value.wrapping_mul(100).wrapping_add(value >> 16)) & 0x0000ffff0000ffff;
    let mut result = value.wrapping_mul(10000).wrapping_add(value >> 32) as u32;
    *p += n;
    while (*data.get_unchecked(*p)).is_ascii_digit() {
        result = result * 10 + (*data.get_unchecked(*p) - b'0') as u32;
        *p += 1;
    }
    result
}
#[inline(always)]
unsafe fn text(data: &[u8], p: &mut usize) -> (usize, usize, u64) {
    if *data.get_unchecked(*p) != b'"' {
        let start = *p;
        while *data.get_unchecked(*p) != b',' {
            *p += 1;
        }
        return (start, *p - start, (*p - start) as u64);
    }
    *p += 1;
    let start = *p;
    let mut escapes = 0u64;
    loop {
        while data.len() - *p >= 32 {
            let bytes = _mm256_loadu_si256(data.as_ptr().add(*p).cast());
            let mask =
                _mm256_movemask_epi8(_mm256_cmpeq_epi8(bytes, _mm256_set1_epi8(b'"' as i8))) as u32;
            if mask != 0 {
                *p += mask.trailing_zeros() as usize;
                break;
            }
            *p += 32;
        }
        while *data.get_unchecked(*p) != b'"' {
            *p += 1;
        }
        if *data.get_unchecked(*p + 1) != b'"' {
            break;
        }
        *p += 2;
        escapes += 1;
    }
    let length = *p - start;
    *p += 1;
    (start, length, length as u64 - escapes)
}
#[inline(always)]
fn name_hash(name: &[u8]) -> u64 {
    let mut first = 0u64;
    let mut last = 0u64;
    let n = name.len();
    unsafe {
        if n >= 8 {
            first = ptr::read_unaligned(name.as_ptr().cast());
            last = ptr::read_unaligned(name.as_ptr().add(n - 8).cast());
        } else if n >= 4 {
            first = ptr::read_unaligned(name.as_ptr().cast::<u32>()) as u64;
            last = ptr::read_unaligned(name.as_ptr().add(n - 4).cast::<u32>()) as u64;
        } else {
            for (i, &b) in name.iter().enumerate() {
                first |= (b as u64) << (i * 8);
            }
        }
    }
    let h =
        first.wrapping_mul(0x9e3779b185ebca87) ^ last.wrapping_mul(0xc2b2ae3d27d4eb4f) ^ n as u64;
    let h = (h ^ (h >> 29)).wrapping_mul(0x165667b19e3779f9);
    if h < 2 {
        h + 2
    } else {
        h
    }
}
#[derive(Clone, Copy, Default)]
struct Aggregate {
    count: u64,
    id_sum: u64,
    name_bytes: u64,
    note_bytes: u64,
    names_distinct: u64,
    amount_sum: i64,
    amount_min: i32,
    amount_max: i32,
}
unsafe fn parse_range(
    data: &[u8],
    begin: usize,
    finish: usize,
    regions: &[u8],
    groups: usize,
) -> Vec<Aggregate> {
    let mut out = vec![Aggregate::default(); groups];
    let mut p = begin;
    while p < finish {
        if data.get(p..p + 3) == Some(&[0xef, 0xbb, 0xbf]) {
            p += 3;
        }
        let id = packed_decimal(data, &mut p);
        p += 1;
        let (name_start, name_len, name_bytes) = text(data, &mut p);
        p += 1;
        let name = data.get_unchecked(name_start..name_start + name_len);
        let hash = name_hash(name);
        _mm_prefetch(
            (&TABLE[hash as usize & (CAP - 1)] as *const Slot).cast(),
            _MM_HINT_T0,
        );
        let (_, _, note_bytes) = text(data, &mut p);
        p += 1;
        let region = packed_decimal(data, &mut p) as usize;
        p += 1;
        let negative = *data.get_unchecked(p) == b'-';
        p += negative as usize;
        let mut cents = packed_decimal(data, &mut p) as i32 * 100;
        p += 1;
        cents += (*data.get_unchecked(p) - b'0') as i32 * 10
            + (*data.get_unchecked(p + 1) - b'0') as i32;
        p += 2;
        if negative {
            cents = -cents;
        }
        if data.get(p) == Some(&b'\r') {
            p += 1;
        }
        if data.get(p) == Some(&b'\n') {
            p += 1;
        }
        let country = regions.get(region).copied().unwrap_or(255) as usize;
        let key = if country == 255 { groups - 1 } else { country };
        let g = out.get_unchecked_mut(key);
        if g.count == 0 || cents < g.amount_min {
            g.amount_min = cents;
        }
        if g.count == 0 || cents > g.amount_max {
            g.amount_max = cents;
        }
        g.count += 1;
        g.id_sum += id as u64;
        g.amount_sum += cents as i64;
        g.name_bytes += name_bytes;
        g.note_bytes += note_bytes;
        let mut index = hash as usize & (CAP - 1);
        let slot = loop {
            let s = TABLE.get_unchecked(index);
            let mut h = s.hash.load(Ordering::Acquire);
            while h == 1 {
                std::hint::spin_loop();
                h = s.hash.load(Ordering::Acquire);
            }
            if h == 0 {
                if s.hash
                    .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Relaxed)
                    .is_ok()
                {
                    *s.offset.get() = name_start as u32;
                    *s.len.get() = name_len as u32;
                    ptr::copy_nonoverlapping(
                        name.as_ptr(),
                        (*s.prefix.get()).as_mut_ptr(),
                        name_len.min(16),
                    );
                    s.hash.store(hash, Ordering::Release);
                    break s;
                }
                h = s.hash.load(Ordering::Acquire);
                while h == 1 {
                    std::hint::spin_loop();
                    h = s.hash.load(Ordering::Acquire);
                }
            }
            if h == hash && *s.len.get() as usize == name_len {
                let prefix = &*s.prefix.get();
                let first = name_len.min(16);
                if prefix[..first] == name[..first]
                    && (name_len <= 16
                        || data.get_unchecked(
                            *s.offset.get() as usize + 16..*s.offset.get() as usize + name_len,
                        ) == &name[16..])
                {
                    break s;
                }
            }
            index = (index + 1) & (CAP - 1);
        };
        let bit = 1u64 << (key & 63);
        let mask = &slot.countries[key >> 6];
        if mask.load(Ordering::Relaxed) & bit == 0
            && mask.fetch_or(bit, Ordering::Relaxed) & bit == 0
        {
            g.names_distinct += 1;
        }
    }
    out
}
fn money(cents: i64) -> String {
    format!(
        "{}{}.{:02}",
        if cents < 0 { "-" } else { "" },
        cents.abs() / 100,
        cents.abs() % 100
    )
}
fn main() -> io::Result<()> {
    let lookup = map_file("/data/regions.csv")?;
    let records = map_file("/data/records.csv")?;
    assert!(records.len() <= u32::MAX as usize);
    let mut regions = vec![255u8; 1_000_000];
    let mut code_index = vec![u16::MAX; 26 * 26 * 26];
    let mut codes = Vec::new();
    let mut p = lookup.iter().position(|&b| b == b'\n').unwrap() + 1;
    while p < lookup.len() {
        let id = unsafe { decimal(lookup, &mut p) } as usize;
        p += 1;
        let code = (lookup[p] - b'A') as usize * 26 * 26
            + (lookup[p + 1] - b'A') as usize * 26
            + (lookup[p + 2] - b'A') as usize;
        if code_index[code] == u16::MAX {
            code_index[code] = codes.len() as u16;
            codes.push(code as u16);
        }
        regions[id] = code_index[code] as u8;
        p += 4;
    }
    let groups = codes.len() + 1;
    let workers = thread::available_parallelism().map_or(12, |n| n.get().min(12));
    let mut bounds = vec![records.len(); workers + 1];
    bounds[0] = records.iter().position(|&b| b == b'\n').unwrap() + 1;
    for i in 1..workers {
        let mut scan = records.len() * i / workers;
        while scan < records.len() {
            let Some(lf) = records[scan..].iter().position(|&b| b == b'\n') else {
                break;
            };
            let start = scan + lf + 1;
            scan = start;
            if records.get(scan..scan + 3) == Some(&[0xef, 0xbb, 0xbf]) {
                scan += 3;
            }
            if records
                .get(scan)
                .is_some_and(|&b| (b'1'..=b'9').contains(&b))
            {
                bounds[i] = start;
                break;
            }
        }
    }
    let parts = thread::scope(|scope| {
        let handles: Vec<_> = bounds
            .windows(2)
            .map(|b| {
                let regions = &regions;
                scope.spawn(move || unsafe { parse_range(records, b[0], b[1], regions, groups) })
            })
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().unwrap())
            .collect::<Vec<_>>()
    });
    let mut totals = vec![Aggregate::default(); groups];
    for part in parts {
        for (g, a) in totals.iter_mut().zip(part) {
            if a.count == 0 {
                continue;
            }
            if g.count == 0 || a.amount_min < g.amount_min {
                g.amount_min = a.amount_min;
            }
            if g.count == 0 || a.amount_max > g.amount_max {
                g.amount_max = a.amount_max;
            }
            g.count += a.count;
            g.id_sum += a.id_sum;
            g.amount_sum += a.amount_sum;
            g.names_distinct += a.names_distinct;
            g.name_bytes += a.name_bytes;
            g.note_bytes += a.note_bytes;
        }
    }
    let mut order: Vec<_> = (0..codes.len()).collect();
    order.sort_unstable_by_key(|&i| codes[i]);
    order.push(groups - 1);
    let mut output = String::with_capacity(20000);
    for key in order {
        let g = totals[key];
        if g.count == 0 {
            continue;
        }
        if key == groups - 1 {
            output.push_str("_UNMATCHED");
        } else {
            let code = codes[key] as usize;
            output.push((b'A' + (code / (26 * 26)) as u8) as char);
            output.push((b'A' + ((code / 26) % 26) as u8) as char);
            output.push((b'A' + (code % 26) as u8) as char);
        }
        writeln!(
            output,
            " {} {} {} {} {} {} {} {}",
            g.count,
            g.id_sum,
            money(g.amount_sum),
            money(g.amount_min as i64),
            money(g.amount_max as i64),
            g.names_distinct,
            g.name_bytes,
            g.note_bytes
        )
        .unwrap();
    }
    File::create("/out/aggregates.txt")?.write_all(output.as_bytes())
}
