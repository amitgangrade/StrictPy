# Session handoff — 2026-05-22 (post-M44)

## Read this FIRST in the next session

Everything you need to resume is in:

1. **This file** — current state + pending work + integration recipes
2. **`docs/thesis/timeline.md`** — milestone-by-milestone narrative through M35
3. **`docs/thesis/stats/per_milestone.csv`** — quantitative ground truth
4. **`THESIS.md`** + **`BLOG_POST.md`** — synthesis documents (frozen at M34;
   needs an M35 refresh pass — see "What comes after M35" below)
5. **`RELEASE_NOTES_v0.2.md`** — v0.2.0 freeze-point summary
6. **`LANGUAGE_GUIDE.md`** — single source of truth for AI tools writing
   StrictPy programs (refreshed post-M35)
7. **Memory file**: `C:\Users\AG\.claude\projects\C--Users-AG-CascadeProjects-PythonCompiler\memory\project_strictpy.md`

## Current head

- Branch: `main`
- Latest commit: `c2a0960` (M44 D: tabular MultiIndex — demo + LANGUAGE_GUIDE update + agent report)
- Tag: `v0.2.0` (commit `121483f`, pushed)
- Tests passing on main: **915** (+27 net over M43 — 25 new vm + 2 new demo; 1 M43 test flipped)

## Status snapshot

| Metric | Value |
|---|---:|
| Milestones complete on main | M0–M44 |
| **v0.2.0 release** | **Tagged at M30 (commit 121483f)** |
| Tests | 915 / 0 fail / 1 ignored |
| Bugs | 35 / 35 / **0 deferred** |
| Stdlib modules | 38 |
| Stdlib classes | 18 (unchanged — M44 adds 6 methods + DataFrame field expansion, no new classes) |
| Example programs | **108** (+1 in M44: `tabular_multiindex_demo.spy`) |
| Lesson 1 streak | **26 consecutive clean-commit agents** (M28 → M44) |

## M44 — completed (single agent, 4 per-phase commits, no STOP CRITERIA cuts)

| Agent | Scope | Var prefix | NativeFn IDs | Commits |
|---|---|---|---|---|
| **M44 MultiIndex** | Storage + multi-col group_by promotion + minimal propagation (filter/head/tail/iloc) | `m44_` | 1027-1032 (6 new) | `cb6c990` (A), `adccf24` (B), `7a66271` (C), `c2a0960` (D) |

### What shipped

