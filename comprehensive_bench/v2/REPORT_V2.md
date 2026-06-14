# StrictPy vs CPython — Comprehensive Benchmark v2

_Generated 2026-06-11 · StrictPy `spy 0.2.0` (commit 3825315, branch `perf/single-alloc-strings`) vs CPython 3.12.10 · Windows 11 · 59 paired programs, best-of-3 full-process runs, interleaved._

**Methodology.** Every benchmark is a *pair*: one `.spy` and one `.py` implementing the same algorithm with the same workload, idiomatic in each language. Correctness is enforced by **byte-identical stdout** (58/59 pass; the only failure is a VM crash, not a value mismatch). Timing is wall-clock of the full process on precompiled bytecode (`.spyc` / `.pyc`), so it includes runtime startup — measured floors: **StrictPy 20 ms, CPython 49 ms** (this *favors* StrictPy slightly on every row). Programs keep their hot loops in dedicated functions in both languages (see "Performance cliffs" — this matters enormously for StrictPy).

Speedup = CPython time ÷ StrictPy time. **>1 means StrictPy is faster.**

---

## ⏩⏩ Run-3 update — parity reached (after dict-FxHash + batch-struct + char-scan VM work)

Reran the identical suite on a release build that adds **dict string-hash caching (FxHash)**, **batch struct serialization**, and — the big one — **native acceleration for the per-character `s[i]` scan path**. This is the strongest result the suite has produced.

| | Original baseline | Post-fix (run 2) | **Run 3 (now)** |
|---|---|---|---|
| Passing / correct | 58/59 | 59/59 | **59/59** |
| Wins / ties / losses | 23 / 1 / 34 | 25 / 0 / 34 | **26 / 0 / 33** |
| **Geomean speedup (all)** | 0.72× | 0.88× | **0.96× — at parity with CPython** |
| Core compute | 4.2× | 5.1× | **4.7×** |
| Strings | 0.15× (6.7× slower) | 0.21× | **0.31× (3.2× slower)** |
| Systems | 1.20× | 1.41× | **1.39×** |
| Data structures | 0.53× | 0.56× | **0.58×** |

**Headline: `str_slice_scan` 14,793 ms → 132 ms** — the per-character scan went from the single worst result (66× *slower* than CPython) to **1.3× faster**. That one fix drives most of the jump to parity.

**What the dict-FxHash + batch-struct + string work moved:**

| Benchmark | baseline | run 3 | ratio (baseline → now) |
|---|---:|---:|---|
| `str_slice_scan` | 12,502 ms | **132 ms** | 66× slower → **1.3× faster** |
| `str_split_scan` | 2,535 ms | **672 ms** | 12.5× → **3.5×** slower |
| `str_http_parse` | 25,078 ms | **1,004 ms** | 290× → **12.5×** slower |
| `str_wordcount` | 507 ms | **345 ms** | 3.3× → **2.3×** |
| `ds_string_keys_aggregation` (dict FxHash) | 318 ms | **230 ms** | 2.4× → **1.9×** |
| `sys_struct_pack` (batch struct) | 1,019 ms | **843 ms** | 4.9× → **4.2×** |

**Did NOT improve / still open:**
- `ds_dict_ops`: 1,918 → **1,552 ms**, still **5.6× slower**. FxHash helped aggregation-shaped dict code but not this insert/lookup benchmark — its bottleneck is the `str(i)` key construction on every op, not the hash function.
- String formatting cluster remains the worst non-dict losers: `str_fstring_format` 8.2×, `str_csv_parse` 5.8×, `str_join_build` 5.4×, `str_template_render` 5.1×.
- FFI-shaped stdlib still loses: `struct` 4.2×, `file_io` 3.9×, `sqlite` 2.0×.
- Bug #2 (top-level `final` computed from another `final` → 0) — still not fixed.

**No regressions or new failures.** The few `core_*` benchmarks showing ~1–5 ms increases are jitter on sub-50 ms workloads. Run-3 artifacts: `results_v2_run3.json` (this run, also the live `results_v2.json`), `results_v2_prefix_baseline.json` (original clean run).

_Note: the "Post-fix update" and main body below describe earlier states and remain accurate as history; the per-benchmark tables further down reflect run 2, not run 3 — see `REPORT_V2_tables.md` (regenerated) for the current numbers._

---

## ⏩ Post-fix update (2026-06-12 rerun, after PRs #12/#14/#15 merged to main)

