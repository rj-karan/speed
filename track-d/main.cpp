#include <algorithm>
#include <atomic>
#include <cstdint>
#include <cstdio>
#include <fcntl.h>
#include <sys/mman.h>
#include <sys/stat.h>
#include <thread>
#include <unistd.h>
#include <vector>

using namespace std;

struct Mapping { const char *data; size_t size; };
static Mapping map_file(const char *path) {
    int fd = open(path, O_RDONLY);
    if (fd < 0) return {nullptr, 0};
    struct stat st;
    if (fstat(fd, &st)) return {nullptr, 0};
    void *p = mmap(nullptr, st.st_size, PROT_READ, MAP_PRIVATE, fd, 0);
    close(fd);
    return p == MAP_FAILED ? Mapping{nullptr, 0} : Mapping{static_cast<const char *>(p), size_t(st.st_size)};
}

static inline uint32_t fast_uint(const char *&p) {
    uint32_t val = 0;
    while (*p >= '0' && *p <= '9') val = val * 10 + (*p++ - '0');
    return val;
}

struct Edge { uint32_t to, weight; };

struct QuadHeap {
    struct Item { uint64_t cost; uint32_t node; };
    vector<Item> data;
    QuadHeap() { data.reserve(16384); }
    inline void clear() { data.clear(); }
    inline bool empty() const { return data.empty(); }
    inline Item top() const { return data[0]; }
    inline void push(uint64_t cost, uint32_t node) {
        size_t idx = data.size();
        data.push_back({cost, node});
        while (idx > 0) {
            size_t parent = (idx - 1) >> 2;
            if (data[idx].cost < data[parent].cost) { swap(data[idx], data[parent]); idx = parent; }
            else break;
        }
    }
    inline Item pop() {
        Item res = data[0], last = data.back();
        data.pop_back();
        if (!data.empty()) {
            data[0] = last;
            size_t idx = 0, len = data.size();
            while (true) {
                size_t child = (idx << 2) + 1;
                if (child >= len) break;
                size_t best = child, last_child = min(child + 4, len);
                for (size_t c = child + 1; c < last_child; ++c) if (data[c].cost < data[best].cost) best = c;
                if (data[best].cost < data[idx].cost) { swap(data[idx], data[best]); idx = best; }
                else break;
            }
        }
        return res;
    }
};

struct Search {
    vector<uint64_t> distance;
    vector<uint16_t> seen;
    QuadHeap heap;
    Search(uint32_t n) : distance(n), seen(n, 0) {}
    inline void start(uint32_t node, uint16_t generation) {
        heap.clear(); distance[node] = 0; seen[node] = generation; heap.push(0, node);
    }
};

static inline void settle(Search &own, const Search &other, const uint32_t *offset,
                          const Edge *edges, uint16_t generation, uint64_t &best) {
    auto [cost, node] = own.heap.pop();
    if (own.seen[node] != generation || own.distance[node] != cost) return;
    if (other.seen[node] == generation) best = min(best, cost + other.distance[node]);
    uint32_t start = offset[node], end = offset[node + 1];
    for (uint32_t j = start; j < end; ++j) {
        const Edge &edge = edges[j];
        uint64_t next = cost + edge.weight;
        if (next >= best) continue;
        if (own.seen[edge.to] != generation || next < own.distance[edge.to]) {
            own.seen[edge.to] = generation; own.distance[edge.to] = next;
            own.heap.push(next, edge.to);
            if (other.seen[edge.to] == generation) best = min(best, next + other.distance[edge.to]);
        }
    }
}

