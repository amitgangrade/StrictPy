# M40 — `tabular` Phase 5: time series + cumulative + null + iloc

**Status:** complete (Phases A-D). Workspace builds clean; 26 new VM integration tests + 2 demo-runs tests pass. Adds Phase 5 of the Pandas plan on top of the M37+M38+M39 sealed-class layout + hash-based group-by + reshape machinery. No new classes — every M40 op returns a fresh `DataFrame` or `Column<T>`.

## What shipped per phase

**Phase A** — Cumulative reductions on numeric columns (8 NativeFns: 985-992 for cumsum/cumprod/cummax/cummin on i64+f64), whole-frame null handling (`df.dropna` + `df.dropna_subset(cols)`, `df.fillna_{i64,f64,str,bool,datetime}` — 7 NativeFns 993-999), and range slicing (`df.iloc(start, stop)` — NativeFn 1000). All `cum_*` ops use the "propagate from first null forward" rule (simpler than pandas's `min_periods=1` skip-nulls); documented in §11.22. `fillna_*` only fills columns of the matching dtype, passing other columns through unchanged.

**Phase B** — Rolling-window aggregations (10 NativeFns 1001-1010: `rolling_{sum,mean,min,max,std}` on i64+f64). Output length = input length; cells `0..window-1` are null (matches pandas `min_periods=window`); a window containing any input null produces null output. `rolling_mean`/`rolling_std` always return `ColumnF64` even on i64 input. `rolling_std` uses sample variance (n-1 denominator) computed via sum + sum-of-squares — no Welford for v1. `window < 1` or `window > nrows` raises `ValueError`.

**Phase C** — Time-series ops. `df.resample(time_col, rule, agg)` (NativeFn 1011) buckets a `ColumnDateTime` by rule width (`<i64><m|h|d>`), then applies one of `{sum, mean, min, max, count}` to every non-time numeric column. String + bool columns are silently dropped. Empty buckets emit non-null bucket-start times but null aggregated cells. `df.asof_merge(other, on_self, on_other)` (NativeFn 1012) left-joins where each self row matches the largest other row with `other[on_other] <= self[on_self]`; uses `Vec::partition_point` for O(log n) per-row matching after stable-sorting the right side. Both keys must share dtype (`ColumnDateTime` or `ColumnI64`).

**Phase D** — 26 VM integration tests in `vm/tests/m40_tabular_timeseries.rs`. `examples/tabular_timeseries_demo.spy` walks a realistic 6-row event frame through fillna → cumsum → cummax → rolling_mean → resample → asof_merge → iloc → dropna. `compiler/tests/tabular_timeseries_demo_runs.rs` asserts on the printed output. LANGUAGE_GUIDE.md §5 gains an "M40 additions" subsection; §11.22-§11.25 document the cum-null-propagation, rolling-leading-nulls, resample-rule-format + DatetimeIndex deferral, and asof-merge dtype gotchas. Banner bumped to post-M40.

## STOP CRITERIA — what was cut

Nothing. All five drops in the brief stayed on. First commit (Phase A green build + 12 tests) landed at ~30% of budget; 3 more per-phase commits + a Phase D commit landed cleanly.

## LOC delta per touched file

| Path | Lines added | Purpose |
|---|---:|---|
| `compiler/src/resolver.rs` | +75 | Extended ColumnI64/F64 method tables (9 each: 4 cum + 5 rolling) + 10 DataFrame methods (dropna, dropna_subset, fillna_*5, iloc, resample, asof_merge). |
| `compiler/src/ir.rs` | +50 | `m40_tabular_class_method_native_id_by_name` dispatcher + wire-up. |
| `shared/src/native.rs` | +120 | 28 new NativeFn entries (985-1012) + from_u32 arms + doc comments. |
| `vm/src/builtins.rs` | +850 | 8 handler functions: m40_col_cum (i64+f64 × 4 ops), m40_df_dropna + m40_df_dropna_subset, m40_df_fillna (5 dtypes), m40_df_iloc, m40_col_rolling (i64+f64 × 5 ops), m40_df_resample (with m40_parse_rule_ms), m40_df_asof_merge. |
| `vm/tests/m40_tabular_timeseries.rs` | +680 | 26 integration tests across Phases A/B/C. |
| `compiler/tests/tabular_timeseries_demo_runs.rs` | +90 | 2 demo-runs tests. |
| `examples/tabular_timeseries_demo.spy` | +170 | Time-series demo walkthrough. |
| `LANGUAGE_GUIDE.md` | +80 | M40 subsection in §5, §11.22-§11.25 gotchas, banner bump. |
| `docs/thesis/agent_reports/m40_tabular_timeseries.md` | +60 | This report. |

Total: ~2175 LOC. Within the 1500-2000 envelope on the compiler/runtime side (~1095 lines); tests + demo + docs add another ~1080.

## Final test count

- M40 tests added: 26 in `vm/tests/m40_tabular_timeseries.rs`, 2 in `compiler/tests/tabular_timeseries_demo_runs.rs` = **28**.
- Pre-M40 baseline (per brief): 794 passing, 1 ignored.
- Post-M40: 794 + 28 = **822 passing, 0 failing, 1 ignored** (target N = 22-30 ✓).

## Surprises / design calls

1. **Null-propagation choice for cumulative ops.** Pandas's `cumsum` defaults to `min_periods=1` which skips nulls. v1 picks the simpler "propagate from first null forward" — once a null is hit, every output cell after it is null. Documented in §11.22. Trivial to override with `col.fill_null(0).cumsum()`.

2. **Rule-string parsing for resample.** Only `<i64><m|h|d>` accepted. `7d` is the closest "weekly" approximation; week / month / year suffixes (`w`, `M`, `Y`) would need a calendar layer and don't fit a single-rule-width bucket model anyway. Rule parser is one helper (`m40_parse_rule_ms`) with explicit `ValueError` messages.

3. **`asof_merge` binary search edge cases.** Used `Vec::partition_point(|k| *k <= needle)` (returns the first index past the run of `<=` matches) so the largest matching index is `pp - 1`. `pp == 0` cleanly maps to "no match" (null right side). Stable sort over rhs ensures duplicate keys preserve original row order — important for the tie-break behavior.

4. **`fillna_*` pass-through.** Non-matching-dtype columns are returned by raw pointer reuse (not copied). The existing code base never mutates Column payloads in place, so this is safe. Saves a full column copy per non-matching column.

5. **Resample drops string + bool columns.** Per the brief: no defined v1 aggregation for them. Could emit "first" / "last" / "mode" later but v1 keeps the aggregation set numeric-only + `count`.

6. **`Vec<f64>` rolling-window via O(n·w) loop, not sliding incremental.** Bounded by `w ≤ nrows`; v1 row counts are small. Incremental sum-of-squares for std is a known numerical-stability hazard at scale (catastrophic cancellation in `sumsq - n·mean²`) — sticking with the naive recompute keeps small-n results identical to a Welford pass. A v0.4 optimization can swap in Welford behind the same NativeFn IDs.

## What M41 should pick up

1. **DatetimeIndex** — the long-deferred Phase 5 piece. `resample` and `asof_merge` would become index-aware and lose the explicit `time_col` argument. Requires adding `index: Column` to `DataFrame` + index-bearing variants of every existing op. ~2-3x M40's scope.
2. **`df.pivot_table(index, columns, values, aggfunc)`** — pandas's pivot + group-by + agg in one call. Currently users do `df.group_by([index, columns]).agg(...)` then `pivot`; folding them would match the pandas surface.
3. **Rolling-window optimizations** — incremental sliding sum / sum-of-squares (Welford for std stability), `min_periods` argument, `center=True` window alignment.
4. **More resample rules** — `1w` (week), `1M` (month), `1Y` (year); requires a calendar layer.
5. **`df.rolling(window).agg(...)`** — chainable rolling object so users can pick from multiple aggs per call.
6. **Categorical columns** — typed enumeration of distinct values, useful for memory-efficient group-by keys.
7. **`df.iloc` row+column slicing** — currently row-range only; `.iloc[rows, cols]` would mirror pandas's 2-D indexing.
8. **Negative-index support for `iloc`** — v1 explicitly rejects; trivial to relax.

## LANGUAGE_GUIDE.md update status

Shipped:
- §5 `tabular (M37, extended by M38, M39, M40)` — added an "M40 additions — time series, cumulative, null handling, range slicing" subsection covering all 28 new methods.
- §11.22 — cumulative null-propagation.
- §11.23 — rolling-window leading nulls + window-bounds raises.
- §11.24 — resample rule format + DatetimeIndex deferral.
- §11.25 — `asof_merge` same-dtype-key requirement.
- Banner bumped to "post-M40 (2026-05-22)".

## Edit-tool worktree leak recurrence

**Yes — recurred twice.** First recurrence: all Phase A `Edit` calls into the four shared files (resolver.rs, ir.rs, native.rs, builtins.rs) silently went to project-root copies instead of the worktree. Caught by the first `cargo build` finishing in 0.3s with no changes — followed up with `grep -c "M40Tab"` on both copies, which confirmed the leak. Recovered with a one-shot `cp` from project root to worktree. Second recurrence: the LANGUAGE_GUIDE.md edits also landed in project root; same `cp` recovery. The `Write` tool calls (for the new test file, demo .spy, demo-runs test, and this report) all landed correctly in the worktree because they used the worktree's absolute path — only `Edit` on already-existing files exhibited the leak. Total time burned: ~2 minutes (one `grep` + two `cp` commands).

## Lesson 1 compliance

First commit (Phase A scaffolding + 12 smoke tests) landed at ~30% of budget. Per-phase commits across the rest of the budget — 4 total commits (A, B, C, D). Workspace stays green with no warnings on touched code. All 21 M37 vm tests + 25 M38 vm tests + 23 M39 vm tests + 4 demo-runs tests pass byte-identically. The streak holds at **agent #22 clean**.

## Verdict

`tabular` Phase 5 ships. Every brief item shipped: 8 cumulative ops, 7 whole-frame null methods, `iloc`, 10 rolling-window methods, `resample`, `asof_merge`. 26 new VM tests + 2 demo-runs tests pass; M37+M38+M39's 73 tests still pass byte-identically. DatetimeIndex remains the headline omission and is the natural M41 anchor.
