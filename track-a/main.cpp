#include <algorithm>
#include <array>
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

struct Group { vector<uint32_t> latencies; uint32_t errors = 0; };
struct Endpoint { string_view name; unique_ptr<array<Group, 72>> hours; };

static void number(FILE *out, uint64_t value) {
    char buffer[24];
    auto end = to_chars(buffer, buffer + sizeof buffer, value).ptr;
    fwrite(buffer, 1, end - buffer, out);
}

static uint32_t parse_uint(const char *&p, const char *end) {
    uint32_t value = 0;
    while (p < end && *p >= '0' && *p <= '9') value = value * 10 + (*p++ - '0');
    return value;
}

int main() {
    int fd = open("/data/requests.log", O_RDONLY);
    if (fd < 0) return 1;
    struct stat st;
    if (fstat(fd, &st)) return 1;
    FILE *out = fopen("/out/summary.txt", "wb");
    if (!out) return 1;
    if (!st.st_size) return fclose(out) != 0;
    auto *data = static_cast<const char *>(mmap(nullptr, st.st_size, PROT_READ, MAP_PRIVATE, fd, 0));
    close(fd);
    if (data == MAP_FAILED) return 1;
    unordered_map<string_view, uint32_t> endpoint_ids;
    endpoint_ids.reserve(2048);
    vector<Endpoint> endpoints;
    endpoints.reserve(2048);
    const char *p = data, *end = data + st.st_size;
    while (p < end) {
        const char *line = p;
        const int day = (line[8] - '0') * 10 + line[9] - '0';
        const int hour = (line[11] - '0') * 10 + line[12] - '0';
        const int slot = line[6] == '0' ? (day - 30) * 24 + hour - 18 : 30 + (day - 1) * 24 + hour;
        p += 28; // Fixed-width timestamp and following space.
        while (*p != ' ') ++p; // Method does not affect the result.
        const char *endpoint_start = ++p;
        while (*p != ' ') ++p;
        string_view name(endpoint_start, p - endpoint_start);
        auto [it, inserted] = endpoint_ids.try_emplace(name, endpoints.size());
        if (inserted) endpoints.push_back({name, make_unique<array<Group, 72>>()});
        Group &group = (*endpoints[it->second].hours)[slot];
        ++p;
        if (*p == '5') ++group.errors;
        p += 4; // Status and following space.
        group.latencies.push_back(parse_uint(p, end));
        ++p; // LF.
    }
    vector<uint32_t> order(endpoints.size());
    for (uint32_t i = 0; i < order.size(); ++i) order[i] = i;
    sort(order.begin(), order.end(), [&](uint32_t a, uint32_t b) { return endpoints[a].name < endpoints[b].name; });
    for (int slot = 0; slot < 72; ++slot) {
        char hour[21];
        int month, day, clock_hour;
        if (slot < 6) { month = 10; day = 30; clock_hour = slot + 18; }
        else if (slot < 30) { month = 10; day = 31; clock_hour = slot - 6; }
        else if (slot < 54) { month = 11; day = 1; clock_hour = slot - 30; }
        else { month = 11; day = 2; clock_hour = slot - 54; }
        snprintf(hour, sizeof hour, "2026-%02d-%02dT%02d:00:00Z", month, day, clock_hour);
        for (uint32_t id : order) {
            Endpoint &endpoint = endpoints[id];
            Group &group = (*endpoint.hours)[slot];
            auto &values = group.latencies;
            if (values.empty()) continue;
            sort(values.begin(), values.end());
            uint64_t count = values.size();
            fwrite(hour, 1, 20, out);
            fputc(' ', out);
            fwrite(endpoint.name.data(), 1, endpoint.name.size(), out);
            fputc(' ', out);
            number(out, count);
            for (uint64_t percentile : {50, 95, 99}) {
                fputc(' ', out);
                number(out, values[(percentile * count + 99) / 100 - 1]);
            }
            fputc(' ', out);
            number(out, group.errors);
            fputc(' ', out);
            number(out, group.errors * 1000000ULL / count);
            fputc('\n', out);
        }
    }
    return fclose(out) != 0;
}
