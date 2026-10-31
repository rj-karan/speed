use std::fs::File;
use std::io::Write;
use std::os::unix::io::AsRawFd;
use std::sync::atomic::{AtomicI64, AtomicUsize, Ordering};
use std::sync::Arc;

extern "C" {
    fn mmap(addr: *mut u8, length: usize, prot: i32, flags: i32, fd: i32, offset: i64) -> *mut u8;
}

#[derive(Clone, Copy)]
struct Edge {
    to: u32,
    weight: u32,
}

struct QuadHeap {
    data: Vec<(u64, u32)>, // (cost, node)
}

impl QuadHeap {
    fn with_capacity(cap: usize) -> Self {
        Self {
            data: Vec::with_capacity(cap),
        }
    }

    #[inline(always)]
    fn clear(&mut self) {
        self.data.clear();
    }

    #[inline(always)]
    fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    #[inline(always)]
    fn peek(&self) -> Option<(u64, u32)> {
        self.data.first().copied()
    }

    #[inline(always)]
    fn push(&mut self, cost: u64, node: u32) {
        let mut idx = self.data.len();
        self.data.push((cost, node));
        while idx > 0 {
            let parent = (idx - 1) / 4;
            if self.data[idx].0 < self.data[parent].0 {
                self.data.swap(idx, parent);
                idx = parent;
            } else {
                break;
            }
        }
    }

    #[inline(always)]
    fn pop(&mut self) -> Option<(u64, u32)> {
        if self.data.is_empty() {
            return None;
        }
        let top = self.data[0];
        let last = self.data.pop().unwrap();
        if !self.data.is_empty() {
            self.data[0] = last;
            let len = self.data.len();
            let mut idx = 0;
            loop {
                let first_child = 4 * idx + 1;
                if first_child >= len {
                    break;
                }
                let mut best_child = first_child;
                let last_child = (first_child + 4).min(len);
                for c in (first_child + 1)..last_child {
                    if self.data[c].0 < self.data[best_child].0 {
                        best_child = c;
                    }
                }
                if self.data[best_child].0 < self.data[idx].0 {
                    self.data.swap(idx, best_child);
                    idx = best_child;
                } else {
                    break;
                }
            }
        }
        Some(top)
    }
}

struct SearchState {
    distance: Vec<u64>,
    seen: Vec<u32>,
    heap: QuadHeap,
}

impl SearchState {
    fn new(n: usize) -> Self {
        Self {
            distance: vec![0u64; n],
            seen: vec![0u32; n],
            heap: QuadHeap::with_capacity(32768),
        }
    }

    #[inline(always)]
    fn start(&mut self, node: u32, generation: u32) {
        self.heap.clear();
        self.distance[node as usize] = 0;
        self.seen[node as usize] = generation;
        self.heap.push(0, node);
    }
}

