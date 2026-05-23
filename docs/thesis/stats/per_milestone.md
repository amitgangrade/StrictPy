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
| **M39** | **794** | **22,219** | **21,967** | **13,977** | 0 | 0 | 0 | 16/0/0 | 13.1 |
| **M40** | **822** | **22,333** | **22,906** | **14,147** | 0 | 0 | 0 | 16/0/0 | 13.1 |
| **M41** | **847** | **22,417** | **23,939** | **14,327** | 0 | 0 | 0 | 16/0/0 | 13.1 |
| **M42** | **868** | **22,417** | **24,222** | **14,537** | 0 | 0 | 0 | 16/0/0 | 13.1 |
| **M43** | **888** | **22,417** | **24,517** | **14,727** | 0 | 0 | 0 | 16/0/0 | 13.1 |
| **M44** | **915** | **22,467** | **25,139** | **14,892** | 0 | 0 | 0 | 16/0/0 | 13.1 |
| **M45** | **934** | **22,467** | **25,426** | **15,091** | 0 | 0 | 0 | 16/0/0 | 13.1 |
| **M46** | **961** | **22,524** | **26,434** | **15,251** | 0 | 0 | 0 | 16/0/0 | 13.1 |
| **M47** | **993** | **22,674** | **26,971** | **15,415** | 0 | 0 | 0 | 16/0/0 | 13.1 |

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
| M39 (`tabular` Phase 4 — reshape ops) | 16 | 0 | 0 | 13.1 ms | After M37+M38 shipped types+IO+filter+sort+agg+group-by, M39 closes the common-80% pandas surface. **Zero STOP CRITERIA cuts**. Phase A: 5 typed `unique_*` per dtype + `value_counts` + `concat_rows` + `concat_cols`. Phase B: `df.merge(other, on, how)` — hash-join inner/left/right/outer reusing M38's `\x01`-joined keys; null cells in `on` never match (pandas `null != null`); merged `on` cols inherit rhs values on right-only outer rows. Phase C: `df.pivot(index, columns, values)` (long→wide; raises on duplicate keys) + `df.melt(id_vars, value_vars)` (wide→long; value_vars must share dtype). Phase D: +25 tests + 1 demo + LANGUAGE_GUIDE §11.20/§11.21. NativeFn IDs 935-984 (11 used). ~2430 LOC across 9 files. Five findings: f64 `unique` keys on `to_bits()` (HashSet<f64> doesn't compile); `m39_join_key` returns None for any-null-cell rows; merged `on` cols inherit rhs on right-only outer; melt machinery bulky per-dtype (pre-read value_vars to avoid virtual-call-per-cell); Edit-tool worktree leak recurred ~5× — confirmed-recurring across M37+M38+M39. Lesson 1 streak: 21. | — |
| M40 (`tabular` Phase 5 — time series + cumulative + null + iloc) | 16 | 0 | 0 | 13.1 ms | **Zero STOP CRITERIA cuts**. DatetimeIndex deferred to M41 (would require adding `index: Column` + index-aware variants of every op). Phase A: 8 cumulative ops (`cumsum`/`cumprod`/`cummax`/`cummin` × i64+f64) with "propagate from first null forward"; `dropna`/`dropna_subset`; 5 per-dtype `fillna_*`; `iloc` half-open range. Phase B: 10 rolling-window ops (`rolling_sum/mean/min/max/std` × i64+f64); leading 0..window-1 nulls; sample n-1 std. Phase C: `df.resample(time_col, rule, agg)` with `<i64><m|h|d>` rule parser; `df.asof_merge` via `Vec::partition_point` after stable-sort. Phase D: +28 tests + 1 demo + LANGUAGE_GUIDE §11.22-§11.25. NativeFn IDs 985-1012 (28 used). ~2175 LOC. Six findings — most consequential: **Edit-tool worktree leak narrowed in M40** to Edit-on-existing-files (Write with absolute paths is unaffected); first cause-narrowing across 4 milestones of observation. Lesson 1 streak: 22 — four consecutive tabular-package single-agent milestones (M37/M38/M39/M40) shipped clean. **Methodology note**: first M40 launch died on a transient 529 in ~3.5 minutes before any tool calls — zero state created; clean retry shipped successfully. | — |
| M41 (`tabular` Phase 5b — DatetimeIndex (minimum viable) + pivot_table) | 16 | 0 | 0 | 13.1 ms | **Zero STOP CRITERIA cuts**. Closes the DatetimeIndex deferral from M40 with the minimum viable shape. Phase A: DataFrame payload **24 → 40 bytes** for optional `index: Column?` + `index_name: str?` (3 constructors updated); 6 methods (set_index/reset_index/has_index/index/index_name/sort_index). Phase B: `resample_index(rule, agg)` + `asof_merge_index(other)` + 3 per-dtype `select_by_label_*`. Phase C: `pivot_table(index_col, columns_col, values_col, aggfunc)` — pivot+group_by+agg in one call. Phase D: +25 tests + 1 demo + LANGUAGE_GUIDE §11.26-§11.28. NativeFn IDs 1015-1026 (12 used). ~2193 LOC. **EXPLICIT scope-down (M42 anchor)**: every existing DataFrame method that returns a fresh frame DROPS the index in v1 — only 4 explicitly-index-aware methods preserve it. **Methodology nuance**: 2 commits (A+B+C combined at ~75% of budget + D) rather than 4 per-phase — phases share cross-cutting infrastructure (40-byte payload + m41_build_df_with_index) so splitting would have been revert-and-reapply. Lesson 1 SPIRIT held; both commits clean; streak (23) holds with first explicit per-phase-cadence nuance since M28 escalation. Edit-tool leak recurred once; cp-recovered in ~30 seconds. | — |
| M42 (`tabular` index propagation through existing methods) | 16 | 0 | 0 | 13.1 ms | **Zero STOP CRITERIA cuts**. Closes the M41 v1 scope-down. The 11 existing DataFrame methods that returned a fresh frame now PROPAGATE the index instead of dropping it. **Pattern**: single helper `m42_permute_index_into_df` (+ sibling `m42_copy_index_into_df`) applied at the emit site of each affected handler — 280 LOC added to `builtins.rs` (4 helpers + 11 emit-call swaps). **No new NativeFn IDs** — modifies existing handlers. Phase A: filter/sort_by/head/tail/iloc + helper + 6 tests + 1 flipped M41 test. Phase B: select/drop/rename + sibling helper + 3 tests. Phase C: dropna/dropna_subset/fillna_* + 5 tests. Phase D: merge — index policy per `how` (lhs wins inner/left/outer; rhs wins right; outer-with-dtype-mismatch → RangeIndex fallback) + 5 tests. Phase E: +21 tests net + 1 demo + LANGUAGE_GUIDE §11.26 rewrite. ~1693 LOC. **M41/M42 cadence contrast confirms the streak nuance**: M41 slipped because shared infra → splitting becomes revert-and-reapply; M42 returned to 5 separable per-phase commits because phases modified disjoint handlers. **First test-flip in project history**: filter_drops_index → filter_preserves_index_m42 (assertion flipped). Edit-tool leak recurred 5× across phases; all `cp`-recovered in seconds. Lesson 1 streak: 24. | — |
| M43 (`tabular` reshape index propagation — closes v1 single-index story) | 16 | 0 | 0 | 13.1 ms | **Zero STOP CRITERIA cuts**. Closes the v1 single-index propagation by extending pivot_table / single-column group_by / pivot / melt / concat_rows / concat_cols. Multi-column group_by retains today's shape (M44 with MultiIndex). 4 separable per-phase commits, ~1715 LOC. Phase A: pivot_table promotes `index_col`; single-column group_by promotes the key column via single read of `group_keys.length()` at handler top. Phase B: pivot promotes index; concat_rows concatenates indexes when dtype + name match (else RangeIndex fallback); concat_cols lhs wins. Phase C: melt repeats input index per value_var. Phase D: +20 net tests + 1 demo + LANGUAGE_GUIDE rewrite. **Test-flip cascade was 9** (vs brief's 2-4 estimate): M41 (1), M39 (2), **M38 (6 — all group_by tests** because group_by was M38's headline feature), 3 demo updates. Lesson: contract changes cascade with how widely the old contract was tested; grep existing tests to estimate. **Edit-tool leak broader than M40 narrowing**: M43 saw Write also leak (M40-M42 thought Edit-only); ~15 cp recoveries in ~90 seconds. Updated workaround: precautionary cp at session start. NativeFn IDs unchanged. Lesson 1 streak: 25 — **seven consecutive `tabular` package agents** (M37-M43). | — |
| M44 (`tabular` MultiIndex — M44a: storage + multi-col group_by promotion + minimal propagation) | 16 | 0 | 0 | 13.1 ms | **Zero STOP CRITERIA cuts**. The headline architectural lift after M43. DataFrame payload bumped 40 → 56 bytes for optional `index_levels` + `index_names`. Phase A: 6 new methods (NativeFns 1027-1032) — set_index_multi / reset_index_multi / index_nlevels / index_level / index_level_name / sort_index_multi. Phase B: all 8 group_by aggregations promote multi-col keys to MultiIndex on the result (single-col stays M41 path). Phase C: minimal MultiIndex propagation through filter / head / tail / iloc via new auto-dispatching helper. All other ops drop a MultiIndex back to RangeIndex (M44b anchor). Phase D: +27 net tests + 1 demo + LANGUAGE_GUIDE §11.32. ~1500-2000 LOC. **Two methodology wins**: (1) **precautionary `cp` workaround eliminated the Edit-tool leak entirely** — zero recoveries mid-session vs M43's ~15; orchestrator-side integration also clean (main completely clean post-agent — cleanest tabular integration in the series); (2) **shared-infra cadence exception worked cleanly when classified explicitly** — brief predicted 30-50% first-commit window matching M41 precedent; agent landed at ~35%. Tests flipped (1): M43 contract test flipped from keys-as-cols to MultiIndex. Lesson 1 streak: 26 — **eight consecutive `tabular` package agents** (M37-M44). | — |
| M45 (`tabular` full MultiIndex propagation through M42 + M43 ops — closes v1 propagation story) | 16 | 0 | 0 | 13.1 ms | **Zero STOP CRITERIA cuts**. Lifts M44's MultiIndex-drops scope-down across 14 handlers. Single agent, 3 separable per-phase commits (disjoint-handler — first commit at ~20% of budget), ~1300 LOC. **Pattern**: route emit calls through M44's auto-dispatching helper `m44_permute_multiindex_into_df` + new sibling `m45_copy_multiindex_into_df` for column-list ops. Phase A: 8 M42 ops (sort_by/dropna/dropna_subset/fillna_*/merge/select/drop/rename) propagate MultiIndex; merge per-`how` policy extends via new `m45_merge_build_multiindex` (inner/left/right preserve; outer dtype-mismatch still RangeIndex — M46 anchor). Phase B: 5 M43 ops — melt repeats each level per value_var; concat_rows uses new `m45_concat_rows_multiindex` with 3-tier fallback; concat_cols lhs MultiIndex wins; pivot + pivot_table explicitly drop MultiIndex (reshape row dim — no clean target). Phase C: +19 net tests + 1 demo + LANGUAGE_GUIDE §11.26+§11.32 rewrites. **Tests flipped (2 — predicted)**: sort_by_drops + select_drops M44b-anchor tests → preserves. **Methodology data point**: agent couldn't run precautionary cp (Bash + PowerShell denied at session start) yet **zero leak recurrences** — refined hypothesis: leak triggers on worktree divergence at Edit-session start, not on first-Edit-on-existing-file. After M45 tabular is fully index-aware for both single-col AND MultiIndex end-to-end. Lesson 1 streak: 27 — **nine consecutive `tabular` package agents** (M37-M45). | — |
| M46 (`tabular` stack/unstack + df.loc range + outer-merge MultiIndex + time-series MI + extensions) | 16 | 0 | 0 | 13.1 ms | **Zero STOP CRITERIA cuts**. Cleanup-and-polish milestone after M45. Single agent, 5 separable per-phase commits (disjoint-handler — first commit at ~20%), ~2358 LOC. **After M46 the `tabular` v1 surface is functionally complete except for v0.4 polish.** Phase A: stack (all cols → new innermost MultiIndex level + value col; must-share-dtype) + unstack (innermost MultiIndex level → wide cols; requires MultiIndex). Phase B: 5 per-dtype `df.loc_range_*` (extends M41 select_by_label_* one-row → range). Phase C: outer-merge dtype-mismatch → NaN-padded 2-level MultiIndex (replaces M42 RangeIndex fallback); set_index_list ergonomic unification; pivot_table_aggfunc_list + pivot_table_margins. Phase D: resample/resample_index drop MultiIndex; asof_merge/asof_merge_index preserve lhs MultiIndex. Phase E: +27 tests + 1 demo + LANGUAGE_GUIDE §11.32 rewrite + §11.33/§11.34 new. NativeFn IDs 1033-1042 (10 new). **Methodology data point — M45 hypothesis REFUTED**: M46 saw the leak return under the same conditions as M45 (cp not run, main sync'd post-prior-push). Honest current state: cause unknown, leak is intermittent. Workaround stays well-routinized. The M45/M46 cycle is itself a methodology data point — don't extrapolate from one milestone when the phenomenon is intermittent. Tests flipped (0). Lesson 1 streak: 28 — **ten consecutive `tabular` package agents** (M37-M46). | — |
| M47 (`tabular` v0.4 polish: iloc 2-D + negative iloc + rolling Welford/min_periods + ColumnCategorical) | 16 | 0 | 0 | 13.1 ms | **Zero STOP CRITERIA cuts**. The v0.4 polish round after M46 closed the v1 surface. **First new Column subclass since M37** (`ColumnCategorical` — 32-byte payload, codes + categories + nulls + length). Single agent, 2 commits (Phase A+B+C combined at ~70% of budget + Phase D), ~2309 LOC. Phase A: iloc_2d + negative iloc (lifts M40 v1 rejection). Phase B: 10 rolling_*_min_periods (sum/mean/min/max/std × i64+f64) + Welford std internal (numerical-stability upgrade via m47_welford_std_sample helper). Phase C: ColumnCategorical via to_strings() coercion in v1 (optimized codes paths deferred to M48). Phase D: +32 tests + 1 demo + LANGUAGE_GUIDE §11.35/§11.36 new. NativeFn IDs 1043-1060 (18 new). **BIG methodology lesson — new cadence classification "cross-dispatch"**: M47's brief miscategorized the milestone as disjoint-handler (predicting first commit at ~20%); agent landed at ~70% because adding a new sealed-class subclass requires every dispatch file (resolver/ir/native/builtins) to compile together. Three cadence classifications now (M41-M47 septet): disjoint-handler (~20%), shared-infra (~30-50%), **cross-dispatch (NEW: ~50-75%, new sealed-class subclass)**. Streak holds at 29 — agent committed cleanly without orchestrator intervention; the cadence slip was a brief miscategorization. Tests flipped (1): iloc_negative_start_raises → iloc_negative_start_works_m47. **Edit-tool leak**: did NOT recur this session (M45/M46/M47 alternation: no leak / leak / no leak confirms intermittence). Stdlib classes 18 → 19. Lesson 1 streak: 29 — **eleven consecutive `tabular` package agents** (M37-M47). | — |

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
