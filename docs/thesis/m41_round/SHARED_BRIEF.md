# M41 — `tabular` Phase 5b: DatetimeIndex (minimum viable) + pivot_table

## Context

M37–M40 shipped the `tabular` Pandas-shaped data package across Phases 1–5 of the original Pandas plan (Column hierarchy + IO + filter + sort + aggregations + group-by + reshape + cumulative + null + rolling + resample + asof_merge). After M40 the module covers roughly ~95% of common pandas workflows. The **headline omission is DatetimeIndex**, deferred from M40 because doing it "properly" — propagating an index through every existing DataFrame method — would have been 3000+ LOC and high-risk for a single agent.

M41 ships the **minimum viable DatetimeIndex** + **`pivot_table`**:

- A minimal index abstraction (set / reset / has / get / sort_index)
- A handful of explicit index-aware ops (`resample_index`, `asof_merge_index`, `select_by_label_*`)
- `df.pivot_table` — pandas's pivot + group-by + agg in one call
- **Explicit scope-down**: every other existing DataFrame method that returns a fresh DataFrame (filter / sort_by / head / tail / iloc / merge / pivot / melt / concat / dropna / fillna / select / drop / etc.) **drops the index in v1**, returning a RangeIndex result. Full index propagation is M42 work — once we have the surface in user code we'll know which propagation paths actually matter.

You are the **23rd** of an unbroken Lesson-1-compliant agent streak (M28 → M40). M37–M40 each delivered ~2100–2800 LOC across 4–5 clean phase commits.

## Files to read FIRST (in order)

1. `LANGUAGE_GUIDE.md` (project root) — §5 `tabular` subsection (especially the §5 M40 additions block + §11.22-§11.25)
2. `docs/thesis/agent_reports/m40_tabular_timeseries.md` — design rationale + the Edit-tool worktree leak narrowing (Edit on existing files leaks; Write with absolute paths is fine)
3. `examples/tabular_timeseries_demo.spy` — M40 demo for stylistic reference
4. `compiler/src/resolver.rs` — search for `m40_` to find M40 registrations; you will extend in the same shape. Also find the existing DataFrame class layout to plan the index field addition.
5. `compiler/src/ir.rs` — search for `m40_tabular_class_method_native_id_by_name` for the dispatch table; add `m41_*` entries.
6. `vm/src/builtins.rs` — search for `m40_df_resample` and `m40_df_asof_merge` for time-series handler shapes; you'll write index-aware variants in similar form. Search for `m38_groupby_build` for hash bucketing reusable for pivot_table.
7. `shared/src/native.rs` — search for `M40Tab` for the NativeFn block style; add an M41 block starting at id 1015.
8. `vm/tests/m40_tabular_timeseries.rs` — test file shape for your new `vm/tests/m41_tabular_index.rs`.

## Constraints

- **Lesson 1**: first commit at ~20% of budget. 22-streak — don't break it.
- **Variable prefix `m41_`** for all new helper functions / locals.
- **NativeFn IDs 1015–1064** (50 slots reserved). M40 used 985–1012.
- **No new classes**. Use the existing DataFrame; extend its layout to carry an optional index column.
- **No new crate deps**.
- **No changes to the M37/M38/M39/M40 surface** other than:
  - **OK to add**: a new internal field on DataFrame for the optional index (must default to "none" / RangeIndex so existing constructors don't need updates).
  - **OK to add**: the new index-aware NativeFns.
  - **NOT OK to change**: any existing method's signature or behavior beyond "the result has a RangeIndex" (which is the v1 default and matches today's behavior).
  - All 101 existing tabular tests (21 M37 + 25 M38 + 23 M39 + 26 M40 + 6 demo-runs) must pass byte-identically.

### Edit-tool worktree leak — NEW M40 data point

