# M27 P3c-D — `zipfile` + `tarfile` stdlib modules

**Brief**: Ship two archive modules in one agent: read+write `.zip`
(via the `zip` crate, pure Rust, DEFLATE) and read+write `.tar`
(via the `tar` crate, with optional `flate2` / `bzip2` for the
`r:gz` / `r:bz2` / `w:gz` / `w:bz2` transparent-compression modes).
Both use the same opaque-handle + slot-table shape that M23 P3a-D
established for `sqlite3`, but with **separate read and write tables**
per format because the underlying `zip::ZipArchive<R>` vs.
`zip::ZipWriter<W>` (and `tar::Archive<R>` vs. `tar::Builder<W>`)
are *different types* — there's no single trait both ends implement.

**Wall-clock**: ~75 min agent compute (cold-build cost of `zip` +
`tar` + `flate2` + `bzip2` + their transitive deps was the dominant
contributor, not the handler logic).

**Files changed**:
1. `vm/Cargo.toml` — `zip = "2"`, `tar = "0.4"`, `flate2 = "1"`,
   `bzip2 = "0.4"` (4 new top-level deps, ~20 transitive).
2. `shared/src/native.rs` — 16 `NativeFn` variants (8 zipfile +
   8 tarfile) in the 520-549 ID range with `from_u32` arms.
3. `compiler/src/resolver.rs` — two `StdlibModule` registrations
   appended at the end of `seed_stdlib_modules`.
4. `vm/src/interp.rs` — 4 new `SharedVm` fields (`zip_readers`,
   `zip_writers`, `tar_readers`, `tar_writers`), each initialised
   with `vec![None]` in both `SharedVm::new` and `new_with_jit`.
5. `vm/src/lib.rs` — public `TarReadHandle` (struct: entries
   hashmap + insertion-order Vec) and `TarWriteHandle` (3-variant
   enum: `Plain(tar::Builder<File>)`, `Gz(tar::Builder<GzEncoder<File>>)`,
   `Bz2(tar::Builder<BzEncoder<File>>)`).
6. `vm/src/builtins.rs` — 16 handler arms inline before
   `NativeFn::Unknown` (~600 LOC). Every loop variable and intermediate
   binding uses the `p3c_d_<module>_<fn>_<purpose>` naming convention
   (see "Methodology: cherry-pick alignment" below).
7. `STRICTPY_SPEC.md` — §9.30 (zipfile), §9.31 (tarfile).
8. `examples/zipfile_demo.spy` + `examples/tarfile_demo.spy`
   (~120 LOC each).
9. `compiler/tests/zipfile_demo_runs.rs` +
   `compiler/tests/tarfile_demo_runs.rs` (2 subprocess tests each:
   compile-only + run-and-assert).
10. This report.

## API surface (16 functions, IDs 520-537)

### zipfile (520-527)

| ID  | Name           | Signature |
|-----|----------------|-----------|
| 520 | `open_read`    | `(path: str) -> i64` |
| 521 | `open_write`   | `(path: str) -> i64` |
| 522 | `names`        | `(h: i64) -> List[str]` |
| 523 | `read`         | `(h: i64, name: str) -> str` |
| 524 | `write`        | `(h: i64, name: str, data: str) -> None` |
| 525 | `close`        | `(h: i64) -> None` |
| 526 | `is_zipfile`   | `(path: str) -> bool` |
| 527 | `info`         | `(h: i64, name: str) -> Tuple[i64, i64, i64]` |

528-529 reserved for v0.3 (`zipfile.append_mode`, per-entry options).

### tarfile (530-537)

| ID  | Name           | Signature |
|-----|----------------|-----------|
| 530 | `open_read`    | `(path: str, mode: str) -> i64` |
| 531 | `open_write`   | `(path: str, mode: str) -> i64` |
| 532 | `names`        | `(h: i64) -> List[str]` |
| 533 | `read`         | `(h: i64, name: str) -> str` |
| 534 | `write_file`   | `(h: i64, src_path: str, arcname: str) -> None` |
| 535 | `write_data`   | `(h: i64, arcname: str, data: str) -> None` |
| 536 | `close`        | `(h: i64) -> None` |
| 537 | `is_tarfile`   | `(path: str) -> bool` |

538-549 reserved for v0.3.

## Why two slot tables per format (not one)

