# Rust Track C evaluation

A standalone, zero-crate Rust port of the selected C++ CSV algorithm. This is an
experiment, outside the submitted track folders. The board builds `track-c/`.
The pinned Rust image builds offline. Run from the repository root:

```sh
docker build --network=none -t speed-local/c-rust experiments/track-c-rust
SPEED_C_IMAGE=speed-local/c-rust python3 -m unittest discover -s tests -p test_join.py -v
```

The public seed-1 full hash, newly generated seed-42 full reference, seed-7 sample,
seed-17 tiny and independent CSV oracle all match. A five-run interleaved local
comparison gave C++ 0.036943 s and Rust 0.040764 s; a later comparison gave
0.030957 and 0.030941 s, within noise. There is no demonstrated gain to offset
Rust's larger canonical source. No official Rust score is claimed. A transposed
membership-bitset Rust variant also tied locally and was not selected.

The immutable key fields use UnsafeCell because only the successful atomic
publication claimant initializes them. A release hash store and acquire loads
publish those fields; membership words stay atomic. SIMD reads are bounded by
the mapping, integer reduction wraps intentionally, and money remains cents.
These unsafe operations rely on the published generator's valid input grammar.
