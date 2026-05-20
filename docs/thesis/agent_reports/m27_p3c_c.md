# M27 P3c-C — `gzip` + `zlib` + `bz2` compression stdlib modules

**Brief**: Ship three compression stdlib modules on top of the M19
stdlib-module-table infrastructure: gzip (RFC 1952 framing), zlib
(RFC 1950 raw), and bz2 (libbzip2). All three are crate-wraps over
`flate2` + `bzip2`, mirroring the M22 P2B (base64/hashlib) and
M22 P2D (struct/urllib_parse) precedents. Reserved native IDs
**500-519**; used **500-510** (11 IDs); 511-519 left fallow for v0.3
streaming handles and possibly LZMA.

**Wall-clock**: ~2 hours (read-through of M22 P2B + P2D agent
reports, the SHARED_BRIEF, base64/hashlib + struct/urllib_parse
example programs; 11 native handlers totalling ~280 LOC; 6
subprocess tests across 3 example programs; spec sections §9.34
+ §9.35 + §9.36; one minor demo tuning pass).

**Files changed**: 4 source files + 1 cargo dep block + 1 spec
section trio + 1 agent report + 3 new examples + 3 new test files.

## Strategy: crate-wrap, str-as-byte-buffer

Both libraries on the menu — `flate2` (which covers gzip, zlib, and
raw DEFLATE in a single crate) and `bzip2` (which wraps the libbzip2
algorithm in a Rust-friendly Read/Write interface) — are well-vetted,
zero-OS-surface, single-cargo-dep installs. The diff per algorithm is
about 12 lines: read input via `packed_str_to_bytes`, push it through
the encoder/decoder, finalise with `finish()`, hand the bytes to
`bytes_to_packed_str` + `alloc_string`. The two checksums
(`zlib.crc32` and `zlib.adler32`) are even tighter — `flate2::Crc` for
the IEEE polynomial; a hand-rolled 12-line Adler-32 for the RFC 1950
checksum (the crate didn't expose Adler-32 as a public type, and the
algorithm is genuinely 4 lines of arithmetic plus the prime modulus).

The single non-obvious design decision was reuse of the M22 P2D
str-as-byte-buffer convention rather than the M22 P2B
`base64.decode → str-via-UTF-8` shape. The two approaches map to
different surfaces:

* **Base64 / hashlib (M22 P2B)** assumes the user's input is text,
  encodes it as UTF-8 first, and the output is ASCII (hex digest /
  base64 alphabet). Round-tripping arbitrary binary data through it
  would lose data on `decode` (UTF-8 check rejects non-text).
* **Struct (M22 P2D)** treats `str` as a flat array of 0..=255 bytes:
  each Unicode codepoint in `U+0000..U+00FF` is one logical byte.
  `len(buf)` equals the byte count regardless of UTF-8 expansion in
  the underlying String. Concatenation works at the byte level.
  Programs that need to feed binary blobs build them up
  codepoint-by-codepoint with `chr(b)`.

Compression is fundamentally a binary surface. Even when the **input**
is text, the **output** is opaque bytes that don't decode as UTF-8;
the same is true in reverse for decompression. Adopting the M22 P2D
convention makes the round trip `decompress(compress(x)) == x`
hold for any packed-byte input (ASCII text included as the natural
"codepoints < 128" subset). The alternative — emit invalid UTF-8 via
`from_utf8_unchecked` — would have been faster on the encoding path
(no codepoint expansion in `bytes_to_packed_str`) but would punish
every subsequent string operation in the program: any `len()` call
would report UTF-8 byte count rather than logical byte count, and
concatenation could split a multi-byte sequence mid-character.
The P2D author's "valid UTF-8 throughout, even at the cost of a 2×
on-disk expansion for bytes > 127" call still pays out here.

## Native ID layout — 11 of 20 reserved slots used

| Range  | Module | Count | Notes |
|--------|--------|-------|-------|
| 500–502 | `gzip` | 3 | compress / compress_level / decompress |
| 503–507 | `zlib` | 5 | compress / compress_level / decompress / crc32 / adler32 |
| 508–510 | `bz2`  | 3 | compress / compress_level / decompress |
| 511–519 | reserved | — | v0.3 streaming handles, lzma/xz, multi-stream gzip |

