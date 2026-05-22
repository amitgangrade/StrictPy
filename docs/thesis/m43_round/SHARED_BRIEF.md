# M43 — `tabular` index propagation through reshape + group_by + pivot_table

## Context

M41 added the optional index to `DataFrame`. M42 propagated it through 11 row/column-transforming methods (filter / sort_by / head / tail / iloc / select / drop / rename / dropna / dropna_subset / fillna_* / merge). M43 finishes the story: the remaining "still drops index" methods become index-aware.

Per the M42 agent's "what M43 should pick up", the punch list is:
- `pivot_table` — `index_col` becomes the result's index
- `group_by` + agg — group-key column becomes the result's index (single-column key first; multi-column requires MultiIndex which is M44)
- `pivot` — `index` value becomes the result's index
- `melt` — input's index repeats per `value_var`
- `concat_rows` — concatenate input indexes if all have them
- `concat_cols` — lhs's index wins (consistent with M42's merge policy)

After M43, the `tabular` package is **fully index-aware end-to-end** (modulo MultiIndex, which is M44+).

**Scope-down on multi-column group_by**: when `df.group_by([col1, col2])` (multi-column), the result's group keys remain regular columns rather than becoming a MultiIndex. Document the contract; M44 ships MultiIndex and lifts this restriction.

You are the **25th** of an unbroken Lesson-1-compliant agent streak (M28 → M42). M42 returned cleanly to per-phase commits because its phases modified disjoint handlers. M43's phases also modify mostly disjoint handlers — should be similarly clean.

## Files to read FIRST (in order)

1. `LANGUAGE_GUIDE.md` (project root) — §5 `tabular` subsection (especially §5 M41 + §5 M42 additions, §11.26 the now-rewritten "what propagates vs drops")
2. `docs/thesis/agent_reports/m42_tabular_index_propagation.md` — the recipe pattern (single helper `m42_permute_index_into_df` applied at the emit site of each handler) — same pattern applies to M43 for handlers that build a `keep_indices` row vector
3. `docs/thesis/agent_reports/m41_tabular_index.md` — the `m41_build_df_with_index` constructor + the 40-byte payload + GC implications
4. `examples/tabular_index_propagation_demo.spy` — current end-to-end demo (M42)
5. `vm/src/builtins.rs` — find:
   - `m42_permute_index_into_df` and `m42_copy_index_into_df` — helpers you'll reuse
   - `m41_build_df_with_index` — the constructor
   - `m38_groupby_*` family — group_by infrastructure
   - `m38_agg_dispatch` + `m38_gdf_sum/mean/min/max/count` — group_by aggregation handlers
   - `m41_df_pivot_table` — pivot_table handler
   - `m39_df_pivot` and `m39_df_melt` — pivot/melt handlers
   - `m39_tabular_concat_rows` and `m39_tabular_concat_cols` — concat handlers
6. `vm/tests/m41_tabular_index.rs` and `vm/tests/m42_tabular_index_propagation.rs` — test file patterns
7. `compiler/src/resolver.rs` — no new method registrations needed; verify by grep

## Constraints

- **Lesson 1**: first commit at ~20% of budget. 24-streak — don't break it.
- **Variable prefix `m43_`** for any new helper functions / locals. Most M43 work modifies existing handlers, so few new helpers.
- **NativeFn IDs**: likely none new in M43 (modifying existing handlers, not adding methods). If you discover a genuinely-needed new method, allocate from 1027-1050.
- **No new classes**, no new crate deps.
- **No changes to method signatures** — every existing method keeps its exact public surface. Only behavior changes for indexed-input cases.
- All 144 existing tabular tests must keep passing — except any "verifies index is dropped on pivot/group_by/pivot_table/etc." tests in `m41_tabular_index.rs` or `m42_tabular_index_propagation.rs`. List every flipped test in your final report (like M42 flipped `filter_drops_index`).

### Edit-tool worktree leak — known recurring

Same M37-M42 pattern. **M40 narrowed the cause**: `Edit` on existing files leaks; `Write` with absolute worktree paths doesn't. M42 hit it 5× (one per phase commit on `builtins.rs`). Mitigation: after the first round of bulk Edits per phase, check `git status`; if there are diffs in the project root, `cp` from project root to worktree. ~5 seconds per recovery.

## Phase A — pivot_table + single-column group_by index promotion (~250-300 LOC)

### `df.pivot_table(index_col, columns_col, values_col, aggfunc)`

Currently: the output's first column is named after `index_col` and holds the unique values; RangeIndex.

M43: the output's `index_col` becomes the **index** (with `index_name = index_col`), and the regular columns are just the unique `columns_col` value-stringifications. Same row count, same data — just promoted from column to index.

### `GroupedDataFrame.*` aggregations — single-column group_by promotes

Currently: `df.group_by(["category"]).sum()` returns a DataFrame with `["category", "qty_sum", "price_sum"]` columns; RangeIndex.

M43 single-column case: returns a DataFrame with `["qty_sum", "price_sum"]` columns and the unique `"category"` values as the **index** (`index_name = "category"`).

