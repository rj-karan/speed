#include <algorithm>
#include <atomic>
#include <thread>
#include <charconv>
#include <cstdint>
#include <cstring>
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
struct Terms { vector<Slot> slots; vector<string_view> names; vector<uint32_t> short_ids; size_t mask; };

// Exact base-32 encoding for one to four ASCII letters. Digits use the
// general collision-checked table. Zero padding preserves the token length.
static uint32_t short_key(const char *p, size_t n, size_t available) {
    if (n > 4) return UINT32_MAX;
    uint32_t bytes = 0;
    if (available >= 4) memcpy(&bytes, p, 4);
    else for (size_t i = 0; i < n; ++i) bytes |= uint32_t(static_cast<unsigned char>(p[i])) << (i * 8);
    uint32_t mask = n == 4 ? UINT32_MAX : (uint32_t(1) << (n * 8)) - 1;
    bytes &= mask;
    if ((bytes & 0x40404040U) != (mask & 0x40404040U)) return UINT32_MAX;
    return _pext_u32(bytes, 0x1f1f1f1fU);
}
static uint64_t hash_term(string_view term) {
    // Tokens contain only ASCII letters/digits. OR 0x20 folds letters and leaves
    // digits unchanged. Length and exact equality distinguish padded values.
    uint64_t first = 0, last = 0;
    size_t n = term.size();
    if (n >= 8) {
        memcpy(&first, term.data(), 8);
        memcpy(&last, term.data() + n - 8, 8);
    } else if (n >= 4) {
        uint32_t a, b;
        memcpy(&a, term.data(), 4);
        memcpy(&b, term.data() + n - 4, 4);
        first = a; last = b;
    } else {
        for (size_t i = 0; i < n; ++i) first |= uint64_t(static_cast<unsigned char>(term[i])) << (i * 8);
    }
    first |= 0x2020202020202020ULL;
    last |= 0x2020202020202020ULL;
    uint64_t h = first * 0x9e3779b185ebca87ULL ^ last * 0xc2b2ae3d27d4eb4fULL ^ term.size();
    return (h ^ (h >> 29)) * 0x165667b19e3779f9ULL;
}
static Terms make_terms(const unordered_map<string_view, uint32_t> &ids) {
    Terms table;
    table.short_ids.resize(1 << 20, UINT32_MAX - 1);
    size_t capacity = 1;
    while (capacity < ids.size() * 2) capacity *= 2;
    table.slots.resize(capacity);
    table.names.resize(ids.size());
    table.mask = capacity - 1;
    for (const auto &[name, id] : ids) {
        table.names[id] = name;
        uint32_t direct = short_key(name.data(), name.size(), name.size());
        if (direct != UINT32_MAX) table.short_ids[direct] = id;
        uint64_t hash = hash_term(name);
        size_t index = hash & table.mask;
        while (table.slots[index].id != UINT32_MAX) index = (index + 1) & table.mask;
        table.slots[index] = {hash, id};
    }
    return table;
}
static bool equal_term(const char *p, string_view word);
struct Pending { const char *start; size_t length; uint64_t hash; uint32_t id; };
static Pending prepare_token(const char *start, size_t length, const char *end, const Terms &terms) {
    uint32_t direct = short_key(start, length, end - start);
    if (direct != UINT32_MAX) return {nullptr, 0, 0, terms.short_ids[direct]};
    uint64_t hash = hash_term({start, length});
    __builtin_prefetch(&terms.slots[hash & terms.mask], 0, 3);
    return {start, length, hash, 0};
}
template<class Consumer>
static void scan_tokens(const char *p, const char *end, Consumer consume) {
    const char *start = nullptr;
    unsigned previous = 0;
    while (end - p >= 32) {
        __m256i bytes = _mm256_loadu_si256(reinterpret_cast<const __m256i *>(p));
        __m256i folded = _mm256_or_si256(bytes, _mm256_set1_epi8(0x20));
        __m256i letters = _mm256_and_si256(_mm256_cmpgt_epi8(folded, _mm256_set1_epi8('a' - 1)), _mm256_cmpgt_epi8(_mm256_set1_epi8('z' + 1), folded));
        __m256i digits = _mm256_and_si256(_mm256_cmpgt_epi8(bytes, _mm256_set1_epi8('0' - 1)), _mm256_cmpgt_epi8(_mm256_set1_epi8('9' + 1), bytes));
        unsigned mask = _mm256_movemask_epi8(_mm256_or_si256(letters, digits));
        unsigned transitions = mask ^ ((mask << 1) | previous);
        while (transitions) {
            unsigned bit = __builtin_ctz(transitions);
            if (mask & (1U << bit)) start = p + bit;
            else consume(start, size_t(p + bit - start));
            transitions &= transitions - 1;
        }
        previous = mask >> 31; p += 32;
    }
    while (p < end) {
        unsigned char c = *p;
        unsigned valid = ((c | 32) >= 'a' && (c | 32) <= 'z') || (c >= '0' && c <= '9');
        if (valid != previous) {
            if (valid) start = p;
            else consume(start, size_t(p - start));
        }
        previous = valid; ++p;
    }
    if (previous) consume(start, size_t(end - start));
}
static uint32_t resolve(const Pending &pending, const Terms &terms) {
    if (!pending.start) return pending.id;
    const char *start = pending.start;
    uint64_t hash = pending.hash;
    size_t index = hash & terms.mask;
    while (terms.slots[index].id != UINT32_MAX) {
        const Slot &slot = terms.slots[index];
        if (slot.hash == hash) {
            string_view want = terms.names[slot.id];
            if (want.size() == pending.length && equal_term(start, want)) return slot.id;
        }
        index = (index + 1) & terms.mask;
    }
    return UINT32_MAX - 1;
}