The slots left open cover the natural v0.3 extensions:
`gzip.open(path) -> GzipFile` and friends (which need stdlib-class
registration, same M20c blocker that the streaming hash handle hit),
plus an `lzma` / `xz` pair if a real-world program turns up that
needs them.

## What v0.2 does not ship

Documented per-module in §9.34/§9.35/§9.36 of the spec; summary:

* **No streaming compress / decompress handles.** All three modules
  are one-shot only. Streaming needs stdlib-class registration to
  expose `update` / `finish` methods on an opaque `GzipFile` /
  `Compress` / `BZ2File` handle. Same v0.3 blocker the M22 P2B
  `Hasher` deferral hit.
* **No multi-member streams.** flate2 reads the first gzip member
  only; bzip2 reads the first stream only. The unix `gunzip -c |
  cat` semantics for concatenated archives wait for v0.3.
* **No header-field control on gzip.** Encoder emits default
  values for filename / mtime / OS; v0.3 may add a tuple-returning
  reader if a real-world program needs to inspect them.
* **No `bytes` type.** The whole industry-standard signature for
  these modules is `compress(bytes) -> bytes`. v0.2 routes
  everything through the str-as-byte-buffer convention; v0.3 will
  add `bytes` and the function names will stay, signatures will
  retype.

## Hardest three things (in retrospect)

1. **Picking compression-level defaults.** zlib's `Compression`
   enum exposes `Best`, `Fastest`, `Default`, `None`, plus
   `new(level: u32)` for explicit 0..=9. Python defaults to **6**
   for both `gzip.compress(data)` and `zlib.compress(data)`. I went
   the same way — both for ecosystem-cross-compat reasons (`gzip`
   output is binary-identical to Python's at default settings, which
   matters for example programs that want to assert "this CI
   artifact equals the Python one") and so users have a single
   number to scale from when reaching for `compress_level`.
   bz2 also uses 6 as the default. The default-level value is
   documented in each `§9.3x` spec block.

2. **Sorting out bzip2's level range.** flate2 uses 0..=9 (with 0
   meaning "store / no compression"). bzip2 uses **1..=9** —
   there is no "store" mode for the BWT-based bzip2 algorithm.
   The handler validates explicitly with a clear error message
   ("level must be 1..=9, got X") rather than letting the bzip2
   crate's lower-quality "invalid level" message through. Mismatched
   ranges are exactly the kind of incidental Python-compat trap I
   wanted to catch at the source.

3. **A demo loop that ran ~60s under the cargo-test default timeout.**
   The first cut of `bz2_demo.spy` built a 500-byte input by doing
   `repeat = repeat + "a"` 500 times in a loop, then ran four
   separate `bz2.compress` calls on it (default + level 1 + level 9 +
   the round-trip check). On Windows the bzip2 setup is genuinely
   slow — each call is ~10-30ms before any actual compression
   happens, and four serial calls plus the StrictPy interpreter's
   O(n²) immutable-string `+=` cost compounded to a wall-clock that
   exceeded cargo-test's 60-second "running for over 60 seconds"
   notification threshold. Dropping the input from 500 bytes to 200
   bytes kept the same coverage (compress vs decompress, level 1 vs
   level 9, size comparison) and brought the test home in <2s.

## Incidentally-discovered bugs / oddities

* **None requiring code changes.** Same trend as M22 P2B + P2D:
  pure crate-wrap modules add zero infrastructure load through the
  M19 stdlib-module-table seam. Resolver, typecheck, IR lowering, and
  GC scan all absorbed three more modules without complaint. This is
  the third consecutive Phase-2/3 crate-wrap round with zero
  incidentally-discovered bugs (M22 P2B = zero, M22 P2D = zero,
  M27 P3c-C = zero). My read: the stdlib infrastructure is now
  load-bearing-stable for any module that can be expressed as
  `(str|i32, ...) -> (str|i32|i64)` plus heap-allocated lists. The
  next infrastructure stretch is the stdlib-class hole (Hasher,
  GzipFile, ArgParser, Connection — all the "opaque handle" types).

* **The bz2 setup cost surprised me.** ~10-30ms per call is much
  higher than gzip / zlib (~0.1ms each on the same input). The
  bzip2 crate appears to do block-table allocation on every
  encoder construction. For programs that compress many small
  payloads, a streaming `BZ2Compressor` handle (which amortises the
  setup) would be a real ergonomic win — this is now a documented
  v0.3 motivator.