int main() {
    Mapping graph = map_file("/data/graph.txt");
    Mapping queries = map_file("/data/queries.txt");
    if (!graph.data || !queries.data) return 1;

    const char *p = graph.data, *g_end = p + graph.size;
    uint32_t n = fast_uint(p); ++p;
    uint32_t m = fast_uint(p); ++p;

    const int NTHREADS = 4;
    vector<const char*> bounds(NTHREADS + 1);
    bounds[0] = p; bounds[NTHREADS] = g_end;
    size_t total_bytes = g_end - p;
    for (int i = 1; i < NTHREADS; ++i) {
        const char *approx = p + (total_bytes * i) / NTHREADS;
        while (approx < g_end && *approx != '\n') ++approx;
        if (approx < g_end) ++approx;
        bounds[i] = approx;
    }

    vector<vector<uint32_t>> local_deg(NTHREADS, vector<uint32_t>(n + 1, 0));
    vector<vector<uint32_t>> local_rdeg(NTHREADS, vector<uint32_t>(n + 1, 0));
    vector<thread> pass1_threads;
    for (int t = 0; t < NTHREADS; ++t) {
        pass1_threads.emplace_back([&, t] {
            const char *cur = bounds[t], *end = bounds[t + 1];
            auto &ld = local_deg[t]; auto &lrd = local_rdeg[t];
            while (cur < end) {
                while (cur < end && *cur <= ' ') ++cur;
                if (cur >= end) break;
                uint32_t u = fast_uint(cur); ++cur;
                uint32_t v = fast_uint(cur); ++cur;
                fast_uint(cur);
                while (cur < end && *cur != '\n') ++cur;
                if (cur < end) ++cur;
                ++ld[u + 1]; ++lrd[v + 1];
            }
        });
    }
    for (auto &t : pass1_threads) t.join();

    vector<uint32_t> offset(n + 1, 0), reverse_offset(n + 1, 0);
    for (int t = 0; t < NTHREADS; ++t) {
        for (uint32_t i = 1; i <= n; ++i) {
            offset[i] += local_deg[t][i]; reverse_offset[i] += local_rdeg[t][i];
        }
    }
    for (uint32_t i = 1; i <= n; ++i) {
        offset[i] += offset[i - 1]; reverse_offset[i] += reverse_offset[i - 1];
    }

    vector<vector<uint32_t>> cursor(NTHREADS, vector<uint32_t>(n + 1, 0));
    vector<vector<uint32_t>> rcursor(NTHREADS, vector<uint32_t>(n + 1, 0));
    for (uint32_t u = 0; u <= n; ++u) {
        uint32_t c = offset[u], rc = reverse_offset[u];
        for (int t = 0; t < NTHREADS; ++t) {
            cursor[t][u] = c; rcursor[t][u] = rc;
            c += local_deg[t][u + 1]; rc += local_rdeg[t][u + 1];
        }
    }
    local_deg.clear(); local_deg.shrink_to_fit();
    local_rdeg.clear(); local_rdeg.shrink_to_fit();

    vector<Edge> edges(m), reverse_edges(m);
    vector<thread> pass2_threads;
    for (int t = 0; t < NTHREADS; ++t) {
        pass2_threads.emplace_back([&, t] {
            const char *cur = bounds[t], *end = bounds[t + 1];
            auto &cur_pos = cursor[t]; auto &rcur_pos = rcursor[t];
            Edge *e_data = edges.data(), *re_data = reverse_edges.data();
            while (cur < end) {
                while (cur < end && *cur <= ' ') ++cur;
                if (cur >= end) break;
                uint32_t u = fast_uint(cur); ++cur;
                uint32_t v = fast_uint(cur); ++cur;
                uint32_t w = fast_uint(cur);
                while (cur < end && *cur != '\n') ++cur;
                if (cur < end) ++cur;
                e_data[cur_pos[u]++] = {v, w}; re_data[rcur_pos[v]++] = {u, w};
            }
        });
    }
    for (auto &t : pass2_threads) t.join();
    cursor.clear(); cursor.shrink_to_fit();
    rcursor.clear(); rcursor.shrink_to_fit();

    vector<pair<uint32_t, uint32_t>> work;
    work.reserve(10000);
    p = queries.data;
    const char *q_end = p + queries.size;
    while (p < q_end) {
        while (p < q_end && *p <= ' ') ++p;
        if (p >= q_end) break;
        uint32_t source = fast_uint(p); ++p;
        uint32_t target = fast_uint(p);
        while (p < q_end && *p != '\n') ++p;
        if (p < q_end) ++p;
        work.emplace_back(source, target);
    }

    vector<int64_t> answers(work.size());
    atomic<size_t> next_query{0};
    const size_t QUERY_THREADS = 8;
    vector<thread> q_threads;
    for (size_t t = 0; t < QUERY_THREADS; ++t) {
        q_threads.emplace_back([&] {
            Search forward(n), backward(n);
            uint16_t generation = 0;
            const uint32_t *off_ptr = offset.data(), *roff_ptr = reverse_offset.data();
            const Edge *edge_ptr = edges.data(), *redge_ptr = reverse_edges.data();

            while (true) {
                size_t i = next_query.fetch_add(1, memory_order_relaxed);
                if (i >= work.size()) break;
                auto [source, target] = work[i];
                int64_t answer = -1;
                if (source == target) answer = 0;
                else if (off_ptr[source] != off_ptr[source + 1] && roff_ptr[target] != roff_ptr[target + 1]) {
                    ++generation;
                    if (generation == 0) {
                        fill(forward.seen.begin(), forward.seen.end(), 0);
                        fill(backward.seen.begin(), backward.seen.end(), 0);
                        generation = 1;
                    }
                    forward.start(source, generation);
                    backward.start(target, generation);
                    uint64_t best = UINT64_MAX;
                    while (!forward.heap.empty() && !backward.heap.empty()) {
                        uint64_t f = forward.heap.top().cost, b = backward.heap.top().cost;
                        if (best != UINT64_MAX && f + b >= best) break;
                        if (f <= b) settle(forward, backward, off_ptr, edge_ptr, generation, best);
                        else settle(backward, forward, roff_ptr, redge_ptr, generation, best);
                    }
                    if (best != UINT64_MAX) answer = static_cast<int64_t>(best);
                }
                answers[i] = answer;
            }
        });
    }
    for (auto &t : q_threads) t.join();

    FILE *out = fopen("/out/distances.txt", "wb");
    if (!out) return 1;
    char out_buf[65536];
    setvbuf(out, out_buf, _IOFBF, sizeof(out_buf));
    for (int64_t ans : answers) fprintf(out, "%lld\n", static_cast<long long>(ans));
    return fclose(out) != 0;
}
