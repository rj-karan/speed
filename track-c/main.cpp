#include <array>
#include <immintrin.h>
#include <algorithm>
#include <atomic>
#include <charconv>
#include <cstdint>
#include <cstring>
#include <cstdio>
#include <fcntl.h>
#include <functional>
#include <memory>
#include <string_view>
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
    void *p = mmap(nullptr, st.st_size, PROT_READ, MAP_SHARED, fd, 0);
    close(fd);
    return p == MAP_FAILED ? Mapping{nullptr, 0} : Mapping{static_cast<const char *>(p), size_t(st.st_size)};
}

static inline uint32_t read_uint(const char *&p) {
    uint32_t n = 0;
    while (*p >= '0' && *p <= '9') n = n * 10 + (*p++ - '0');
    return n;
}

static inline uint32_t read_record_uint(const char *&p, const char *end) {
    if (end - p < 8) return read_uint(p);
    uint64_t bytes; memcpy(&bytes, p, 8);
    // The first high bit marks a byte outside ASCII 0..9. Cross-byte carries
    // cannot change the first marked byte, whose preceding bytes are digits.
    uint64_t stop = ((bytes + 0x4646464646464646ULL) | (bytes - 0x3030303030303030ULL)) & 0x8080808080808080ULL;
    unsigned n = stop ? __builtin_ctzll(stop) / 8 : 8;
    if (!n) return 0;
    uint64_t value = (bytes & 0x0f0f0f0f0f0f0f0fULL) << ((8 - n) * 8);
    value = (value * 10 + (value >> 8)) & 0x00ff00ff00ff00ffULL;
    value = (value * 100 + (value >> 16)) & 0x0000ffff0000ffffULL;
    uint32_t result = uint32_t(value * 10000 + (value >> 32));
    p += n;
    while (*p >= '0' && *p <= '9') result = result * 10 + (*p++ - '0');
    return result;
}

struct Field { string_view raw; uint32_t bytes; };
static inline Field read_text(const char *&p, const char *end) {
    if (*p != '"') {
        const char *start = p;
        while (*p != ',') ++p;
        return {{start, size_t(p - start)}, uint32_t(p - start)};
    }
    ++p;
    const char *start = p;
    uint32_t escapes = 0;
    while (true) {
        while (end - p >= 32) {
            unsigned mask = _mm256_movemask_epi8(_mm256_cmpeq_epi8(
                _mm256_loadu_si256(reinterpret_cast<const __m256i *>(p)), _mm256_set1_epi8('"')));
            if (mask) { p += __builtin_ctz(mask); break; }
            p += 32;
        }
        if (end - p < 32) p = static_cast<const char *>(memchr(p, '"', end - p));
        if (p[1] != '"') break;
        p += 2;
        ++escapes;
    }
    const char *finish = p++;
    return {{start, size_t(finish - start)}, uint32_t(finish - start) - escapes};
}

struct Aggregate {
    uint64_t count = 0, id_sum = 0, name_bytes = 0, note_bytes = 0, names_distinct = 0;
    int64_t amount_sum = 0;
    int32_t amount_min = 0, amount_max = 0;
};

static inline uint64_t name_hash(string_view name) {
    uint64_t first = 0, last = 0;
    size_t n = name.size();
    if (n >= 8) {
        memcpy(&first, name.data(), 8);
        memcpy(&last, name.data() + n - 8, 8);
    } else if (n >= 4) {
        uint32_t a, b;
        memcpy(&a, name.data(), 4);
        memcpy(&b, name.data() + n - 4, 4);
        first = a; last = b;
    } else {
        for (size_t i = 0; i < n; ++i) first |= uint64_t(static_cast<unsigned char>(name[i])) << (i * 8);
    }
    uint64_t h = first * 0x9e3779b185ebca87ULL ^ last * 0xc2b2ae3d27d4eb4fULL ^ name.size();
    h = (h ^ (h >> 29)) * 0x165667b19e3779f9ULL;
    // Preserve all home-bucket bits while reserving 0 and 1 for publication.
    return h < 2 ? h + 2 : h;

}

constexpr size_t GLOBAL_TABLE_CAP = 1 << 17; // 131,072 slots for the specified 100,000-name pool.
constexpr size_t GLOBAL_TABLE_MASK = GLOBAL_TABLE_CAP - 1;

struct alignas(64) GlobalSlot {
    atomic<uint64_t> hash{0}; // 0: free, 1: publishing, >=2: published hash.
    uint32_t name_offset{0}, name_len{0};
    char prefix[16]{}; // Compare short names without revisiting the CSV mapping.
    atomic<uint64_t> countries[4]{};
};

