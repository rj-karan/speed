# Track C: Join (BeyondBug)

## Approach

Our Track C solution is implemented in pure standard-library Rust with zero runtime dependencies ($D = 1$).

### Key Architectural Decisions

1. **Direct Array Region Indexing**:
   Regions are mapped into a 1,000,000-entry flat table for $O(1)$ direct array index lookup.
2. **Compact 3-Letter Country Encoding**:
   Country codes are encoded into base-26 integers $(c_0 \times 676 + c_1 \times 26 + c_2)$ to avoid string allocations during join lookups.
3. **Zero-Copy RFC 4180 CSV Parser**:
   Custom parser reads directly from memory-mapped slices, correctly handling quotation marks, escaped quotes (`""`), multi-line fields, and UTF-8 BOM markers.
4. **256-Bit Country Membership Masks for Distinct Names**:
   Instead of storing per-country hash sets, each unique name holds four 64-bit words representing country membership bits. Testing and updating country membership is a single bitwise OR operation.
5. **Zero Dependencies**:
   $D = 1$ to maximize the dependency scoring term $(1/D)^{0.15} = 1.0$.
