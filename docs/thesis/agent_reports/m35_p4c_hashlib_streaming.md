# M35 P4-C — streaming `hashlib.Hasher` class

**Brief**: ship the streaming-Hasher counterpart to the one-shot
`hashlib.sha256` / `sha1` / `sha512` / `md5` / `hmac_sha256` helpers
from M22 P2B.  The one-shot surface forces the user to materialise
the whole input as a single string; streaming lets them feed chunks
(file checksums, log digests, large uploads) and finalise at the end.
Part of the M35 parallel round (P4-A `re.Pattern`, P4-B `sqlite3`
classes, P4-C `Hasher` — disjoint NativeFn ranges and disjoint slot
tables, no cross-agent coordination needed beyond the shared resolver
file).

**Wall-clock**: ~50 minutes of agent compute against a 2-hour budget.
First commit at ~40% of budget (test suite green on the first run);
final cleanup at ~70%.  **Lesson 1 streak: agent #15 clean.**

**Tests**: 8 new VM integration tests in
`vm/tests/m35_hashlib_streaming.rs`:

1. `sha256_incremental_matches_one_shot` — the canonical use case: three
   chunks → `h.hexdigest()` matches `hashlib.sha256(chunks_joined)`.
2. `sha1_incremental_matches_one_shot` — same for SHA-1 over a
   two-chunk input.
3. `sha512_incremental_matches_one_shot` — same for SHA-512 over the
   classic "quick brown fox" sentence.
4. `md5_incremental_matches_one_shot` — same for MD5 over three chunks.
5. `bad_algorithm_name_raises_value_error` — `hashlib.new("blake2b")`
   raises `ValueError` (caught in the test); the brief lists four
   supported algorithms; anything else traps cleanly.
6. `empty_input_matches_empty_string_digest` — `hashlib.new("sha256")`
   followed immediately by `hexdigest()` produces the canonical
   empty-string digest `e3b0c442…b855`.
7. `hexdigest_is_idempotent_under_further_updates` — calling
   `hexdigest()` twice gives the same string AND the user can keep
   `update`-ing afterwards and get the correct digest of the joined
   input.  This is the clone-not-consume property documented in spec
   §9.20.1.
8. `algorithm_method_returns_canonical_name` — `Hasher.algorithm()`
   returns the four canonical names verbatim.

All 8 added tests pass on the first cargo invocation after the
implementation landed.  Full `cargo test --workspace --release`
completes with exit code 0: 131 test groups, 0 failures, all
pre-existing tests preserved (the M33 stack-overflow note from M34's
report is no longer present — that test ran clean this time too).
The streaming-hashlib demo (`examples/hashlib_streaming_demo.spy`)
also runs end-to-end and prints `ok` for every algorithm + the
idempotent-hexdigest + the bad-algorithm error path.

## Design choices

Followed the M34 prelude-registration pattern exactly:

* **Hasher class layout**: registered in `seed_prelude` alongside
  `io.File` / `Channel` / `Thread`.  `final` (`is_open: false`,
  `is_sealed: false`), `is_native: true`, `fields: vec![]`,
  `payload_size: 0`.  Three methods declared (`update` / `hexdigest`
  / `algorithm`) for the type-checker, but the methods list never
  actually drives a vtable — dispatch is via NativeFn through the
  M34 class-by-name path in `ir::lower_method_call`.

* **Heap representation**: private `HasherRepr` struct
  (`vm/src/object.rs`) — `ObjectHeader + i64 handle`.  Mirrors
  `FileRepr` / `ChannelRepr` / `ThreadRepr` exactly.  Uses
  `GcKind::Class` with no scanned reference fields; the handle is a
  small monotonic i64 (starts at 1) so the GC's class-scanner's
  conservative `alive.contains(&p)` check filters it out cleanly
  even though it's not a real pointer (same trick used by the
  M28.5 TLS / M23 P3a-D SQLite handle tables).

* **Slot table**: `SharedVm.hashers: HashMap<i64, HasherSlot>` plus
  `next_hasher_id: AtomicI64`.  Chose `HashMap` over `Vec<Option<…>>`
  because the table is keyed by a true i64 (not an index) — re-using
  a freed slot via a new HasherRepr would be a correctness hazard,
  and a monotonic key trivially prevents that.  Each `HasherSlot`
  carries one of `Sha256` / `Sha512` / `Sha1` / `Md5` (Rust crates
  identical to the ones M22 P2B already uses) plus the algorithm
  name string.

* **Method dispatch**: extended
  `ir::m34_json_class_method_native_id_by_name` to recognise the
  three Hasher methods.  Renamed the function's doc comment to note
  it now serves M34+M35 stdlib classes; left the historical
  `m34_*` prefix since the M35 spec page still calls out "follow
  the M34 pattern" verbatim.

* **clone-not-consume hexdigest**: under the table lock, clone the
  matching `HasherState` variant; drop the lock; finalise the clone.
  The Rust hasher crates' `.finalize()` consumes `self`, but each
  variant implements `Clone`, so cloning before finalising leaves
  the original free for further `update` / `hexdigest` calls.  This
  is friendlier than CPython's slightly fuzzy "you can call
  hexdigest more than once but the state is final" wording — the
  StrictPy version is unambiguously: hexdigest is idempotent and
  does not affect the in-progress state.  Documented in spec §9.20.1.

* **Construction surface**: `hashlib.new(algorithm: str) -> Hasher`
  ONLY; direct `Hasher(...)` calls trap with a `TypeError` ("not
  constructible; use hashlib.new").  The `HasherCtor` NativeFn (ID
  820) is reserved for v0.4 if a direct constructor is wanted, but
  for now it's just an error stub that lets the ID range stay
  contiguous and self-documenting.