The leak (now confirmed across M37+M38+M39+M40) was narrowed in M40: it affects **`Edit` calls on already-existing files**, not `Write` calls (which take absolute worktree paths). Mitigation:
- After bulk-editing existing shared files (`resolver.rs`, `ir.rs`, `native.rs`, `builtins.rs`), check `git status` once; if there are diffs in the project root, `cp` from project root to worktree.
- Don't fight the leak; the M40 agent burned ~2 minutes total on it.
- `Write` for new files (your test file, demo .spy, demo-runs test, agent report) doesn't have this problem.

## Phase A — Index storage + set/reset/has/get/sort_index (~400-500 LOC)

### DataFrame layout change

Add an optional index slot to DataFrame. Suggested shape (you decide the exact field placement based on the existing struct):

```
DataFrame:
  names: List[str]
  columns: List[Column]
  nrows: i64
  index: Option<Column>     // NEW. None = RangeIndex (today's behavior).
  index_name: Option<str>   // NEW. Original column name when set_index was called; None if no index.
```

Constructors (`from_columns`, `from_rows`, `from_dict`, `read_csv`, etc.) default `index = None` — every existing construction path keeps current behavior. The presence of an index is opt-in via `df.set_index(col_name)`.

### Surface

```python
# Index manipulation:
df.set_index(col_name: str) -> DataFrame
# Removes col_name from columns, makes it the new index. Raises ValueError if
# col_name is absent or if df already has an index (must reset_index first).

df.reset_index() -> DataFrame
# Removes the index; re-inserts it as a regular column at position 0 with its
# original name (or "index" if no original name). Returns RangeIndex DataFrame.
# No-op if df has no index.

df.has_index() -> bool
df.index() -> Column?              # none if RangeIndex
df.index_name() -> str?            # none if RangeIndex

# Sort by index (stable, ascending):
df.sort_index(ascending: bool) -> DataFrame
# Raises ValueError if df has no index. Returns DataFrame WITH the index (this
# is the one existing-pattern exception — sort_index obviously must preserve
# what it sorts by). Per-Column-dtype comparator dispatch on the index column.
```

### Commit checkpoint after Phase A

`M41 A: tabular DatetimeIndex — set_index / reset_index / has_index / index / index_name / sort_index`. Build clean + smoke tests for set_index round-trip + sort_index.

## Phase B — Index-aware time-series + select by label (~400-500 LOC)

### `df.resample_index(rule, agg) -> DataFrame`

Variant of M40's `resample(time_col, rule, agg)` that uses the DataFrame's index instead of a column argument. Raises ValueError if the index isn't a `ColumnDateTime`. Output: a fresh DataFrame whose index IS the bucket-start times (so the output preserves its own index — the one exception, like sort_index). All non-index numeric columns are aggregated; string + bool columns are dropped (same v1 behavior as M40 resample).

### `df.asof_merge_index(other) -> DataFrame`

Variant of M40's `asof_merge` that uses both DataFrames' indexes. Both must have indexes of the same dtype (ColumnDateTime or ColumnI64); raises ValueError otherwise. Left-join semantics, same as M40. Output DataFrame has self's index preserved + self's columns + other's columns (no index column duplication).

### `df.select_by_label_*` per dtype (3 NativeFns)