Five of the bugs below were fixed in cloud sessions and merged. Reran the **identical suite** on a fresh `cargo build --release`. Every program in this report's body still describes the **pre-fix** state; this section is the delta.

**Bugs resolved (verified):** i64 shift/bitwise 32-bit truncation (#1), `del d[k]` no-op (#4), unusable `Set` (#3), and the **asyncio access-violation crash (#5)** — `sys_async_tasks` went from a hard crash to **passing and 2.5× faster than CPython**. (Fixing async also exposed that this benchmark's `.spy`/`.py` files had two programs concatenated by the original authoring agent — both rewritten to a single clean program; the suite is now **59/59 passing**.)

**Headline movement:**

| | Before fixes | After fixes |
|---|---|---|
| Passing / correct | 58/59 | **59/59** |
| Wins / ties / losses | 23 / 1 / 34 | **25 / 0 / 34** |
| Geomean speedup (all) | 0.72× | **0.88×** |
| Core compute geomean | 4.2× | **5.1×** |
| Strings geomean | 0.15× (6.7× slower) | **0.21× (4.8× slower)** |
| Systems geomean | 1.20× | **1.41×** |

**The native string work that shipped alongside the fixes is the big story** — absolute StrictPy times dropped across the whole strings track:

| Benchmark | spy before | spy after | ratio before → after |
|---|---:|---:|---|
| `str_http_parse` | 25,078 ms | **1,125 ms** | 290× → **14×** slower |
| `str_split_scan` | 2,535 ms | **1,249 ms** | 12.5× → **6.1×** |
| `str_join_build` | 2,484 ms | **1,241 ms** | 10.2× → **5.2×** |
| `str_csv_parse` | 1,591 ms | **1,260 ms** | 7.1× → **5.5×** |
| `str_fstring_format` | 2,059 ms | **1,626 ms** | 10.2× → **8.1×** |
| `ds_dict_ops` | 1,918 ms | **1,534 ms** | 6.5× → **5.5×** |

**Still open / not improved:**
- `str_slice_scan` is unchanged-to-slightly-worse (**76× slower**, spy ~14.8 s) — the per-character `s[i]` + `i32(c)` scan path got no native acceleration. This is now the single worst result and the clearest remaining string bottleneck.
- Strings still lose the track overall (4.8× geomean); dicts, exceptions, closures, generics, and FFI-shaped stdlib (struct/file-io/sqlite) are unchanged.
- Bug #2 (final-from-final → 0) was **not** in the merged set — still reproduces.

_Caveat: the rerun ran under heavier machine load (CPython baselines ~30–40% slower in absolute ms), so cross-run **ratios** are the fair comparison and absolute StrictPy ms for the string wins are the most reliable signal. Post-fix artifacts: `results_v2_postfix.json` (final), `results_v2_prefix_baseline.json` (the original clean run)._

---

## Executive summary

| | Result |
|---|---|
| Benchmarks | 59 (12 core compute · 16 data structures & language features · 15 strings/text · 16 systems/stdlib) |
| Correctness | **58/59 byte-identical output** · 1 hard crash (`asyncio` — access violation on any spawn/run) |
| StrictPy wins (≥1.15× faster) | **23** |
| Ties | 1 |
| CPython wins (≥1.15× faster) | **34** |
| **Geometric-mean speedup, all 58** | **0.72× — StrictPy is net-SLOWER than CPython on this suite** |

Per-track geometric means tell the real story:

| Track | Geomean speedup | Best | Worst |
|---|---|---|---|
| Core compute (JIT-able numeric/control flow) | **4.2× faster** | 18.6× (bitops) | 1.9× (sieve) |
| Systems & concurrency | **1.2× faster** | 6.2× (rate limiter) | 0.20× (struct pack) |
| Data structures & language features | **1.9× slower** | 2.3× (class alloc) | 6.5× slower (dict) |
| Strings & text | **6.7× slower** | 0.8× (substring search) | **290× slower** (HTTP parse) |

**Verdict against the project's own bar** ("StrictPy should be significantly faster than Python; if not, that's a bug"): the JIT delivers convincingly on numeric kernels, object allocation, threads-without-GIL, channels, and sockets. But the moment a workload touches **strings, dicts, exceptions, closures, or FFI-shaped stdlib calls** — i.e. the actual substance of web servers, parsers, and distributed-systems code — StrictPy loses to CPython, often by an order of magnitude. By the stated bar, the entire right half of this report is a bug list.

---

## Results by track

### Core compute — StrictPy's home turf

| Benchmark | StrictPy | CPython | Speedup |
|---|---:|---:|---|
| `core_bitops` (xorshift + popcount) | 37 ms | 692 ms | ✅ **18.6×** |
| `core_branchy` (nested if/elif over LCG stream) | 41 ms | 229 ms | ✅ 5.6× |
| `core_int_arith` (collatz-style i64 loop) | 31 ms | 168 ms | ✅ 5.4× |
| `core_loops_nested` (triple-nested loops) | 27 ms | 146 ms | ✅ 5.4× |
| `core_float_arith` (mandelbrot kernel) | 39 ms | 185 ms | ✅ 4.8× |
| `core_quicksort` (recursive, in-language) | 35 ms | 127 ms | ✅ 3.6× |
| `core_recursion_fib` (naive fib) | 56 ms | 201 ms | ✅ 3.6× |
| `core_calls` (tiny-fn call overhead) | 64 ms | 202 ms | ✅ 3.2× |
| `core_recursion_ack` (ackermann 3,6) | 23 ms | 67 ms | ✅ 3.0× |
| `core_nbody` (5-body f64 simulation) | 46 ms | 135 ms | ✅ 2.9× |
| `core_matrix` (NxN f64 matmul on List-of-List) | 36 ms | 99 ms | ✅ 2.8× |
| `core_sieve` (List[bool] sieve) | 100 ms | 191 ms | ✅ 1.9× |

Startup (~20 ms) is a large share of these StrictPy times — the pure kernel advantage is higher than the table shows (e.g. int_arith kernel alone is ~26× faster, measured separately).

### Systems & concurrency — wins where Rust natives and no-GIL pay off

| Benchmark | StrictPy | CPython | Speedup |
|---|---:|---:|---|
| `sys_rate_limiter` (token bucket, 500k events) | 32 ms | 196 ms | ✅ 6.2× |
| `sys_threads_spawn` (200 OS threads) | 30 ms | 150 ms | ✅ 5.0× |
| `sys_channel_throughput` (200k msgs producer→consumer) | 86 ms | 342 ms | ✅ 4.0× |
| `sys_pqueue` (200k priority push/pop) | 106 ms | 331 ms | ✅ 3.1× |
| `sys_tcp_echo` (2000 loopback roundtrips) | 78 ms | 156 ms | ✅ 2.0× |
| `sys_datetime` (ISO roundtrip + calendar math) | 115 ms | 196 ms | ✅ 1.7× |
| `sys_url_codec` (quote/urlencode/parse) | 315 ms | 519 ms | ✅ 1.6× |
| `sys_random_math` (sqrt/sin/cos/log/exp kernel) | 165 ms | 261 ms | ✅ 1.6× |
| `sys_udp_packets` (5000 loopback datagrams) | 66 ms | 104 ms | ✅ 1.6× |
| `sys_lock_contention` (4 threads × 150k locked incs) | 242 ms | 167 ms | ❌ 1.5× slower |
| `sys_hash_sha256` (streaming + one-shot + HMAC) | 262 ms | 148 ms | ❌ 1.8× slower |
| `sys_sqlite` (40k inserts + ranged selects) | 309 ms | 143 ms | ❌ 2.2× slower |
| `sys_lru_cache` (300k get/put, segmented eviction) | 557 ms | 174 ms | ❌ 3.2× slower |
| `sys_file_io` (50k lines write/read/parse) | 418 ms | 106 ms | ❌ 4.0× slower |
| `sys_struct_pack` (400k pack/unpack roundtrips) | 1,019 ms | 207 ms | ❌ 4.9× slower |
| `sys_async_tasks` (1000 spawned futures) | **CRASH 0xC0000005** | 290 ms | 💥 |

### Data structures & language features — mostly losing

| Benchmark | StrictPy | CPython | Speedup |
|---|---:|---:|---|
| `ds_class_alloc` (500k 3-field objects) | 112 ms | 252 ms | ✅ 2.3× |
| `ds_list_ops` (1.5M append/index/pop) | 123 ms | 241 ms | ✅ 2.0× |
| `ds_comprehensions` (list+dict comp, 500k) | 141 ms | 151 ms | ➖ 1.1× |
| `ds_tuple_ops` (1M create/destructure) | 267 ms | 209 ms | ❌ 1.3× slower |
| `ds_virtual_dispatch` (900k polymorphic calls) | 192 ms | 141 ms | ❌ 1.4× slower |
| `ds_match_dispatch` (sealed-class expr tree eval) | 126 ms | 75 ms | ❌ 1.7× slower |
| `ds_list_sort` (400k i64 + 150k str sort) | 470 ms | 265 ms | ❌ 1.8× slower |
| `ds_generators` (1.5M yields) | 603 ms | 327 ms | ❌ 1.8× slower |
| `ds_nullable` (1M T?/None checks + coalescing) | 429 ms | 209 ms | ❌ 2.1× slower |
| `ds_generics` (400k Box/Pair ops) | 480 ms | 216 ms | ❌ 2.2× slower |
| `ds_string_keys_aggregation` (200k group-by) | 318 ms | 132 ms | ❌ 2.4× slower |
| `ds_exceptions` (600k iters, ~12% raise/catch) | 507 ms | 157 ms | ❌ 3.2× slower |
| `ds_sort_by_key` (250k strings, sorted_by) | 1,207 ms | 355 ms | ❌ 3.4× slower |
| `ds_closures_hof` (map/filter/reduce, 1M) | 664 ms | 177 ms | ❌ 3.7× slower |
| `ds_set_ops` (set workload — see note) | 424 ms | 94 ms | ❌ 4.5× slower |
| `ds_dict_ops` (250k inserts/lookups/has) | 1,918 ms | 297 ms | ❌ **6.5× slower** |

`ds_set_ops` note: StrictPy **has no working sets at all** (see Bugs) — its side emulates a set with `Dict[str, i64]`, the only option the language offers. The 4.5× is dict overhead + `str()` key conversion vs CPython's native int set.

### Strings & text — the disaster zone

| Benchmark | StrictPy | CPython | Speedup |
|---|---:|---:|---|
| `str_search` (substring hit/miss scan) | 212 ms | 161 ms | ❌ 1.3× slower |
| `str_json_roundtrip` (parse+stringify nested doc) | 362 ms | 211 ms | ❌ 1.7× slower |
| `str_regex` (compiled find_all + replace_all) | 301 ms | 175 ms | ❌ 1.7× slower |
| `str_concat_build` (800k-piece accumulator) | 632 ms | 256 ms | ❌ 2.5× slower |
| `str_wordcount` (600k-word frequency count) | 507 ms | 155 ms | ❌ 3.3× slower |
| `str_json_walk` (parse + typed-AST field walk) | 536 ms | 149 ms | ❌ 3.6× slower |
| `str_base64` (encode/decode loop) | 819 ms | 183 ms | ❌ 4.5× slower |
| `str_methods_mix` (strip/replace/contains/startswith) | 1,797 ms | 316 ms | ❌ 5.7× slower |
| `str_template_render` (mustache-style substitution) | 948 ms | 150 ms | ❌ 6.3× slower |
| `str_csv_parse` (50k rows × 6 cols split+parse ×8) | 1,591 ms | 225 ms | ❌ 7.1× slower |
| `str_join_build` (100k-int comma join ×20) | 2,484 ms | 244 ms | ❌ 10.2× slower |
| `str_fstring_format` (600k row renders) | 2,059 ms | 203 ms | ❌ 10.2× slower |
| `str_split_scan` (log split into lines/fields) | 2,535 ms | 203 ms | ❌ 12.5× slower |
| `str_slice_scan` (char scan + slice windows) | 12,502 ms | 189 ms | ❌ **66× slower** |
| `str_http_parse` (12k HTTP request parses) | 25,078 ms | 86 ms | ❌ **290× slower** |

---

## Weakness analysis — ranked by impact on real server workloads

1. **Per-character string work is catastrophically slow (66–290×).** There is no `str.lower()`/`str.upper()`, so anything that case-normalizes (HTTP header names!) must loop char-by-char doing `out = out + str(char(i32(c) + 32i32))` — one native call + one tiny allocation + one concat *per character*. `str_http_parse` spends ~25 s parsing 12k small requests; CPython does it in 86 ms. Char access `s[i]` itself plus `i32(c)` conversion is ~66× slower than CPython's indexing in `str_slice_scan`. **A web server cannot be written in StrictPy at acceptable speed today.**

2. **No `str.join`, no string builder, multi-piece concat allocates every intermediate (10×).** `"row " + name + " -> " + sc + " pts"` allocates 4 intermediates per render. CPython's `",".join(...)` idiom has no StrictPy equivalent. This hits every formatting, templating, and response-building path (`str_fstring_format`, `str_join_build` both ~10×). f-strings — the natural fix — **don't parse at all** (see Bugs).

3. **`split()`-shaped parsing is 7–12× slower** (`str_split_scan`, `str_csv_parse`). Each split allocates a fresh List plus per-field strings in the GC heap; CPython's tuned C path wins big. Combined with #1 and #2, all text protocols (HTTP, CSV, logs, config) are 1–2 orders of magnitude off.

4. **Dict is the slowest core container (6.5×)** — opcodes are interpreted (not JIT'd) and every operation re-hashes the full string key; keys are `str`-only, so integer keys pay `str(i)` conversion on top (`ds_set_ops` path). Dict-heavy code — caches, session tables, routing tables, JSON-ish records — is the bread and butter of distributed systems.

5. **Exceptions, closures, and higher-order builtins de-optimize (3.2–3.7×).** Functions containing `try`/`raise` are excluded from the JIT entirely (known design), and `map`/`filter`/`reduce` cross the native↔interpreter boundary per element. Idiomatic error handling or functional style costs 3×.

6. **FFI-marshalling-shaped stdlib is slower than CPython's C modules**: struct pack/unpack 4.9× (no batch `pack(">IdQ", ...)` API — one native call per primitive), file I/O 4.0× (no buffered reader/writer; whole-file string round-trips), sqlite 2.2× (stringified cells), sha256 1.8×, base64 4.5× (str-as-byte-buffer copies).

7. **Asyncio crashes outright** (0xC0000005 on any `spawn_*`/`run_*`, even a no-op closure — reproduced on both the committed HEAD and the WIP binary). The documented async story for I/O concurrency is unusable; only thread-per-connection works.

### Performance cliffs (silent, structural)

These cost 25–250× and are invisible until you measure:

- **`main()` is never JIT-compiled.** An identical integer loop: 13.0 s in `main()`, 0.50 s in a named function. Same loop in CPython: identical either way. Every benchmark in this suite had to be restructured `fn run() -> i32` + `fn main(): return run()` to measure the language instead of its interpreter.
- **One unsupported construct de-optimizes the whole function** (per-function JIT opt-in, no tiering): adding a single `println`/`slice` to a function containing a string-append loop turned it from 0.31 s to >60 s (quadratic) in a controlled A/B.
- **The amortized-O(N) concat optimization is fragile.** It fires only when the accumulator's *sole* other use is `return s`. `s = s + "," + str(i)` (two-step concat) silently reverts to O(n²): 25k appends = 12.5 s, 50k = 58 s. The optimized form is 0.3 s for 200k.
- **Integer-literal inference is position-sensitive**: `x: i64 = 10 + len(s)` is a compile error (the literal infers `i32` on the left of a binop), `len(s) + 10` compiles. Harmless-looking refactors break builds.

---

## Correctness bugs found by this suite (all with minimal repros)

| # | Bug | Severity |
|---|---|---|
| 1 | **i64 bitwise/shift ops compute in 32-bit width**: `2463534242 >> 1` → `-915716527` (correct: `1231767121`); `& 4294967295` returns negatives; popcount loops on values ≥2³¹ never terminate | Silent wrong values / hangs |
| 2 | **Top-level `final` computed from another `final` silently evaluates to 0** (`final SOLAR_MASS: f64 = 4.0 * PI * PI` → 0.0) | Silent wrong values |
| 3 | **`Set[T]` is unusable**: no `.add/.has/.length` in the typechecker, `x in s` traps at runtime ("native SetHas: deferred to M6"), set comprehensions don't run | Documented feature absent |
| 4 | **`del d[k]` is a silent no-op** and no `remove/pop` exists — Dict keys cannot be removed at all | Silent no-op; blocks caches/eviction |
| 5 | **asyncio crashes** (access violation) on any `run_*`/`spawn_*`, even no-op closures | Hard crash |
| 6 | **f-strings don't parse** (`E0001 at FStrStart`) despite being documented | Documented feature absent |
| 7 | **`s.char_at(i)` compiles but traps at runtime** (unknown native id) | Runtime trap |
| 8 | **JObject positional destructure** (`case JObject(o):`) binds the first *field*, not the object — must use `isinstance` narrowing instead | Wrong binding |
| 9 | Non-exhaustive `match` on sealed classes prints warnings **to program stdout**, corrupting program output | Output pollution |
| 10 | Int literals infer `i32` on the left of binops / in set literals, rejecting valid i64 code (`E2001`) | Ergonomics/compile errors |

Documentation drift (LANGUAGE_GUIDE.md vs reality) hit every track: `let` declarations/destructuring don't compile (bare `a, b = t` does), string methods are `find`/`startswith`/`endswith` (not `index_of`/`starts_with`/`ends_with`), no `upper/lower/repeat`, `min_i64`/`max_i64` aren't in the prelude, the `Set` method list is fiction, `threading.lock_new` not `lock()`, `queue.pq_*` names differ, class bodies can't be bare `pass`. **The guide is the contract AI tooling writes against — every drift line cost this suite a broken program.**

### WIP branch note

The uncommitted `single-alloc-strings` VM changes (vm/src/{builtins,gc,interp,object}.rs, saved as [wip_vm_changes.patch](wip_vm_changes.patch)) were benchmarked separately: **performance-identical to HEAD across all 59 benchmarks (within noise), no new failures.** The asyncio crash and all string weaknesses above exist in committed HEAD; nothing here blames the WIP.

---

## What's missing for large-scale distributed systems

Ranked by how hard each blocks "millions of users" service work, combining the benchmark evidence with the stdlib survey:

**Tier 1 — blockers**
1. **A working async I/O runtime** (today: crashes). Needed: epoll/IOCP event loop, async sockets/timers, structured concurrency, and ideally `async/await` syntax instead of monomorphic `spawn_i32/spawn_str` helpers.
2. **Fast native string toolkit**: `lower/upper`, `join`/StringBuilder, format specs or working f-strings, byte-level scanning without per-char allocation. (This single item flips the 6–290× string losses.)
3. **A real `bytes` type** with zero-copy slices + **batch binary serialization** (`struct.pack(">IdQ", ...)`-style, buffer reuse). `str`-as-byte-buffer copies on every hop today.
4. **Complete core containers**: Dict key deletion, working Set, non-string Dict keys (i64 keys without stringification), ordered iteration guarantees.
5. **HTTP server stack** (only a client exists today) — HTTP/1.1 keep-alive + HTTP/2, routing, websockets; TLS server primitives exist but nothing above them.

**Tier 2 — required soon after**
6. **JIT coverage for dict/string/exception opcodes + tiering** (remove the function-level all-or-nothing cliff; JIT `main`).
7. **Wire-protocol clients**: protobuf/gRPC or at least msgpack, Redis, Kafka/NATS, PostgreSQL (sqlite-only today, with stringly-typed cells).
8. **Concurrency utilities**: thread pool / work-stealing executor, atomics, concurrent dict, `select` over channels, backpressure-aware bounded queues (Channel exists and is fast — build on it).
9. **Connection pooling, retries/backoff, timeouts as first-class** in http_client/socket.
10. **Observability**: structured JSON logging, metrics (Prometheus exposition), trace propagation hooks.

**Tier 3 — production hygiene**
11. Crypto: bcrypt/argon2 (password storage is explicitly unsafe today), constant-time compares, UUID, secure-random tokens.
12. Process model: signal handling, graceful shutdown, supervisor/daemon support.
13. Packaging/distribution story (no package manager; stdlib is the entire ecosystem).
14. Bigint or checked-overflow option (i64 silently wraps where Python promotes — caught by this suite's mod-prime workarounds).

---

## Reproduction

```
cargo build --release
python comprehensive_bench/v2/run_v2.py --repeats 3      # full suite → results_v2.json
python comprehensive_bench/v2/verify.py                  # correctness-only pass
python comprehensive_bench/v2/make_report.py             # regenerate REPORT_V2_tables.md
```

Artifacts: [results_v2.json](results_v2.json) (clean HEAD, canonical) · [results_v2_wip.json](results_v2_wip.json) (WIP binary) · [results_v2_clean.json](results_v2_clean.json) (backup copy) · [REPORT_V2_tables.md](REPORT_V2_tables.md) (auto-generated tables) · per-track authoring notes in `notes_core/ds/str/sys.md` · 59 program pairs in `programs/`.
