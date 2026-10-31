#include <array>
#include <algorithm>
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
    void *p = mmap(nullptr, st.st_size, PROT_READ, MAP_PRIVATE, fd, 0);
    close(fd);
    return p == MAP_FAILED ? Mapping{nullptr, 0} : Mapping{static_cast<const char *>(p), size_t(st.st_size)};
}

static uint32_t read_uint(const char *&p) {
    uint32_t n = 0;
    while (*p >= '0' && *p <= '9') n = n * 10 + (*p++ - '0');
    return n;
}

struct Field { string_view raw; uint32_t bytes; };
static Field read_text(const char *&p) {
    if (*p != '"') {
        const char *start = p;
        while (*p != ',') ++p;
        return {{start, size_t(p - start)}, uint32_t(p - start)};
    }
    ++p;
    const char *start = p;
    uint32_t escapes = 0;
    while (true) {
        if (*p != '"') { ++p; continue; }
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
    uint64_t h = 14695981039346656037ULL;
    for (unsigned char c : name) h = (h ^ c) * 1099511628211ULL;
    return h;
}

struct NameState { string_view name; uint64_t hash; array<uint64_t, 4> countries{}; };

struct NameTable {
    vector<NameState> slots = vector<NameState>(1 << 17);
    size_t count = 0;
    // The caller passes the precomputed hash so probing compares hashes first;
    // a stored-hash mismatch skips the string compare, which would otherwise
    // dereference the name back into the large read-only CSV mapping every probe.
    array<uint64_t, 4> &find_or_insert(string_view name, uint64_t hash) {
        if (count * 8 >= slots.size() * 7) {
            vector<NameState> old = move(slots);
            slots = vector<NameState>(old.size() * 2);
            count = 0;
            for (NameState &entry : old) {
                if (entry.name.data()) find_or_insert(entry.name, entry.hash) = entry.countries;
            }
        }
        size_t index = hash & (slots.size() - 1);
        while (slots[index].name.data()) {
            if (slots[index].hash == hash && slots[index].name == name) return slots[index].countries;
            index = (index + 1) & (slots.size() - 1);
        }
        slots[index].name = name;
        slots[index].hash = hash;
        ++count;
        return slots[index].countries;
    }
};

struct WorkerData {
    vector<Aggregate> groups;
    NameTable names;
    explicit WorkerData(size_t group_count) : groups(group_count) {}
};

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
    vector<uint16_t> regions(1000000, no_country);
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
    constexpr size_t worker_count = 3;
    array<const char *, worker_count + 1> bounds{};
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
    vector<WorkerData> states;
    states.reserve(worker_count);
    for (size_t i = 0; i < worker_count; ++i) states.emplace_back(unmatched + 1);
    auto parse_range = [&](size_t worker) {
        const char *cursor = bounds[worker];
        const char *finish = bounds[worker + 1];
        WorkerData &state = states[worker];
        while (cursor < finish) {
            // Mid-file BOMs are legal only at the start of a record.
            if (end - cursor >= 3 && (unsigned char)cursor[0] == 0xEF && (unsigned char)cursor[1] == 0xBB && (unsigned char)cursor[2] == 0xBF) cursor += 3;
            uint32_t id = read_uint(cursor);
            ++cursor;
            Field name = read_text(cursor);
            ++cursor;
            // Hash the name now and prefetch its table slot; the random DRAM miss
            // then overlaps the note/region/amount parsing and aggregate updates
            // below, which is where this track spends most of its time.
            const uint64_t nhash = name_hash(name.raw);
            __builtin_prefetch(&state.names.slots[nhash & (state.names.slots.size() - 1)]);
            Field note = read_text(cursor);
            ++cursor;
            uint32_t region = read_uint(cursor);
            ++cursor;
            bool negative = *cursor == '-';
            if (negative) ++cursor;
            int32_t cents = read_uint(cursor) * 100;
            ++cursor;
            cents += (*cursor++ - '0') * 10;
            cents += *cursor++ - '0';
            if (negative) cents = -cents;
            if (cursor < end && *cursor == '\r') ++cursor;
            if (cursor < end && *cursor == '\n') ++cursor;
            uint16_t key = region < regions.size() ? regions[region] : no_country;
            if (key == no_country) key = unmatched;
            Aggregate &g = state.groups[key];
            if (!g.count || cents < g.amount_min) g.amount_min = cents;
            if (!g.count || cents > g.amount_max) g.amount_max = cents;
            ++g.count;
            g.id_sum += id;
            g.amount_sum += cents;
            g.name_bytes += name.bytes;
            g.note_bytes += note.bytes;
            // Generated nonempty names are always quoted. Their raw slices uniquely
            // represent the decoded name; quoted and unquoted empty names both use "".
            auto &flags = state.names.find_or_insert(name.raw, nhash);
            uint64_t bit = 1ULL << (key & 63);
            uint64_t &word = flags[key >> 6];
            if (!(word & bit)) { word |= bit; ++g.names_distinct; }
        }
    };
    vector<thread> threads;
    for (size_t i = 0; i < worker_count; ++i) threads.emplace_back(parse_range, i);
    for (thread &thread : threads) thread.join();
    vector<Aggregate> groups = move(states[0].groups);
    for (size_t worker = 1; worker < worker_count; ++worker) {
        for (size_t key = 0; key < groups.size(); ++key) {
            Aggregate &dst = groups[key];
            const Aggregate &src = states[worker].groups[key];
            if (!src.count) continue;
            if (!dst.count || src.amount_min < dst.amount_min) dst.amount_min = src.amount_min;
            if (!dst.count || src.amount_max > dst.amount_max) dst.amount_max = src.amount_max;
            dst.count += src.count;
            dst.id_sum += src.id_sum;
            dst.amount_sum += src.amount_sum;
            dst.name_bytes += src.name_bytes;
            dst.note_bytes += src.note_bytes;
        }
        for (const NameState &entry : states[worker].names.slots) {
            if (!entry.name.data()) continue;
            auto &flags = states[0].names.find_or_insert(entry.name, entry.hash);
            for (size_t word = 0; word < flags.size(); ++word) {
                uint64_t fresh = entry.countries[word] & ~flags[word];
                flags[word] |= entry.countries[word];
                while (fresh) {
                    unsigned bit = __builtin_ctzll(fresh);
                    ++groups[word * 64 + bit].names_distinct;
                    fresh &= fresh - 1;
                }
            }
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
