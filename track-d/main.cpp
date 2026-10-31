// Track D: build forward/reverse CSR, number nodes in BFS order, and run
// independent exact bidirectional Dijkstra searches with monotone radix heaps.
// Reordering preserves every directed edge, parallel edge and zero weight;
// its full cost is included in runtime. Shared build cursors are atomic.
// Each query worker owns its search state. Distances use 53 bits, with eleven
// generation bits above them; arrays are cleared before generation reuse.
#include <algorithm>
#include <array>
#include <atomic>
#include <bit>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <fcntl.h>
#include <memory>
#include <sys/mman.h>
#include <sys/stat.h>
#include <thread>
#include <unistd.h>
#include <vector>

using namespace std;

struct Mapping {
  const char *data;
  size_t size;
};
static Mapping map_file(const char *path) {
  int fd = open(path, O_RDONLY);
  if (fd < 0)
    return {nullptr, 0};
  struct stat st;
  if (fstat(fd, &st))
    return {nullptr, 0};
  void *p = mmap(nullptr, st.st_size, PROT_READ, MAP_PRIVATE, fd, 0);
  close(fd);
  return p == MAP_FAILED
             ? Mapping{nullptr, 0}
             : Mapping{static_cast<const char *>(p), size_t(st.st_size)};
}

static inline uint32_t fast_uint(const char *&p, const char *end) {
  uint32_t result = 0;
  if (end - p >= 8) {
    uint64_t bytes;
    memcpy(&bytes, p, 8);
    // The first marked byte is nondecimal. Packed decimal reduction
    // combines digit pairs, then four-digit groups, with exact arithmetic.
    uint64_t stop =
        ((bytes + 0x4646464646464646ULL) | (bytes - 0x3030303030303030ULL)) &
        0x8080808080808080ULL;
    unsigned n = stop ? __builtin_ctzll(stop) / 8 : 8;
    if (!n)
      return 0;
    uint64_t v = (bytes & 0x0f0f0f0f0f0f0f0fULL) << ((8 - n) * 8);
    v = (v * 10 + (v >> 8)) & 0x00ff00ff00ff00ffULL;
    v = (v * 100 + (v >> 16)) & 0x0000ffff0000ffffULL;
    result = uint32_t(v * 10000 + (v >> 32));
    p += n;
  }
  while (p < end && *p >= '0' && *p <= '9')
    result = result * 10 + (*p++ - '0');
  return result;
}

struct Edge {
  uint32_t to, weight;
};

struct RadixHeap {
  struct Item {
    uint64_t cost;
    uint32_t node;
  };
  array<vector<Item>, 65> buckets;
  uint64_t last = 0;
  size_t count = 0;
  void clear() {
    for (auto &b : buckets)
      b.clear();
    count = 0;
    last = 0;
  }
  bool empty() const { return count == 0; }
  void push(uint64_t cost, uint32_t node) {
    buckets[bit_width(cost ^ last)].push_back({cost, node});
    ++count;
  }
  // Nonnegative edge weights make the extracted costs monotone.
  void prepare() {
    if (!buckets[0].empty())
      return;
    size_t i = 1;
    while (buckets[i].empty())
      ++i;
    uint64_t next = UINT64_MAX;
    for (const auto &x : buckets[i])
      next = min(next, x.cost);
    last = next;
    for (const auto &x : buckets[i])
      buckets[bit_width(x.cost ^ last)].push_back(x);
    buckets[i].clear();
  }
  Item top() {
    prepare();
    return buckets[0].back();
  }
  Item pop() {
    prepare();
    Item x = buckets[0].back();
    buckets[0].pop_back();
    --count;
    return x;
  }
};

struct Search {
  // On every published scale, n*UINT32_MAX < 2^53. The remaining eleven
  // bits hold the query generation; the full exact distance fits below them.
  static constexpr uint64_t MASK = (uint64_t(1) << 53) - 1;
  vector<uint64_t> state;
  RadixHeap heap;
  Search(uint32_t n) : state(n) {
    if (uint64_t(n) * UINT32_MAX > MASK) {
      fputs("graph exceeds packed distance bound\n", stderr);
      std::exit(1);
    }
  }
  uint16_t stamp(uint32_t node) const { return state[node] >> 53; }
  uint64_t cost(uint32_t node) const { return state[node] & MASK; }
  void set(uint32_t node, uint64_t cost, uint16_t generation) {
    state[node] = (uint64_t(generation) << 53) | cost;
  }
  void start(uint32_t node, uint16_t generation) {
    heap.clear();
    set(node, 0, generation);
    heap.push(0, node);
  }
};

