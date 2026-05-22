# M40 — `tabular` Phase 5: time series + cumulative + null handling + range slicing

## Context

M37–M39 shipped the `tabular` Pandas-shaped data package in three consecutive single-agent milestones (Phases 1–4 of the original Pandas plan). After M39, `tabular` covers the common-80% of pandas workflows. M40 closes the rest of the pandas surface that real workflows hit constantly: cumulative ops, whole-frame null handling, range slicing, rolling-window aggregations, time-based resampling, and asof joins.

You are the **22nd** of an unbroken Lesson-1-compliant agent streak (M28 → M39). M37/M38/M39 each delivered ~2,400–2,800 LOC across 4–5 clean phase commits. M40 is similar scope (~1500–2000 LOC estimated), organized as 4 phases.

**No DatetimeIndex in M40.** True pandas-style index-aware operations would require adding an `index: Column` field to `DataFrame` plus index-bearing variants of every existing op — too much architectural churn for this milestone. M40's time-series operations (`resample`, `asof_merge`) take a column name argument identifying the time column, matching the existing `tabular` idiom (`df.sort_by("date", true)`, `df.group_by(["category"])`). DatetimeIndex can be a future v0.4 milestone if the demand is there.

## Files to read FIRST (in order)

1. `LANGUAGE_GUIDE.md` (project root) — §5 `tabular (M37, extended by M38, M39)` subsection + §6.2 + §11.18 / §11.19 / §11.20 / §11.21
2. `docs/thesis/agent_reports/m39_tabular_reshape.md` — the M39 design, especially the Five Findings (f64 bit-pattern hashing, null-cell join keys, the Edit-tool worktree leak)
3. `examples/tabular_demo.spy` + `examples/tabular_groupby_demo.spy` + `examples/tabular_reshape_demo.spy` — current demos
4. `compiler/src/resolver.rs` — search for `m39_` to find the M39 registration extensions to the tabular module; you will extend in the same shape
5. `compiler/src/ir.rs` — search for `m39_tabular_class_method_native_id_by_name` for the M39 dispatch table; you will add `m40_*` entries (or a new dispatcher fn) in the same shape
6. `vm/src/builtins.rs` — search for `m39_pluck_column` and `m38_groupby_build` — the latter has the `\x01`-joined hashing machinery you may reuse for resample bucketing
7. `shared/src/native.rs` — search for `M39Tab` to see the NativeFn block style; you will add an M40 block starting at id 985
8. `vm/tests/m39_tabular_reshape.rs` — test file shape for your new `vm/tests/m40_tabular_timeseries.rs`

## Constraints

- **Lesson 1**: first commit at ~20% of budget (Phase A green build + 1 smoke test). 21-streak — don't break it.
- **Variable prefix `m40_`** for all new helper functions / locals in shared files.
- **NativeFn IDs 985–1034** (50 slots reserved). M39 used 935–984.
- **Use the M36 `StdlibItemKind::Class` path** if you need any new classes. (Likely none — all M40 ops return existing `DataFrame` or `Column<T>`.) Do not touch `seed_prelude`.
- **No new crate deps**.
- **No changes to the M37/M38/M39 surface** other than additions. All 73 existing tabular tests (21 M37 + 25 M38 + 23 M39 + 4 demo-runs) must continue passing byte-identically.
- **Edit-tool worktree leak**: confirmed-recurring across M37+M38+M39. Same workaround — if `git status` shows project-root diffs after your edits, keep working in the worktree; the orchestrator integrates via `git checkout --` main + `git merge --ff-only` against the worktree HEAD.

## Phase A — Cumulative ops + null handling (~300-400 LOC)

### Cumulative reductions on numeric columns (8 NativeFns)

```python
# On ColumnI64:
ci.cumsum() -> ColumnI64        # null cells propagate (output null at that position
                                 # AND every position after — pandas default min_periods=1
                                 # would skip nulls; use the simpler propagation rule)
ci.cumprod() -> ColumnI64
ci.cummax() -> ColumnI64
ci.cummin() -> ColumnI64

# On ColumnF64 — same 4 signatures, all returning ColumnF64
```

**Null handling**: choose the simpler "propagate from first null forward" semantics rather than pandas's `min_periods` skip-nulls behavior. Documented as a v1 simplification. Result columns have the same length as input.

NaN on f64: NaN propagates per IEEE rules (sum + NaN = NaN). Document.

### Whole-frame null handling

```python
# Drop rows that have at least one null in any column:
df.dropna() -> DataFrame

# Drop rows with nulls only in specified columns:
df.dropna_subset(cols: List[str]) -> DataFrame

# Fill nulls in every column with a per-dtype value:
df.fillna_i64(v: i64) -> DataFrame    # fills only ColumnI64 columns; other dtypes unchanged
df.fillna_f64(v: f64) -> DataFrame
df.fillna_str(v: str) -> DataFrame
df.fillna_bool(v: bool) -> DataFrame
df.fillna_datetime(v: i64) -> DataFrame  # epoch-ms
```

