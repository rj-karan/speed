// Track A: parse each byte range once into private four-byte hour records.
// Two barriers publish private dictionaries and then the global endpoint IDs.
// Each subsequent hour has one owner: count, scatter, select and format without
// per-request atomics. All hashes use exact byte equality; hour order is
// arbitrary. Low 20-bit latency codes preserve ordering. A sentinel refers to
// an ordinal full-uint32 overflow stream; quantiles in that tail are selected
// exactly. Calendar and endpoint bounds: official specs/a-ingest.md sections 2
// and 3.
#include <algorithm>
#include <array>
#include <atomic>
#include <barrier>
#include <bit>
#include <charconv>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <fcntl.h>
#include <immintrin.h>
#include <memory>
#include <string>
#include <string_view>
#include <sys/mman.h>
#include <sys/stat.h>
#include <thread>
#include <unistd.h>
#include <unordered_map>
#include <vector>
using namespace std;
constexpr size_t HOURS = 72, KEYS = 2048;
// Section 2.1 fixes this calendar window for every seed and scale.
constexpr unsigned SPAN_YEAR = 2026, SPAN_START_HOUR = 18;
constexpr array<unsigned, 4> SPAN_MONTHS = {10, 10, 11, 11};
constexpr array<unsigned, 4> SPAN_DAYS = {30, 31, 1, 2};

