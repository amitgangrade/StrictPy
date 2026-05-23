# Session handoff — 2026-05-23 (post-M47)

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
- Latest commit: `325fcba` (M47 D: tabular polish — demo + LANGUAGE_GUIDE update + agent report)
- Tag: `v0.2.0` (commit `121483f`, pushed)
- Tests passing on main: **993** (+32 net over M46 — 30 new vm + 2 new demo; 1 M40 test flipped)

## Status snapshot

| Metric | Value |
|---|---:|
| Milestones complete on main | M0–M47 |
| **v0.2.0 release** | **Tagged at M30 (commit 121483f)** |
| Tests | 993 / 0 fail / 1 ignored |
| Bugs | 35 / 35 / **0 deferred** |
| Stdlib modules | 38 |
| Stdlib classes | **19** (M47 added `ColumnCategorical` as a new sealed Column subclass) |
| Example programs | **111** (+1 in M47: `tabular_m47_polish_demo.spy`) |
| Lesson 1 streak | **29 consecutive clean-commit agents** (M28 → M47) |

## M47 — completed (single agent, 2 commits with first at ~70% of budget, no STOP CRITERIA cuts)

| Agent | Scope | Var prefix | NativeFn IDs | Commits |
|---|---|---|---|---|
| **M47 polish** | iloc 2-D + negative iloc + rolling Welford/min_periods + ColumnCategorical | `m47_` | 1043-1060 (18 new) | `ff010c9` (A+B+C combined), `325fcba` (D) |

### What shipped

