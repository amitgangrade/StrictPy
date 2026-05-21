# Per-milestone metrics

Source data: [per_milestone.csv](per_milestone.csv).
Benchmark data: `bench/history/`.

## Quantitative progression

| M | Tests | Compiler LOC | VM LOC | Examples LOC | Bugs found | Bugs fixed | Deferred | Bench W/T/L | fib(30) ms |
|---|---:|---:|---:|---:|---:|---:|---:|:---:|---:|
| M0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | — | — |
| M1 | 55 | 5,618 | 0 | 0 | 0 | 0 | 0 | — | — |
| M2 | 74 | 8,662 | 0 | 0 | 0 | 0 | 0 | — | — |
| M3 | 89 | 11,900 | 0 | 202 | 0 | 0 | 0 | — | — |
| M4 | 95 | 11,900 | 3,125 | 202 | **3** | 0 | 3 | — | — |
| M3.5 | 107 | 12,500 | 3,125 | 202 | 3 | **3** | 0 | — | — |
| M5 | 121 | 12,500 | 5,200 | 202 | 0 | 0 | 0 | — | — |
| M6 | 132 | 13,000 | 5,500 | 202 | 6 | 6 | 0 | — | — |
| M7 | 134 | 13,200 | 5,650 | 202 | 3 | 3 | 0 | — | — |
| **M8** | 134 | 13,200 | 6,900 | 202 | 0 | 0 | 0 | **10/2/4** | **14.6** |
| **M9** | 134 | 13,200 | 7,300 | 202 | 0 | 0 | 0 | **16/0/0** | **13.5** |
| **M10** | **173** | 13,700 | 8,600 | **1,660** | **17** | 11 | **6** | 16/0/0 | 15.8 |
| **M11** | **201** | 13,900 | 8,600 | **3,470** | 6 | **10** | 2 | 16/0/0 | 13.1 |
| **M12** | **206** | 13,553 | 7,437 | **4,947** | 2 | 1 | 1 | 16/0/0 | 13.1 |
| **M13** | 212 | 13,700 | 7,437 | 4,947 | 0 | 1 | 0 | 16/0/0 | 13.1 |
| **M14** | 222 | 14,100 | 7,437 | 5,200 | 0 | 0 | 0 | 16/0/0 | 13.1 |
| **M15** | 234 | 14,600 | 7,900 | 5,300 | 0 | 1 | 0 | 16/0/0 | 13.1 |
| **M16** | 245 | 15,000 | 8,000 | 5,400 | 0 | 0 | 0 | 16/0/0 | 13.1 |
| **M17** | **255** | **15,631** | **7,780** | **5,517** | 0 | 0 | 0 | 16/0/0 | 13.1 |
| **M18** | **267** | 15,700 | 7,820 | **7,400** | 1 | 1 | 0 | 16/0/0 | 13.1 |
| **M19** | 285 | 16,100 | 7,950 | 7,600 | 0 | 0 | 0 | 16/0/0 | 13.1 |
| **M20a** | 314 | 16,300 | 8,050 | 8,000 | 1 | 0 | 1 | 16/0/0 | 13.1 |
| **M20b** | 348 | 16,500 | 8,200 | 8,200 | 0 | 0 | 0 | 16/0/0 | 13.1 |
| **M20c** | 370 | 16,650 | 8,400 | 8,350 | 0 | 0 | 0 | 16/0/0 | 13.1 |
| **M21** | **379** | **16,823** | **8,789** | **8,467** | 0 | **1** | 0 | 16/0/0 | 13.1 |
| **M22** | **468** | **17,656** | **10,603** | **9,218** | 0 | 0 | 0 | 16/0/0 | 13.1 |
| **M23** | **553** | **18,800** | **12,200** | **9,700** | 1 | 1 | 0 | 16/0/0 | 13.1 |
| **M24** | **578** | 18,900 | 12,200 | **11,200** | 1 | 1 | 0 | 16/0/0 | 13.1 |
| **M25** | **586** | 18,895 | 12,397 | 11,200 | 0 | 0 | 0 | 16/0/0 | 13.1 |
| ... | ... | ... | ... | ... | ... | ... | ... | ... | ... |
| **M34** | **690** | 20,760 | 17,800 | 13,290 | 0 | 0 | 0 | 16/0/0 | 13.1 |
| **M35** | **723** | **21,100** | **18,450** | **13,587** | 0 | 0 | 0 | 16/0/0 | 13.1 |
| **M36** | 723 | **21,581** | 18,450 | 13,587 | 0 | 0 | 0 | 16/0/0 | 13.1 |
| **M37** | **744** | **21,881** | **19,620** | **13,717** | 0 | 0 | 0 | 16/0/0 | 13.1 |
| **M38** | **769** | **22,152** | **20,866** | **13,827** | 0 | 0 | 0 | 16/0/0 | 13.1 |