struct Posting { size_t offset = 0; uint32_t count = 0; bool dense = false; };

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
    const size_t words = (docs.size() + 63) / 64, term_count = terms.names.size();
    vector<vector<uint32_t>> counts(workers, vector<uint32_t>(term_count));
    vector<Posting> postings(term_count);
    vector<uint32_t> sparse;
    vector<uint64_t> dense;
    auto scan = [&](size_t worker, bool write) {
        // Word-aligned document ranges make bitmap writes disjoint, so the
        // second pass needs no atomics, including for very frequent terms.
        size_t begin = (docs.size() * worker / workers) & ~size_t(63);
        size_t finish = worker + 1 == workers ? docs.size() : (docs.size() * (worker + 1) / workers) & ~size_t(63);
        vector<uint32_t> last_doc(term_count, UINT32_MAX);
        auto &positions = counts[worker];
        for (size_t doc = begin; doc < finish; ++doc) {
            const char *t = docs[doc].data(), *doc_end = t + docs[doc].size();
            Pending batch[16]; size_t size = 0;
            auto flush = [&]() {
                for (size_t j = 0; j < size; ++j) {
                    uint32_t id = resolve(batch[j], terms);
                    if (id >= term_count || last_doc[id] == doc) continue;
                    last_doc[id] = doc;
                    if (!write) ++positions[id];
                    else if (postings[id].dense) dense[postings[id].offset + doc / 64] |= uint64_t(1) << (doc & 63);
                    else sparse[positions[id]++] = doc;
                }
                size = 0;
            };
            scan_tokens(t, doc_end, [&](const char *start, size_t length) {
                batch[size++] = prepare_token(start, length, doc_end, terms);
                if (size == 16) flush();
            });
            flush();
        }
    };
    auto pass = [&](bool write) {
        vector<thread> threads;
        for (size_t w = 0; w < workers; ++w) threads.emplace_back(scan, w, write);
        for (auto &t : threads) t.join();
    };
    pass(false);
    size_t sparse_size = 0, dense_size = 0;
    for (size_t id = 0; id < term_count; ++id) {
        auto &list = postings[id];
        for (auto &part : counts) list.count += part[id];
        list.dense = size_t(list.count) * 4 > words * 8;
        if (list.dense) { list.offset = dense_size; dense_size += words; }
        else {
            list.offset = sparse_size;
            for (auto &part : counts) { uint32_t n = part[id]; part[id] = sparse_size; sparse_size += n; }
        }
    }
    sparse.resize(sparse_size); dense.resize(dense_size);
    pass(true);
    counts.clear(); counts.shrink_to_fit();
    auto visit = [&](uint32_t id, auto consume) {
        const auto &list = postings[id];
        if (list.dense) {
            for (size_t j = 0; j < words; ++j) {
                uint64_t bits = dense[list.offset + j];
                while (bits) { unsigned bit = __builtin_ctzll(bits); consume(uint32_t(j * 64 + bit)); bits &= bits - 1; }
            }
        } else for (size_t j = 0; j < list.count; ++j) consume(sparse[list.offset + j]);
    };
    vector<string> output(queries.size());
    atomic<size_t> next_query{0};
    auto query_worker = [&]() {
        vector<uint64_t> result(words), scratch(words);
        while (true) {
            size_t query_id = next_query.fetch_add(1, memory_order_relaxed);
            if (query_id >= queries.size()) break;
            const Query &q = queries[query_id];
            uint64_t sum = 0, count = 0;
            uint32_t first[10];
            if (q.phrase) {
                size_t anchor = 0;
                for (size_t j = 1; j < q.terms.size(); ++j)
                    if (postings[q.terms[j]].count < postings[q.terms[anchor]].count) anchor = j;
                visit(q.terms[anchor], [&](uint32_t doc) {
                    for (uint32_t id : q.terms) {
                        if (id == q.terms[anchor]) continue;
                        const Posting &list = postings[id];
                        if (list.dense) {
                            if (!(dense[list.offset + doc / 64] & (uint64_t(1) << (doc & 63)))) return;
                        } else if (!list.count || !binary_search(sparse.data() + list.offset, sparse.data() + list.offset + list.count, doc)) return;
                    }
                    if (contains_phrase(docs[doc], q, terms, anchor)) {
                        if (count < 10) first[count] = doc;
                        ++count; sum += doc;
                    }
                });
            } else {
                fill(result.begin(), result.end(), 0);
                visit(q.terms[0], [&](uint32_t doc) { result[doc / 64] |= uint64_t(1) << (doc & 63); });
                for (size_t i = 0; i < q.ops.size(); ++i) {
                    const auto &next = postings[q.terms[i + 1]];
                    char op = q.ops[i];
                    if (next.dense) {
                        const uint64_t *bits = dense.data() + next.offset;
                        if (op == 'A') for (size_t j = 0; j < words; ++j) result[j] &= bits[j];
                        else if (op == 'O') for (size_t j = 0; j < words; ++j) result[j] |= bits[j];
                        else for (size_t j = 0; j < words; ++j) result[j] &= ~bits[j];
                    } else if (op == 'A') {
                        fill(scratch.begin(), scratch.end(), 0);
                        visit(q.terms[i + 1], [&](uint32_t doc) { scratch[doc / 64] |= result[doc / 64] & (uint64_t(1) << (doc & 63)); });
                        result.swap(scratch);
                    } else {
                        visit(q.terms[i + 1], [&](uint32_t doc) {
                            uint64_t bit = uint64_t(1) << (doc & 63);
                            if (op == 'O') result[doc / 64] |= bit;
                            else result[doc / 64] &= ~bit;
                        });
                    }
                }
            }
            if (!q.phrase) for (size_t j = 0; j < words; ++j) {
                uint64_t bits = result[j];
                while (bits) {
                    uint32_t doc = j * 64 + __builtin_ctzll(bits);
                    if (count < 10) first[count] = doc;
                    ++count; sum += doc; bits &= bits - 1;
                }
            }
            string &line = output[query_id];
            append_number(line, count); line.push_back(' '); append_number(line, sum);
            for (size_t i = 0; i < min(uint64_t(10), count); ++i) { line.push_back(' '); append_number(line, first[i]); }
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
