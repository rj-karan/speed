#include <algorithm>
#include <atomic>
#include <thread>
#include <charconv>
#include <cstdint>
#include <cstdio>
#include <fcntl.h>
#include <immintrin.h>
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

struct Slot { uint64_t hash = 0; uint32_t id = UINT32_MAX; };
struct Terms { vector<Slot> slots; vector<string_view> names; size_t mask; };

static uint64_t hash_term(string_view term) {
    uint64_t hash = 14695981039346656037ULL;
    for (unsigned char c : term) hash = (hash ^ c) * 1099511628211ULL;
    return hash;
}
static Terms make_terms(const unordered_map<string_view, uint32_t> &ids) {
    Terms table;
    size_t capacity = 1;
    while (capacity < ids.size() * 2) capacity *= 2;
    table.slots.resize(capacity);
    table.names.resize(ids.size());
    table.mask = capacity - 1;
    for (const auto &[name, id] : ids) {
        table.names[id] = name;
        uint64_t hash = hash_term(name);
        size_t index = hash & table.mask;
        while (table.slots[index].id != UINT32_MAX) index = (index + 1) & table.mask;
        table.slots[index] = {hash, id};
    }
    return table;
}
static uint32_t token(const char *&p, const char *end, const Terms &terms) {
    // The spec tokenizes ASCII bytes only; every UTF-8 byte is a separator.
    while (p < end && !((*p >= 'a' && *p <= 'z') || (*p >= 'A' && *p <= 'Z') || (*p >= '0' && *p <= '9'))) ++p;
    if (p == end) return UINT32_MAX;
    const char *start = p;
    uint64_t hash = 14695981039346656037ULL;
    while (p < end && ((*p >= 'a' && *p <= 'z') || (*p >= 'A' && *p <= 'Z') || (*p >= '0' && *p <= '9'))) {
        unsigned char c = *p++;
        if (c >= 'A' && c <= 'Z') c += 'a' - 'A';
        hash = (hash ^ c) * 1099511628211ULL;
    }
    size_t index = hash & terms.mask;
    while (terms.slots[index].id != UINT32_MAX) {
        const Slot &slot = terms.slots[index];
        if (slot.hash == hash) {
            string_view name = terms.names[slot.id];
            if (name.size() == size_t(p - start)) {
                size_t i = 0;
                while (i < name.size()) {
                    unsigned char c = start[i];
                    if (c >= 'A' && c <= 'Z') c += 'a' - 'A';
                    if (c != name[i]) break;
                    ++i;
                }
                if (i == name.size()) return slot.id;
            }
        }
        index = (index + 1) & terms.mask;
    }
    return UINT32_MAX - 1;
}

static vector<uint32_t> intersect(const vector<uint32_t> &a, const vector<uint32_t> &b) {
    vector<uint32_t> result; result.reserve(min(a.size(), b.size()));
    // Sparse-vs-common terms should not scan a whole common posting list.
    const auto &small = a.size() < b.size() ? a : b;
    const auto &large = a.size() < b.size() ? b : a;
    if (small.size() * 32 < large.size()) {
        auto pos = large.begin();
        for (uint32_t doc : small) {
            pos = lower_bound(pos, large.end(), doc);
            if (pos == large.end()) break;
            if (*pos == doc) result.push_back(doc);
        }
        return result;
    }
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
    if (a.size() * 32 < b.size()) {
        auto pos = b.begin();
        for (uint32_t doc : a) {
            pos = lower_bound(pos, b.end(), doc);
            if (pos == b.end() || *pos != doc) result.push_back(doc);
        }
        return result;
    }
    size_t i = 0, j = 0;
    while (i < a.size()) {
        while (j < b.size() && b[j] < a[i]) ++j;
        if (j == b.size() || a[i] != b[j]) result.push_back(a[i]);
        ++i;
    }
    return result;
}

