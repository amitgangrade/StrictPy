# M39 — `tabular` Phase 4: reshape (pivot / melt / merge / concat / unique / value_counts)

## Context

M37 + M38 shipped Phases 1-3 of the Pandas plan: Column/DataFrame/IO/filter/sort + aggregations + group-by. M39 ships Phase 4 — reshape operations. After M39 the `tabular` module covers the common-80% of Pandas workflows. M40 will handle time series (Phase 5); the desktop UI (Phase 6) follows.

You are the **21st** of an unbroken Lesson-1-compliant agent streak (M28 → M38). M37 + M38 each delivered ~2500-2800 LOC across 5 clean phase commits without breaking the streak. M39 is similar scope, organized as 4 phases (one fewer than M37/M38).

## Files to read FIRST (in order)

1. `LANGUAGE_GUIDE.md` (project root) — §5 `tabular (M37, extended by M38)` subsection + §6.2 + §11.18 + §11.19
2. `docs/thesis/agent_reports/m38_tabular_ops.md` — the M38 design (especially the hash-based group-by + the four findings, including the **Edit-tool worktree leak** — see "Methodology" below)
3. `examples/tabular_demo.spy` + `examples/tabular_groupby_demo.spy` — current demos
4. `compiler/src/resolver.rs` — search for `m37_register_tabular` and the M38 additions to that function; you will extend it
5. `compiler/src/ir.rs` — search for `m37_tabular_class_method_native_id_by_name` and the M38 extensions; you will add `m39_*` dispatch arms in the same shape
6. `vm/src/builtins.rs` — search for `m38_groupby_build` and `m38_agg_dispatch` (Phase D's hash-based group-by) — the same hashing/bucketing machinery is what `merge` will use for hash-joins
7. `shared/src/native.rs` — search for `M38Tab` to see the NativeFn block style; you will add an M39 block starting at id 935
8. `vm/tests/m38_tabular_ops.rs` — test file shape for your new `vm/tests/m39_tabular_reshape.rs`

## Constraints

- **Lesson 1**: first commit at ~20% of budget (Phase A green build + 1 smoke test). 20-streak — don't break it.
- **Variable prefix `m39_`** for all new helper functions / locals in shared files.
- **NativeFn IDs 935-984** (50 slots reserved). M38 used 880-934.
- **Use the M36 `StdlibItemKind::Class` path** if you need any new classes. Don't touch `seed_prelude`.
- **No new crate deps**.
- **No changes to the M37/M38 surface** other than additions. All 48 existing M37+M38 tests (21+25 vm + 2+2 demo-runs) must continue to pass byte-identically.
- **Edit-tool worktree leak**: this happened in M37 + M38. When you use the Edit tool, occasionally writes land in the project-root copy of files instead of the worktree. If you notice `git status` showing diffs in the project root, that's the leak. Recovery: `cp -r` from your worktree's copy to project root, OR just keep working in the worktree — the orchestrator will checkout-and-merge-ff against the worktree HEAD on integration regardless. Don't burn time fighting it; just deliver on your branch.

## Phase A — `unique`, `value_counts`, `concat_rows`, `concat_cols` (~400-500 LOC)

### Per-Column-type `unique` (5 NativeFns)

```python
# Returns a Column of distinct non-null values in encounter order.
# (Pandas pd.unique preserves first-occurrence order; we match.)
df.unique_i64(col: str) -> ColumnI64?      # none if col absent or wrong dtype
df.unique_f64(col: str) -> ColumnF64?
df.unique_str(col: str) -> ColumnStr?
df.unique_bool(col: str) -> ColumnBool?
df.unique_datetime(col: str) -> ColumnDateTime?
```

The typed accessors mirror the M38 `get_column_*` pattern (one NativeFn per dtype because the return type is monomorphic). Null cells are excluded from the result.

### `df.value_counts(col: str) -> DataFrame`

Returns a 2-column DataFrame: the source column's values + a `count: i64` column. Sorted by count descending; ties broken by encounter order (stable). Null cells are excluded.

Output column name for values matches the source column name. The count column is named `"count"`.

### `tabular.concat_rows(dfs: List[DataFrame]) -> DataFrame`

Module-level function. Vertical concatenation (stack rows). All input dfs must have identical column schemas:
- Same column count
- Same column names in the same order
- Same column dtypes in the same order

Raise ValueError with a clear message on schema mismatch. Empty input list raises ValueError too.

Output preserves the first df's column names + dtypes. Null masks are concatenated parallel to values. Per-column nrows = sum of input nrows.

### `tabular.concat_cols(dfs: List[DataFrame]) -> DataFrame`

Module-level function. Horizontal concatenation (stitch columns side by side). All input dfs must have identical row counts. Column names must be globally unique across all input dfs (raise ValueError otherwise — no auto-rename in v1).

### Commit checkpoint after Phase A

`M39 A: tabular unique / value_counts / concat_rows / concat_cols`. Build clean + at least one smoke test exercising `unique_i64` and `concat_rows`.

## Phase B — `df.merge` (the biggest single piece, ~500-600 LOC)

```python
df.merge(other: DataFrame, on: List[str], how: str) -> DataFrame
```

- `on`: list of column names that exist in BOTH dataframes with matching dtypes. Used as join keys.
- `how`: `"inner"` | `"left"` | `"right"` | `"outer"`. Raise ValueError for any other value.

### Output schema

- All columns of `self`, in order
- Then all columns of `other` that are NOT in `on`, in order
- No duplicate column names (the `on` columns are NOT repeated)

### Algorithm — hash join

Reuse the M38 group-by hashing machinery: serialize the `on` columns of each row into a `\x01`-joined key string.

1. **Build phase**: scan `other`, build a `Dict[str, List[i64]]` mapping each key → row indices in `other`.
2. **Probe phase**: scan `self`, look up each row's key in the map.
   - `inner`: emit `len(self_matches) × len(other_matches)` rows for matched keys; skip unmatched left rows
   - `left`: emit matched rows as inner; emit unmatched left rows with right-side columns = null
   - `right`: same as `left` but with `self` and `other` swapped (or implement directly — your call)
   - `outer`: left + any right rows whose keys never appeared in self (right-side columns from `other`, left columns null)

### Null handling in join keys

A row with `null` in any `on` column does NOT match anything (matches pandas's `null != null` SQL semantics). Such rows in `self`:
- `inner`/`right`: dropped
- `left`/`outer`: emitted with right side null

### Commit checkpoint after Phase B

`M39 B: tabular merge — hash join inner/left/right/outer`. Build clean + tests covering all four how values on a small dataset.

## Phase C — `df.pivot` + `df.melt` (~400-500 LOC)

### `df.pivot(index: str, columns: str, values: str) -> DataFrame`

- `index`: column name whose unique values become row labels
- `columns`: column name whose unique values become NEW column headers
- `values`: column name whose values fill the cells

Output: one row per unique `index` value; columns = `index` + one column per unique `columns` value; cell `(i, c)` = the `values` cell from the source row where `index == i and columns == c` (null if no such row exists; ValueError if multiple such rows exist — duplicate (index, columns) pairs).

The output column names for the pivot are stringified versions of the unique `columns` values (use the M37 stringify logic).

The pivoted value columns inherit the dtype of the source `values` column.

### `df.melt(id_vars: List[str], value_vars: List[str]) -> DataFrame`

- `id_vars`: column names to keep as identifier columns (zero or more)
- `value_vars`: column names to unpivot. Must all have the same dtype; raise ValueError otherwise.

Output: `len(id_vars) + 2` columns × `nrows × len(value_vars)` rows. The two new columns are:
- `variable`: str column with the column name being unpivoted
- `value`: same dtype as `value_vars` columns

Output row `(i, v)` (zero-indexed over id_var-rows × value_vars) has:
- id_vars columns = source row `i`'s id_vars
- `variable` = name of `value_vars[v]`
- `value` = source row `i`'s value in `value_vars[v]`

### Commit checkpoint after Phase C

`M39 C: tabular pivot + melt`. Build clean + tests for each.

## Phase D — tests + demo + docs (~250-300 LOC)

### Tests (`vm/tests/m39_tabular_reshape.rs`)

Aim for 22-28 tests. Cover:
- Phase A: `unique` on each dtype (i64, f64, str, bool); `value_counts` with ties; `concat_rows` happy path + schema mismatch error; `concat_cols` happy path + row-count mismatch error
- Phase B: `merge` inner/left/right/outer on a small 2-table dataset; null join keys; unmatched rows; output schema correctness
- Phase C: `pivot` happy path; pivot with missing cells (null fill); pivot duplicate-key error; `melt` with multiple value_vars; melt dtype-mismatch error

### Demo

Add `examples/tabular_reshape_demo.spy` (~100-150 lines) — a realistic walkthrough that combines pivot/merge/concat:
1. Load two CSVs (orders + customers)
2. `merge` on `customer_id` (left join)
3. Filter
4. `pivot` to a category × month sales matrix
5. Print

Testable via `compiler/tests/tabular_reshape_demo_runs.rs`.

### LANGUAGE_GUIDE.md update

Extend §5 `tabular (M37, extended by M38, M39)` with the new operations. Add a sub-block per phase. Add §11 entries if there are gotchas (e.g., "null keys never match in merge", "pivot raises on duplicate keys"). Bump banner to "post-M39".

### Commit checkpoint after Phase D

`M39 D: tabular reshape — tests + demo + LANGUAGE_GUIDE update + agent report`.

## Acceptance criteria (machine-checkable)

1. `cargo build --workspace --release` — clean, no new warnings.
2. `cargo test --release -p strictpy-vm --test m39_tabular_reshape` — all pass.
3. `cargo test --release -p strictpy-compiler --test tabular_reshape_demo_runs` — passes.
4. **No M37/M38 regressions**: targeted M37/M38 sweeps all pass byte-identically.
5. **Full sweep**: `cargo test --workspace --release --no-fail-fast` reports 769 + N passing where N = the new M39 test count (target 22-28), 0 failing, 1 ignored.

## Constraints — files NOT to modify

- `seed_prelude` in `compiler/src/resolver.rs`
- The 48 existing M37+M38 test cases — they must keep passing untouched
- `examples/tabular_demo.spy`, `examples/tabular_groupby_demo.spy` — add a separate `tabular_reshape_demo.spy`

## STOP CRITERIA — priority drops if budget runs out

Five priority drops, in order:

1. **Drop `melt`** — keep `pivot`. Wide-to-long is less common than long-to-wide.
2. **Drop `merge(how="outer")`** — keep inner / left / right. Outer is the most code-heavy variant.
3. **Drop `concat_cols`** — keep `concat_rows`. Row-wise concat is more common.
4. **Drop `value_counts`** — `unique` + `Column.length()` is a workaround.
5. **Drop LANGUAGE_GUIDE.md update** — orchestrator finishes it in 5 min.

After applying any drop, document what was cut in the agent report with a "what M40 should pick up" list.

## Methodology discipline

1. **First commit at ~20% of budget** (Phase A green build + smoke test).
2. **Per-phase commits** — 4 commits expected (A, B, C, D). Each builds clean.
3. **Variable prefix `m39_`** in shared files.
4. **Name-based dispatch in `ir.rs`** (mirror `m37_tabular_class_method_native_id_by_name`); do NOT add new IR opcodes.
5. **Edit-tool worktree leak workaround**: if you see `git status` modifications in the project root vs. your worktree, the orchestrator's recovery is already documented — don't try to "fix" it via destructive operations. Just keep your worktree commits clean.

## Final report

Write `docs/thesis/agent_reports/m39_tabular_reshape.md` (under 600 words) covering:
- What shipped per phase (A-D)
- What was cut from STOP CRITERIA (if anything)
- LOC delta per touched file
- Final test count + verification
- Surprises / design calls
- "What M40 should pick up" — concrete follow-up list (DatetimeIndex? rolling windows? resample? categorical columns?)
- LANGUAGE_GUIDE.md update status
- Whether the Edit-tool worktree leak recurred (yes/no)

Commit this report in Phase D's commit.

## Commit message shape (final)

```
M39: tabular reshape — pivot / melt / merge / concat / unique / value_counts

Phase 4 of the Pandas-shaped data package. After M37+M38 shipped
core types + filter/sort + aggregations + group-by, M39 closes the
common-80% of Pandas workflows.

Phase A: unique_* per dtype + value_counts + concat_rows + concat_cols.
Phase B: merge with inner/left/right/outer hash-join (reuses M38
group-by hashing machinery).
Phase C: pivot (long→wide) + melt (wide→long).
Phase D: 22-28 new tests + tabular_reshape_demo.spy + LANGUAGE_GUIDE
update.

NativeFn IDs 935-984. Variable prefix m39_.
Tests: 769 → 769+N. Examples: +1 (tabular_reshape_demo.spy).
```