static GlobalSlot global_table[GLOBAL_TABLE_CAP];

static void unsigned_number(FILE *out, uint64_t value) {
    char buf[24]; auto end = to_chars(buf, buf + sizeof buf, value).ptr;
    fwrite(buf, 1, end - buf, out);
}
static void signed_number(FILE *out, int64_t value) {
    if (value < 0) { fputc('-', out); value = -value; }
    unsigned_number(out, value);
}
static void money(FILE *out, int64_t cents) {
    if (cents < 0) { fputc('-', out); cents = -cents; }
    unsigned_number(out, cents / 100);
    fprintf(out, ".%02lld", static_cast<long long>(cents % 100));
}

int main() {
    Mapping lookup = map_file("/data/regions.csv");
    Mapping records = map_file("/data/records.csv");
    if (!lookup.data || !records.data) return 1;

    constexpr uint16_t no_country = 65535;
    // At most 240 country indexes; 255 marks a missing region.
    vector<uint8_t> regions(1000000, 255);
    vector<uint16_t> code_to_index(26 * 26 * 26, no_country), country_codes;
    const char *p = lookup.data, *end = p + lookup.size;
    while (p < end && *p++ != '\n') {} // Header.
    while (p < end) {
        uint32_t id = read_uint(p);
        ++p;
        uint16_t code = (p[0] - 'A') * 26 * 26 + (p[1] - 'A') * 26 + p[2] - 'A';
        uint16_t &index = code_to_index[code];
        if (index == no_country) { index = country_codes.size(); country_codes.push_back(code); }
        regions[id] = index;
        p += 4;
    }
    const uint16_t unmatched = country_codes.size();
    p = records.data;
    end = p + records.size;
    while (p < end && *p++ != '\n') {} // Initial BOM and header.

    const unsigned int hw_threads = thread::hardware_concurrency();
    const size_t worker_count = hw_threads ? min(hw_threads, 12u) : 12;

    vector<const char *> bounds(worker_count + 1);
    bounds[0] = p;
    bounds[worker_count] = end;
    for (size_t i = 1; i < worker_count; ++i) {
        const char *scan = records.data + records.size * i / worker_count;
        while (scan < end) {
            const char *lf = static_cast<const char *>(memchr(scan, '\n', end - scan));
            if (!lf) { bounds[i] = end; break; }
            const char *start = lf + 1;
            scan = start;
            if (end - scan >= 3 && (unsigned char)scan[0] == 0xEF && (unsigned char)scan[1] == 0xBB && (unsigned char)scan[2] == 0xBF) scan += 3;
            if (scan < end && *scan >= '1' && *scan <= '9') { bounds[i] = start; break; }
        }
    }

    vector<vector<Aggregate>> thread_groups(worker_count, vector<Aggregate>(unmatched + 1));

    auto parse_range = [&](size_t worker) {
        const char *cursor = bounds[worker];
        const char *finish = bounds[worker + 1];
        auto &local_groups = thread_groups[worker];

        while (cursor < finish) {
            if (end - cursor >= 3 && (unsigned char)cursor[0] == 0xEF && (unsigned char)cursor[1] == 0xBB && (unsigned char)cursor[2] == 0xBF) cursor += 3;
            uint32_t id = read_record_uint(cursor, end);
            ++cursor;
            Field name = read_text(cursor, end);
            ++cursor;
            const uint64_t nhash = name_hash(name.raw);
            __builtin_prefetch(&global_table[nhash & GLOBAL_TABLE_MASK], 1, 3);
            Field note = read_text(cursor, end);
            ++cursor;
            uint32_t region = read_record_uint(cursor, end);
            ++cursor;
            bool negative = *cursor == '-';
            if (negative) ++cursor;
            int32_t cents = read_record_uint(cursor, end) * 100;
            ++cursor;
            cents += (*cursor++ - '0') * 10;
            cents += *cursor++ - '0';
            if (negative) cents = -cents;
            if (cursor < end && *cursor == '\r') ++cursor;
            if (cursor < end && *cursor == '\n') ++cursor;

            uint16_t key = region < regions.size() ? regions[region] : 255;
            if (key == 255) key = unmatched;
            Aggregate &g = local_groups[key];
            if (!g.count || cents < g.amount_min) g.amount_min = cents;
            if (!g.count || cents > g.amount_max) g.amount_max = cents;
            ++g.count;
            g.id_sum += id;
            g.amount_sum += cents;
            g.name_bytes += name.bytes;
            g.note_bytes += note.bytes;

            size_t idx = nhash & GLOBAL_TABLE_MASK;

            GlobalSlot *slot = nullptr;
            while (true) {
                GlobalSlot &s = global_table[idx];
                uint64_t cur_h = s.hash.load(memory_order_acquire);
                while (cur_h == 1) { __builtin_ia32_pause(); cur_h = s.hash.load(memory_order_acquire); }
                if (cur_h == 0) {
                    uint64_t expected = 0;
                    if (s.hash.compare_exchange_strong(expected, 1, memory_order_acq_rel, memory_order_relaxed)) {
                        s.name_offset = name.raw.data() - records.data;
                        memcpy(s.prefix, name.raw.data(), min(name.raw.size(), size_t(16)));
                        s.name_len = name.raw.size();
                        s.hash.store(nhash, memory_order_release);
                        slot = &s;
                        break;
                    }
                    cur_h = s.hash.load(memory_order_acquire);
                    while (cur_h == 1) { __builtin_ia32_pause(); cur_h = s.hash.load(memory_order_acquire); }
                }
                if (cur_h == nhash) {
                    if (s.name_len == name.raw.size() &&
                        memcmp(s.prefix, name.raw.data(), min(size_t(s.name_len), size_t(16))) == 0 &&
                        (s.name_len <= 16 || memcmp(records.data + s.name_offset + 16, name.raw.data() + 16, s.name_len - 16) == 0)) {
                        slot = &s;
                        break;
                    }
                }
                idx = (idx + 1) & GLOBAL_TABLE_MASK;
            }

            size_t word_idx = key >> 6;
            uint64_t bit = 1ULL << (key & 63);
            uint64_t cur_mask = slot->countries[word_idx].load(memory_order_relaxed);
            if (!(cur_mask & bit)) {
                uint64_t prev = slot->countries[word_idx].fetch_or(bit, memory_order_relaxed);
                if (!(prev & bit)) {
                    ++g.names_distinct;
                }
            }
        }
    };

    vector<thread> threads;
    threads.reserve(worker_count);
    for (size_t i = 0; i < worker_count; ++i) threads.emplace_back(parse_range, i);
    for (thread &th : threads) th.join();

    // Sum aggregates across threads
    vector<Aggregate> groups = move(thread_groups[0]);
    for (size_t worker = 1; worker < worker_count; ++worker) {
        for (size_t key = 0; key < groups.size(); ++key) {
            Aggregate &dst = groups[key];
            const Aggregate &src = thread_groups[worker][key];
            if (!src.count) continue;
            if (!dst.count || src.amount_min < dst.amount_min) dst.amount_min = src.amount_min;
            if (!dst.count || src.amount_max > dst.amount_max) dst.amount_max = src.amount_max;
            dst.count += src.count;
            dst.id_sum += src.id_sum;
            dst.amount_sum += src.amount_sum;
            dst.name_bytes += src.name_bytes;
            dst.note_bytes += src.note_bytes;
            dst.names_distinct += src.names_distinct;
        }
    }

    FILE *out = fopen("/out/aggregates.txt", "wb");
    if (!out) return 1;
    vector<uint16_t> order(country_codes.size());
    for (uint16_t i = 0; i < order.size(); ++i) order[i] = i;
    sort(order.begin(), order.end(), [&](uint16_t a, uint16_t b) { return country_codes[a] < country_codes[b]; });
    order.push_back(unmatched);
    for (uint16_t key : order) {
        const Aggregate &g = groups[key];
        if (!g.count) continue;
        if (key == unmatched) fputs("_UNMATCHED", out);
        else {
            uint16_t code = country_codes[key];
            fputc('A' + code / (26 * 26), out);
            fputc('A' + (code / 26) % 26, out);
            fputc('A' + code % 26, out);
        }
        fputc(' ', out); unsigned_number(out, g.count);
        fputc(' ', out); unsigned_number(out, g.id_sum);
        fputc(' ', out); money(out, g.amount_sum);
        fputc(' ', out); money(out, g.amount_min);
        fputc(' ', out); money(out, g.amount_max);
        fputc(' ', out); unsigned_number(out, g.names_distinct);
        fputc(' ', out); unsigned_number(out, g.name_bytes);
        fputc(' ', out); unsigned_number(out, g.note_bytes);
        fputc('\n', out);
    }
    return fclose(out) != 0;
}