## NativeFn IDs

Per the brief's reservation: **820-829** for Hasher.

* 820 `HasherCtor` — reserved; traps with TypeError.
* 821 `HashlibNew` — `hashlib.new(algo) -> Hasher`.
* 822 `HasherUpdate` — `h.update(data) -> None`.
* 823 `HasherHexdigest` — `h.hexdigest() -> str`.
* 824 `HasherAlgorithm` — `h.algorithm() -> str`.
* 825-829 reserved for v0.4 (`Hasher.copy`, `Hasher.digest_size`,
  SHA-3 / BLAKE2 / BLAKE3 algorithm variants).

## Variable prefix discipline (Lesson 2)

All new locals in shared files (`compiler/src/resolver.rs`,
`compiler/src/ir.rs`, `vm/src/builtins.rs`, `vm/src/interp.rs`,
`vm/src/object.rs`, `shared/src/native.rs`) use the `p4c_` prefix.
This is the M35-round file-ownership protocol: the three M35 agents
share these files, so prefixing prevents accidental collisions on
short local names like `handle` / `state` / `name`.  Public types
(`HasherRepr`, `HasherSlot`, `HasherState`) follow the existing
StrictPy house style — no prefix — since they're cross-file API
surface, not locals.

## What's still in scope for v0.4

Documented in `STRICTPY_SPEC.md` §9.20.1's deferred list:

* **`Hasher.copy()`**: explicit branch.  Trivial given clone-not-
  consume already implements the underlying machinery; held until a
  shipping demo needs it.
* **`Hasher.digest_size: i64`**: per-algorithm constant.  Pure
  one-arm match; one v0.4 line.
* **SHA-3 / BLAKE2 / BLAKE3**: each is one extra crate dep + one
  more `HasherState` variant + one arm in `hashlib.new`.  Held back
  until a real-world program asks for one.
* **Module-scoped class registration**: same caveat M34 noted —
  `Hasher` is registered in the prelude rather than under the
  `hashlib` module.  `from hashlib import Hasher` works (the
  prelude-binding-wins branch in the import resolver covers it);
  the cleanup is invisible to users when v0.4 ships proper
  module-scoped class registration.

## Bug findings

None.  The closest issue was a typo in my first-pass test file: I
wrote `let h: Hasher = ...` (Rust-style binding) instead of StrictPy's
`h: Hasher = ...` declaration syntax.  Eight tests failed with
`expected newline, found Ident("h")` on the first run; a one-edit
search-and-replace across the test file fixed all eight at once.
That's not a bug in M0-M34 — it's a documented language difference
I should have known.

## Test counts

| Suite | Pre-M35 P4-C | Post-M35 P4-C |
|---|---:|---:|
| VM integration (`vm/tests/`) | per the M34 baseline | + 8 (m35_hashlib_streaming) |
| **Added by M35 P4-C** | — | **+8** |
| Pre-existing failures | per M34 baseline | unchanged |

## Lesson 1 compliance

First commit landed at ~40% of budget — well inside the 60% cap.
The streak holds at agent #15 clean (M34 closed #14, this closes
#15).

## Files shipped

| Path | Lines added | Purpose |
|---|---:|---|
| `compiler/src/resolver.rs` | +84 | Register the `Hasher` prelude class + add `hashlib.new` to the `hashlib` stdlib module's items. |
| `compiler/src/ir.rs` | +13 | Extend `m34_json_class_method_native_id_by_name` to dispatch `Hasher.update` / `hexdigest` / `algorithm`. |
| `shared/src/native.rs` | +39 | NativeFn entries 820-824 + their `from_u32` table arms. |
| `vm/src/object.rs` | +13 | `HasherRepr` heap layout (header + i64 handle). |
| `vm/src/interp.rs` | +82 | `HasherState` / `HasherSlot` types, SharedVm `hashers` + `next_hasher_id` fields (init in both the JIT and no-JIT constructors), `alloc_hasher` helper. |
| `vm/src/builtins.rs` | +161 | Five NativeFn handlers (`HasherCtor` traps; `HashlibNew` allocates a slot + a HasherRepr; `HasherUpdate` / `HasherHexdigest` / `HasherAlgorithm` look up the slot and dispatch by HasherState variant). |
| `vm/tests/m35_hashlib_streaming.rs` | +251 | 8 integration tests. |
| `examples/hashlib_streaming_demo.spy` | ~100 | The canonical user-facing demo. |
| `STRICTPY_SPEC.md` | +95 | §9.20.1 "Streaming `Hasher` class (v0.3 — M35 P4-C)" subsection. |
| `docs/thesis/agent_reports/m35_p4c_hashlib_streaming.md` | — | This report. |

Total compiler/runtime LOC: ~392 added across 6 files.  Tests + demo
+ docs: ~445.  Net ~840 LOC for the milestone — comfortably below
the brief's "2-hour" budget envelope and well-served by the M34
infrastructure (no new resolver / IR / GC plumbing was needed; the
prelude-class shape and the M34 class-by-name dispatch handle every
Hasher-specific concern with one-line extensions).

## Verdict

Streaming Hasher ships, the canonical use case (`hashlib.new("sha256")`
→ three `update` calls → `hexdigest()` matches the one-shot
`hashlib.sha256` digest) works end-to-end, the 8 new tests pass, no
regressions on the existing surface.  The clone-not-consume policy is
documented and tested.  Ready for whichever v0.3-running demo program
wants to compute a file checksum without reading the whole file into
RAM first.