- **Phase A**: DataFrame payload bumped 40 → 56 bytes for optional `index_levels: List[Column]?` + `index_names: List[str]?` (mutually exclusive with M41's single-col index). New helper `m44_build_df_with_multiindex`. 6 new methods (NativeFns 1027-1032): `set_index_multi(cols)`, `reset_index_multi()`, `index_nlevels()`, `index_level(i)`, `index_level_name(i)`, `sort_index_multi(ascending)`. 11 tests.
- **Phase B**: all 8 group_by aggregation methods (`size`/`keys`/`sum`/`mean`/`min`/`max`/`count`/`agg`) now dispatch on key count. Single-col → M41 path (today's behavior); multi-col (≥ 2 keys) → new M44 MultiIndex path with all keys promoted to index levels. 7 tests + 1 M43 contract test flipped.
- **Phase C**: new helper `m44_permute_multiindex_into_df` auto-dispatches on the parent's index state (no index → RangeIndex result; single-col → M42 single-col permute; MultiIndex → permute each level). Wired into `filter` / `head` / `tail` / `iloc`. All OTHER ops still drop a MultiIndex back to RangeIndex (M44b anchor). 7 tests.
- **Phase D**: demo (`examples/tabular_multiindex_demo.spy` ~165 LOC), LANGUAGE_GUIDE.md banner + §5 M44 subsection + §11.26 rewrite + new §11.32 (MultiIndex propagation v1 scope-down).

### The big methodology win

**The precautionary `cp` workaround eliminated the Edit-tool worktree leak entirely.** Zero recoveries mid-session vs M43's ~15 (which burned ~90 seconds). The agent ran one `cp` block at session start syncing `vm/src/builtins.rs`, `compiler/src/resolver.rs`, `compiler/src/ir.rs`, `shared/src/native.rs`, `LANGUAGE_GUIDE.md` from project root to worktree — and the leak never showed up again. **This is the mitigation pattern now**: defensive copy at start, skip the per-phase discovery loops entirely.

Combined with the **clean fast-forward integration on the orchestrator side** (main was completely clean post-agent — no leaked files), M44 was the cleanest tabular-package integration since the series began.

### Tests flipped (1 total)

- `vm/tests/m43_tabular_index_reshape.rs::multi_col_group_by_does_not_promote_to_index` → `multi_col_group_by_promotes_to_multiindex_m44`. Old: `ncols=3, has=false` (keys retained as columns). New: `ncols=1, nlev=2` (keys promoted to 2-level MultiIndex).
- **Zero M38 tests flipped** — M38's `group_by_multi_column` only checks group count, not column shape.

### EXPLICIT v1 scope-down (M44b anchor)

**MultiIndex propagation in M44a is limited to filter / head / tail / iloc.** Every other op drops a MultiIndex back to RangeIndex:
- M42 ops: `sort_by`, `dropna`, `dropna_subset`, `fillna_*`, `merge`, `select`, `drop`, `rename`
- M43 ops: `pivot`, `melt`, `concat_rows`, `concat_cols`, `pivot_table`
- M41 ops: `sort_index`, `resample_index`, `asof_merge_index`, `select_by_label_*` (single-col only)

M44b's job: lift this. Plus stack/unstack, `df.loc[label_list]` range-by-label, and outer-merge MultiIndex fallback.

## M43 — completed (single agent, 4 per-phase commits, no STOP CRITERIA cuts)

| Agent | Scope | Var prefix | NativeFn IDs | Commits |
|---|---|---|---|---|
| **M43 reshape index propagation** | Extend reshape + group_by + pivot_table to index-promote | `m43_` | none new | `f4b4249` (A), `61fd3d5` (B), `13e64fe` (C), `fdd2e35` (D) |

### What shipped

Closes the v1 single-index propagation story. After M43 the `tabular` package is **fully index-aware end-to-end for single-column indexes** (multi-column / MultiIndex still M44+).

- **Phase A**: `pivot_table` promotes `index_col` to the result's index; **single-column** `group_by([col])` with `sum/mean/min/max/count/agg/size/keys` promotes the key column to the index. **Multi-column** `group_by` retains today's keys-as-regular-columns shape (deferred to M44 MultiIndex).
- **Phase B**: `pivot` promotes `index` to the output's index. `concat_rows` concatenates input indexes when all share dtype + name (else RangeIndex fallback). `concat_cols` takes lhs's index (consistent with M42's merge policy).
- **Phase C**: `melt` repeats the input index per `value_var` (preserves name + dtype). Matches pandas's default behavior for indexed melt.
- **Phase D**: 18 VM tests + 2 demo-runs + `examples/tabular_index_reshape_demo.spy` (~190 LOC) + LANGUAGE_GUIDE.md §5 + §11.26 + §11.28 + new §11.30 (melt index repetition) + §11.31 (concat_rows index reconciliation rules).

### Two methodology data points worth elevating

**1. Test flip cascade was larger than estimated (9 vs brief's 2-4).**

The brief estimated 2-4 M41/M42 tests to flip. Actual: **9 tests across M38/M39/M41 + 3 demo updates**:

| Source | Count | Reason |
|---|---:|---|
| M41 | 1 | `pivot_table_sum_happy_path` (ncols 3→2 + index checks) |
| M39 | 2 | `pivot_happy_path` + `pivot_missing_cell_is_null` (pivot promotes index) |
| M38 | 6 | `group_by_*` tests had keys-as-columns assertions; **single-column group_by promotion cascaded into all 6 group_by test cases** |
| Demos | 3 | `tabular_groupby_demo.spy`, `tabular_index_demo.spy`, `tabular_reshape_demo.spy` updated to use `sort_index` and adjust column counts |

**Generalizable lesson**: when a contract change is cross-cutting (every group_by now promotes its key), the test-flip count scales with how widely the old contract was tested. M38 had 6 group_by tests because group_by was M38's headline feature. **Next brief that changes a feature with broad existing test coverage should explicitly estimate the flip count from existing test files.**

**2. Edit-tool worktree leak is broader than the M40 narrowing claimed.**

M40 said: "Edit on already-existing files leaks; Write with absolute worktree paths doesn't." M43 found: **`Write` of new files ALSO leaked** at first-edit-per-file boundaries. Agent burned ~90 seconds across ~15 `cp` recoveries (vs ~5 seconds in M42, ~30 in M41, ~2 minutes in M40).

**M43 agent's recommendation, now adopted**: future briefs should suggest a **precautionary `cp` of all shared files at session start** rather than waiting for `git status` to surface the leak per phase. The defensive copy is cheap; the per-phase discovery loops are not.

## M42 — completed (single agent, 5 per-phase commits, no STOP CRITERIA cuts)

| Agent | Scope | Var prefix | NativeFn IDs | Commits |
|---|---|---|---|---|
| **M42 index propagation** | Extend 11 existing handlers to propagate the M41 index | `m42_` | none new (modifies existing handlers) | `e84160c` (A), `b02de3e` (B), `98d977a` (C), `5a73af2` (D), `cbcab82` (E) |

### What shipped

Closes the M41 explicit v1 scope-down. The 11 existing DataFrame methods that returned a fresh frame now PROPAGATE the index instead of dropping it.

- **Phase A** (filter, sort_by, head, tail, iloc): one new helper `m42_permute_index_into_df` + 5 handler edits + 6 tests + 1 flipped M41 test.
- **Phase B** (select, drop, rename): sibling helper `m42_copy_index_into_df` + 3 handler edits + 3 tests.
- **Phase C** (dropna, dropna_subset, fillna_*): 2 handler edits (fillna's per-dtype dispatch via the shared `m40_df_fillna` body) + 5 tests.
- **Phase D** (merge): `m42_merge_build_index` + `m42_merge_outer_index_column` with dtype-mismatch fallback to RangeIndex; index_name policy = lhs wins for inner/left/outer, rhs wins for right + 5 tests.
- **Phase E**: 19 VM tests + 2 demo-runs; `examples/tabular_index_propagation_demo.spy` (~210 LOC end-to-end pipeline); LANGUAGE_GUIDE.md §5 + §11.26 rewrite (the M41 v1 scope-down section now reads as "closed by M42"); banner bumped to post-M42.

### Key M42 finding — methodology streak nuance closed

M41 introduced the first per-phase-cadence slip in the streak (combined Phases A+B+C because they shared cross-cutting infrastructure). **M42 returned to clean per-phase commits** because its phases modify disjoint handlers — each Phase has a green build + targeted tests at commit time, no shared revert-and-reapply risk. This confirms the M41 nuance is a true *infrastructure-then-uses* exception, not a general drift in agent discipline.

### Architectural pattern worth recording

The whole M42 milestone is a single recipe applied 11 times:

```rust
// In each row-transforming handler:
let keep_indices: Vec<usize> = /* existing code that builds row selection */;
let permuted_columns: Vec<u64> = /* existing per-column permute by keep_indices */;
// NEW: one line, replacing the existing m37_build_df call.
m42_permute_index_into_df(interp, parent_df_ptr, names, permuted_columns, &keep_indices)
```

The helper reads the parent's optional index, permutes it by the same `keep_indices`, and emits via `m41_build_df_with_index` (or `m37_build_df` if there was no index — preserving today's behavior for unindexed inputs). 280 LOC added to `builtins.rs` total — 4 helpers + 11 emit-call swaps.

### M41 tests flipped (1 total)

- `vm/tests/m41_tabular_index.rs::filter_drops_index` → `filter_preserves_index_m42`. Old asserted `has=false` (drops index per M41 v1 scope-down); new asserts `has=true` (M42 propagates).

### Edit-tool worktree leak — 5 recurrences

Detected at every "first Edit on a shared file" boundary: `vm/src/builtins.rs` (4× across phases), `vm/tests/m41_tabular_index.rs` (1×), `LANGUAGE_GUIDE.md` (1×). Each recovered with one `cp`. Total ~5 seconds. `Write` calls all landed correctly. Pattern now well-routinized — M40 narrowing (Edit-on-existing-files leaks, Write-with-absolute-paths doesn't) holds across 6 milestones now.

## M41 — completed (single agent, 2 commits, no STOP CRITERIA cuts)

| Agent | Scope | Var prefix | NativeFn IDs | Commits |
|---|---|---|---|---|
| **M41 DatetimeIndex + pivot_table** | `tabular` Phase 5b — minimum viable index abstraction | `m41_` | 1015-1026 (12 used) | `eec3dc9` (A+B+C combined), `dad1b6d` (D) |

### What shipped

- **Phase A**: DataFrame payload grew **24 → 40 bytes** (added optional `index: Column?` + `index_name: str?`, both zero = RangeIndex default). Three existing constructors (`m37_from_columns` / `m37_from_rows` / `m37_build_df`) updated to allocate the larger payload. 6 new methods: `set_index(col)` / `reset_index()` / `has_index()` / `index()` / `index_name()` / `sort_index(ascending)`. NativeFns 1015-1020.
- **Phase B**: Index-aware time-series + per-dtype select. `resample_index(rule, agg)` mirrors M40's `resample` but reads bucket keys from the index (must be `ColumnDateTime`). `asof_merge_index(other)` mirrors `asof_merge` but joins on both frames' indexes. `select_by_label_{i64,str,datetime}(label) -> DataFrame?` returns a one-row frame (or `none` if absent). NativeFns 1021-1025.
- **Phase C**: `pivot_table(index_col, columns_col, values_col, aggfunc)` — pandas's most-loved DataFrame method, combining pivot + group-by + agg in one call. Aggfunc vocabulary: `sum/mean/min/max/count` (same as M38). Per-cell accumulator enum for the (dtype × agg) cross-product. NativeFn 1026.
- **Phase D**: 25 new tests (23 vm + 2 demo); `examples/tabular_index_demo.spy` (~180 LOC: trades → set_index → resample_index → sort_index → pivot_table → asof_merge_index → select_by_label_str → reset_index pipeline); LANGUAGE_GUIDE.md §5 M41 subsection + §11.26-§11.28 gotchas.

### EXPLICIT scope-down (M42 anchor)

**Every existing DataFrame method that returns a fresh frame DROPS the index in v1** — only the 4 explicitly-index-aware methods (`sort_index`, `resample_index`, `asof_merge_index`, `select_by_label_*`) preserve it. M42's job: index propagation through filter / sort_by / head / tail / iloc / dropna / fillna / merge / select / drop / rename. Per the agent's report, that's ~600-800 LOC concentrated in 6 existing handlers, each gaining: (a) read parent index + index_name, (b) permute the index by the same row-selection vector, (c) emit via `m41_build_df_with_index` instead of `m37_build_df`.

### Five findings worth knowing

1. **DataFrame payload bump to 40 bytes** — GC's Class scanner walks every 8-byte slot in payload; zero slots safely treated as "not pointers" (matches the M11 pointer-vs-i64 false-positive analysis, benign because mark-phase is additive). Three constructors updated.
2. **`sort_index` dispatch by index dtype** — single `m41_sort_index_perm(col, ascending)` helper reads class name and runs per-dtype comparator inline. Descending = ascending + `perm.reverse()` (preserves stability within non-null cells).
3. **`m41_clone_column` for the index slot** — `set_index` clones the column rather than aliasing, keeping the index physically independent. Cost: one extra column allocation per `set_index`; safe for v1 row counts.
4. **`pivot_table` accumulator as an enum** — single `Acc` enum carries variant-per-(dtype × agg) accumulators. Per-bucket update is a single `match` (vs. nested dispatch).
5. **Edit-tool worktree leak recurred once** (down from 5× in M39, 2× in M40). Confirms the M40 narrowing: `Edit` on already-existing files leaks; `Write` with absolute worktree paths is unaffected. Agent caught via `git status` check + recovered via one-shot `cp` of 4 shared files in ~30 seconds.

### Methodology nuance worth flagging

**M41 deviated from the per-phase-commit discipline**: Phases A+B+C landed as one combined commit at ~75% of budget (rather than the brief's 20% first-commit + per-phase target). Reason: all three phases share `m41_build_df_with_index` + the 40-byte payload change — splitting would have required revert-and-reapply with extra leak-recovery overhead. The Lesson 1 SPIRIT (commit before orchestrator intervenes, green build + tests passing at each commit) held — both M41 commits were clean. The streak counter (23) does not break, but the commit granularity slipped. **Generalizable lesson**: when phases share cross-cutting infrastructure (struct layout changes, new shared helpers), per-phase splitting becomes an antipattern. Future briefs for "cross-cutting infrastructure + downstream uses" rounds should accept "first commit after the infrastructure lands, even if late" as the right shape.

## M40 — completed (single agent, 4 phases, no STOP CRITERIA cuts)

| Agent | Scope | Var prefix | NativeFn IDs | Commits |
|---|---|---|---|---|
| **M40 time-series** | `tabular` Phase 5 — cumulative + null + iloc + rolling + resample + asof_merge | `m40_` | 985-1012 (28 used) | `1b5c523` (A), `a2e1699` (B), `066a50f` (C), `a9f9354` (D) |

**Note**: a previous launch attempt died on a transient 529 (API overloaded) within ~3.5 minutes. Zero state was created. The successful run is the second attempt.

### What shipped

- **Phase A**: cumulative ops on numeric columns (`ColumnI64`/`F64` × `cumsum`/`cumprod`/`cummax`/`cummin` = 8 NativeFns); whole-frame null handling (`df.dropna` / `df.dropna_subset(cols)` + per-dtype `df.fillna_{i64,f64,str,bool,datetime}` = 7 NativeFns); range slicing (`df.iloc(start, stop)` — half-open, no negative indices). Null-propagation rule on cumulative: once a null is hit, every output cell after it is null (simpler than pandas's `min_periods=1` skip).
- **Phase B**: rolling-window aggregations (`ColumnI64`/`F64` × `rolling_{sum,mean,min,max,std}` = 10 NativeFns). Output length = input length; cells 0..window-1 are null; window-with-any-input-null produces null output. `rolling_mean`/`rolling_std` return `ColumnF64` regardless of input dtype. Sample n-1 std.
- **Phase C**: `df.resample(time_col, rule, agg)` — buckets a `ColumnDateTime` by rule width (`<i64><m|h|d>` parser), aggregates per-bucket via `sum`/`mean`/`min`/`max`/`count`. Empty buckets emit non-null bucket-start times but null aggregated cells. `df.asof_merge(other, on_self, on_other)` — left-join via `Vec::partition_point` after stable-sorting rhs. Both keys must share dtype (`ColumnDateTime` or `ColumnI64`).
- **Phase D**: 28 new tests (26 vm + 2 demo) + `examples/tabular_timeseries_demo.spy` (~170 LOC: fillna → cumsum → cummax → rolling_mean → resample → asof_merge → iloc → dropna pipeline). LANGUAGE_GUIDE.md §5 M40 subsection + §11.22-§11.25 gotchas.

### Six findings worth knowing

1. **Cumulative null-propagation choice**: "propagate from first null forward" is simpler than pandas's `min_periods=1`. Trivial user-side override: `col.fill_null(0).cumsum()`. Documented as §11.22.
2. **Resample rule parser** accepts only `<i64><m|h|d>` (e.g. `"15m"`, `"1d"`). Week/month/year require a calendar layer; M41 work if needed.
3. **`asof_merge` binary search** uses `Vec::partition_point(|k| *k <= needle)` which returns the first index past the run of `<=` matches — the largest matching index is `pp - 1`; `pp == 0` cleanly maps to "no match" (null right-side).
4. **`fillna_*` returns non-matching-dtype columns by raw pointer reuse** (not copies). Safe because no codepath mutates Column payloads in place.
5. **Resample drops string + bool columns** — no defined v1 aggregation. Could add `"first"` / `"last"` / `"mode"` later.
6. **Edit-tool worktree leak — key new finding**: the leak is specific to `Edit` calls on already-existing files; `Write` calls (with absolute worktree paths) land correctly. The agent recovered both leak instances in M40 with a one-shot `cp` from project root to worktree. ~2 minutes total burned. **Workaround for the M41 agent brief**: when bulk-editing existing shared files (`resolver.rs`, `ir.rs`, `native.rs`, `builtins.rs`), check `git status` after the first edit and `cp` if needed; `Write` calls for new files don't have this problem.

## M39 — completed (single agent, 4 phases, no STOP CRITERIA cuts)

| Agent | Scope | Var prefix | NativeFn IDs | Commits |
|---|---|---|---|---|
| **M39 reshape** | `tabular` Phase 4 — reshape ops | `m39_` | 935-984 (11 used: 935-942, 945, 950-951) | `5411a9f` (A), `e4f2ed7` (B), `24859c1` (C), `0d73905` (D) |

### What shipped

- **Phase A**: 5 typed `df.unique_*` accessors (i64/f64/str/bool/datetime — mirrors M38 `get_column_*` pattern); `df.value_counts(col)` returns 2-col DataFrame sorted by count desc; module-level `tabular.concat_rows(dfs)` (vertical, schema-strict) and `tabular.concat_cols(dfs)` (horizontal, row-count-strict + unique col names).
- **Phase B**: `df.merge(other, on, how)` — hash-join inner/left/right/outer reusing M38's `\x01`-joined key encoding. Output column order = lhs cols + rhs non-`on` cols (no duplicates). Null cells in `on` columns never match (pandas/SQL `null != null`). Merged `on` columns inherit rhs values on right-only outer rows (matches `pd.merge` behavior).
- **Phase C**: `df.pivot(index, columns, values)` — long→wide; raises ValueError on duplicate (index, columns) pairs; missing pairs → null cells. `df.melt(id_vars, value_vars)` — wide→long; all `value_vars` must share a dtype.
- **Phase D**: 23 VM tests + 2 demo-runs; `examples/tabular_reshape_demo.spy` (~150 LOC, orders+customers workflow); LANGUAGE_GUIDE.md §5 / §11.20 / §11.21 updates.

### Five findings worth knowing

1. **f64 `unique` keys on `to_bits()`** — `HashSet<f64>` doesn't compile (`f64: !Hash`); bit-pattern keying distinguishes ±0.0 and lets multiple NaN payloads be distinct. Canonical workaround.
2. **`m39_join_key` returns `None` for any-null-cell rows** — different from M38's `m38_row_key` which encoded nulls as `\x02null` for grouping. For merge's `null != null` semantics, `None` shortcut is cleaner than a never-matching key.
3. **Merge `on` columns inherit rhs values on right-only outer rows** — matches pandas's "merged key column" behavior so the join key never goes null in outer/right outputs.
4. **Melt machinery is bulky** — each dtype needs per-value-var read + per-output-row write. Pre-read all `value_vars` into Vec<>s up front to avoid virtual-call-per-cell overhead.
5. **Edit-tool worktree leak recurred ~5 times in M39** — same as M37+M38. The agent caught each via `git status` after substantial edits and `cp`-recovered from project root to worktree. **This is now a confirmed-recurring harness issue across 3 consecutive milestones**; orchestrator integration workaround (checkout-and-merge-ff) is reliable.

## M38 — completed (single big agent, 5 phases, no STOP CRITERIA cuts)

| Agent | Scope | Var prefix | NativeFn IDs | Commits |
|---|---|---|---|---|
| **M38 round-out** | `tabular` aggregations + group-by | `m38_` | 880-934 | `8e2c045` (A), `f95fa0c` (B), `294a6d7` (C), `604a912` (D), `ec9d9d0` (E) |

### What shipped

- **Phase A**: typed `df.get_column_i64 / f64 / str / bool / datetime` accessors (resolves the M37 sealed-class-return-type finding); restored Phase C ops — `between / ne / ge / le` on i64+f64, `starts_with / ends_with` on str, `df.rename`.
- **Phase B**: per-column aggregations — `sum / mean / min / max / count / std / var / median` on numeric columns (with sample n-1 std/var); `min / max / count` on str + datetime; `count` on bool. Null-skipping semantics throughout.
- **Phase C**: `df.describe() -> DataFrame` (count/mean/std/min/max/50% for numeric; count only for non-numeric); `Column.fill_null(v)` per subclass (5 methods); `tabular.from_dict(d: Dict[str, Column])` constructor.
- **Phase D**: new `GroupedDataFrame` class (registered via M36 `StdlibItemKind::Class`); `df.group_by(cols) -> GroupedDataFrame`; `gdf.size / keys / sum / mean / min / max / count` shortcuts; `gdf.agg(specs: List[Tuple[str, str]])` custom aggregator. Hash-based with `\x01`-joined multi-column keys.
- **Phase E**: 25 new tests (23 VM + 2 demo); `examples/tabular_groupby_demo.spy` (~110 LOC); LANGUAGE_GUIDE.md §5/§6.2/§11.18/§11.19 updates.

### Four findings worth knowing

1. **`Dict` has no insertion order** — M5's `Dict` is a `HashMap`. `tabular.from_dict` lex-sorts column names by key. Documented as LANGUAGE_GUIDE.md §11.19.
2. **NaN propagation on f64 aggregations** — matches `numpy.sum` (NaN propagates) NOT `numpy.nansum` (skips NaN). Nulls ARE skipped; NaN values are NOT. Documented as §11.18.
3. **Null-keyed group bucket** — rows with a null in any group-key column go into a synthesized null-group bucket (pandas's `dropna=False` mode).
4. **Edit-tool worktree leak (recurring)**: same as M37 — the agent's Edit tool writes leaked into the project-root copy mid-implementation. The agent recovered with a `cp -r` patch. **Orchestrator workaround**: when integrating, ALWAYS check `git status` on main first; if main has partial modifications, `git checkout --` them and `git merge --ff-only` the worktree branch. The worktree branch HEAD is authoritative.

## M37 — completed (single big agent, 5 phases, integrated as fast-forward)

| Agent | Scope | Var prefix | NativeFn IDs | Commits |
|---|---|---|---|---|
| **M37 tabular** | First Pandas-shaped stdlib package | `m37_` | 830-877 | `0f40eaf` (A), `c01e3f1` (B), `2c74e39` (C), `1978346` (D), `895da03` (E) |

### What shipped

- **Module**: `tabular` (named to avoid `import pandas` confusion — see LANGUAGE_GUIDE.md §11.11)
- **6 classes**: sealed `Column` + 5 final subclasses (`ColumnI64` / `F64` / `Str` / `Bool` / `DateTime`) + `DataFrame`. **First stdlib package using the post-M36 canonical class-registration path** — classes registered via `StdlibItemKind::Class` in `seed_stdlib_modules`, NOT in `seed_prelude`. Validates the M36 refactor end-to-end.
- **NA semantics**: per-column `nulls: List[bool]` parallel to `values: List[T]`. Uniform across dtypes; no NaN sentinel games.
- **Phase A (~400 LOC)**: Column/DataFrame allocation + construction helpers (`tabular.col_i64`, etc.) + inspection (shape/columns/dtypes/get_column) + `df.show(n)` ASCII table.
- **Phase B (~300 LOC)**: `read_csv` / `write_csv` / `from_sql` (reuses M35 Cursor!) / `from_rows`. Schema-driven parsing; empty cells → null.
- **Phase C (~400 LOC)**: per-column comparison ops (i64+f64: `eq`/`gt`/`lt`; str: `eq`/`contains`; bool: `eq`; datetime: `eq`/`gt`/`lt`) producing null-aware ColumnBool masks; combinators `and_` / `or_` / `not_` / `count_true`; `df.filter` / `select` / `drop` / `head` / `tail` / `row`.
- **Phase D (~150 LOC)**: stable `df.sort_by(col, ascending)` with nulls-go-to-end, per-Column-type comparator dispatch.
- **Phase E (~150 LOC)**: 19 VM tests + 2 compiler integration tests + `examples/tabular_demo.spy` + LANGUAGE_GUIDE.md updates + agent report.

### STOP CRITERIA invoked

Phase C cut `between`, `ne`, `ge`, `le`, `starts_with` — saved ~10 NativeFn slots. The kept set covers the common 80% filtering cases.

### Three findings worth knowing

1. **`(*hdr).vtable` not `.ty`**: ObjectHeader field name caught the agent in early Phase A; documented.
2. **No `get_column(name) -> Column?`**: sealed-class return type can't be cleanly chosen at NativeFn time. Demo works around by holding typed Column references from construction. **M38 follow-up**: add typed `get_column_i64` / `get_column_str` / etc.
3. **No bare-name fallback for tabular classes**: confirms the M36 refactor's promise. Users MUST write `from tabular import DataFrame`; `import tabular` + `tabular.DataFrame` works only as an annotation type. This is the post-M36 canonical behavior — M34/M35 classes still have the legacy bare-name fallback for back-compat.

## M36 — completed (single agent, integrated as fast-forward)

| Agent | Scope | Var prefix | Commits |
|---|---|---|---|
| **M36 refactor** | `StdlibItemKind::Class` infrastructure | `m36_` | `e72c9fb` (A+B+C+D), `91b581e` (E + report) |

### Design call (worth knowing)

The agent did NOT delete the prelude bindings for the 11 stdlib classes
— every M34/M35 integration test reaches the class names by bare lookup
after just `import json` / `import re` / `import sqlite3` / `import hashlib`
(no `from … import` form). Removing the prelude bindings would have
regressed 39 tests. **M36 is a metadata refactor**: the 11 classes are
NOW also published through their home stdlib modules as
`StdlibItemKind::Class { class_id }` items, but the legacy prelude
bindings remain for back-compat. The infrastructure is in place for v0.4
stdlib classes to register module-scoped from the start.

Phase D added an explicit "still load-bearing for these 11 classes"
comment on the legacy "prelude wins" branch. A future agent that flips
the M34/M35 tests to explicit `from json import JsonValue` forms can
then delete the branch in one go.

### Key takeaway for v0.4 stdlib growth

When you add a new stdlib class, the new path is now:

```rust
// in seed_stdlib_modules (or a per-module helper):
items.push(StdlibItem {
    name: "Foo".into(),
    kind: StdlibItemKind::Class { class_id: foo_cid },
    ty: Ty::Class(foo_cid),
    native_id: 0,  // unused for Class variant
});
```

Do NOT add to `seed_prelude`. Users will import via `from foo_mod import Foo`
or use `foo_mod.Foo` after `import foo_mod`.

## M35 — completed (3 parallel agents, all integrated)

| Agent | Class | NativeFn IDs | Var prefix | Commit |
|---|---|---|---|---|
| **P4-A** | `re.Pattern` (compiled regex) | 790-799 | `p4a_` | `dd80ce2` |
| **P4-B** | `sqlite3.Connection` + `Cursor` | 800-819 | `p4b_` | `ad1200c` |
| **P4-C** | `hashlib.Hasher` (streaming) | 820-829 | `p4c_` | `e2d69bd` |

All three used the **M34 prelude-registration pattern** (no
`StdlibItemKind::Class` infrastructure — classes go in
`compiler/src/resolver.rs::seed_prelude` alongside Channel/Thread/
JsonValue). Each shipped tests + a demo + a spec subsection in the
existing module's section.

**Integration shape that worked**: 3 worktree branches diffed against
the pre-M35 base (`475ab47`), applied additively with `git apply --3way`,
manual conflict resolution at adjacent prelude/match-arm sites
(matches the M27+ pattern). The distinctive `p4a_` / `p4b_` / `p4c_`
prefixes prevented the M27 alignment hazard cleanly.

**Three unpushed commits on local main** (P4-C, P4-B, P4-A) — the
M35 round did not push. `git push origin main` to publish.

## What comes after M35

Per the THESIS §8.4 next-pass priority list + M34/M35 deferred items:

### Highest leverage (in order)

1. **THESIS + BLOG_POST refresh to M44** (small writing task, ~30-45 min).
   Both are at post-M39 currently. Concrete deltas for M40-M44:
   - Tests: 794 → 915 (+28 M40, +25 M41, +21 M42, +20 M43, +27 M44)
   - Stdlib classes: unchanged at 18 (M40-M44 ship methods + two
     optional DataFrame field expansions; no new classes)
   - Examples: 103 → 108
   - Lesson 1 streak: 21 → 26
   - `tabular` coverage: common-80% (post-M39) → ~95% (post-M40
     adds cumulative/null/iloc/rolling/resample/asof) → DatetimeIndex
     opt-in (post-M41) → DatetimeIndex propagates through 11 row/col
     methods (post-M42) → fully index-aware for single-column
     indexes (post-M43) → **MultiIndex for multi-column group_by**
     with minimal propagation (post-M44).
   - **Methodology notes worth flagging in BLOG**: (a) the M41/M44
     shared-infra cadence exception (combined Phase A acceptable
     when phases share new infrastructure); (b) the M43 9-test-flip
     cascade lesson; (c) the **M44 precautionary-cp workaround
     eliminated the Edit-tool leak entirely** (zero recoveries vs
     M43's ~15) — the mitigation pattern is now defensive copy at
     session start.

2. **M44b — Full MultiIndex propagation + stack/unstack + loc range**
   (the natural M44a follow-up):
   - **Full MultiIndex propagation** through M42 ops (sort_by /
     dropna / dropna_subset / fillna_* / merge / select / drop /
     rename) and M43 reshape ops (pivot / melt / concat_rows /
     concat_cols / pivot_table). Currently M44a drops a MultiIndex
     for all of these. Pattern: extend `m44_permute_multiindex_into_df`
     auto-dispatch logic to every M42+M43 handler that today calls
     `m42_permute_index_into_df` / `m42_copy_index_into_df`.
   - **`stack` / `unstack`** — pandas's MultiIndex bread-and-butter.
     `stack` rotates columns into a MultiIndex level; `unstack`
     does the reverse. Significant new code (~400-600 LOC).
   - **`df.loc[label_list]`** / range-by-label — single-col is M41's
     `select_by_label_*`; range + multi-key needs new methods.
   - **Outer-merge MultiIndex fallback** — replace M42's RangeIndex
     fallback for dtype-mismatched indexes with a proper NaN-padded
     MultiIndex.
   - Estimated: ~1500-2000 LOC. Mostly extending the existing
     handlers like M42 did for single-col — the recipe pattern
     applies again. Plus stack/unstack as net-new code.

3. **M45+ — Rolling-window optimizations + categorical**:
   - Welford incremental sum-of-squares for rolling_std stability
   - `min_periods` argument
   - `center=True` window alignment
   - `1w` / `1M` / `1Y` resample rules (needs calendar layer)
   - `df.rolling(window).agg(...)` chainable rolling object
   - Categorical column type (memory-efficient group-by keys)
   - `df.iloc[rows, cols]` — 2-D indexing
   - Negative-index support for `iloc`
   - `pivot_table margins=True` + `aggfunc=list`

3. **M36 follow-up — flip M34/M35 tests to explicit imports + delete
   the legacy "prelude wins" branch.** Mechanical migration; ~39 test
   files. M37+M38+M39 all confirmed the canonical path works.

4. **Edit-tool worktree leak — workaround now bulletproof; harness
   investigation deprioritized.** Recurred M37-M43 (7 consecutive
   milestones), narrowed in M40 (Edit-on-existing-files; later M43
   showed Write of new files also affected). **M44 solved it
   operationally**: the agent ran a precautionary `cp` of all shared
   files (`vm/src/builtins.rs`, `compiler/src/resolver.rs`,
   `compiler/src/ir.rs`, `shared/src/native.rs`, `LANGUAGE_GUIDE.md`)
   from project root to worktree at session start. **Zero recoveries
   needed mid-session.** This is now the standard pattern in agent
   briefs going forward. The orchestrator side also saw zero leak in
   M44 — clean fast-forward integration with main completely clean
   post-agent. Harness root-cause investigation is no longer urgent
   (workaround is cheap and effective); deprioritize.

5. **Real Cranelift safepoints** (replaces M33 shadow stack):
   `cranelift-jit 0.115` doesn't stably expose PC ranges; check if
   a newer cranelift-jit (0.116+ or trunk) exposes
   `MachBufferFinalized::pc_range_for_inst` or similar. If yes,
   this is a focused agent. If not, the shadow-stack approach is
   fine for now.

4. **Real `mio` event loop** (replaces M32 thread façade): swap
   `asyncio.spawn`'s thread-per-task implementation for a single-
   threaded event loop with state-machine coroutines or
   thread-coordinated tasks. Public surface unchanged.

5. **Rewrite the M29 framework using JsonValue + Pattern +
   Connection + Hasher**: clean LOC measurement of how much v0.3
   stdlib classes shrink user code. The M29 framework was ~2,400 LOC;
   estimated ~1,500-1,700 LOC post-rewrite (30-35% reduction). One
   focused agent.

6. **Phase 3d stdlib**: `traceback`, `enum`, `functools`, `uuid`,
   `secrets`. Smaller modules; the M27 parallel-worktree pattern
   handles them cleanly. 4-5 parallel agents.

7. **Bounded generics + variance + explicit type-arg syntax**:
   extends M31. The `Box[i64]()` explicit-arg form would let
   `asyncio.spawn[T]` work generically.

8. **User-defined exception subclasses**: parser already accepts
   `class MyError(Exception):`; resolver currently rejects. Small fix.

9. **HTTP/2** + **WebSockets**: separate v0.4 stdlib modules.

### Lower priority

- More benchmarks (extended suite already has 30 cells; the M29
  framework throughput could be added as cells)
- Generic methods on non-generic classes (currently scoped-out per
  M17)
- Recursive generic classes (currently scoped-out per M31)
- M34/M35 scope-down cleanup (the helper-vs-constructor double-NativeFn-ID
  thing is mildly ugly; could unify via a constructor-flavour flag
  on `StdlibItemKind::Function`)

## CRITICAL: keep `LANGUAGE_GUIDE.md` up to date

`LANGUAGE_GUIDE.md` (project root, refreshed post-M35) is the
**single source of truth** for AI coding tools writing StrictPy
programs. Every agent brief that touches **language syntax**,
**type system**, or **stdlib** MUST include:

> Update `LANGUAGE_GUIDE.md` to document the new feature in the
> appropriate section. The doc is the single source of truth for
> AI coding tools; if it's out of date, AI tools generate wrong
> code. See §13 "Maintaining this file" at the bottom of the
> guide for the per-feature update pattern.

When integrating an agent's worktree, verify the guide was updated;
if not, write the update yourself before pushing. The doc is what
makes StrictPy usable by other AI tools — losing freshness here
costs more than the integration time saves.

After v0.4 language/stdlib work, update:
- Version banner at the top ("Last refresh: post-M..")
- The relevant §3 / §4 / §5 / §10 sub-section
- A §11 entry if there's a gotcha worth flagging
- §12 examples if the new feature deserves a worked demo

## Methodology lessons that have held

Document these in any new agent brief:

1. **"FIRST commit before 60% of your time budget"** with explicit
   20%/40%/60%/80% checkpoint discipline. **26 consecutive clean
   agents** (M28 → M44) — the streak is the strongest empirical
   data point in the project. M37-M40 each ran 4-5 phase commits
   across ~2100-2800 LOC milestones. M41 slipped to combined Phase
   A+B+C (cross-cutting infrastructure exception). M42 + M43 returned
   to clean per-phase commits (disjoint handlers). M44 was the second
   shared-infra exception (combined Phase A landing at ~35% of
   budget — explicitly classified in the brief). **Lesson confirmed
   across the M41/M42/M43/M44 quartet**: brief language should call
   out "shared-infra" vs "disjoint-handler" phases AND set the
   first-commit threshold accordingly (20% disjoint, 30-50%
   shared-infra). M44's brief made this explicit and the agent
   landed squarely in the predicted window.

2. **Test-flip cascade lesson (M43)**: when a contract change is
   cross-cutting (every single-column group_by now promotes its
   key), the test-flip count scales with how widely the old contract
   was tested. M43 flipped **9 tests** vs the brief's 2-4 estimate —
   M38's 6 group_by tests cascaded because group_by was M38's
   headline feature. **Next brief that changes a feature with broad
   existing test coverage should explicitly grep existing tests
   for old-contract assertions and estimate the flip count from
   that, not from intuition.**

2. **Distinctive variable prefixes per agent** in shared files
   (resolver.rs, builtins.rs, interp.rs) — `p3b_a_` / `p3b_b_` /
   `p3c_a_` / `p3c_b_` / `p4a_` / `p4b_` / `p4c_` / etc. Avoids the
   M27 closing-brace alignment hazard that bit two M27 + M28
   integrations. M35 reconfirmed this works.

3. **Always diff against the pre-round common ancestor** when
   cherry-picking sequentially. NEVER `git diff main..worktree` if
   another worktree has already landed on main — produces
   reverse-deletions. The M28 P3b-B integration disaster (1806
   lines deleted) is the cautionary tale. M35 followed this
   discipline (pre-M35 base `475ab47`) and integrated cleanly.

4. **Auto-resolve "keep-both" Python script** for git-apply conflicts
   that produce simple `<<<<<<<` markers around purely additive
   blocks. Works for ~80% of multi-agent integrations.

5. **Scope-down discretion**: agents who hit STOP CRITERIA and ship
   a smaller working version are the most useful. M33 (shadow-stack
   instead of full Cranelift safepoints), M34 (prelude registration
   instead of `StdlibItemKind::Class`), and M35 ×3 (inheriting M34's
   prelude path rather than building module-level class infra) are
   the exemplars — each shipped working features that v0.4 can
   extend.

## Honest open items to revisit

- **`m33_precise_gc::recursive_allocation_does_not_leak_or_crash`**
  — Windows stack overflow under specific recursive-allocation load.
  Pre-existing flake noted by both M33 + M34 agents. Not blocking;
  may indicate the shadow-stack approach has overhead that recursive
  StrictPy code hits at depth. Investigate during the
  Cranelift-safepoints v0.4 work.

- **The prelude is getting crowded**: M34 added 7 JsonValue classes,
  M35 added 4 more (Pattern + Connection + Cursor + Hasher). The
  prelude now hosts **17 stdlib classes** (6 base + 11 v0.3 stdlib).
  The `StdlibItemKind::Class` refactor is now urgent. Probably
  "before M40" rather than "before M50".

- **Async I/O perf delta**: M32 ships Shape A (thread-backed). The
  M29 framework's ~2× gap to Flask+gunicorn was supposed to be
  closed by async; Shape A doesn't close it (each spawned task is
  still an OS thread). The real perf win requires the v0.4 mio
  event loop. Worth measuring the gap explicitly with a "rewrite
  M29 framework using async" before/after benchmark.

## Useful one-liners

```bash
# Status summary
cd C:/Users/AG/CascadeProjects/PythonCompiler
git log --oneline -10
git status
git tag --list  # should show v0.2.0

# Quick smoke test (M35-specific)
cargo build --workspace --release && \
  cargo test --release -p strictpy-vm --test m35_re_pattern && \
  cargo test --release -p strictpy-vm --test m35_sqlite_class && \
  cargo test --release -p strictpy-vm --test m35_hashlib_streaming

# Full test sweep (~5 min on Windows; reports total at end)
cargo test --workspace --release --no-fail-fast 2>&1 | grep -E "^test result:" | \
  awk '{passed+=$4; failed+=$6; ignored+=$8} END {print "passed:",passed,"failed:",failed,"ignored:",ignored}'

# Pre-M35 base (kept for reference):
PRE_M35=475ab47

# List active worktrees:
git worktree list
```

## Memory file location

```
C:\Users\AG\.claude\projects\C--Users-AG-CascadeProjects-PythonCompiler\memory\project_strictpy.md
```

Update the "Status as of end of M..." block when v0.4 lands. The
file is ~155 lines; keep additions concise.
