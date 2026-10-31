#include <algorithm>
#include <cstdint>
#include <cstdio>
#include <fcntl.h>
#include <functional>
#include <queue>
#include <sys/mman.h>
#include <sys/stat.h>
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

int main() {
    Mapping graph = map_file("/data/graph.txt");
    Mapping queries = map_file("/data/queries.txt");
    if (!graph.data || !queries.data) return 1;
    const char *p = graph.data;
    uint32_t n = number(p); ++p;
    uint32_t m = number(p); ++p;
    const char *edges_start = p;
    vector<uint32_t> offset(n + 1, 0), indegree(n, 0);
    for (uint32_t i = 0; i < m; ++i) {
        uint32_t u = number(p); ++p;
        uint32_t v = number(p); ++p;
        number(p); ++p;
        ++offset[u + 1];
        ++indegree[v];
    }
    for (uint32_t i = 1; i <= n; ++i) offset[i] += offset[i - 1];
    vector<uint32_t> cursor(offset);
    vector<Edge> edges(m);
    p = edges_start;
    for (uint32_t i = 0; i < m; ++i) {
        uint32_t u = number(p); ++p;
        uint32_t v = number(p); ++p;
        uint32_t w = number(p); ++p;
        edges[cursor[u]++] = {v, w};
    }
    cursor.clear(); cursor.shrink_to_fit();
    vector<uint64_t> distance(n);
    vector<uint32_t> seen(n, 0);
    vector<pair<uint64_t, uint32_t>> heap;
    heap.reserve(262144);
    FILE *out = fopen("/out/distances.txt", "wb");
    if (!out) return 1;
    p = queries.data;
    const char *end = p + queries.size;
    uint32_t generation = 0;
    while (p < end) {
        uint32_t source = number(p); ++p;
        uint32_t target = number(p); ++p;
        int64_t answer = -1;
        if (source == target) answer = 0;
        else if (offset[source] != offset[source + 1] && indegree[target]) {
            ++generation;
            seen[source] = generation;
            distance[source] = 0;
            heap.clear();
            heap.emplace_back(0, source);
            while (!heap.empty()) {
                pop_heap(heap.begin(), heap.end(), greater<>{});
                auto [cost, node] = heap.back();
                heap.pop_back();
                if (seen[node] != generation || distance[node] != cost) continue;
                if (node == target) { answer = cost; break; }
                for (uint32_t j = offset[node]; j < offset[node + 1]; ++j) {
                    const Edge &edge = edges[j];
                    uint64_t next = cost + edge.weight;
                    if (seen[edge.to] != generation || next < distance[edge.to]) {
                        seen[edge.to] = generation;
                        distance[edge.to] = next;
                        heap.emplace_back(next, edge.to);
                        push_heap(heap.begin(), heap.end(), greater<>{});
                    }
                }
            }
        }
        fprintf(out, "%lld\n", static_cast<long long>(answer));
    }
    return fclose(out) != 0;
}