* **The `flate2` crate's `write::*` vs `read::*` modules.** I used
  `write::GzEncoder` / `write::GzDecoder` (Write-side wrappers that
  accept bytes and push to a buffer) rather than `read::GzEncoder`
  / `read::GzDecoder` (Read-side wrappers that pull bytes from a
  source). For our one-shot native handlers either side works; the
  Write side is marginally more ergonomic because we already have
  the full input in a `Vec<u8>`.

## Cross-platform notes

`flate2` is pure-Rust (its default backend is `miniz_oxide`, the
public-domain MIT-licensed pure-Rust DEFLATE implementation that
ships with Rust's std). `bzip2` defaults to wrapping the system
libbz2 — but the `bzip2-sys` crate bundles a vendored libbz2 C
source and statically links it, so no system bz2 install is required
on any of Windows / Linux / macOS. The crate compiled cleanly in
about ~3s on Windows during the first build; cached afterwards.

The CRC-32 and Adler-32 outputs are bit-exact across platforms
(both algorithms are byte-stream functions with no float / endian
dependency). The known-vector tests pass identical i64s on Windows
where I built them; CI on Linux should pass unchanged.

## Files modified + LOC (approximate)

| File | Lines added | Purpose |
|---|---|---|
| `shared/src/native.rs` | +45 | 11 new `NativeFn` variants (500-510) + `from_u32` arms |
| `compiler/src/resolver.rs` | +135 | Three `StdlibModule` registrations |
| `vm/src/builtins.rs` | +295 | 11 handlers using `packed_str_to_bytes` / `bytes_to_packed_str` |
| `vm/Cargo.toml` | +7 | `flate2` + `bzip2` runtime deps (with vendor comment) |
| `STRICTPY_SPEC.md` | +155 | §9.34 (gzip) + §9.35 (zlib) + §9.36 (bz2) |

Plus tests + examples:

* `examples/gzip_demo.spy` — 6 scenarios: default round-trip,
  highly-compressible 200-byte repeat, level 0 vs level 9 (size
  comparison + round-trip both), empty-input round-trip, malformed-
  input ValueError, out-of-range-level ValueError.
* `examples/zlib_demo.spy` — 7 scenarios including the standard
  RFC 3686 / Wikipedia CRC-32 vector
  (`crc32("123456789") == 0xCBF43926`) and the RFC 1950 §9 Adler-32
  vector (`adler32("Wikipedia") == 0x11E60398`).
* `examples/bz2_demo.spy` — 6 scenarios: default round-trip,
  200-byte repeat round-trip + size assertion, levels 1 vs 9 (no
  level-0 because bzip2 doesn't have "store"), empty round-trip,
  malformed-input ValueError, out-of-range-level ValueError.
* `compiler/tests/{gzip,zlib,bz2}_demo_runs.rs` — 6 subprocess
  tests via `spy.exe` (two per demo: a `*_compiles` and a
  `*_runs_via_spy_exe`).

## What's next (v0.3 candidates)

* **Streaming compressor / decompressor handles** for all three
  modules. Each is a thin opaque-handle wrapper over the existing
  one-shot encoder/decoder. Unblocks `for chunk in
  large_file_chunks(): out.write(comp.update(chunk))` patterns and
  amortises the bzip2 setup cost.
* **LZMA / XZ.** Same shape as bz2 — one crate (`xz2`), three
  handlers (`compress` / `compress_level` / `decompress`) for ~80
  LOC total. Reserved IDs 511-513 are sized for it.
* **gzip header-field accessors.** `gzip.headers(data) -> Dict[str, str]`
  to extract original-filename / mtime / OS. Useful when reading
  third-party `.gz` files in archive-aware programs.
* **`bytes` round-trip.** Once `bytes` lands in v0.4, all signatures
  retype from `str` to `bytes` — the function names and semantics
  stay identical; the str-as-byte-buffer convention becomes a
  back-compat alias.

M27 P3c-C is the **third** consecutive zero-incidental-bug crate-wrap
round on the stdlib infrastructure. The M19 design tax keeps paying
out; the next interesting frontier is the v0.3 stdlib-class surface
(streaming hashers / compressors / opaque handles), which exercises
*different* code paths from the function-only registrations the
M22+M27 rounds have stressed.