- **Phase A**: `df.iloc_2d(row_start, row_stop, col_start, col_stop)` half-open 2-D slice with Python-style negatives on both axes; extends existing `df.iloc(start, stop)` to accept negative indices (lifting M40's v1 rejection).
- **Phase B**: 10 new `Column.rolling_*_min_periods(window, min_periods)` methods (sum/mean/min/max/std × i64+f64). Welford's online algorithm for std via new `m47_welford_std_sample` helper (Option 1 — recompute over window each step; bit-equivalent to M40 on small inputs). Original `rolling_std` unchanged for backwards compat.
- **Phase C**: new `ColumnCategorical` sealed Column subclass with `codes: List[i64]` + `categories: List[str]` + `nulls: List[bool]` + `length: i64` (32-byte payload, first 3 slots aligned with the M37 Column layout so the shared `length`/`is_null`/`null_count` handlers work unmodified). New methods: `tabular.col_categorical(values)`, `col_categorical_with_nulls`, `cc.codes()`, `cc.categories()`, `cc.to_strings()`, `cc.get(i)`, `df.get_column_categorical(name)`. v1 op integration via `to_strings()` coercion — optimized codes paths deferred to M48.
- **Phase D**: 32 new tests + `examples/tabular_m47_polish_demo.spy` (~155 LOC) + LANGUAGE_GUIDE.md §5 M47 subsection + §11.35 (negative iloc) + §11.36 (categorical alphabetical-sort v1) + agent report.

### Big methodology lesson — brief classification needs a new category

The brief classified M47 as **disjoint-handler** (per-phase commits at ~20%). **This was wrong**: adding a new sealed-class subclass (ColumnCategorical) means **every dispatch file has to grow together** before the build goes green. The agent's first commit landed at ~70% of budget — not because of agent error but because the **task itself** required combined commits of resolver.rs + ir.rs + native.rs + builtins.rs together.

This is a NEW classification beyond shared-infra:

- **"disjoint-handler"** (M42, M43, M45, M46): per-phase commits at ~20%. Each phase modifies independent handler bodies.
- **"shared-infra"** (M41, M44): combined Phase A at ~35%. Phases share a new helper or struct field that downstream phases use.
- **NEW: "cross-dispatch"** (M47): combined commit at ~50-75%. Adding a new sealed-class subclass requires every dispatch site to compile together — the build goes red until they all agree.

**Future brief language**: when adding a new sealed-class subclass (Column*, GroupedDataFrame-shape, etc.), classify the milestone as **cross-dispatch** and predict a 50-75% first-commit window. M48's brief should make this explicit if categorical optimized paths get a similar shape.

The streak holds at 29 because the agent committed cleanly without orchestrator intervention — the cadence slip was a brief miscategorization, not an agent error.

### Tests flipped (1)

`vm/tests/m40_tabular_timeseries.rs::iloc_negative_start_raises` → `iloc_negative_start_works_m47`. Old: asserted ValueError on `iloc(-1, 1)`. New: asserts `nrows=2` on `iloc(-2, 3)` (Python negative semantics).

### Edit-tool worktree leak

No recurrence this session. Precautionary `cp` block was blocked by Bash policy (same as M44/M46) but `wc -l` between worktree and project root at session start showed identical file sizes — the worktree had a clean baseline from M46's clean integration. Every Edit/Write landed correctly.

The M45/M46 hypothesis-refutation cycle plus M47's no-leak-from-clean-baseline tentatively suggest the leak might be related to **whether the worktree starts in sync** — but M46 refuted that. **Honest current state remains**: cause unknown, intermittent, workaround reliable.

## M46 — completed (single agent, 5 per-phase commits, no STOP CRITERIA cuts)

| Agent | Scope | Var prefix | NativeFn IDs | Commits |
|---|---|---|---|---|
| **M46 stack/unstack + extensions** | Pandas's MultiIndex bread-and-butter + ergonomic polish | `m46_` | 1033-1042 (10 new) | `e02f08a` (A), `73616c6` (B), `b958a2b` (C), `4878426` (D), `d8dcbef` (E) |

### What shipped

- **Phase A**: `df.stack()` (rotates all columns into a new innermost MultiIndex level + single value column; requires shared-dtype across columns) + `df.unstack()` (takes innermost MultiIndex level, turns into columns; raises on no-MultiIndex). NativeFns 1033-1034.
- **Phase B**: `df.loc_range_{i64,f64,str,bool,datetime}(start, stop)` — per-dtype inclusive range lookup (extends M41's one-row `select_by_label_*`). NativeFns 1035-1039.
- **Phase C**: Outer-merge dtype-mismatch now produces a NaN-padded 2-level MultiIndex (replaces M42's RangeIndex fallback — hook `m46_merge_outer_dtype_mismatch_multiindex` into existing `m39_df_merge`). `set_index_list(cols)` unifies set_index/set_index_multi via length dispatch (NativeFn 1040). `pivot_table_aggfunc_list` (1041) emits one value-column set per aggfunc; `pivot_table_margins` (1042) adds "All" row + column.
- **Phase D**: time-series ops MultiIndex handling — `resample` + `resample_index` explicitly drop MultiIndex (reshape row dim); `asof_merge` + `asof_merge_index` preserve lhs MultiIndex via M45's merge MultiIndex pattern. No new NativeFns.
- **Phase E**: 25 new VM tests + 2 demo-runs + `examples/tabular_m46_extensions_demo.spy` (~160 LOC) + LANGUAGE_GUIDE.md §5 M46 subsection + §11.32 rewrite + §11.33/§11.34 new (stack must-share-dtype, unstack must-have-MultiIndex).

### Methodology data point — Edit-tool leak hypothesis partially refuted

M45 proposed: "leak triggers when worktree state diverges from project root at session start." M44 (cp run, no leak) and M45 (cp NOT run because Bash denied, no leak) supported this.

**M46 refutes it.** The cp block was unavailable again (Bash denied for the loop form) — same as M45. But the leak **DID recur** this round, with edits landing in the project root instead of the worktree. Agent recovered via per-file `cp` recoveries.

So M45 was the lucky outlier, not the new normal. The hypothesis is wrong; the leak is genuinely intermittent or has triggers we haven't identified. **The workaround stays in briefs**: precautionary `cp` at session start AND vigilance via `git status` per phase. Cause unknown — but the workaround is well-routinized at this point.

### Tests flipped (0)

The M45 outer-merge-fallback test exercises a different case (same-dtype outer with one side missing) than the M46 outer-merge MultiIndex fallback (mismatched-dtype outer). No flips needed.

## M45 — completed (single agent, 3 per-phase commits, no STOP CRITERIA cuts)

| Agent | Scope | Var prefix | NativeFn IDs | Commits |
|---|---|---|---|---|
| **M45 MultiIndex propagation** | Lift M44's MultiIndex-drops scope-down across 14 M42+M43 handlers | `m45_` | none new | `384d357` (A), `49f1a22` (B), `90f1def` (C) |

### What shipped

Lifts the M44 v1 scope-down. The 14 row/column-transforming and reshape handlers that previously dropped MultiIndex back to RangeIndex now propagate it correctly — same recipe pattern as M42 (single-col propagation), routed through M44's auto-dispatching `m44_permute_multiindex_into_df` helper plus a new sibling `m45_copy_multiindex_into_df` for column-list ops.

- **Phase A** (M42 ops): `sort_by` / `dropna` / `dropna_subset` route through `m44_permute_multiindex_into_df`. `select` / `drop` / `rename` / `fillna_*` route through new `m45_copy_multiindex_into_df`. `merge` extends per-`how` index policy to MultiIndex via new `m45_merge_build_multiindex` (inner/left/right preserve MultiIndex; **outer with dtype-mismatch still falls back to RangeIndex — M46 anchor**).
- **Phase B** (M43 reshape ops): `melt` repeats each MultiIndex level per `value_var`. `concat_rows` uses new `m45_concat_rows_multiindex` with strict per-level reconciliation (3-tier fallback: MultiIndex → single-col → RangeIndex). `concat_cols` takes lhs's MultiIndex. `pivot` and `pivot_table` explicitly **drop a MultiIndex** (reshape the row dimension — no clean target).
- **Phase C**: 19 new tests + `examples/tabular_multiindex_propagation_demo.spy` (~175 LOC, 9 M45-aware ops with `index_nlevels()` checks at every step) + LANGUAGE_GUIDE.md §5 M45 subsection + §11.26 + §11.32 rewrites.

### Tests flipped (2 — predicted)

- `vm/tests/m44_tabular_multiindex.rs::sort_by_drops_multiindex_m44b_anchor` → `sort_by_preserves_multiindex_m45` (`nlev=0` → `nlev=2`).
- `vm/tests/m44_tabular_multiindex.rs::select_drops_multiindex_m44b_anchor` → `select_preserves_multiindex_m45` (same flip shape).

### Methodology data point worth recording — the leak workaround story has a twist

The brief asked the agent to run the precautionary `cp` block at session start. **The agent could NOT run it** because Bash and PowerShell were both denied at session start. **Yet zero leak recurrences happened anyway** — every subsequent `Edit` / `Write` landed in the worktree directly. Likely cause: the M44 archive commit had already landed on main cleanly, leaving worktree state in sync with project root from the orchestrator's prior `git checkout` operations. **Refined hypothesis**: the leak triggers when worktree state diverges from project root at the start of an Edit session, NOT just "the first Edit on an existing file" as M40 narrowed or "Write also leaks" as M43 broadened.

If this hypothesis holds, the workaround is even simpler: as long as the orchestrator's prior milestone integration left main + worktree in agreement, no `cp` block needed. The M44 cp-at-start-success could have been redundant for the same reason. **Worth confirming on M46**: if M46 also starts with a sync'd worktree (which it should after this M45 push), it might skip the `cp` block and still see no leak.

### EXPLICIT M46 anchor

What still drops a MultiIndex (or doesn't propagate it):
- `pivot` / `pivot_table`: reshape the row dimension; no clean target for input MultiIndex. Likely stays a doc'd drop unless M46 adds a smart-fallback design.
- **Outer-merge with dtype-mismatch indexes**: still falls back to RangeIndex. M46 should add NaN-padded MultiIndex fallback.
- **`stack` / `unstack`**: pandas's MultiIndex bread-and-butter. Net-new code.
- **`df.loc[label_list]` / range-by-label**: net-new methods.
- **Time-series ops MultiIndex propagation** (resample / asof_merge / resample_index / asof_merge_index): currently single-col only.
- **`set_index([col])` accepting a 1-element list**: minor ergonomics — currently `set_index(col_name)` takes a string and `set_index_multi([cols])` takes a list; pandas unifies these.
- **`pivot_table(aggfunc=List)` + `margins=True`**: small extensions.

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

1. **THESIS + BLOG_POST refresh to M47** (small writing task, ~30-45 min).
   Both are at post-M39 currently. Concrete deltas for M40-M47:
   - Tests: 794 → 993 (+28 M40, +25 M41, +21 M42, +20 M43, +27 M44, +19 M45, +27 M46, +32 M47)
   - Stdlib classes: 18 → 19 (M47 added `ColumnCategorical` as a
     new sealed Column subclass)
   - Examples: 103 → 111
   - Lesson 1 streak: 21 → 29
   - `tabular` coverage: common-80% (post-M39) → ~95% (post-M40) →
     single-col DatetimeIndex with full propagation (post-M43) →
     MultiIndex with minimal propagation (post-M44) → fully
     index-aware for both single-col AND MultiIndex (post-M45) →
     v1 surface functionally complete (post-M46) → **v0.4 polish
     mostly done** (post-M47 with iloc 2-D + negative iloc +
     rolling Welford/min_periods + categorical dtype).
   - **Methodology notes worth flagging in BLOG**: (a) the M41/M44
     shared-infra cadence exception; (b) the **M47 new
     classification: "cross-dispatch"** — adding a new sealed-class
     subclass requires every dispatch file to compile together,
     so first commit lands at 50-75% of budget, not 20%; (c) the
     M43 9-test-flip cascade lesson; (d) the precautionary-cp
     workaround; (e) the M45/M46 hypothesis-refutation cycle —
     leak cause remains unknown.

2. **M48 — categorical optimized codes paths + more resample rules + rolling chainable**
   (the natural M47 follow-up). Per M47 agent's "what M48 should pick up":
   - **Categorical optimized codes paths** — group_by + merge
     equality on codes instead of strings (M47 v1 coerces via
     to_strings). Significant speedup for categorical-heavy
     workloads.
   - **Ordered categorical** with `Categorical.from_codes` reverse
     constructor + categories-ordering for sort.
   - **More resample rules** — `1w` / `1M` / `1Y` (needs calendar
     arithmetic layer for month/year).
   - **`df.rolling(window).agg(...)` chainable rolling object** —
     fluent API; new `RollingWindow` class shaped like
     `GroupedDataFrame`.
   - **`center=True` rolling window alignment** — deferred from M47.
   - **Outer-merge with MultiIndex on either side** — M46's
     fallback only handled dtype-mismatched single-col; MultiIndex
     outer needs its own NaN-padded shape.
   - **`unstack` distributing every regular column** — v1 only
     distributes first.
   - **`loc_range_*` on MultiIndex** — currently single-col only.
   - Estimated: ~1000-1500 LOC. Mix of optimized paths + small
     extensions.

3. **M49+ — Desktop UI** (the M37-design Phase 6) — its own
   substantial milestone or milestone-sequence.
   - Approach: webview-served (reuse M29 webserver) OR Tauri/wry
     hybrid. Compute backend is settled.
   - The "send a DataFrame to a browser tab" surface is the v1
     deliverable: pretty-printed table + filter UI + group_by
     pivot UI.
   - Significantly larger scope than typical milestones —
     probably worth splitting M49a (HTTP transport from `tabular`
     to JS frontend) and M49b (filter + pivot UI).

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

4. **Edit-tool worktree leak — cause still unknown; M45 hypothesis refuted by M46.**
   Recurred M37-M43 (7 consecutive milestones), narrowed in M40
   (Edit-on-existing-files), broadened in M43 (Write also affected).
   M44 fixed it operationally with a precautionary `cp` block at
   session start. **M45** saw zero leak recurrences even though the
   cp wasn't run (Bash denied) — leading to the M45 hypothesis that
   "the leak only triggers on worktree-divergence at session start."
   **M46 REFUTED this hypothesis**: same conditions as M45 (Bash
   denied for cp loop form, main was sync'd post-M45-push), but
   the leak DID recur. M45 was the lucky outlier, not a stable
   improvement.

   **Honest current state**: cause unknown. The leak is intermittent
   or triggered by something we haven't identified. The
   workaround stays well-routinized:
   - Precautionary `cp` block at session start if Bash is available.
   - Per-file `cp` recovery when symptoms appear mid-session
     (`git status` shows project-root diffs after Edits).
   - Orchestrator integration via `git checkout --` (modified
     files) + remove (untracked leaked files) + `git merge --ff-only`
     against the worktree HEAD — works regardless of how bad the
     leak got in-session.

   Harness root-cause investigation remains deprioritized because
   the workaround is reliable and cheap. The M45/M46 hypothesis-
   refutation cycle is recorded so future thinking doesn't claim
   we understand the leak — only that we can survive it.

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
   20%/40%/60%/80% checkpoint discipline. **29 consecutive clean
   agents** (M28 → M47) — the streak is the strongest empirical
   data point in the project. M37-M40 each ran 4-5 phase commits
   across ~2100-2800 LOC milestones. M41 + M44 slipped to combined
   commits (shared-infra exception). M42 + M43 + M45 + M46 returned
   to clean per-phase commits (disjoint handlers). **M47 introduced
   a new classification — "cross-dispatch"**: adding a new sealed-
   class subclass requires every dispatch file (resolver/ir/native/
   builtins) to grow together before the build goes green. The
   brief miscategorized M47 as "disjoint-handler" (predicted 20%),
   but the agent landed first commit at ~70% — not an error but a
   task-shape that doesn't fit either prior category.

   **Three classifications now (M41-M47):**
   - **disjoint-handler**: per-phase commits at ~20% (M42/M43/M45/M46)
   - **shared-infra**: combined Phase A at ~30-50% (M41/M44)
   - **cross-dispatch**: combined commit at ~50-75% (M47) — new
     sealed-class subclass forces a single build-green checkpoint
     across all dispatch sites

   Future brief language should classify accordingly. M48 should
   classify the categorical optimized codes paths as **disjoint-
   handler** (since the class already exists; M48 just extends
   match arms in existing dispatchers).

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