Notes:
- LOC is at end-of-milestone; M1's LOC are mostly lexer+parser+pretty (already in their final shape).
- "Bugs found" = bugs first observed in this milestone (whether fixed here or later).
- "Bugs fixed" = bugs closed during this milestone.
- "Bench W/T/L" = StrictPy wins / ties / losses on the 16-cell benchmark suite (only meaningful M7 onwards).
- The "M3.5" row is a sub-milestone for the IR-bug-fix detour between M4 and M5.

## Benchmark progression

Same suite (16 cells across fib, quicksort, dot, mandelbrot) across snapshots.
See `bench/history/` for the underlying JSON.

| Snapshot | W | T | L | fib(30) | best vs CPython | worst vs CPython |
|---|---:|---:|---:|---:|:---:|:---:|
| M7-unfair* | 5 | 3 | 8 | 967 ms | 4× faster (fib(20)) | 5.4× slower (fib(33)) |
| M7-fair | 5 | 3 | 8 | 931 ms | 4.7× faster (fib(20)) | 5.1× slower (fib(32)) |
| M8 (JIT) | 10 | 2 | 4 | 14.6 ms | **11× faster** (fib(30)) | 3× slower (quicksort 100K) |
| M9 (full JIT) | **16** | 0 | 0 | 13.5 ms | **17× faster** (fib(32)) | **5× faster** (dot 1M) |
| M10 (stress test) | 16 | 0 | 0 | 15.8 ms | **17× faster** (fib(33)) | **4× faster** (dot 100K, mandelbrot) |
| M11 (class fix) | 16 | 0 | 0 | 13.1 ms | **16× faster** (fib(33)) | **5× faster** (dot 1M, mandelbrot) |
| M12 (stress test 2 + torture) | 16 | 0 | 0 | 13.1 ms | (same as M11; no codegen change affecting perf) | — |
| M13–M17 (language completeness) | 16 | 0 | 0 | 13.1 ms | (no benchmark cells touch the new surface yet) | — |
| M18 (round 4 stress test) | 16 | 0 | 0 | 13.1 ms | (correctness round; no perf delta) | — |
| M19–M21 (Phase 1 stdlib) | 16 | 0 | 0 | 13.1 ms | (8 new stdlib modules; no codegen affecting perf) | — |
| M22 (Phase 2 stdlib, 4× parallel) | 16 | 0 | 0 | 13.1 ms | (9 more stdlib modules; first parallel-agent round) | — |
| M23 (Phase 3a stdlib, 4× parallel) | 16 | 0 | 0 | 13.1 ms | (7 more stdlib modules; second parallel-agent round) | — |
| M24 (Phase 3a stress, 4× parallel) | 16 | 0 | 0 | 13.1 ms | (4 stress programs + BUG-039 fix; no codegen affecting perf) | — |
| M25 (unified `spy` CLI) | 16 | 0 | 0 | 13.1 ms | (CLI refactor only — no codegen, lib API, or VM core changes) | — |
| M26 (extended bench: 5 compute + 5 stdlib, 30 cells) | 28 | 2 | 0 | 13.1 ms | **0.02× (heapsort 500k)** | **1.13× (btree 10k — narrow CPython win on allocation-bound shape)** |
| M27 (Phase 3c stdlib: 5 parallel agents, 9 modules) | 16 | 0 | 0 | 13.1 ms | (no codegen-affecting change; 33 stdlib modules total) | — |
| M28 (Phase 3b stdlib: 3 parallel agents, 3 networking modules) | 16 | 0 | 0 | 13.1 ms | (no codegen-affecting change; 36 stdlib modules total — closes the networking gap) | — |
| M28.5 (server-side TLS extension) | 16 | 0 | 0 | 13.1 ms | (single agent, 3 new NativeFns; closes the HTTPS-server gap) | — |
| M29 (webserver framework stress test) | 16 | 0 | 0 | 13.1 ms | (1,446-LOC user-code framework; **zero new bugs** in M28/M28.5 — first stress round in project history with zero finds) | — |
| M29.5 (framework Tier 1 round-out) | 16 | 0 | 0 | 13.1 ms | (+keep-alive, chunked TE, multipart, graceful shutdown, HTML errors; **found BUG-040** `socket.close_listener` doesn't unblock accept) | — |
| M30 (last two open bugs closed) | 16 | 0 | 0 | 13.1 ms | (BUG-028 lexer line continuation + BUG-040 socket.close_listener; **35 found / 35 fixed / 0 deferred** — v0.2-frozen-clean state) | — |
| **v0.2.0 tag** (2026-05-21) | 16 | 0 | 0 | 13.1 ms | First frozen release. 656 tests, 35 bugs all fixed, 36 stdlib modules, web framework working. Cargo workspace 0.1.0 → 0.2.0; spec banner updated; RELEASE_NOTES_v0.2.md shipped. | — |
| M31 (generic classes — first v0.3 feature) | 16 | 0 | 0 | 13.1 ms | `class Box[T]:` / `Pair[K,V]:` / `Stack[T]:` via extension of M17 worklist. Per-instantiation type_id + method bodies. +8 tests. Unblocks v0.3 stdlib classes. | — |
| M32 (async I/O — Shape A thread-backed Future façade) | 16 | 0 | 0 | 13.1 ms | New `asyncio` stdlib module (IDs 700-714) + async-socket variants (720-722). Future[T] as TypeCtor (not M31 generic class — agent's call). +9 tests. v0.4 will swap internals for real mio event loop. | — |
| M33 (precise GC stack maps — shadow-stack fallback) | 16 | 0 | 0 | 13.1 ms | Replaces M9 `in_jit` pause with shadow-stack root enumeration. JIT spills registers + pushes window before each heap-allocating helper. **Zero cherry-pick conflicts** with M32 despite both touching interp.rs + lib.rs — first parallel-v0.3-agent round, cleanest parallel integration in project history. +4 tests. v0.4 swaps to full Cranelift safepoints. | — |
| M34 (typed JsonValue tree — first stdlib classes) | 16 | 0 | 0 | 13.1 ms | sealed `JsonValue` + 6 subclasses (JNull/JBool/JInt/JFloat/JString/JList/JObject) registered in prelude (scope-down per brief). `json.parse(s) -> JsonValue` + `stringify` + constructor helpers. Closes the #1 M29 ergonomics gap (~50 LOC → ~10 LOC). +13 tests. Lesson 1 streak: 14. | — |
| M35 (four more stdlib classes — 3 parallel agents) | 16 | 0 | 0 | 13.1 ms | `re.Pattern` (P4-A, NativeFn IDs 790-799), `sqlite3.Connection` + `Cursor` (P4-B, IDs 800-819), `hashlib.Hasher` streaming (P4-C, IDs 820-829). All four prelude-registered, extending M34's pattern. 11 stdlib classes now in the prelude → `StdlibItemKind::Class` refactor becomes urgent before M40. +33 tests, +3 demo programs. Lesson 1 streak: 17 consecutive clean agents. | — |
| M36 (`StdlibItemKind::Class` refactor) | 16 | 0 | 0 | 13.1 ms | Single agent closes the M34/M35 scope-down debt. New `Class { class_id }` variant on `StdlibItemKind`; all 11 stdlib classes (JsonValue + Pattern + Connection + Cursor + Hasher) now published through their home modules. Honest scope-down: prelude bindings RETAINED for back-compat — M34/M35 tests reach class names by bare lookup, so hard removal would have regressed 39 tests. Phase D annotated the legacy "prelude wins" branch for future deletion. Tests unchanged at 723 / 0 / 1 (pure refactor). Lesson 1 streak: 18. | — |
| M37 (`tabular` stdlib — first Pandas-shaped package) | 16 | 0 | 0 | 13.1 ms | First v0.3 stdlib package shipped via the post-M36 canonical class-registration path (no prelude additions). 6 new classes: sealed `Column` + 5 final subclasses (ColumnI64/F64/Str/Bool/DateTime) + `DataFrame`. Per-column null mask NA semantics. Phases A-E: core types + I/O (read_csv/write_csv/from_sql) + comparisons → ColumnBool masks + df.filter/select/drop/head/tail + stable sort_by. STOP CRITERIA cut Phase C between/ne/ge/le/starts_with — saved 10 NativeFn slots; M38 picks up. NativeFn IDs 830-877. +21 tests, +1 demo (130 LOC). Largest single-agent milestone to date (~2800 LOC). Lesson 1 streak: 19. | — |
| M38 (`tabular` round-out — aggregations + group-by) | 16 | 0 | 0 | 13.1 ms | Picks up M37 debt + Phase 3 of Pandas plan. **Zero STOP CRITERIA cuts**. Phase A: typed `get_column_i64/...` accessors + restored cmp ops. Phase B: sum/mean/min/max/count/std/var/median (numeric); min/max/count (str+datetime). Phase C: `df.describe()` + `Column.fill_null` + `tabular.from_dict`. Phase D: new `GroupedDataFrame` class (M36 canonical path) + `df.group_by(cols)` + size/keys/sum/mean/min/max/count + custom `agg(specs)` — hash-based with `\x01`-joined multi-column keys. Phase E: +25 tests + 1 demo. NativeFn IDs 880-934. ~2530 LOC across 10 files. Four findings: Dict has no insertion order (`from_dict` lex-sorts); NaN propagates on f64 sums (not nansum semantics); null-keyed group bucketing; Edit-tool worktree leak recurred (orchestrator workaround documented). Lesson 1 streak: 20. | — |

\* M7-unfair: Python timing included parse+compile time. Methodology bug
caught and fixed at M10-prep; the M7-fair snapshot is the honest baseline.

## Bug discovery vs. fix rate

| Source of bug | Count | Notes |
|---|---:|---|
| Caught by tests during milestone | 11 | The expected case |
| Surfaced by NEXT milestone running broken code | 6 | M4 caught 3 M3 bugs; M6 caught 3 M3.5 bugs |
| Caught by stress test (CSV agg / 6 real-world programs) | 11 | The "real world surfaces bugs" hypothesis confirming itself |
| Caught by post-bug audit (M10 AB nullable audit) | 4 | One bug found → four siblings |
| Caught by formal spec review | 0 | Spec didn't drive bug discovery; running programs did |

Total: ~32 bug observations, ~24 distinct underlying defects (some bugs had
multiple symptoms reported separately).

## Test growth pattern

Linear-ish from M1 to M7, then nearly flat M7–M9, then a +39 jump at M10:

```
M1   55  ████████████
M2   74  ████████████████
M3   89  ███████████████████
M4   95  ████████████████████
M3.5 107 ███████████████████████
M5   121 ██████████████████████████
M6   132 ████████████████████████████
M7   134 ████████████████████████████▌
M8   134 ████████████████████████████▌
M9   134 ████████████████████████████▌
M10  173 █████████████████████████████████████
```

The M7–M9 plateau reflects that those milestones were heavy *runtime* work
(dispatch, JIT, GC) where new functionality came with regression tests that
exercised existing code paths rather than adding fresh test categories.

The M10 jump is from real-world programs (each example gets a `*_runs.rs`
integration test) plus the nullable-audit regression suite (8 tests for the
silent miscompiles AB found) plus the real_world_fixes.rs suite (8 tests
for the bugs C2/C3 surfaced).
