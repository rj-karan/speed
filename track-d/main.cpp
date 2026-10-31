#include <algorithm>
#include <atomic>
#include <cstdint>
#include <cstdio>
#include <fcntl.h>
#include <functional>
#include <queue>
#include <sys/mman.h>
#include <sys/stat.h>
#include <thread>
#include <unistd.h>
#include <utility>
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
static uint32_t number(const char *&p) {
    uint32_t value = 0;
    while (*p >= '0' && *p <= '9') value = value * 10 + (*p++ - '0');
    return value;
}
struct Edge { uint32_t to, weight; };
struct Search {
    vector<uint64_t> distance;
    vector<uint32_t> seen;
    vector<pair<uint64_t, uint32_t>> heap;
    Search(uint32_t n) : distance(n), seen(n, 0) { heap.reserve(65536); }
    void start(uint32_t node, uint32_t generation) {
        heap.clear();
        distance[node] = 0;
        seen[node] = generation;
        heap.emplace_back(0, node);
    }
};
static void settle(Search &own, const Search &other, const vector<uint32_t> &offset,
                   const vector<Edge> &edges, uint32_t generation, uint64_t &best) {
    pop_heap(own.heap.begin(), own.heap.end(), greater<>{});
    auto [cost, node] = own.heap.back();
    own.heap.pop_back();
    if (own.seen[node] != generation || own.distance[node] != cost) return;
    if (other.seen[node] == generation) best = min(best, cost + other.distance[node]);
    for (uint32_t j = offset[node]; j < offset[node + 1]; ++j) {
        const Edge &edge = edges[j];
        uint64_t next = cost + edge.weight;
        if (own.seen[edge.to] != generation || next < own.distance[edge.to]) {
            own.seen[edge.to] = generation;
            own.distance[edge.to] = next;
            own.heap.emplace_back(next, edge.to);
            push_heap(own.heap.begin(), own.heap.end(), greater<>{});
            if (other.seen[edge.to] == generation)
                best = min(best, next + other.distance[edge.to]);
        }
    }
}

int main() {
    Mapping graph = map_file("/data/graph.txt");
    Mapping queries = map_file("/data/queries.txt");
    if (!graph.data || !queries.data) return 1;
    const char *p = graph.data;
    uint32_t n = number(p); ++p;
    uint32_t m = number(p); ++p;
    const char *edges_start = p;
    vector<uint32_t> offset(n + 1, 0), reverse_offset(n + 1, 0);
    for (uint32_t i = 0; i < m; ++i) {
        uint32_t u = number(p); ++p;
        uint32_t v = number(p); ++p;
        number(p); ++p;
        ++offset[u + 1];
        ++reverse_offset[v + 1];
    }
    for (uint32_t i = 1; i <= n; ++i) offset[i] += offset[i - 1];
    for (uint32_t i = 1; i <= n; ++i) reverse_offset[i] += reverse_offset[i - 1];
    vector<uint32_t> cursor(offset), reverse_cursor(reverse_offset);
    vector<Edge> edges(m), reverse_edges(m);
    p = edges_start;
    for (uint32_t i = 0; i < m; ++i) {
        uint32_t u = number(p); ++p;
        uint32_t v = number(p); ++p;
        uint32_t w = number(p); ++p;
        edges[cursor[u]++] = {v, w};
        reverse_edges[reverse_cursor[v]++] = {u, w};
    }
    cursor.clear(); cursor.shrink_to_fit();
    reverse_cursor.clear(); reverse_cursor.shrink_to_fit();
    vector<pair<uint32_t, uint32_t>> work;
    p = queries.data;
    const char *end = p + queries.size;
    while (p < end) {
        uint32_t source = number(p); ++p;
        uint32_t target = number(p); ++p;
        work.emplace_back(source, target);
    }
    vector<int64_t> answers(work.size());
    atomic<size_t> next_query{0};
    auto worker = [&]() {
        Search forward(n), backward(n);
        uint32_t generation = 0;
        while (true) {
            size_t i = next_query.fetch_add(1, memory_order_relaxed);
            if (i >= work.size()) break;
            auto [source, target] = work[i];
            int64_t answer = -1;
            if (source == target) answer = 0;
            else if (offset[source] != offset[source + 1] &&
                     reverse_offset[target] != reverse_offset[target + 1]) {
                ++generation;
                forward.start(source, generation);
                backward.start(target, generation);
                uint64_t best = UINT64_MAX;
                while (!forward.heap.empty() && !backward.heap.empty()) {
                    uint64_t f = forward.heap.front().first, b = backward.heap.front().first;
                    // No unseen path can beat best once the two frontier bounds meet.
                    if (best != UINT64_MAX && f + b >= best) break;
                    if (f <= b) settle(forward, backward, offset, edges, generation, best);
                    else settle(backward, forward, reverse_offset, reverse_edges, generation, best);
                }
                if (best != UINT64_MAX) answer = static_cast<int64_t>(best);
            }
            answers[i] = answer;
        }
    };
    vector<thread> threads;
    for (int i = 0; i < 6; ++i) threads.emplace_back(worker);
    for (thread &t : threads) t.join();
    FILE *out = fopen("/out/distances.txt", "wb");
    if (!out) return 1;
    for (int64_t answer : answers) {
        fprintf(out, "%lld\n", static_cast<long long>(answer));
    }
    return fclose(out) != 0;
}