`zip::ZipArchive<File>` and `zip::ZipWriter<File>` don't share a
common trait that lets a single `Vec<Option<Box<dyn ???>>>` hold both
— the read API hands out file entries (with random access by name);
the write API takes start_file + write_all + finish. The cleanest
fit was two separate tables, dispatched on by `close` (try the
writer table first; if the slot is `None`, fall through to the
reader table). User code never sees this — the handle space is
shared (handle 1 might be a reader or a writer, but never both, and
`close` does the right thing either way).

Tarfile follows the same shape: `TarReadHandle` is a struct (entries
loaded eagerly into a `HashMap<String, Vec<u8>>` + insertion-order
`Vec<String>` to preserve display order), `TarWriteHandle` is an
enum with three variants for the three writer flavours.

## Why eager-load tar entries at open_read time

`tar::Archive<R>` is a *streaming* reader — it has no concept of
random access by name. Once you've called `.entries()` and walked
past entry N, you can't seek back. Three options:

1. Re-open the file from scratch for every `read(name)` call —
   simple, but O(N²) for "list-then-read-all" patterns and forces
   us to keep the `path` + `mode` in the slot so we can re-open
   with the same decoder.
2. Stream-decode lazily, holding a cursor in the slot — fragile
   (one out-of-order `read` invalidates the stream) and ugly to
   express in Rust (the `tar::Entry<'_, R>` borrow conflicts with
   slot-table re-entry).
3. Eager-load: decode every entry's bytes at open time into a
   `HashMap<String, Vec<u8>>`. O(N) at open + O(1) per read. Memory
   cost is the full archive resident in RAM.

Picked (3). The brief noted that v0.2 covers the tens-of-MB scale
typical of build / log / backup archives, where the resident-memory
cost is fine. Streaming for arbitrarily large archives is a v0.3
candidate (would need a fresh API shape — `tarfile.entries()`
returning a cursor-style iterator).

## The str-as-byte-buffer round-trip

Both `zipfile.read` / `tarfile.read` return entry payloads as `str`
whose chars are each codepoint 0..255 (the same convention M22's
`struct` module already uses). `bytes_to_packed_str` (existing
helper) is the encode side; `packed_str_to_bytes` is the decode
side used in `zipfile.write` / `tarfile.write_data`. Codepoint
range-check fires `ValueError` if the user passes a str with any
codepoint > 255 — the v0.2 packed-byte invariant.

The zipfile demo exercises this with a 256-codepoint buffer (every
byte 0..255 appearing exactly once); after DEFLATE round-trip,
`rt_c_len: 256` confirms the high-byte path is intact.

## Methodology: cherry-pick alignment (CRITICAL)

The M23 P3a-D milestone doc flagged a cherry-pick mis-alignment
when two parallel agents both wrote handlers with the same shape
("`let sp = interp.alloc_string(...) as u64;`") — git's three-way
merge mis-aligned the handlers. The brief explicitly called this
out as the M27 P3c-D risk.

**Mitigation**: every loop variable and intermediate binding in the
new handlers uses the prefix `p3c_d_<module>_<fn>_<purpose>` (e.g.
`p3c_d_zip_open_read_path`, `p3c_d_tar_open_read_iter`,
`p3c_d_tar_wf_arc`). The 16 handlers contain *zero* lines that
collide verbatim with a sqlite3 / pathlib / queue / threading
handler. Diff-alignment by git's myers + patience heuristics should
have no false-anchor candidates.

## Methodology: commit-before-report