#[inline(always)]
fn settle(
    own: &mut SearchState,
    other: &SearchState,
    offsets: &[u32],
    edges: &[Edge],
    generation: u32,
    best: &mut u64,
) {
    let (cost, node) = match own.heap.pop() {
        Some(x) => x,
        None => return,
    };
    if own.seen[node as usize] != generation || own.distance[node as usize] != cost {
        return;
    }
    if other.seen[node as usize] == generation {
        let cand = cost + other.distance[node as usize];
        if cand < *best {
            *best = cand;
        }
    }
    let start = offsets[node as usize] as usize;
    let end = offsets[node as usize + 1] as usize;
    for &edge in &edges[start..end] {
        let next_cost = cost + edge.weight as u64;
        if next_cost >= *best {
            continue;
        }
        let to = edge.to as usize;
        if own.seen[to] != generation || next_cost < own.distance[to] {
            own.seen[to] = generation;
            own.distance[to] = next_cost;
            own.heap.push(next_cost, edge.to);
            if other.seen[to] == generation {
                let cand = next_cost + other.distance[to];
                if cand < *best {
                    *best = cand;
                }
            }
        }
    }
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

#[inline(always)]
unsafe fn read_uint(mut p: *const u8, end: *const u8) -> (u32, *const u8) {
    while p < end && *p <= b' ' {
        p = p.add(1);
    }
    let mut val: u32 = 0;
    while p < end && *p >= b'0' && *p <= b'9' {
        val = val * 10 + (*p - b'0') as u32;
        p = p.add(1);
    }
    (val, p)
}

fn main() {
    let graph_path = if std::path::Path::new("/data/graph.txt").exists() {
        "/data/graph.txt"
    } else {
        "data/d/graph.txt"
    };

    let queries_path = if std::path::Path::new("/data/queries.txt").exists() {
        "/data/queries.txt"
    } else {
        "data/d/queries.txt"
    };

    let (_, graph_size, graph_ptr) = map_file(graph_path).expect("open graph");
    let (queries_data, queries_size, _) = map_file(queries_path).expect("open queries");

    let output_path = if std::path::Path::new("/out").is_dir() {
        "/out/distances.txt"
    } else {
        "distances.txt"
    };

    let mut out_file = File::create(output_path).expect("create output");

    if graph_size == 0 || queries_size == 0 {
        return;
    }

    let g_end = unsafe { graph_ptr.add(graph_size) };
    let (n, p) = unsafe { read_uint(graph_ptr, g_end) };
    let (m, mut p) = unsafe { read_uint(p, g_end) };

    let n_usize = n as usize;
    let m_usize = m as usize;

    let mut offsets = vec![0u32; n_usize + 1];
    let mut rev_offsets = vec![0u32; n_usize + 1];

    #[derive(Clone, Copy)]
    struct RawEdge {
        u: u32,
        v: u32,
        w: u32,
    }
    let mut raw = vec![RawEdge { u: 0, v: 0, w: 0 }; m_usize];

    unsafe {
        for i in 0..m_usize {
            let (u, next_p) = read_uint(p, g_end);
            let (v, next_p2) = read_uint(next_p, g_end);
            let (w, next_p3) = read_uint(next_p2, g_end);
            p = next_p3;
            raw[i] = RawEdge { u, v, w };
            offsets[u as usize + 1] += 1;
            rev_offsets[v as usize + 1] += 1;
        }
    }

    for i in 1..=n_usize {
        offsets[i] += offsets[i - 1];
        rev_offsets[i] += rev_offsets[i - 1];
    }

    let mut cursor = offsets.clone();
    let mut rev_cursor = rev_offsets.clone();

    let mut edges = vec![Edge { to: 0, weight: 0 }; m_usize];
    let mut rev_edges = vec![Edge { to: 0, weight: 0 }; m_usize];

    for e in raw {
        let pos = cursor[e.u as usize] as usize;
        cursor[e.u as usize] += 1;
        edges[pos] = Edge { to: e.v, weight: e.w };

        let rev_pos = rev_cursor[e.v as usize] as usize;
        rev_cursor[e.v as usize] += 1;
        rev_edges[rev_pos] = Edge { to: e.u, weight: e.w };
    }
    drop(cursor);
    drop(rev_cursor);

    // Read queries
    let mut work = Vec::with_capacity(10_000);
    let mut qp = queries_data.as_ptr();
    let q_end = unsafe { qp.add(queries_size) };
    while qp < q_end {
        let (src, next_qp) = unsafe { read_uint(qp, q_end) };
        if next_qp >= q_end {
            break;
        }
        let (dst, next_qp2) = unsafe { read_uint(next_qp, q_end) };
        qp = next_qp2;
        work.push((src, dst));
    }

    let num_queries = work.len();
    let answers: Arc<Vec<AtomicI64>> = Arc::new((0..num_queries).map(|_| AtomicI64::new(0)).collect());

    let offsets = Arc::new(offsets);
    let rev_offsets = Arc::new(rev_offsets);
    let edges = Arc::new(edges);
    let rev_edges = Arc::new(rev_edges);
    let work = Arc::new(work);
    let next_query = Arc::new(AtomicUsize::new(0));

    let num_threads = std::thread::available_parallelism().map_or(12, |n| n.get()).min(12);
    let mut handles = Vec::with_capacity(num_threads);

    for _ in 0..num_threads {
        let next_q = Arc::clone(&next_query);
        let answers_ref = Arc::clone(&answers);
        let work_ref = Arc::clone(&work);
        let offsets_ref = Arc::clone(&offsets);
        let rev_offsets_ref = Arc::clone(&rev_offsets);
        let edges_ref = Arc::clone(&edges);
        let rev_edges_ref = Arc::clone(&rev_edges);

        let handle = std::thread::spawn(move || {
            let mut forward = SearchState::new(n_usize);
            let mut backward = SearchState::new(n_usize);
            let mut generation: u32 = 0;

            loop {
                let idx = next_q.fetch_add(1, Ordering::Relaxed);
                if idx >= work_ref.len() {
                    break;
                }
                let (source, target) = work_ref[idx];
                let mut answer = -1i64;

                if source == target {
                    answer = 0;
                } else if offsets_ref[source as usize] != offsets_ref[source as usize + 1]
                    && rev_offsets_ref[target as usize] != rev_offsets_ref[target as usize + 1]
                {
                    generation = generation.wrapping_add(1);
                    if generation == 0 {
                        forward.seen.fill(0);
                        backward.seen.fill(0);
                        generation = 1;
                    }

                    forward.start(source, generation);
                    backward.start(target, generation);
                    let mut best = u64::MAX;

                    while !forward.heap.is_empty() && !backward.heap.is_empty() {
                        let f = forward.heap.peek().unwrap().0;
                        let b = backward.heap.peek().unwrap().0;
                        if best != u64::MAX && f + b >= best {
                            break;
                        }
                        if f <= b {
                            settle(
                                &mut forward,
                                &backward,
                                &offsets_ref,
                                &edges_ref,
                                generation,
                                &mut best,
                            );
                        } else {
                            settle(
                                &mut backward,
                                &forward,
                                &rev_offsets_ref,
                                &rev_edges_ref,
                                generation,
                                &mut best,
                            );
                        }
                    }

                    if best != u64::MAX {
                        answer = best as i64;
                    }
                }

                answers_ref[idx].store(answer, Ordering::Relaxed);
            }
        });
        handles.push(handle);
    }

    for h in handles {
        h.join().unwrap();
    }

    let mut out_buffer = Vec::with_capacity(num_queries * 12);
    for a in answers.iter() {
        let val = a.load(Ordering::Relaxed);
        let _ = writeln!(out_buffer, "{}", val);
    }

    out_file.write_all(&out_buffer).expect("write output");
}