The `fillna_*` per-dtype split mirrors M38's `get_column_*` design: the value argument is monomorphic per dtype, so each method is its own NativeFn. Whole-frame `fillna(any)` with a runtime-dispatched value doesn't fit StrictPy's typing.

### Range slicing

```python
df.iloc(start: i64, stop: i64) -> DataFrame
# Half-open [start, stop). Negative indices NOT supported in v1
# (pandas accepts -1; we raise ValueError). stop > nrows clamps to nrows.
```

### Commit checkpoint after Phase A

`M40 A: tabular cumulative ops + dropna/fillna + iloc`. Build clean + at least one smoke test exercising `cumsum` and `iloc`.

## Phase B — Rolling-window aggregations (~400-500 LOC)

```python
# On ColumnI64 (5 NativeFns):
ci.rolling_sum(window: i64) -> ColumnI64     # output[i] = sum of input[i-window+1..=i]
ci.rolling_mean(window: i64) -> ColumnF64    # mean is f64 even on i64 input
ci.rolling_min(window: i64) -> ColumnI64
ci.rolling_max(window: i64) -> ColumnI64
ci.rolling_std(window: i64) -> ColumnF64     # sample std (n-1 denominator)

# On ColumnF64 — same 5 signatures, all returning ColumnF64 (mean/std too)
```

### Behavior