struct Entry {
  uint64_t hash = 0;
  uint32_t offset = 0;
  uint16_t length = 0, id = 0;
};
struct Dictionary {
  Entry slots[4096]{};
  vector<Entry> names;
  uint16_t intern(const char *base, const char *p, uint16_t len,
                  uint64_t hash) {
    size_t pos = hash & 4095;
    while (slots[pos].hash) {
      const Entry &e = slots[pos];
      if (e.hash == hash && e.length == len && !memcmp(base + e.offset, p, len))
        return e.id;
      pos = (pos + 1) & 4095;
    }
    Entry e{hash, uint32_t(p - base), len, uint16_t(names.size())};
    slots[pos] = e;
    names.push_back(e);
    return e.id;
  }
};
// Bits 31..21: endpoint ID; bit 20: 5xx; bits 19..0: latency code.
using Record = uint32_t;
constexpr unsigned VALUE_BITS = 20, ID_SHIFT = VALUE_BITS + 1;
constexpr uint32_t VALUE_MASK = (uint32_t(1) << VALUE_BITS) - 1;
static_assert(KEYS == (uint32_t(1) << (32 - ID_SHIFT)));
struct Rows {
  Record *data;
  size_t size = 0, capacity = 65536;
  Rows() {
    data = static_cast<Record *>(mmap(nullptr, capacity * sizeof(Record),
                                      PROT_READ | PROT_WRITE,
                                      MAP_PRIVATE | MAP_ANONYMOUS, -1, 0));
    if (data == MAP_FAILED) {
      perror("allocate request records");
      std::exit(1);
    }
  }
  Rows(const Rows &) = delete;
  Rows &operator=(const Rows &) = delete;
  ~Rows() { munmap(data, capacity * sizeof(Record)); }
  void push_back(Record value) {
    if (size == capacity) {
      void *next = mremap(data, capacity * sizeof(Record),
                          capacity * 2 * sizeof(Record), MREMAP_MAYMOVE);
      if (next == MAP_FAILED) {
        perror("grow request records");
        std::exit(1);
      }
      data = static_cast<Record *>(next);
      capacity *= 2;
    }
    data[size++] = value;
  }
  const Record *begin() const { return data; }
  const Record *end() const { return data + size; }
};
struct Hour {
  Rows rows;
  vector<uint32_t> large;
  // count = low byte + a multiple of 256; a wrap spills without losing
  // requests.
  uint8_t counts[KEYS]{};
  unordered_map<uint16_t, uint32_t> count_overflow;
};
struct Worker {
  Dictionary keys;
  unique_ptr<Hour> hours[HOURS];
  uint16_t global[KEYS]{};
};
static inline unsigned hour(const char *p) {
  unsigned h = (p[11] - '0') * 10 + p[12] - '0';
  return h + (p[6] == '0' ? (p[9] == '0' ? -int(SPAN_START_HOUR)
                                         : int(24 - SPAN_START_HOUR))
                          : (p[9] == '1' ? int(48 - SPAN_START_HOUR)
                                         : int(72 - SPAN_START_HOUR)));
}
static inline const char *endpoint(const char *p) {
  p += 28;
  if (p[3] == ' ')
    return p + 4;
  if (p[4] == ' ')
    return p + 5;
  if (p[5] == ' ')
    return p + 6;
  if (p[6] == ' ')
    return p + 7;
  return p + 8;
}
static inline pair<uint16_t, uint64_t> key(const char *p, const char *end) {
  const char *start = p;
  while (end - p >= 32) {
    __m256i bytes = _mm256_loadu_si256(reinterpret_cast<const __m256i *>(p));
    unsigned mask =
        _mm256_movemask_epi8(_mm256_cmpeq_epi8(bytes, _mm256_set1_epi8(' ')));
    if (mask) {
      p += __builtin_ctz(mask);
      break;
    }
    p += 32;
  }
  while (*p != ' ')
    ++p;
  unsigned len = p - start;
  uint64_t first, last, middle = 0;
  memcpy(&first, start, 8);
  if (len < 8)
    first &= (uint64_t(1) << (len * 8)) - 1;
  if (len >= 8)
    memcpy(&last, p - 8, 8);
  else
    last = first;
  if (len > 16)
    memcpy(&middle, start + 8, 8);
  uint64_t hash =
      (first ^ rotl(last, 21) ^ rotl(middle, 42) ^ len) * 0x9e3779b185ebca87ULL;
  return {uint16_t(len), (hash ^ (hash >> 29)) | 1};
}
static inline uint32_t latency(const char *&p, const char *end, bool parse) {
  if (end - p >= 8) {
    uint64_t bytes;
    memcpy(&bytes, p, 8);
    // Detect LF bytes, then reduce up to seven decimal digits with SWAR.
    uint64_t x = bytes ^ 0x0a0a0a0a0a0a0a0aULL;
    uint64_t stop = (x - 0x0101010101010101ULL) & ~x & 0x8080808080808080ULL;
    if (stop) {
      unsigned digits = __builtin_ctzll(stop) / 8;
      p += digits + 1;
      if (!parse)
        return 0;
      uint64_t v = (bytes & 0x0f0f0f0f0f0f0f0fULL) << ((8 - digits) * 8);
      v = (v * 10 + (v >> 8)) & 0x00ff00ff00ff00ffULL;
      v = (v * 100 + (v >> 16)) & 0x0000ffff0000ffffULL;
      return uint32_t(v * 10000 + (v >> 32));
    }
  }
  uint32_t value = 0;
  while (*p != '\n') {
    if (parse)
      value = value * 10 + (*p - '0');
    ++p;
  }
  ++p;
  return value;
}
struct Output {
  char *data = nullptr;
  size_t capacity = 0, size = 0;
};
static void percentiles(uint32_t *p, size_t n, uint32_t *v) {
  size_t a = (50 * n + 99) / 100 - 1, b = (95 * n + 99) / 100 - 1,
         c = (99 * n + 99) / 100 - 1;
  if (n <= 32) {
    sort(p, p + n);
    v[0] = p[a];
    v[1] = p[b];
    v[2] = p[c];
    return;
  }
  nth_element(p, p + b, p + n);
  v[1] = p[b];
  if (a < b)
    nth_element(p, p + a, p + b);
  v[0] = p[a];
  if (c > b)
    nth_element(p + b + 1, p + c, p + n);
  v[2] = p[c];
}
int main() {
  int fd = open("/data/requests.log", O_RDONLY);
  if (fd < 0)
    return 1;
  struct stat st;
  if (fstat(fd, &st)) {
    close(fd);
    return 1;
  }
  FILE *out = fopen("/out/summary.txt", "wb");
  if (!out) {
    close(fd);
    return 1;
  }
  if (!st.st_size) {
    close(fd);
    return fclose(out) != 0;
  }
  const char *base = static_cast<const char *>(
      mmap(nullptr, st.st_size, PROT_READ, MAP_SHARED, fd, 0));
  close(fd);
  if (base == MAP_FAILED)
    return 1;
  unsigned workers = min(12u, max(1u, thread::hardware_concurrency()));
  vector<const char *> bounds(workers + 1);
  bounds[0] = base;
  bounds[workers] = base + st.st_size;
  for (unsigned i = 1; i < workers; ++i) {
    const char *p = base + st.st_size * i / workers;
    while (p < base + st.st_size && *p != '\n')
      ++p;
    bounds[i] = p < base + st.st_size ? p + 1 : p;
  }
  vector<unique_ptr<Worker>> states;
  for (unsigned i = 0; i < workers; ++i)
    states.push_back(make_unique<Worker>());
  Dictionary global;
  vector<uint16_t> order;
  vector<Output> output(HOURS);
  atomic<unsigned> next_hour{0};
  barrier sync(workers);
  vector<thread> threads;
  for (unsigned w = 0; w < workers; ++w)
    threads.emplace_back([&, w] {
      Worker &local = *states[w];
      const char *p = bounds[w], *finish = bounds[w + 1];
      while (p < finish) {
        unsigned h = hour(p);
        const char *ep = endpoint(p);
        auto [len, hash] = key(ep, base + st.st_size);
        uint16_t id = local.keys.intern(base, ep, len, hash);
        p = ep + len + 1;
        bool error = *p == '5';
        p += 4;
        uint32_t value = latency(p, base + st.st_size, true);
        if (!local.hours[h])
          local.hours[h] = make_unique<Hour>();
        local.hours[h]->rows.push_back((uint32_t(id) << ID_SHIFT) |
                                       (uint32_t(error) << VALUE_BITS) |
                                       min(value, VALUE_MASK));
        if (++local.hours[h]->counts[id] == 0)
          local.hours[h]->count_overflow[id] += 256;
        if (value >= VALUE_MASK)
          local.hours[h]->large.push_back(value);
      }
      sync.arrive_and_wait();
      if (w == 0) {
        for (auto &state : states)
          for (const Entry &e : state->keys.names) {
            uint16_t id =
                global.intern(base, base + e.offset, e.length, e.hash);
            state->global[e.id] = id;
          }
        for (const Entry &e : global.names)
          order.push_back(e.id);
        sort(order.begin(), order.end(), [&](uint16_t a, uint16_t b) {
          const Entry &x = global.names[a], &y = global.names[b];
          return string_view(base + x.offset, x.length) <
                 string_view(base + y.offset, y.length);
        });
      }
      sync.arrive_and_wait();
      vector<uint32_t> combined;
      uint32_t counts[KEYS], offsets[KEYS + 1], cursors[KEYS], errors[KEYS];
      for (;;) {
        unsigned h = next_hour.fetch_add(1, memory_order_relaxed);
        if (h >= HOURS)
          break;
        char timestamp[24];
        unsigned day = (h + SPAN_START_HOUR) / 24,
                 clock = (h + SPAN_START_HOUR) % 24;
        snprintf(timestamp, sizeof timestamp, "%04u-%02u-%02uT%02u:00:00Z",
                 SPAN_YEAR, SPAN_MONTHS[day], SPAN_DAYS[day], clock);
        fill(begin(counts), end(counts), 0);
        fill(begin(errors), end(errors), 0);
        for (auto &state : states)
          if (state->hours[h]) {
            for (const Entry &e : state->keys.names)
              counts[state->global[e.id]] += state->hours[h]->counts[e.id];
            for (const auto &[id, extra] : state->hours[h]->count_overflow)
              counts[state->global[id]] += extra;
          }
        offsets[0] = 0;
        for (size_t i = 0; i < KEYS; ++i)
          offsets[i + 1] = offsets[i] + counts[i];
        copy(offsets, offsets + KEYS, cursors);
        combined.resize(offsets[KEYS]);
        vector<vector<uint32_t>> large(KEYS);
        for (auto &state : states)
          if (state->hours[h]) {
            // Parse order preserves the one-to-one sentinel/overflow
            // association.
            size_t large_index = 0;
            for (uint32_t row : state->hours[h]->rows) {
              uint16_t id = state->global[row >> ID_SHIFT];
              combined[cursors[id]++] = row & VALUE_MASK;
              errors[id] += (row >> VALUE_BITS) & 1;
              if ((row & VALUE_MASK) == VALUE_MASK)
                large[id].push_back(state->hours[h]->large[large_index++]);
            }
            state->hours[h].reset();
          }
        Output &buf = output[h];
        buf.capacity = max(size_t(1), global.names.size() * 224);
        buf.data = static_cast<char *>(
            mmap(nullptr, buf.capacity, PROT_READ | PROT_WRITE,
                 MAP_PRIVATE | MAP_ANONYMOUS, -1, 0));
        if (buf.data == MAP_FAILED) {
          perror("allocate hour output");
          std::exit(1);
        }
        char *cursor = buf.data;
        for (uint16_t id : order) {
          uint32_t n = counts[id];
          if (!n)
            continue;
          uint64_t error = errors[id];
          uint32_t values[3];
          percentiles(combined.data() + offsets[id], n, values);
          size_t ranks[] = {(50 * size_t(n) + 99) / 100 - 1,
                            (95 * size_t(n) + 99) / 100 - 1,
                            (99 * size_t(n) + 99) / 100 - 1};
          for (unsigned q = 0; q < 3; ++q) {
            // All ordinary codes sort below the sentinel, so subtract their
            // count.
            if (values[q] == VALUE_MASK) {
              auto &overflow = large[id];
              size_t index = ranks[q] - (n - overflow.size());
              nth_element(overflow.begin(), overflow.begin() + index,
                          overflow.end());
              values[q] = overflow[index];
            }
          }
          const Entry &e = global.names[id];
          memcpy(cursor, timestamp, 20);
          cursor += 20;
          *cursor++ = ' ';
          memcpy(cursor, base + e.offset, e.length);
          cursor += e.length;
          uint64_t fields[] = {n,         values[0], values[1],
                               values[2], error,     error * 1000000 / n};
          for (uint64_t value : fields) {
            *cursor++ = ' ';
            cursor = to_chars(cursor, buf.data + buf.capacity, value).ptr;
          }
          *cursor++ = '\n';
        }
        buf.size = cursor - buf.data;
      }
    });
  for (auto &t : threads)
    t.join();
  bool ok = true;
  for (auto &buf : output) {
    if (fwrite(buf.data, 1, buf.size, out) != buf.size)
      ok = false;
    if (munmap(buf.data, buf.capacity))
      ok = false;
  }
  if (fclose(out))
    ok = false;
  munmap(const_cast<char *>(base), st.st_size);
  return ok ? 0 : 1;
}
