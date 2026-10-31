use std::fmt::Write as FmtWrite;
use std::fs::File;
use std::io::Write;
use std::os::unix::io::AsRawFd;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

extern "C" {
    fn mmap(addr: *mut u8, length: usize, prot: i32, flags: i32, fd: i32, offset: i64) -> *mut u8;
}

const TABLE_CAP: usize = 262144;
const TABLE_MASK: usize = TABLE_CAP - 1;

#[derive(Clone, Copy)]
struct TermEntry {
    hash: u32,
    offset: u32,
    len: u16,
    id: u32,
}

#[inline(always)]
fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811c9dc5;
    for &b in bytes {
        h = (h ^ (b as u32)).wrapping_mul(0x01000193);
    }
    h
}

struct Query {
    phrase: bool,
    terms: Vec<u32>,
    ops: Vec<u8>,
}

#[inline(always)]
fn intern(
    word: &[u8],
    query_data: &[u8],
    table: &mut [TermEntry],
    terms: &mut Vec<TermEntry>,
) -> u32 {
    let h = fnv1a(word);
    let mut slot = (h as usize) & TABLE_MASK;
    loop {
        let entry = &mut table[slot];
        if entry.len == 0 {
            let id = terms.len() as u32;
            entry.hash = h;
            entry.offset = (word.as_ptr() as usize - query_data.as_ptr() as usize) as u32;
            entry.len = word.len() as u16;
            entry.id = id;
            terms.push(*entry);
            return id;
        }
        if entry.hash == h && entry.len as usize == word.len() {
            let off = entry.offset as usize;
            if &query_data[off..off + word.len()] == word {
                return entry.id;
            }
        }
        slot = (slot + 1) & TABLE_MASK;
    }
}

#[inline(always)]
unsafe fn next_token_fast(
    p: *const u8,
    end: *const u8,
    query_data: *const u8,
    table: *const TermEntry,
) -> (u32, *const u8) {
    let mut cur = p;
    while cur < end && !(*cur).is_ascii_alphanumeric() {
        cur = cur.add(1);
    }
    if cur >= end {
        return (u32::MAX, cur);
    }
    let word_start = cur;
    let mut h: u32 = 0x811c9dc5;
    let mut len: usize = 0;
    while cur < end && (*cur).is_ascii_alphanumeric() {
        let mut c = *cur;
        if c >= b'A' && c <= b'Z' {
            c += 32;
        }
        if len < 33 {
            h = (h ^ (c as u32)).wrapping_mul(0x01000193);
            len += 1;
        }
        cur = cur.add(1);
    }

    let mut slot = (h as usize) & TABLE_MASK;
    loop {
        let entry = &*table.add(slot);
        if entry.len == 0 {
            return (u32::MAX - 1, cur);
        }
        if entry.hash == h && entry.len as usize == len {
            let mut match_ok = true;
            for i in 0..len {
                let mut c = *word_start.add(i);
                if c >= b'A' && c <= b'Z' {
                    c += 32;
                }
                if *query_data.add(entry.offset as usize + i) != c {
                    match_ok = false;
                    break;
                }
            }
            if match_ok {
                return (entry.id, cur);
            }
        }
        slot = (slot + 1) & TABLE_MASK;
    }
}

fn intersect(a: &[u32], b: &[u32]) -> Vec<u32> {
    let mut res = Vec::with_capacity(a.len().min(b.len()));
    let (mut i, mut j) = (0, 0);
    while i < a.len() && j < b.len() {
        if a[i] < b[j] {
            i += 1;
        } else if a[i] > b[j] {
            j += 1;
        } else {
            res.push(a[i]);
            i += 1;
            j += 1;
        }
    }
    res
}

fn unite(a: &[u32], b: &[u32]) -> Vec<u32> {
    let mut res = Vec::with_capacity(a.len() + b.len());
    let (mut i, mut j) = (0, 0);
    while i < a.len() || j < b.len() {
        if j == b.len() || (i < a.len() && a[i] < b[j]) {
            res.push(a[i]);
            i += 1;
        } else if i == a.len() || b[j] < a[i] {
            res.push(b[j]);
            j += 1;
        } else {
            res.push(a[i]);
            i += 1;
            j += 1;
        }
    }
    res
}

fn subtract(a: &[u32], b: &[u32]) -> Vec<u32> {
    let mut res = Vec::with_capacity(a.len());
    let (mut i, mut j) = (0, 0);
    while i < a.len() {
        while j < b.len() && b[j] < a[i] {
            j += 1;
        }
        if j == b.len() || a[i] != b[j] {
            res.push(a[i]);
        }
        i += 1;
    }
    res
}

unsafe fn contains_phrase(
    doc_text: *const u8,
    doc_len: usize,
    terms: &[u32],
    query_data: *const u8,
    table: *const TermEntry,
) -> bool {
    let mut seq = Vec::with_capacity(64);
    let mut p = doc_text;
    let end = doc_text.add(doc_len);
    while p < end {
        let (id, next_p) = next_token_fast(p, end, query_data, table);
        p = next_p;
        if id == u32::MAX {
            break;
        }
        seq.push(id);
    }
    if seq.len() < terms.len() {
        return false;
    }
    for i in 0..=(seq.len() - terms.len()) {
        if &seq[i..i + terms.len()] == terms {
            return true;
        }
    }
    false
}

