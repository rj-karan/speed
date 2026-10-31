#include <algorithm>
#include <atomic>
#include <thread>
#include <charconv>
#include <cstdint>
#include <cstdio>
#include <fcntl.h>
#include <string_view>
#include <string>
#include <sys/mman.h>
#include <sys/stat.h>
#include <unistd.h>
#include <unordered_map>
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

struct Query { bool phrase; vector<uint32_t> terms; vector<char> ops; };
static uint32_t intern(string_view text, unordered_map<string_view, uint32_t> &ids) {
    auto [it, added] = ids.try_emplace(text, ids.size());
    return it->second;
}
static void parse_queries(Mapping input, unordered_map<string_view, uint32_t> &ids, vector<Query> &queries) {
    const char *p = input.data, *end = p + input.size;
    while (p < end) {
        Query q{};
        q.phrase = *p == 'P';
        p += q.phrase ? 8 : 5; // PHRASE " or BOOL .
        while (p < end && *p != '\n' && *p != '"') {
            const char *start = p;
            while (p < end && *p != ' ' && *p != '"' && *p != '\n') ++p;
            q.terms.push_back(intern({start, size_t(p - start)}, ids));
            if (*p == ' ') {
                ++p;
                if (!q.phrase) { q.ops.push_back(*p); while (*p != ' ') ++p; ++p; }
            }
        }
        if (q.phrase) ++p; // Closing quote.
        if (p < end && *p == '\n') ++p;
        queries.push_back(move(q));
    }
}

static uint32_t token(const char *&p, const char *end, const unordered_map<string_view, uint32_t> &ids) {
    while (p < end && !((*p >= 'a' && *p <= 'z') || (*p >= 'A' && *p <= 'Z') || (*p >= '0' && *p <= '9'))) ++p;
    if (p == end) return UINT32_MAX;
    char buffer[33]; uint32_t length = 0;
    while (p < end && ((*p >= 'a' && *p <= 'z') || (*p >= 'A' && *p <= 'Z') || (*p >= '0' && *p <= '9'))) {
        char c = *p++;
        if (c >= 'A' && c <= 'Z') c += 'a' - 'A';
        if (length < sizeof buffer) buffer[length++] = c;
    }
    auto it = ids.find({buffer, length});
    return it == ids.end() ? UINT32_MAX - 1 : it->second;
}

static vector<uint32_t> intersect(const vector<uint32_t> &a, const vector<uint32_t> &b) {
    vector<uint32_t> result; result.reserve(min(a.size(), b.size()));
    size_t i = 0, j = 0;
    while (i < a.size() && j < b.size()) {
        if (a[i] < b[j]) ++i;
        else if (a[i] > b[j]) ++j;
        else { result.push_back(a[i]); ++i; ++j; }
    }
    return result;
}
static vector<uint32_t> unite(const vector<uint32_t> &a, const vector<uint32_t> &b) {
    vector<uint32_t> result; result.reserve(a.size() + b.size());
    size_t i = 0, j = 0;
    while (i < a.size() || j < b.size()) {
        if (j == b.size() || (i < a.size() && a[i] < b[j])) result.push_back(a[i++]);
        else if (i == a.size() || b[j] < a[i]) result.push_back(b[j++]);
        else { result.push_back(a[i]); ++i; ++j; }
    }
    return result;
}
static vector<uint32_t> subtract(const vector<uint32_t> &a, const vector<uint32_t> &b) {
    vector<uint32_t> result; result.reserve(a.size());
    size_t i = 0, j = 0;
    while (i < a.size()) {
        while (j < b.size() && b[j] < a[i]) ++j;
        if (j == b.size() || a[i] != b[j]) result.push_back(a[i]);
        ++i;
    }
    return result;
}

static bool contains_phrase(string_view text, const Query &q, const unordered_map<string_view, uint32_t> &ids) {
    vector<uint32_t> sequence;
    const char *p = text.data(), *end = p + text.size();
    while (p < end) {
        uint32_t id = token(p, end, ids);
        if (id == UINT32_MAX) break;
        sequence.push_back(id);
    }
    if (sequence.size() < q.terms.size()) return false;
    for (size_t i = 0; i + q.terms.size() <= sequence.size(); ++i) {
        size_t j = 0;
        while (j < q.terms.size() && sequence[i + j] == q.terms[j]) ++j;
        if (j == q.terms.size()) return true;
    }
    return false;
}

