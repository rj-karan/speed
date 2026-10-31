#include <array>
#include <algorithm>
#include <charconv>
#include <cstdio>
#include <fcntl.h>
#include <memory>
#include <string_view>
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

struct NameState { array<uint64_t, 4> countries{}; };

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
    auto groups = make_unique<Aggregate[]>(unmatched + 1);
    unordered_map<string_view, NameState> name_states;
    name_states.reserve(120000);
    p = records.data;
    end = p + records.size;
    while (p < end && *p++ != '\n') {} // BOM and header.
    while (p < end) {
        if (end - p >= 3 && (unsigned char)p[0] == 0xEF && (unsigned char)p[1] == 0xBB && (unsigned char)p[2] == 0xBF) p += 3;
        uint32_t id = read_uint(p);
        ++p;
        Field name = read_text(p);
        ++p;
        Field note = read_text(p);
        ++p;
        uint32_t region = read_uint(p);
        ++p;
        bool negative = *p == '-';
        if (negative) ++p;
        int32_t cents = read_uint(p) * 100;
        ++p;
        cents += (*p++ - '0') * 10;
        cents += *p++ - '0';
        if (negative) cents = -cents;
        if (p < end && *p == '\r') ++p;
        if (p < end && *p == '\n') ++p;
        uint16_t key = region < regions.size() ? regions[region] : no_country;
        if (key == no_country) key = unmatched;
        Aggregate &g = groups[key];
        if (!g.count || cents < g.amount_min) g.amount_min = cents;
        if (!g.count || cents > g.amount_max) g.amount_max = cents;
        ++g.count;
        g.id_sum += id;
        g.amount_sum += cents;
        g.name_bytes += name.bytes;
        g.note_bytes += note.bytes;
        auto &flags = name_states.try_emplace(name.raw).first->second.countries;
        uint64_t bit = 1ULL << (key & 63);
        uint64_t &word = flags[key >> 6];
        if (!(word & bit)) { word |= bit; ++g.names_distinct; }
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