fn map_file(path: &str) -> Option<(&'static [u8], usize, *mut u8)> {
    let file = File::open(path).ok()?;
    let metadata = file.metadata().ok()?;
    let size = metadata.len() as usize;
    if size == 0 {
        return Some((&[], 0, std::ptr::null_mut()));
    }
    let ptr = unsafe { mmap(std::ptr::null_mut(), size, 1, 2, file.as_raw_fd(), 0) };
    if ptr == usize::MAX as *mut u8 {
        return None;
    }
    let slice = unsafe { std::slice::from_raw_parts(ptr, size) };
    Some((slice, size, ptr))
}

fn main() {
    let queries_path = if std::path::Path::new("/data/queries.txt").exists() {
        "/data/queries.txt"
    } else {
        "data/b/queries.txt"
    };

    let docs_path = if std::path::Path::new("/data/docs.txt").exists() {
        "/data/docs.txt"
    } else {
        "data/b/docs.txt"
    };

    let (query_data, query_size, _) = map_file(queries_path).expect("open queries");
    let (_, docs_size, docs_ptr) = map_file(docs_path).expect("open docs");

    let output_path = if std::path::Path::new("/out").is_dir() {
        "/out/results.txt"
    } else {
        "results.txt"
    };

    let mut out_file = File::create(output_path).expect("create output");

    if query_size == 0 || docs_size == 0 {
        return;
    }

    let mut term_table = vec![
        TermEntry {
            hash: 0,
            offset: 0,
            len: 0,
            id: 0,
        };
        TABLE_CAP
    ];
    let mut term_list = Vec::with_capacity(100_000);
    let mut queries = Vec::with_capacity(60_000);

    let mut p = 0;
    while p < query_size {
        let is_phrase = query_data[p] == b'P';
        p += if is_phrase { 8 } else { 5 }; // Skip 'PHRASE "' or 'BOOL '
        let mut q_terms = Vec::with_capacity(4);
        let mut q_ops = Vec::with_capacity(4);

        if is_phrase {
            while p < query_size && query_data[p] != b'\n' && query_data[p] != b'"' {
                let start = p;
                while p < query_size && query_data[p] != b' ' && query_data[p] != b'"' && query_data[p] != b'\n' {
                    p += 1;
                }
                let word = &query_data[start..p];
                let tid = intern(word, query_data, &mut term_table, &mut term_list);
                q_terms.push(tid);
                if p < query_size && query_data[p] == b' ' {
                    p += 1;
                }
            }
            if p < query_size && query_data[p] == b'"' {
                p += 1;
            }
        } else {
            while p < query_size && query_data[p] != b'\n' {
                let start = p;
                while p < query_size && query_data[p] != b' ' && query_data[p] != b'\n' {
                    p += 1;
                }
                let word = &query_data[start..p];
                let tid = intern(word, query_data, &mut term_table, &mut term_list);
                q_terms.push(tid);
                if p < query_size && query_data[p] == b' ' {
                    p += 1;
                    q_ops.push(query_data[p]);
                    while p < query_size && query_data[p] != b' ' {
                        p += 1;
                    }
                    if p < query_size && query_data[p] == b' ' {
                        p += 1;
                    }
                }
            }
        }

        if p < query_size && query_data[p] == b'\r' {
            p += 1;
        }
        if p < query_size && query_data[p] == b'\n' {
            p += 1;
        }

        queries.push(Query {
            phrase: is_phrase,
            terms: q_terms,
            ops: q_ops,
        });
    }

    // Parse docs offsets
    let mut docs_spans = Vec::with_capacity(300_000);
    let mut dp = docs_ptr as *const u8;
    let d_end = unsafe { dp.add(docs_size) };
    unsafe {
        while dp < d_end {
            while dp < d_end && *dp != b'\t' {
                dp = dp.add(1);
            }
            dp = dp.add(1); // Skip tab
            let start = dp;
            while dp < d_end && *dp != b'\n' {
                dp = dp.add(1);
            }
            let mut finish = dp;
            if finish > start && *finish.sub(1) == b'\r' {
                finish = finish.sub(1);
            }
            docs_spans.push((start.offset_from(docs_ptr as *const u8) as u32, finish.offset_from(start) as u32));
            if dp < d_end && *dp == b'\n' {
                dp = dp.add(1);
            }
        }
    }

    let num_docs = docs_spans.len();
    let num_terms = term_list.len();
    let num_workers = std::thread::available_parallelism().map_or(12, |n| n.get()).min(12);

    let docs_spans = Arc::new(docs_spans);
    let term_table = Arc::new(term_table);
    let docs_data_ptr = docs_ptr as usize;
    let query_data_ptr = query_data.as_ptr() as usize;

    // Parallel indexing
    let mut build_handles = Vec::with_capacity(num_workers);
    for w in 0..num_workers {
        let spans_ref = Arc::clone(&docs_spans);
        let table_ref = Arc::clone(&term_table);
        let begin = num_docs * w / num_workers;
        let finish = num_docs * (w + 1) / num_workers;

        let handle = std::thread::spawn(move || {
            let mut local_postings: Vec<Vec<u32>> = (0..num_terms).map(|_| Vec::new()).collect();
            let mut last_doc = vec![u32::MAX; num_terms];
            let q_base = query_data_ptr as *const u8;
            let d_base = docs_data_ptr as *const u8;
            let t_base = table_ref.as_ptr();

            for doc_id in begin..finish {
                let (off, len) = spans_ref[doc_id];
                let text_ptr = unsafe { d_base.add(off as usize) };
                let end_ptr = unsafe { text_ptr.add(len as usize) };
                let mut pos = text_ptr;

                unsafe {
                    while pos < end_ptr {
                        let (id, next_p) = next_token_fast(pos, end_ptr, q_base, t_base);
                        pos = next_p;
                        if id == u32::MAX {
                            break;
                        }
                        if id != u32::MAX - 1 && last_doc[id as usize] != doc_id as u32 {
                            last_doc[id as usize] = doc_id as u32;
                            local_postings[id as usize].push(doc_id as u32);
                        }
                    }
                }
            }
            local_postings
        });
        build_handles.push(handle);
    }

    let mut parts = Vec::with_capacity(num_workers);
    for h in build_handles {
        parts.push(h.join().unwrap());
    }

    // Merge postings
    let mut postings: Vec<Vec<u32>> = Vec::with_capacity(num_terms);
    for id in 0..num_terms {
        let total_size: usize = parts.iter().map(|p| p[id].len()).sum();
        let mut list = Vec::with_capacity(total_size);
        for part in &parts {
            list.extend_from_slice(&part[id]);
        }
        postings.push(list);
    }
    drop(parts);

    // Parallel query evaluation
    let num_queries = queries.len();
    let queries = Arc::new(queries);
    let postings = Arc::new(postings);
    let next_query = Arc::new(AtomicUsize::new(0));

    let mut query_handles = Vec::with_capacity(num_workers);
    for _ in 0..num_workers {
        let nq = Arc::clone(&next_query);
        let q_ref = Arc::clone(&queries);
        let p_ref = Arc::clone(&postings);
        let spans_ref = Arc::clone(&docs_spans);
        let table_ref = Arc::clone(&term_table);

        let handle = std::thread::spawn(move || {
            let mut thread_output = Vec::with_capacity(num_queries / num_workers + 16);
            let q_base = query_data_ptr as *const u8;
            let d_base = docs_data_ptr as *const u8;
            let t_base = table_ref.as_ptr();

            loop {
                let qid = nq.fetch_add(1, Ordering::Relaxed);
                if qid >= q_ref.len() {
                    break;
                }
                let q = &q_ref[qid];
                let mut result = Vec::new();

                if q.phrase {
                    if !q.terms.is_empty() {
                        let mut rarest = q.terms[0];
                        for &id in &q.terms {
                            if p_ref[id as usize].len() < p_ref[rarest as usize].len() {
                                rarest = id;
                            }
                        }
                        for &doc in &p_ref[rarest as usize] {
                            let mut candidate = true;
                            for &id in &q.terms {
                                if p_ref[id as usize].binary_search(&doc).is_err() {
                                    candidate = false;
                                    break;
                                }
                            }
                            if candidate {
                                let (off, len) = spans_ref[doc as usize];
                                let doc_text = unsafe { d_base.add(off as usize) };
                                if unsafe { contains_phrase(doc_text, len as usize, &q.terms, q_base, t_base) } {
                                    result.push(doc);
                                }
                            }
                        }
                    }
                } else if !q.terms.is_empty() {
                    result = p_ref[q.terms[0] as usize].clone();
                    for i in 0..q.ops.len() {
                        let next = &p_ref[q.terms[i + 1] as usize];
                        match q.ops[i] {
                            b'A' => result = intersect(&result, next),
                            b'O' => result = unite(&result, next),
                            _ => result = subtract(&result, next),
                        }
                    }
                }

                let count = result.len();
                let sum: u64 = result.iter().map(|&x| x as u64).sum();
                let mut line = String::new();
                let _ = write!(line, "{} {}", count, sum);
                let limit = count.min(10);
                for &doc in &result[..limit] {
                    let _ = write!(line, " {}", doc);
                }
                line.push('\n');
                thread_output.push((qid, line));
            }
            thread_output
        });
        query_handles.push(handle);
    }

    let mut all_results: Vec<String> = (0..num_queries).map(|_| String::new()).collect();
    for h in query_handles {
        let chunk = h.join().unwrap();
        for (qid, line) in chunk {
            all_results[qid] = line;
        }
    }

    let mut out_buffer = Vec::with_capacity(num_queries * 40);
    for line in all_results {
        out_buffer.extend_from_slice(line.as_bytes());
    }

    out_file.write_all(&out_buffer).expect("write output");
}