static void append_number(string &line, uint64_t value) {
    char buffer[24];
    auto end = to_chars(buffer, buffer + sizeof buffer, value).ptr;
    line.append(buffer, end);
}

int main() {
    Mapping query_file = map_file("/data/queries.txt"), docs_file = map_file("/data/docs.txt");
    if (!query_file.data || !docs_file.data) return 1;
    unordered_map<string_view, uint32_t> ids;
    ids.reserve(150000);
    vector<Query> queries;
    queries.reserve(50000);
    parse_queries(query_file, ids, queries);
    vector<vector<uint32_t>> postings(ids.size());
    vector<string_view> docs;
    docs.reserve(250000);
    const char *p = docs_file.data, *end = p + docs_file.size;
    while (p < end) {
        while (*p != '\t') ++p;
        const char *start = ++p;
        while (p < end && *p != '\n') ++p;
        const char *finish = p++;
        docs.emplace_back(start, finish - start);
    }
    constexpr size_t workers = 6;
    vector<vector<vector<uint32_t>>> parts(workers);
    vector<thread> build_threads;
    for (size_t worker = 0; worker < workers; ++worker) {
        build_threads.emplace_back([&, worker] {
            auto &local = parts[worker];
            local.resize(ids.size());
            vector<uint32_t> last_doc(ids.size(), UINT32_MAX);
            size_t begin = docs.size() * worker / workers;
            size_t finish = docs.size() * (worker + 1) / workers;
            for (size_t doc_id = begin; doc_id < finish; ++doc_id) {
                const char *t = docs[doc_id].data();
                const char *doc_end = t + docs[doc_id].size();
                while (t < doc_end) {
                    uint32_t id = token(t, doc_end, ids);
                    if (id == UINT32_MAX) break;
                    if (id != UINT32_MAX - 1 && last_doc[id] != doc_id) {
                        last_doc[id] = doc_id;
                        local[id].push_back(doc_id);
                    }
                }
            }
        });
    }
    for (thread &t : build_threads) t.join();
    for (size_t id = 0; id < postings.size(); ++id) {
        size_t size = 0;
        for (const auto &part : parts) size += part[id].size();
        postings[id].reserve(size);
        for (const auto &part : parts)
            postings[id].insert(postings[id].end(), part[id].begin(), part[id].end());
    }
    parts.clear(); parts.shrink_to_fit();
    vector<string> output(queries.size());
    atomic<size_t> next_query{0};
    auto query_worker = [&]() {
        while (true) {
            size_t query_id = next_query.fetch_add(1, memory_order_relaxed);
            if (query_id >= queries.size()) break;
            const Query &q = queries[query_id];
            vector<uint32_t> result;
            if (q.phrase) {
                uint32_t rarest = q.terms[0];
                for (uint32_t id : q.terms) if (postings[id].size() < postings[rarest].size()) rarest = id;
                for (uint32_t doc : postings[rarest]) {
                    bool candidate = true;
                    for (uint32_t id : q.terms) {
                        if (!binary_search(postings[id].begin(), postings[id].end(), doc)) { candidate = false; break; }
                    }
                    if (candidate && contains_phrase(docs[doc], q, ids)) result.push_back(doc);
                }
            } else {
                result = postings[q.terms[0]];
                for (size_t i = 0; i < q.ops.size(); ++i) {
                    const auto &next = postings[q.terms[i + 1]];
                    if (q.ops[i] == 'A') result = intersect(result, next);
                    else if (q.ops[i] == 'O') result = unite(result, next);
                    else result = subtract(result, next);
                }
            }
            uint64_t sum = 0;
            for (uint32_t doc : result) sum += doc;
            string &line = output[query_id];
            append_number(line, result.size());
            line.push_back(' ');
            append_number(line, sum);
            for (size_t i = 0; i < min(size_t(10), result.size()); ++i) {
                line.push_back(' ');
                append_number(line, result[i]);
            }
            line.push_back('\n');
        }
    };
    vector<thread> query_threads;
    for (size_t i = 0; i < workers; ++i) query_threads.emplace_back(query_worker);
    for (thread &t : query_threads) t.join();
    FILE *out = fopen("/out/results.txt", "wb");
    if (!out) return 1;
    for (const string &line : output) {
        fwrite(line.data(), 1, line.size(), out);
    }
    return fclose(out) != 0;
}