static bool is_token_byte(unsigned char c) {
    return (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') || (c >= '0' && c <= '9');
}
static bool equal_term(const char *p, string_view word) {
    for (unsigned char c : word) {
        unsigned char actual = *p++;
        if (actual >= 'A' && actual <= 'Z') actual += 'a' - 'A';
        if (actual != c) return false;
    }
    return true;
}
static bool contains_phrase(string_view text, const Query &q, const Terms &terms, size_t anchor) {
    // Search for the rarest term, then check its immediate token neighbours.
    // Unlike hashing every token against every phrase term, this scans irrelevant
    // text 32 bytes at a time. Loads stay inside this document, including tails.
    const char *begin = text.data(), *end = begin + text.size(), *p = begin;
    string_view word = terms.names[q.terms[anchor]];
    __m256i first = _mm256_set1_epi8(word[0]);
    __m256i fold = _mm256_set1_epi8(0x20);
    while (p < end) {
        if (end - p >= 32) {
            __m256i bytes = _mm256_loadu_si256(reinterpret_cast<const __m256i *>(p));
            unsigned hits = _mm256_movemask_epi8(_mm256_cmpeq_epi8(_mm256_or_si256(bytes, fold), first));
            if (!hits) { p += 32; continue; }
            p += __builtin_ctz(hits);
        } else {
            while (p < end && (static_cast<unsigned char>(*p) | 0x20) != static_cast<unsigned char>(word[0])) ++p;
            if (p == end) break;
        }
        const char *start = p++;
        if ((start != begin && is_token_byte(start[-1])) || size_t(end - start) < word.size()) continue;
        const char *after = start + word.size();
        if ((after != end && is_token_byte(*after)) || !equal_term(start, word)) continue;
        bool matches = true;
        const char *left = start;
        for (size_t j = anchor; j > 0 && matches; --j) {
            while (left > begin && !is_token_byte(left[-1])) --left;
            const char *finish = left;
            while (left > begin && is_token_byte(left[-1])) --left;
            string_view want = terms.names[q.terms[j - 1]];
            matches = size_t(finish - left) == want.size() && equal_term(left, want);
        }
        for (size_t j = anchor + 1; j < q.terms.size() && matches; ++j) {
            while (after < end && !is_token_byte(*after)) ++after;
            const char *next = after;
            while (after < end && is_token_byte(*after)) ++after;
            string_view want = terms.names[q.terms[j]];
            matches = size_t(after - next) == want.size() && equal_term(next, want);
        }
        if (matches) return true;
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
    Terms terms = make_terms(ids);
    vector<vector<uint32_t>> postings(terms.names.size());
    ids.clear(); ids.rehash(0);
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
    // The board gives 12 threads; indexing and query evaluation both scale with
    // them, and the per-worker posting overhead barely moves peak memory.
    size_t workers = thread::hardware_concurrency();
    if (!workers) workers = 1;
    if (workers > 12) workers = 12;
    vector<vector<vector<uint32_t>>> parts(workers);
    vector<thread> build_threads;
    for (size_t worker = 0; worker < workers; ++worker) {
        build_threads.emplace_back([&, worker] {
            auto &local = parts[worker];
            local.resize(terms.names.size());
            vector<uint32_t> last_doc(terms.names.size(), UINT32_MAX);
            size_t begin = docs.size() * worker / workers;
            size_t finish = docs.size() * (worker + 1) / workers;
            for (size_t doc_id = begin; doc_id < finish; ++doc_id) {
                const char *t = docs[doc_id].data();
                const char *doc_end = t + docs[doc_id].size();
                while (t < doc_end) {
                    uint32_t id = token(t, doc_end, terms);
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
        // Worker ranges are disjoint and ordered by document id, so concatenation
        // retains sorted postings without a merge sort.
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
                size_t anchor = 0;
                for (size_t j = 1; j < q.terms.size(); ++j)
                    if (postings[q.terms[j]].size() < postings[q.terms[anchor]].size()) anchor = j;
                uint32_t rarest = q.terms[anchor];
                for (uint32_t doc : postings[rarest]) {
                    bool candidate = true;
                    for (uint32_t id : q.terms) {
                        if (!binary_search(postings[id].begin(), postings[id].end(), doc)) { candidate = false; break; }
                    }
                    if (candidate && contains_phrase(docs[doc], q, terms, anchor)) result.push_back(doc);
                }
            } else {
                result = postings[q.terms[0]];
                // Boolean operators are strictly left-associative, even OR then AND.
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