M43 multi-column case: `df.group_by(["category", "region"]).sum()` still returns ALL group keys as regular columns + the aggregated columns + RangeIndex. **No change for multi-column group_by in v1** — documented as the M44 anchor for MultiIndex.

Applies to all five GroupedDataFrame methods: `sum / mean / min / max / count`, plus `agg(specs)` and `size()` and `keys()`. (For `keys()`, single-column → a 0-column DataFrame with just the index; multi-column → today's behavior.)

### Detection mechanism for "single-column"

Read `group_keys.length()` from the `GroupedDataFrame` instance at the start of each aggregation handler; if it's 1, promote the single group-key column to the index of the output instead of inserting it as the first regular column. Otherwise behavior is unchanged.

### Commit checkpoint after Phase A

`M43 A: tabular index promotion — pivot_table + single-column group_by`. Build clean + 5+ tests covering pivot_table + single-column group_by sum/mean/agg + the multi-column-NOT-promoted contract test.

## Phase B — pivot + concat_rows + concat_cols (~250-300 LOC)

### `df.pivot(index, columns, values)`

Currently: the output's first column is named after `index` and holds the unique values; RangeIndex.

M43: the output's `index` becomes the **index** (just like `pivot_table` in Phase A). `index_name = index`.

### `tabular.concat_rows(dfs)`

Currently: returns a fresh DataFrame; RangeIndex.

M43 logic:
- If all input dfs have an index AND all indexes share dtype AND all share the same `index_name`: concatenate the indexes into the output's index (cell-by-cell concat, parallel to the column concatenation).
- If any input df has no index, or indexes have mismatched dtypes or names: output uses RangeIndex (today's behavior — document the conditions).

### `tabular.concat_cols(dfs)`

Currently: returns a fresh DataFrame; RangeIndex.

M43 logic: **lhs's index wins** (consistent with M42's merge policy). If the first df has an index, the output gets that index. If not, RangeIndex. Other dfs' indexes are ignored.

### Commit checkpoint after Phase B

`M43 B: tabular index propagation — pivot + concat_rows + concat_cols`. Build clean + tests for pivot index promotion + concat_rows happy path + concat_rows mismatched-dtype fallback + concat_cols lhs-wins.

## Phase C — melt with index repetition (~150-200 LOC)

### `df.melt(id_vars, value_vars)`

Currently: returns a DataFrame with `id_vars + variable + value` columns; output row count is `nrows × len(value_vars)`; RangeIndex.

M43 logic:
- If input has an index: the output's index is the input's index **repeated `len(value_vars)` times** (each input row's index label appears once per value_var). Pandas's default behavior, matches user expectation.
- If input has no index: output uses RangeIndex (today's behavior).

The index name is preserved. The index dtype is preserved.

### Commit checkpoint after Phase C

`M43 C: tabular melt index repetition`. Build clean + tests for melt-with-index + melt-without-index passthrough.

## Phase D — Tests + demo + LANGUAGE_GUIDE update + agent report (~200-250 LOC)

### Tests (`vm/tests/m43_tabular_index_reshape.rs`)

Aim for 18-25 tests. Cover:
- Phase A: pivot_table promotes index_col; pivot_table mean produces ColumnF64 + index preserved; single-column group_by sum/mean/min/max/count promotes key to index; single-column group_by agg(specs) promotes; multi-column group_by does NOT promote (contract test); `keys()` and `size()` behavior.
- Phase B: pivot promotes index value to index; concat_rows happy path with shared indexes; concat_rows with mismatched dtypes falls back to RangeIndex; concat_rows with mismatched names falls back; concat_cols lhs wins.
- Phase C: melt with input index produces repeated-label output index; melt without input index produces RangeIndex; melt preserves index name + dtype across repetition.

### M41/M42 test flips (likely 2-4)

Search `vm/tests/m41_tabular_index.rs` and `vm/tests/m42_tabular_index_propagation.rs` for any test asserting "pivot_table/pivot/group_by/melt/concat_rows/concat_cols output uses RangeIndex" or "drops the index". M43 flips those. List every flip in your final report.

### Demo

Add `examples/tabular_index_reshape_demo.spy` (~120 LOC) — a workflow showing end-to-end index awareness across the now-fully-index-aware tabular surface. Suggested shape:

1. Load trades CSV, `set_index("trade_id")`
2. `pivot_table("symbol", "side", "qty", "sum")` — symbol becomes index of the pivot output
3. From original trades: `group_by(["symbol"]).mean()` — symbol becomes index
4. Concat two daily-aggregated frames via `concat_rows` — date indexes concatenate
5. Melt a wide frame for plotting — input date-index repeats per metric
6. Print results, exercising `sort_index` on each indexed output

Testable via `compiler/tests/tabular_index_reshape_demo_runs.rs`.

### LANGUAGE_GUIDE.md update

Major §11.26 rewrite:
- **Preserve the index now (post-M43)**: filter, sort_by, head, tail, iloc, select, drop, rename, dropna, dropna_subset, fillna_*, merge, pivot_table, single-column group_by, pivot, melt, concat_rows (when all share index), concat_cols (lhs wins), and the 4 from M41 (sort_index, resample_index, asof_merge_index, select_by_label_*). **`tabular` is now fully index-aware for single-column indexes.**
- **Still drop the index (v1 scope, M44+ anchors)**: multi-column group_by (waits for MultiIndex). The `unique_*` / `value_counts` ops trivially don't carry an index (they return Column or 2-col frame).

Add §11.30 "melt repeats the input index per value_var" and §11.31 "concat_rows index reconciliation rules (shared dtype + name OR fallback to RangeIndex)" if these aren't covered elsewhere.

Bump banner to "post-M43".

### Commit checkpoint after Phase D

`M43 D: tabular reshape index propagation — tests + demo + LANGUAGE_GUIDE update + agent report`.

## Acceptance criteria (machine-checkable)

1. `cargo build --workspace --release` — clean, no new warnings.
2. `cargo test --release -p strictpy-vm --test m43_tabular_index_reshape` — all pass.
3. `cargo test --release -p strictpy-compiler --test tabular_index_reshape_demo_runs` — passes.
4. **No M37-M40 regressions**: targeted sweeps pass byte-identically.
5. **M41/M42 mostly unchanged** except for the tests you flip — list every flip.
6. **Full sweep**: 868 + N - K passing (N new M43 tests, K flipped M41/M42 tests). Should net at least 868 + 15.

## Constraints — files NOT to modify

- `seed_prelude` in `compiler/src/resolver.rs`.
- M37-M40 tests — must keep passing untouched.
- Only flip M41/M42 tests that explicitly assert "index is dropped on [pivot_table/pivot/group_by/melt/concat]" — document every flip.
- The 6 existing tabular demos — add a separate `tabular_index_reshape_demo.spy`.

## STOP CRITERIA — priority drops if budget runs out

Five priority drops, in order:

1. **Drop Phase C (melt)** — the most architecturally weird piece (output row count differs from input). Ships A+B without melt; M44 picks up.
2. **Drop Phase B `concat_cols`** — least-used; the lhs-wins rule can be retrofitted later.
3. **Drop multi-column group_by detection** in Phase A — make all group_by aggregations still drop their keys to columns (today's behavior); ship only pivot_table + pivot index promotion. M44 takes both single + multi.
4. **Drop the LANGUAGE_GUIDE rewrite** — orchestrator finishes it.
5. **Drop the demo** — orchestrator extends an existing one.

After applying any drop, document what was cut with a "what M44 should pick up" list.

## Methodology discipline

1. **First commit at ~20% of budget** — Phase A's `pivot_table` index promotion + 1 test.
2. **Per-phase commits** — 4 commits (A, B, C, D). M43 phases ARE disjoint (different handlers), so per-phase is the right cadence (like M42, not M41).
3. **Variable prefix `m43_`** for any new helpers.
4. **No new IR opcodes** — pure handler-body changes.
5. **Edit-tool worktree leak workaround**: M40 narrowing (Edit-on-existing-files leaks; Write doesn't) holds across 6 milestones now. After first round of Edits per phase, check `git status`; `cp` if needed.

## Final report

Write `docs/thesis/agent_reports/m43_tabular_index_reshape.md` (under 500 words) covering:
- What shipped per phase (A-D)
- What was cut from STOP CRITERIA (if anything)
- LOC delta per touched file
- Final test count + verification + list of M41/M42 tests flipped
- Surprises / design calls (e.g., how did you detect single-column-vs-multi-column group_by? did concat_rows index reconciliation hit edge cases? did melt's index repetition handle null-keyed input rows correctly?)
- "What M44 should pick up" — MultiIndex is the headline; df.loc range; outer-merge dtype-mismatch MultiIndex fallback; multi-column group_by promotion to MultiIndex
- LANGUAGE_GUIDE.md update status
- Whether the Edit-tool worktree leak recurred (yes/no, count, mitigation effectiveness)

Commit this report in Phase D's commit.

## Commit message shape (final)

```
M43: tabular index propagation through reshape + group_by + pivot_table

Closes the v1 single-index propagation story. After M43, the
`tabular` package is fully index-aware end-to-end for single-
column indexes (multi-column / MultiIndex is M44+).

Phase A: pivot_table promotes index_col to the result's index;
  single-column group_by + agg/sum/mean/min/max/count promotes
  the group-key column to the index (multi-column unchanged
  pending M44 MultiIndex).
Phase B: pivot promotes index value to index; concat_rows
  concatenates input indexes when they share dtype+name (else
  RangeIndex fallback); concat_cols takes lhs's index.
Phase C: melt repeats input index per value_var (pandas default).
Phase D: ~20 new tests + reshape demo + LANGUAGE_GUIDE §11.26
  rewrite (now "fully index-aware for single-column indexes") +
  agent report.

NativeFn IDs unchanged (M43 modifies existing handlers; no new
methods). Variable prefix m43_.

Tests: 868 → 868 + N - K (N new, K M41/M42 flipped).
```