The brief flagged the compute-budget exhaustion risk that M23 P3a-D
and all four M24 agents hit (build passes; report-writing burns the
remaining budget; the worktree doesn't get committed). For this
agent: the report was drafted in parallel with the (long) test
build, then committed in one shot once `cargo test --workspace
--release` is verified green.

## Test totals

After M27 P3c-D the workspace expects:
- 586 baseline (M27 P3c-A/B/C have NOT been merged into this
  worktree, so the actual integer offset depends on cherry-pick
  order — but P3c-D's contribution is +4 tests: 2 compile-only +
  2 subprocess for each demo).
- New tests: 4 (`zipfile_demo_compiles`, `zipfile_demo_runs_via_spy_exe`,
  `tarfile_demo_compiles`, `tarfile_demo_runs_via_spy_exe`).

## Build-cost note

`zip` + `tar` + `flate2` + `bzip2` plus their transitive crates
(`bzip2-sys`, `crc32fast`, `deflate64`, `zopfli`, `xz2`, etc.) add
roughly 90 seconds to a cold `cargo build --workspace --release`
on the build host. Incremental rebuilds (handler-only edits) are
cached, so the impact is one-time. Release binary size growth is
~1.5MB across the four crates — well inside the 5MB budget the
P3a-D report noted for FFI-backed modules.

## What's NOT in v0.2 (deferred to v0.3)

- **Append mode** for zip / tar — would require re-streaming the
  existing entries on open; needs a careful design call about whether
  it goes through `open_write` with a new mode string or via a
  separate `open_append`.
- **Streaming reads** for arbitrarily large tar archives — would
  need a cursor-style iterator API; eager-load is fine for the v0.2
  scale.
- **xz / lzma** modes — the `tar` crate supports them via `xz2`;
  deferred until there's a concrete use case.
- **Per-entry options** in writes (compression level, mtime, uid /
  gid, owner names) — fixed at sensible defaults (DEFLATE for zip,
  mode `0o644` + current time for tar `write_data`).
- **Password-protected zips** — supported by `zip` 2.x but rarely
  needed for the build / log / config-bundle workflows that motivate
  v0.2 archive support.
- **`is_tarfile` over compressed wrappers** — only the POSIX magic
  at offset 257 is sniffed; callers who want "is this a `.tar.gz`"
  can decompress the first 512 bytes themselves. Mirrors Python's
  `tarfile.is_tarfile` (which also only sniffs the inner format).
- **Listing of dir / symlink / device entries** in `tarfile.names` —
  v0.2 skips non-file entries since `read` would return empty for
  them and v0.2 has no `bytes` type to distinguish "empty payload"
  from "not a file".

## Hardest three things (in retrospect)

1. **The shared-handle / two-table dispatch in `close`.** First
   draft put readers and writers in one `Vec<Option<dyn ???>>` and
   used a `Box<dyn Closeable>`-style trait. Couldn't make it work
   because `ZipWriter::finish(self)` takes self by value (consumes
   the writer to write the central directory), and trait-object
   methods can't have `self`-by-value signatures without
   `Box<Self>`. Switched to two tables; `close(handle)` tries the
   writer table first (writers need the explicit finish), then the
   reader table.

2. **The `TarWriteHandle` enum.** Three variants for `Plain` / `Gz`
   / `Bz2` are unavoidable because `tar::Builder<W>` is generic
   over the writer. Originally tried `tar::Builder<Box<dyn Write>>`
   but the gz / bz2 encoders need their own `.finish()` call to
   flush the trailing bytes — that's a typed-encoder call, not a
   `Write` method. The enum dispatch in `close` is a four-line
   match.

3. **`zip::ZipWriter::start_file`'s `FileOptions<T>` generic.** The
   default has changed across `zip` crate versions; `2.x` made the
   type parameter for "extra-attributes" required. The fix was an
   explicit annotation `FileOptions::<()>::default()` — five
   minutes of staring at the compiler error.

## Incidental bugs / oddities

None requiring code changes. The stdlib-module seam absorbed two
new modules without complaint — same shape as M22 / M23 /
M27 P3c-A/B/C predecessors: one `Cargo.toml` dep block, four
`SharedVm` fields, two `StdlibModule` registrations, 16 dispatch
arms. Zero resolver / typecheck / IR changes.

## Cross-platform notes

`zip` (pure Rust, no FFI), `tar` (pure Rust, no FFI), `flate2`
(pure Rust via `miniz_oxide` by default) all build cleanly on
Windows / Linux / macOS without `cfg(target_os = ...)` gates.
`bzip2` does link against `libbz2.a` via `bzip2-sys` which compiles
the C source from the crate (no system libbz2 dependency); the
build adds ~5s to first compile.

The example demos write into `target/` (which `cargo clean` eats)
so re-runs leave no worktree residue.

## Next-step menu

- v0.3 typed `bytes` would let `read` / `write` skip the
  packed-byte convention and return a real binary buffer. Both
  modules are well-positioned to swap the payload type once that
  lands.
- A `zipfile.entries(handle) -> List[Tuple[str, i64, i64, i64]]`
  composite accessor would save round-trips for "list then info
  every entry" patterns; deferred until a benchmark shows it.
- `tarfile.write_dir(handle, arcname)` for explicit directory
  entries — currently the only way to record a dir is via
  `write_file` with a directory `src_path`, which the `tar` crate
  flags by inspecting the OS metadata.