- `window` must be >= 1 and <= column length; else raise ValueError
- Output length = input length
- Cells 0..window-1 in the output are **null** (incomplete window — matches pandas's default `min_periods=window`)
- Null cells in the input: a window containing any null produces a null in that output position
- For numeric stability: implement sum/min/max via sliding window (O(n) total); mean = sum / count; std via sum-of-values + sum-of-squares (Welford's method optional but not required for v1)

### Commit checkpoint after Phase B

`M40 B: tabular rolling windows — sum/mean/min/max/std on i64+f64`. Build clean + tests for at least rolling_sum and rolling_mean.

## Phase C — Time-series ops (~400-500 LOC)

### `df.resample(time_col, rule, agg)`

```python
df.resample(time_col: str, rule: str, agg: str) -> DataFrame
```

- `time_col`: name of a `ColumnDateTime` column in self. Raise ValueError if absent or wrong dtype.
- `rule`: bucket size. Accept `"1m"` (minute), `"5m"`, `"15m"`, `"1h"`, `"1d"`, `"7d"`. Parse pattern: `<i64><suffix>` where suffix ∈ {`m`, `h`, `d`}. Other formats raise ValueError.
- `agg`: aggregation name applied to every NON-time numeric column. One of `"sum" | "mean" | "min" | "max" | "count"`. Other values raise ValueError.

### Algorithm

1. Find the time column's min and max non-null values.
2. Define bucket boundaries as evenly-spaced intervals of `rule` width starting at the min, floor-aligned to the rule.
3. For each row, compute its bucket index by `(time - first_bucket_start) / rule_ms`.
4. Reuse the M38 group-by hashing machinery (string the bucket index, hash, accumulate row indices per bucket).
5. Output DataFrame: a new `ColumnDateTime` named after `time_col` with the bucket start times, then one column per non-time numeric source column with the aggregated values. Null buckets (no rows in that interval) get a null in numeric columns; the bucket-start time is still emitted.

Drop string + bool source columns (they have no defined aggregation in this v1).

### `df.asof_merge(other, on_self, on_other)`

```python
df.asof_merge(other: DataFrame, on_self: str, on_other: str) -> DataFrame
```

- Left-join `self` against `other` where each self row matches the latest other row with `other[on_other] <= self[on_self]`.
- `on_self` and `on_other` must both be `ColumnDateTime` (or both `ColumnI64`) — same dtype. Raise ValueError otherwise.
- Output: all self columns + all other columns except `on_other` (no duplicate keys). Self rows with no matching other row get null in the other-column slots.

### Algorithm

1. Stable-sort `other` by `on_other` (you'll need its sort permutation to read other columns post-sort).
2. For each self row, binary-search the sorted other for the largest `on_other <= self[on_self]`.
3. Emit the merged row.

### Commit checkpoint after Phase C

`M40 C: tabular resample + asof_merge`. Build clean + tests for both.

## Phase D — Tests + demo + docs (~200-300 LOC)

### Tests (`vm/tests/m40_tabular_timeseries.rs`)

Aim for 20–28 tests. Cover at minimum:
- Phase A: `cumsum` happy path + null propagation; `cumprod` / `cummax` / `cummin` smoke; `dropna` happy path; `dropna_subset`; `fillna_i64` (and one f64 / str test for variety); `iloc` happy path + clamp at end + ValueError on negative start.
- Phase B: `rolling_sum(window=3)` on a 5-row column → first 2 cells null, then sliding sums; `rolling_mean` returns f64 from i64 input; `rolling_std` sanity on a known input; window > nrows raises; null cells in window produce nulls.
- Phase C: `resample(time_col, "1d", "sum")` on a 5-day spread; `resample` with empty bucket gets null aggregations; `asof_merge` happy path matches the latest preceding row; `asof_merge` with no match emits null right-side columns; `asof_merge` with dtype mismatch raises.

### Demo

Add `examples/tabular_timeseries_demo.spy` (~100–150 LOC) — a realistic walkthrough:
1. Construct a 30-row time-stamped DataFrame (events with timestamp + amount + category)
2. `resample("1d", "sum")` to get daily totals
3. `cumsum` to get running cumulative
4. `rolling_mean(window=3)` for a 3-day moving average
5. `asof_merge` against a separate small DataFrame of category → tax_rate to enrich
6. Print

Testable via `compiler/tests/tabular_timeseries_demo_runs.rs`.

### LANGUAGE_GUIDE.md update

Extend §5 `tabular (M37, extended by M38, M39, M40)` with the new operations. Add a sub-block per phase. Add §11 entries for any gotchas (esp. cumulative null-propagation, rolling-window leading nulls, resample rule format, asof_merge dtype-match requirement). Bump banner to "post-M40".

### Commit checkpoint after Phase D

`M40 D: tabular time-series — tests + demo + LANGUAGE_GUIDE update + agent report`.

## Acceptance criteria (machine-checkable)

1. `cargo build --workspace --release` — clean, no new warnings.
2. `cargo test --release -p strictpy-vm --test m40_tabular_timeseries` — all pass.
3. `cargo test --release -p strictpy-compiler --test tabular_timeseries_demo_runs` — passes.
4. **No M37/M38/M39 regressions**: targeted M37/M38/M39 sweeps all pass byte-identically.
5. **Full sweep**: `cargo test --workspace --release --no-fail-fast` reports 794 + N passing, 0 failing, 1 ignored (target N = 22–30).

## Constraints — files NOT to modify

- `seed_prelude` in `compiler/src/resolver.rs`.
- The 73 existing tabular tests — must keep passing untouched.
- `examples/tabular_demo.spy` / `tabular_groupby_demo.spy` / `tabular_reshape_demo.spy` — add a separate `tabular_timeseries_demo.spy`.

## STOP CRITERIA — priority drops if budget runs out

Five priority drops, in order:

1. **Drop `df.asof_merge`** — biggest single piece in Phase C; M41 picks up.
2. **Drop `df.resample`** entirely — keeps Phase C tractable as nothing.
3. **Drop `rolling_std`** — sum/mean/min/max cover most uses; std needs sum-of-squares bookkeeping that adds bulk.
4. **Drop `cumprod` / `cummin`** — keep `cumsum` / `cummax` (more commonly used).
5. **Drop LANGUAGE_GUIDE.md update** — orchestrator finishes it.

After applying any drop, document what was cut with a "what M41 should pick up" list.

## Methodology discipline

1. **First commit at ~20% of budget** (Phase A green build + smoke test).
2. **Per-phase commits** — 4 commits expected. Each builds clean.
3. **Variable prefix `m40_`** in shared files.
4. **Name-based dispatch in `ir.rs`** (mirror `m39_tabular_class_method_native_id_by_name`); do NOT add new IR opcodes.
5. **Edit-tool worktree leak workaround**: confirmed-recurring across M37+M38+M39. Don't fight it — keep your worktree commits clean; the orchestrator handles integration.

## Final report

Write `docs/thesis/agent_reports/m40_tabular_timeseries.md` (under 600 words) covering:
- What shipped per phase (A–D)
- What was cut from STOP CRITERIA (if anything)
- LOC delta per touched file
- Final test count + verification
- Surprises / design calls (e.g., null-propagation choice for cumulative ops, rule-string parsing for resample, asof binary-search edge cases)
- "What M41 should pick up" — concrete follow-up list (DatetimeIndex, Categorical columns, pivot_table, more rolling stats?)
- LANGUAGE_GUIDE.md update status
- Whether the Edit-tool worktree leak recurred (yes/no, how many times)

Commit this report in Phase D's commit.

## Commit message shape (final)

```
M40: tabular time series — cumulative + dropna/fillna + iloc + rolling + resample + asof_merge

Phase 5 of the Pandas-shaped data package. After M37–M39 shipped
core + IO + filter + sort + aggregations + group-by + reshape,
M40 closes the time-series and null-handling surface that real
workflows hit constantly.

Phase A: Column.{cumsum,cumprod,cummax,cummin} on i64+f64;
df.dropna / dropna_subset; per-dtype df.fillna_*; df.iloc range.
Phase B: rolling_{sum,mean,min,max,std} on i64+f64.
Phase C: df.resample(time_col, rule, agg) with rule parser;
df.asof_merge(other, on_self, on_other) via sorted binary search.
Phase D: 22–30 new tests + tabular_timeseries_demo.spy +
LANGUAGE_GUIDE.md update.

DatetimeIndex deferred — time-series ops take a column-name
argument instead, matching the existing tabular idiom.

NativeFn IDs 985–1034. Variable prefix m40_.
Tests: 794 → 794+N. Examples: +1 (tabular_timeseries_demo.spy).
```