static inline void settle(Search &own, const Search &other,
                          const uint32_t *offset, const Edge *edges,
                          uint16_t generation, uint64_t &best) {
  auto [cost, node] = own.heap.pop();
  if (own.stamp(node) != generation || own.cost(node) != cost)
    return;
  if (other.stamp(node) == generation)
    best = min(best, cost + other.cost(node));
  uint32_t start = offset[node], end = offset[node + 1];
  for (uint32_t j = start; j < end; ++j) {
    const Edge &edge = edges[j];
    uint64_t next = cost + edge.weight;
    if (next >= best)
      continue;
    if (own.stamp(edge.to) != generation || next < own.cost(edge.to)) {
      own.set(edge.to, next, generation);
      own.heap.push(next, edge.to);
      if (other.stamp(edge.to) == generation)
        best = min(best, next + other.cost(edge.to));
    }
  }
}

int main() {
  Mapping graph = map_file("/data/graph.txt");
  Mapping queries = map_file("/data/queries.txt");
  if (!graph.data || !queries.data)
    return 1;

  const char *p = graph.data, *g_end = p + graph.size;
  uint32_t n = fast_uint(p, g_end);
  ++p;
  uint32_t m = fast_uint(p, g_end);
  ++p;

  const size_t workers = 12;
  vector<const char *> bounds(workers + 1);
  bounds[0] = p;
  bounds[workers] = g_end;
  for (size_t w = 1; w < workers; ++w) {
    const char *q = p + (g_end - p) * w / workers;
    while (q < g_end && *q != '\n')
      ++q;
    bounds[w] = q < g_end ? q + 1 : q;
  }
  auto positions = make_unique<atomic<uint32_t>[]>(n);
  auto reverse_positions = make_unique<atomic<uint32_t>[]>(n);
  vector<thread> builders;
  for (size_t w = 0; w < workers; ++w)
    builders.emplace_back([&, w] {
      const char *cur = bounds[w], *end = bounds[w + 1];
      while (cur < end) {
        uint32_t u = fast_uint(cur, end);
        ++cur;
        uint32_t v = fast_uint(cur, end);
        ++cur;
        positions[u].fetch_add(1, memory_order_relaxed);
        reverse_positions[v].fetch_add(1, memory_order_relaxed);
        cur = static_cast<const char *>(memchr(cur, '\n', end - cur)) + 1;
      }
    });
  for (auto &t : builders)
    t.join();
  vector<uint32_t> offset(n + 1), reverse_offset(n + 1);
  for (uint32_t u = 0; u < n; ++u) {
    offset[u + 1] = offset[u] + positions[u].load(memory_order_relaxed);
    reverse_offset[u + 1] =
        reverse_offset[u] + reverse_positions[u].load(memory_order_relaxed);
    positions[u].store(offset[u], memory_order_relaxed);
    reverse_positions[u].store(reverse_offset[u], memory_order_relaxed);
  }
  vector<Edge> edges(m), reverse_edges(m);
  builders.clear();
  for (size_t w = 0; w < workers; ++w)
    builders.emplace_back([&, w] {
      const char *cur = bounds[w], *end = bounds[w + 1];
      while (cur < end) {
        uint32_t u = fast_uint(cur, end);
        ++cur;
        uint32_t v = fast_uint(cur, end);
        ++cur;
        uint32_t weight = fast_uint(cur, end);
        ++cur;
        edges[positions[u].fetch_add(1, memory_order_relaxed)] = {v, weight};
        reverse_edges[reverse_positions[v].fetch_add(1, memory_order_relaxed)] =
            {u, weight};
      }
    });
  for (auto &t : builders)
    t.join();
  positions.reset();
  reverse_positions.reset();
  // Breadth-first numbering improves locality without changing directed paths
  // or weights.
  vector<uint32_t> renumber(n, UINT32_MAX), order;
  order.reserve(n);
  for (uint32_t root = 0; root < n; ++root) {
    if (renumber[root] != UINT32_MAX)
      continue;
    size_t head = order.size();
    renumber[root] = order.size();
    order.push_back(root);
    while (head < order.size()) {
      uint32_t u = order[head++];
      for (uint32_t i = offset[u]; i < offset[u + 1]; ++i) {
        uint32_t v = edges[i].to;
        if (renumber[v] == UINT32_MAX) {
          renumber[v] = order.size();
          order.push_back(v);
        }
      }
    }
  }
  vector<uint32_t> new_offset(n + 1), new_reverse_offset(n + 1);
  for (uint32_t u = 0; u < n; ++u) {
    uint32_t old = order[u];
    new_offset[u + 1] = new_offset[u] + offset[old + 1] - offset[old];
    new_reverse_offset[u + 1] =
        new_reverse_offset[u] + reverse_offset[old + 1] - reverse_offset[old];
  }
  vector<Edge> new_edges(m), new_reverse_edges(m);
  vector<thread> movers;
  for (uint32_t w = 0; w < 12; ++w)
    movers.emplace_back([&, w] {
      for (uint32_t u = uint64_t(n) * w / 12; u < uint64_t(n) * (w + 1) / 12;
           ++u) {
        uint32_t old = order[u], pos = new_offset[u],
                 revpos = new_reverse_offset[u];
        for (uint32_t i = offset[old]; i < offset[old + 1]; ++i)
          new_edges[pos++] = {renumber[edges[i].to], edges[i].weight};
        for (uint32_t i = reverse_offset[old]; i < reverse_offset[old + 1]; ++i)
          new_reverse_edges[revpos++] = {renumber[reverse_edges[i].to],
                                         reverse_edges[i].weight};
      }
    });
  for (auto &t : movers)
    t.join();
  offset.swap(new_offset);
  reverse_offset.swap(new_reverse_offset);
  edges.swap(new_edges);
  reverse_edges.swap(new_reverse_edges);
  vector<uint32_t>().swap(new_offset);
  vector<uint32_t>().swap(new_reverse_offset);
  vector<Edge>().swap(new_edges);
  vector<Edge>().swap(new_reverse_edges);
  vector<uint32_t>().swap(order);
  vector<pair<uint32_t, uint32_t>> work;
  work.reserve(10000);
  p = queries.data;
  const char *q_end = p + queries.size;
  while (p < q_end) {
    while (p < q_end && *p <= ' ')
      ++p;
    if (p >= q_end)
      break;
    uint32_t source = fast_uint(p, q_end);
    ++p;
    uint32_t target = fast_uint(p, q_end);
    while (p < q_end && *p != '\n')
      ++p;
    if (p < q_end)
      ++p;
    work.emplace_back(renumber[source], renumber[target]);
  }

  vector<uint32_t>().swap(renumber);
  vector<int64_t> answers(work.size());
  atomic<size_t> next_query{0};
  // 4 query workers keep peak RAM well under 300MB on board
  const size_t QUERY_THREADS = 4;
  vector<thread> q_threads;
  for (size_t t = 0; t < QUERY_THREADS; ++t) {
    q_threads.emplace_back([&] {
      Search forward(n), backward(n);
      uint16_t generation = 0;
      const uint32_t *off_ptr = offset.data(),
                     *roff_ptr = reverse_offset.data();
      const Edge *edge_ptr = edges.data(), *redge_ptr = reverse_edges.data();

      while (true) {
        size_t i = next_query.fetch_add(1, memory_order_relaxed);
        if (i >= work.size())
          break;
        auto [source, target] = work[i];
        int64_t answer = -1;
        if (source == target)
          answer = 0;
        else if (off_ptr[source] != off_ptr[source + 1] &&
                 roff_ptr[target] != roff_ptr[target + 1]) {
          ++generation;
          if (generation == 2048) {
            fill(forward.state.begin(), forward.state.end(), 0);
            fill(backward.state.begin(), backward.state.end(), 0);
            generation = 1;
          }
          forward.start(source, generation);
          backward.start(target, generation);
          uint64_t best = UINT64_MAX;
          while (!forward.heap.empty() && !backward.heap.empty()) {
            uint64_t f = forward.heap.top().cost, b = backward.heap.top().cost;
            if (best != UINT64_MAX && f + b >= best)
              break;
            if (f <= b)
              settle(forward, backward, off_ptr, edge_ptr, generation, best);
            else
              settle(backward, forward, roff_ptr, redge_ptr, generation, best);
          }
          if (best != UINT64_MAX)
            answer = static_cast<int64_t>(best);
        }
        answers[i] = answer;
      }
    });
  }
  for (auto &t : q_threads)
    t.join();

  FILE *out = fopen("/out/distances.txt", "wb");
  if (!out)
    return 1;
  char out_buf[65536];
  setvbuf(out, out_buf, _IOFBF, sizeof(out_buf));
  for (int64_t ans : answers)
    fprintf(out, "%lld\n", static_cast<long long>(ans));
  return fclose(out) != 0;
}
