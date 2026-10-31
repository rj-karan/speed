#include <array>
#include <charconv>
#include <cstdio>
#include <fcntl.h>
#include <memory>
#include <string_view>
#include <sys/mman.h>
#include <sys/stat.h>
#include <unistd.h>
#include <unordered_set>
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
    uint64_t count = 0, id_sum = 0, name_bytes = 0, note_bytes = 0;
    int64_t amount_sum = 0;
    int32_t amount_min = 0, amount_max = 0;
    unordered_set<string_view> names;
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
    constexpr uint16_t unmatched = 26 * 26 * 26;
    vector<uint16_t> regions(1000000, unmatched);
    const char *p = lookup.data, *end = p + lookup.size;
    while (p < end && *p++ != '\n') {} // Header.
    while (p < end) {
        uint32_t id = read_uint(p);
        ++p;
        uint16_t code = (p[0] - 'A') * 26 * 26 + (p[1] - 'A') * 26 + p[2] - 'A';
        regions[id] = code;
        p += 4;
    }
    auto groups = make_unique<Aggregate[]>(unmatched + 1);
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
        Aggregate &g = groups[region < regions.size() ? regions[region] : unmatched];
        if (!g.count || cents < g.amount_min) g.amount_min = cents;
        if (!g.count || cents > g.amount_max) g.amount_max = cents;
        ++g.count;
        g.id_sum += id;
        g.amount_sum += cents;
        g.name_bytes += name.bytes;
        g.note_bytes += note.bytes;
        g.names.insert(name.raw);
    }
    FILE *out = fopen("/out/aggregates.txt", "wb");
    if (!out) return 1;
    for (uint32_t key = 0; key <= unmatched; ++key) {
        const Aggregate &g = groups[key];
        if (!g.count) continue;
        if (key == unmatched) fputs("_UNMATCHED", out);
        else {
            fputc('A' + key / (26 * 26), out);
            fputc('A' + (key / 26) % 26, out);
            fputc('A' + key % 26, out);
        }
        fputc(' ', out); unsigned_number(out, g.count);
        fputc(' ', out); unsigned_number(out, g.id_sum);
        fputc(' ', out); money(out, g.amount_sum);
        fputc(' ', out); money(out, g.amount_min);
        fputc(' ', out); money(out, g.amount_max);
        fputc(' ', out); unsigned_number(out, g.names.size());
        fputc(' ', out); unsigned_number(out, g.name_bytes);
        fputc(' ', out); unsigned_number(out, g.note_bytes);
        fputc('\n', out);
    }
    return fclose(out) != 0;
}