Look up a row by its index label. Returns a one-row DataFrame (or `none` if the label isn't in the index). The index dtype must match the lookup method.

```python
df.select_by_label_i64(label: i64) -> DataFrame?
df.select_by_label_str(label: str) -> DataFrame?
df.select_by_label_datetime(label: i64) -> DataFrame?   # epoch-ms
```

Output DataFrame has the matching index preserved (one row); raises ValueError if df has no index or the index dtype doesn't match. Returns `none` (not error) if the label is genuinely absent.

If the index has duplicate labels (legal but unusual), return only the first matching row in v1. Document this in LANGUAGE_GUIDE.

### Commit checkpoint after Phase B

`M41 B: tabular index-aware ops — resample_index + asof_merge_index + select_by_label_*`. Build clean + tests.

## Phase C — `df.pivot_table` (~300-400 LOC)

```python
df.pivot_table(index_col: str, columns_col: str, values_col: str, aggfunc: str) -> DataFrame
```

Pandas's most-loved DataFrame method: pivot + group-by + aggregate in one call.

- `index_col`: column whose unique values become row labels
- `columns_col`: column whose unique values become column headers
- `values_col`: column whose values get aggregated
- `aggfunc`: one of `"sum" | "mean" | "min" | "max" | "count"` — same vocabulary as M38 group-by shortcuts. Other values → ValueError.

### Algorithm (reuses M38 + M39 machinery)

1. Group rows by (index_col, columns_col) — reuse the M38 `\x01`-joined hash bucketing.
2. For each bucket, aggregate `values_col` via the named aggfunc.
3. Reshape into a wide DataFrame: rows are unique index_col values; columns are unique columns_col values (stringified); cells are the aggregated values (null where no matching bucket).

### Output dtype

The output value-cells dtype matches the source `values_col` dtype, except for `mean` which always produces `ColumnF64` and `count` which always produces `ColumnI64` (matching M38 aggregation behavior).

The index_col becomes a regular column in the output named `index_col`'s name. (No index propagation — see scope-down.) This makes pivot_table's output usable with all other DataFrame ops without index gymnastics.

### Commit checkpoint after Phase C

`M41 C: tabular pivot_table — pivot + group_by + agg combined`. Build clean + tests.

## Phase D — Tests + demo + docs (~250-300 LOC)

### Tests (`vm/tests/m41_tabular_index.rs`)

Aim for 22–28 tests. Cover:
- Phase A: set_index round-trip (set_index then reset_index returns equivalent frame); has_index; index() / index_name() accessors; set_index on absent column raises; set_index on already-indexed frame raises; sort_index ascending + descending; sort_index without index raises.
- Phase B: resample_index happy path on a 5-day dataset; resample_index without index or with non-datetime index raises; asof_merge_index happy path; asof_merge_index with mismatched index dtypes raises; select_by_label_i64 hit + miss; select_by_label_str hit; select_by_label_datetime hit.
- Phase C: pivot_table happy path (3 index values × 2 columns values, sum agg); pivot_table mean produces ColumnF64; pivot_table count produces ColumnI64; pivot_table with missing (index, columns) cells produces null; pivot_table with bad aggfunc raises.

### Demo

Add `examples/tabular_index_demo.spy` (~120-150 LOC) — a realistic walkthrough:
1. Read a CSV of stock trades (datetime, symbol, price, qty)
2. `set_index("datetime")` to make it a time-indexed DataFrame
3. `resample_index("1d", "sum")` for daily totals
4. `sort_index(false)` for most-recent-first
5. Separately: from the original trades, `pivot_table("symbol", "side", "qty", "sum")` for a buys/sells matrix
6. `asof_merge_index` against a rates DataFrame
7. Print results

Testable via `compiler/tests/tabular_index_demo_runs.rs`.

### LANGUAGE_GUIDE.md update

Extend §5 `tabular (M37, extended by M38, M39, M40, M41)` with the new operations. Add a sub-block per phase. Add §11 entries for:
- The v1 scope-down: most ops drop the index, returning RangeIndex; only sort_index, resample_index, asof_merge_index, select_by_label_* preserve it. M42 will round this out.
- Duplicate-label behavior of select_by_label_*.
- pivot_table aggfunc vocabulary.

Bump banner to "post-M41".

### Commit checkpoint after Phase D

`M41 D: tabular DatetimeIndex + pivot_table — tests + demo + LANGUAGE_GUIDE update + agent report`.

## Acceptance criteria (machine-checkable)

1. `cargo build --workspace --release` — clean, no new warnings.
2. `cargo test --release -p strictpy-vm --test m41_tabular_index` — all pass.
3. `cargo test --release -p strictpy-compiler --test tabular_index_demo_runs` — passes.
4. **No M37-M40 regressions**: targeted M37/M38/M39/M40 sweeps all pass byte-identically.
5. **Full sweep**: `cargo test --workspace --release --no-fail-fast` reports 822 + N passing, 0 failing, 1 ignored (target N = 22-28).

## Constraints — files NOT to modify

- `seed_prelude` in `compiler/src/resolver.rs`.
- The 101 existing tabular tests — must keep passing untouched.
- The 4 existing tabular demos in `examples/` — add a separate `tabular_index_demo.spy`.

## STOP CRITERIA — priority drops if budget runs out

Five priority drops, in order:

1. **Drop `df.pivot_table`** — the standalone-most-additional piece. Index work is the M41 anchor; pivot_table can be M42 alongside the index-propagation work.
2. **Drop `df.asof_merge_index`** — keep resample_index. asof's binary-search is bulkier.
3. **Drop `select_by_label_datetime`** and `select_by_label_i64`. Keep `select_by_label_str` (most common index dtype).
4. **Drop `df.sort_index` descending** — keep ascending only.
5. **Drop LANGUAGE_GUIDE.md update** — orchestrator finishes it.

After applying any drop, document what was cut with a "what M42 should pick up" list.

## Methodology discipline

1. **First commit at ~20% of budget** (Phase A green build + smoke test).
2. **Per-phase commits** — 4 commits expected. Each builds clean.
3. **Variable prefix `m41_`** in shared files.
4. **Name-based dispatch in `ir.rs`** (mirror `m40_tabular_class_method_native_id_by_name`); do NOT add new IR opcodes.
5. **Edit-tool worktree leak workaround**: bulk Edits to shared files may leak to project root. Check `git status` after the first round of Edits; `cp` if needed. Write to absolute worktree paths is unaffected.

## Final report

Write `docs/thesis/agent_reports/m41_tabular_index.md` (under 600 words) covering:
- What shipped per phase (A–D)
- What was cut from STOP CRITERIA (if anything)
- LOC delta per touched file
- Final test count + verification
- Surprises / design calls (especially: how did you plumb the optional index through DataFrame allocation? what GC implications? how does sort_index dispatch by index dtype?)
- "What M42 should pick up" — concrete follow-up list. Critically: which existing methods you think should propagate the index in M42 (filter/sort/head/...) and the cost of doing so. This list shapes the M42 brief.
- LANGUAGE_GUIDE.md update status
- Whether the Edit-tool worktree leak recurred (yes/no, how many times, your mitigation effectiveness)

Commit this report in Phase D's commit.

## Commit message shape (final)

```
M41: tabular DatetimeIndex (minimum viable) + pivot_table

Phase 5b of the Pandas plan. After M40 deferred DatetimeIndex
to keep scope manageable, M41 ships the minimum viable index
abstraction plus pandas's most-loved DataFrame method.

Phase A: DataFrame gains optional index/index_name fields;
df.set_index / reset_index / has_index / index / index_name /
sort_index.
Phase B: df.resample_index(rule, agg) + df.asof_merge_index(other)
+ 3 typed df.select_by_label_*.
Phase C: df.pivot_table(index, columns, values, aggfunc) —
combines pivot + group-by + agg in one call.
Phase D: ~25 new tests + tabular_index_demo.spy +
LANGUAGE_GUIDE.md §5/§11 updates.

EXPLICIT SCOPE-DOWN: every existing DataFrame method that returns
a fresh frame drops the index in v1 (returns RangeIndex). Only
sort_index / resample_index / asof_merge_index / select_by_label_*
preserve it. Full index propagation through filter/sort/head/etc.
is M42 work, once we see which paths users actually exercise.

NativeFn IDs 1015–1064. Variable prefix m41_.
Tests: 822 → 822+N. Examples: +1 (tabular_index_demo.spy).
```
