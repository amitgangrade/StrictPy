# M46 — `tabular` stack/unstack + df.loc range + outer-merge MultiIndex + time-series MI + pivot_table extensions

## Context

M45 closed the v1 MultiIndex propagation story — every M42/M43 op now propagates MultiIndex correctly. The remaining `tabular` work from the M45 agent's "M46 pick-up list" falls into 5 disjoint pieces, each its own phase:

1. **stack / unstack** — pandas's MultiIndex bread-and-butter. `stack` rotates columns into a new innermost MultiIndex level; `unstack` does the reverse. Net-new code.
2. **`df.loc` range-by-label** — extends M41's `select_by_label_*` (one row) to range support (`df.loc["a":"c"]` shape). Per-dtype on the index.
3. **Outer-merge MultiIndex fallback** — replaces M42's RangeIndex fallback for dtype-mismatched outer joins with a NaN-padded MultiIndex (matches pandas).
4. **Time-series ops MultiIndex handling** — `resample` / `asof_merge` / `resample_index` / `asof_merge_index` are single-col-index-only today. Most will doc'd-drop or raise on MultiIndex inputs; `asof_merge` may propagate via the same lhs-wins pattern as merge.
5. **set_index 1-element list + pivot_table(aggfunc=list, margins=True)** — small ergonomics.

After M46 the `tabular` v1 surface is functionally complete except for v0.4 polish items (rolling Welford std, categorical column dtype, `df.iloc[rows, cols]` 2-D indexing, etc.).

You are the **28th** of an unbroken Lesson-1-compliant agent streak (M28 → M45). M46 phases are disjoint — first commit at ~20% of budget (matching M42/M43/M45's pattern).

## Files to read FIRST (in order)

1. `LANGUAGE_GUIDE.md` (project root) — §5 tabular subsection M37-M45 + §11.26-§11.32
2. `docs/thesis/agent_reports/m45_tabular_multiindex_propagation.md` — the recipe pattern + the leak-hypothesis refinement
3. `docs/thesis/agent_reports/m44_tabular_multiindex.md` — MultiIndex storage + `m44_build_df_with_multiindex` constructor
4. `examples/tabular_multiindex_propagation_demo.spy` — M45's end-to-end demo
5. `vm/src/builtins.rs` — find:
   - `m44_build_df_with_multiindex` + `m41_build_df_with_index` constructors
   - `m44_permute_multiindex_into_df` + `m45_copy_multiindex_into_df` helpers
   - `m45_merge_build_multiindex` (you'll extend the outer-join fallback)
   - `m40_df_resample` + `m40_df_asof_merge` + `m41_df_resample_index` + `m41_df_asof_merge_index` (you'll add MultiIndex handling)
   - `m41_df_set_index` + `m44_df_set_index_multi` (you'll unify the 1-element-list case)
   - `m41_df_pivot_table` (you'll extend aggfunc-list + margins)
   - `m41_df_select_by_label_*` (M41's one-row label lookup — you'll add range variants)
6. `vm/tests/m44_tabular_multiindex.rs` + `vm/tests/m45_tabular_multiindex_propagation.rs` — test file patterns
7. `compiler/src/resolver.rs` — search `m45_` for the most-recent registration patterns (and `m41_df_select_by_label_*` registration block for the loc_range pattern)

## Constraints

- **Lesson 1**: first commit at ~20% of budget. 27-streak — don't break it.
- **Variable prefix `m46_`** for any new helpers / locals in shared files.
- **NativeFn IDs 1033-1064** reserved (30 slots from the M44-era reserve). M46 expected to use ~8-12 new ones.
- **No new classes**, no new crate deps, no payload changes.
- **No changes to existing method signatures** — every existing method keeps its public surface. New methods are net-additive.
- All 224 existing tabular tests must keep passing — except any tests asserting the old outer-merge-RangeIndex-fallback behavior (those flip to MultiIndex fallback). List every flip.

### Edit-tool worktree leak — defensive measure

M45 confirmed the workaround was likely redundant when worktree starts in sync with project root. **Still run the precautionary `cp` block at session start as a defensive measure** — it's cheap, and if Bash is denied (as in M45) you'll know to be vigilant if any leak symptoms appear:

```bash
for f in vm/src/builtins.rs compiler/src/resolver.rs compiler/src/ir.rs \
         shared/src/native.rs LANGUAGE_GUIDE.md; do
    cp /c/Users/AG/CascadeProjects/PythonCompiler/$f $f 2>/dev/null
done
```

If the leak symptoms do appear mid-session (`git status` showing project-root diffs after Edits), recover via `cp` per the established pattern.

## Phase A — stack / unstack (~500-700 LOC)

### `df.stack() -> DataFrame`

Pivots **all regular columns** into a new innermost MultiIndex level + a single "value" column. Output's MultiIndex has one more level than the input (`nlevels + 1`). The new innermost level's values are the original column names; the value column's name is the stringified concatenation of the original column names with `,` (or just `"value"` as a v1 simplification — your call, document).

**Constraint**: all regular columns must share a dtype (else raise ValueError). Same restriction as `melt(value_vars)`.

**Behavior on input index**:
- No index: output gets a RangeIndex on the original-row dimension + the new innermost level for the column names → producing a single-col index (the column-name level).
- Single-col index: output gets a 2-level MultiIndex (original index → original row label; new innermost → column name).
- MultiIndex: output gets an (N+1)-level MultiIndex.

### `df.unstack() -> DataFrame`

The inverse: takes the **innermost MultiIndex level** and turns it into a wide column dimension. Input must have a MultiIndex (raises on single-col or no-index). Output's MultiIndex has one fewer level (`nlevels - 1`); if `nlevels - 1 == 1`, the result has a single-col index (M41-shape); if `0`, RangeIndex.

The original "value" column's values get distributed across new columns named by the popped level's unique values. Missing combinations become null.

### NativeFn IDs

- `1033`: `stack` (on `DataFrame`)
- `1034`: `unstack` (on `DataFrame`)

### Commit checkpoint after Phase A

`M46 A: tabular stack + unstack`. Build clean + 4-6 tests covering happy paths + the must-share-dtype constraint on stack + the must-have-MultiIndex constraint on unstack + a stack/unstack round-trip.

## Phase B — `df.loc` range-by-label (~300-400 LOC)

```python
df.loc_range_i64(start: i64, stop: i64) -> DataFrame      # inclusive both ends
df.loc_range_f64(start: f64, stop: f64) -> DataFrame
df.loc_range_str(start: str, stop: str) -> DataFrame
df.loc_range_bool(start: bool, stop: bool) -> DataFrame
df.loc_range_datetime(start: i64, stop: i64) -> DataFrame  # epoch-ms
```

Per-dtype (mirrors M41's `select_by_label_*` shape). Requires `df` to have a single-col index of the matching dtype; raises on no-index, MultiIndex (M47 follow-up), or dtype mismatch.

**Behavior**:
- Returns rows where `start <= index_label <= stop` (inclusive both ends — pandas's `.loc` semantics).
- Preserves the order of the original frame (does NOT sort).
- The returned frame keeps the (single-col) index containing only the labels in range.
- Empty range (no labels match) → empty frame with the same column schema.

### NativeFn IDs

- `1035-1039`: `loc_range_i64` / `_f64` / `_str` / `_bool` / `_datetime`.

### Commit checkpoint after Phase B

`M46 B: tabular df.loc range-by-label per dtype`. Build clean + 5 tests covering each dtype + happy-path + empty-range + raise-on-MultiIndex.

## Phase C — Outer-merge MultiIndex fallback + set_index unification + pivot_table extensions (~350-450 LOC)

### Outer-merge MultiIndex fallback

M42's `merge` (and M45's `m45_merge_build_multiindex`) currently falls back to RangeIndex when outer-joining with dtype-mismatched single-col indexes (e.g. lhs has `ColumnI64` index, rhs has `ColumnStr` index). M46 replaces with a **NaN-padded 2-level MultiIndex**: level 0 is the lhs key column (with NaN where lhs has no match), level 1 is the rhs key column (with NaN where rhs has no match). Level names are `lhs.index_name()` and `rhs.index_name()` (or fallback `"lhs"` / `"rhs"`).

This matches pandas's outer-merge-with-mismatched-indexes behavior.

### set_index 1-element list unification

```python
df.set_index_list(cols: List[str]) -> DataFrame
# If len(cols) == 1: behaves like set_index(cols[0]) — single-col index
# If len(cols) >= 2: behaves like set_index_multi(cols) — MultiIndex
# If len(cols) == 0: raises ValueError
```

This is the ergonomic unification pandas users expect. Existing `set_index(name)` and `set_index_multi(cols)` keep working unchanged; `set_index_list` is the new convenience.

### pivot_table extensions

```python
df.pivot_table_aggfunc_list(index_col: str, columns_col: str,
                            values_col: str, aggfuncs: List[str]) -> DataFrame
# Same as pivot_table but emits one set of value columns per aggfunc.
# Output column name shape: "{columns_value}_{aggfunc}" (e.g., "north_sum",
# "north_mean", "south_sum", ...). aggfunc names: "sum"/"mean"/"min"/"max"/"count".

df.pivot_table_margins(index_col: str, columns_col: str, values_col: str,
                       aggfunc: str) -> DataFrame
# Same as pivot_table but adds a trailing "All" row + "All" column with
# the aggfunc applied to the full row/column slice. The intersecting
# cell is the aggfunc over the whole values column.
```

Each is a new method; users opt in. The vanilla `pivot_table` stays unchanged.

### NativeFn IDs

- `1040`: `set_index_list`
- `1041`: `pivot_table_aggfunc_list`
- `1042`: `pivot_table_margins`

The outer-merge fallback is internal to `m39_df_merge` / `m45_merge_build_multiindex` — no new NativeFn needed.

### Commit checkpoint after Phase C

`M46 C: tabular outer-merge MultiIndex fallback + set_index_list + pivot_table aggfunc-list + margins`. Build clean + tests covering each.

## Phase D — Time-series ops MultiIndex handling (~150-200 LOC)

Mostly **explicit behavior choices and documentation**, not new code:

- **`resample(time_col, rule, agg)`**: explicitly **drops MultiIndex** (reshapes the row dimension — no clean target). Same shape as M45's pivot/pivot_table decision. Document in §11.32.
- **`resample_index(rule, agg)`**: same — drop MultiIndex if input has one (uses index implicitly; MultiIndex's row dimension gets reshaped). Document.
- **`asof_merge(other, on_self, on_other)`**: same shape as M42's merge — left-join semantics, lhs MultiIndex wins. Extend by routing through M45's MultiIndex-aware merge emit path.
- **`asof_merge_index(other)`**: same — lhs MultiIndex preserved. Adjust the existing handler if needed.

### NativeFn IDs

None new — these are behavior changes to existing handlers.

### Commit checkpoint after Phase D

`M46 D: tabular time-series ops MultiIndex handling`. Build clean + 4 tests (resample-drops-MI, resample_index-drops-MI, asof_merge-preserves-lhs-MI, asof_merge_index-preserves-lhs-MI).

## Phase E — Tests + demo + LANGUAGE_GUIDE update + agent report (~250-300 LOC)

### Tests (`vm/tests/m46_tabular_extensions.rs`)

Aim for 22-30 tests. Cover:
- Phase A: stack happy path + must-share-dtype raise + unstack on 2-level MultiIndex + unstack on 3-level MultiIndex + round-trip stack(unstack(x)) ≅ x for well-formed input.
- Phase B: loc_range per dtype (5 tests) + empty range + raise on MultiIndex.
- Phase C: outer-merge dtype-mismatch produces 2-level NaN-padded MultiIndex (replaces RangeIndex fallback); set_index_list with 1 element → single-col; set_index_list with N elements → MultiIndex; set_index_list with empty → ValueError; pivot_table_aggfunc_list with 2 aggfuncs produces twice the value-columns; pivot_table_margins adds "All" row + column.
- Phase D: resample drops MultiIndex; asof_merge preserves lhs MultiIndex.

### Tests to flip

Search for any test asserting "outer-merge with dtype-mismatch produces RangeIndex". M45 likely has one in `m45_tabular_multiindex_propagation.rs::merge_outer_dtype_mismatch_falls_back_to_range_m46_anchor` or similar. M46 flips it.

### Demo

Add `examples/tabular_m46_extensions_demo.spy` (~120 LOC) — a workflow exercising stack/unstack + loc_range:
1. Load wide sales CSV (months as columns)
2. `set_index_list(["region"])` (1-element-list unification)
3. `stack()` to get long-form with (region, month) MultiIndex
4. `unstack()` to round-trip
5. `loc_range_str("east", "south")` to filter by index
6. `pivot_table_aggfunc_list("category", "month", "total", ["sum", "mean"])`
7. Print intermediate frames with `index_nlevels()` checks

Testable via `compiler/tests/tabular_m46_extensions_demo_runs.rs`.

### LANGUAGE_GUIDE.md update

§5 tabular gets an "M46 additions" subsection covering stack/unstack + loc_range + set_index_list + pivot_table extensions + outer-merge MultiIndex fallback. §11.32 rewrite: time-series ops now have explicit MultiIndex policy (asof_merge preserves; resample drops). §11.33-§11.34 (new): stack must-share-dtype constraint, unstack must-have-MultiIndex constraint.

Bump banner to post-M46.

### Commit checkpoint after Phase E

`M46 E: tabular M46 extensions — tests + demo + LANGUAGE_GUIDE update + agent report`.

## Acceptance criteria (machine-checkable)

1. `cargo build --workspace --release` — clean, no new warnings.
2. `cargo test --release -p strictpy-vm --test m46_tabular_extensions` — all pass.
3. `cargo test --release -p strictpy-compiler --test tabular_m46_extensions_demo_runs` — passes.
4. **No M37-M44 regressions**: targeted sweeps pass byte-identically.
5. **M45 mostly unchanged** except for any outer-merge-RangeIndex-fallback assertions flipped to MultiIndex fallback. List every flip.
6. **Full sweep**: 934 + N - K (N new M46, K flipped M45). Net should be at least 934 + 18.

## Constraints — files NOT to modify

- `seed_prelude` in `compiler/src/resolver.rs`.
- M37-M44 tests — must keep passing untouched.
- Only flip M45 tests that explicitly assert the old outer-merge-with-dtype-mismatch RangeIndex fallback — document every flip.
- The 9 existing tabular demos — add a separate `tabular_m46_extensions_demo.spy`.

## STOP CRITERIA — priority drops if budget runs out

Five priority drops, in order (cut from the BOTTOM — keep stack/unstack):

1. **Drop `pivot_table_margins`** in Phase C — keep `pivot_table_aggfunc_list`. Margins is the bulkiest single small piece.
2. **Drop `pivot_table_aggfunc_list`** in Phase C — keep set_index_list + outer-merge fallback.
3. **Drop Phase D entirely** — leave time-series ops dropping MultiIndex as today (already documented as M45's M46-anchor item; the documentation update can roll into the demo).
4. **Drop Phase B (`df.loc` range)** — biggest single drop after stack/unstack. Per-dtype methods are 5 NativeFns; M47 picks up.
5. **Drop `unstack`** — keep `stack` only (stack alone is still useful; unstack requires MultiIndex which has its own complexity).

After applying any drop, document what was cut with a "what M47 should pick up" list.

## Methodology discipline

1. **First commit at ~20% of budget** — Phase A's stack + 1 test. M46 is disjoint-phase work (each phase modifies independent handlers or adds independent methods).
2. **Per-phase commits** — 5 commits (A, B, C, D, E). Each builds clean.
3. **Variable prefix `m46_`** for any new helpers.
4. **No new IR opcodes** — pure handler bodies + new NativeFn registrations.
5. **Edit-tool worktree leak**: run the precautionary `cp` block at session start as defensive measure. M45's hypothesis (leak triggers on worktree-divergence-at-session-start) suggests this might be redundant if worktree starts in sync — but it's cheap insurance.

## Final report

Write `docs/thesis/agent_reports/m46_tabular_extensions.md` (under 600 words) covering:
- What shipped per phase (A-E)
- What was cut from STOP CRITERIA (if anything)
- LOC delta per touched file
- Final test count + verification + list of M45 tests flipped (old → new)
- Surprises / design calls (e.g., how did you handle stack's null-when-missing-combination cells? did unstack's level-popping have edge cases at nlevels=1 vs nlevels=N?)
- "What M47 should pick up" — concrete list (rolling Welford std, categorical dtype, df.iloc 2-D, negative iloc, 1w/1M/1Y resample, plus anything from M46 STOP CRITERIA)
- LANGUAGE_GUIDE.md update status
- Whether the Edit-tool worktree leak recurred (yes/no — and did you run the precautionary cp?)

Commit this report in Phase E's commit.

## Commit message shape (final)

```
M46: tabular stack/unstack + df.loc range + outer-merge MultiIndex fallback + time-series MI + extensions

Closes the M45 "what M46 should pick up" list:

Phase A: stack (columns → innermost MultiIndex level) +
  unstack (innermost MultiIndex level → columns). Both require
  shared-dtype across columns / proper MultiIndex on input;
  raise otherwise.
Phase B: df.loc_range_{i64,f64,str,bool,datetime}(start, stop)
  inclusive — extends M41's select_by_label_* one-row lookup to
  ranges, single-col index only.
Phase C: outer-merge dtype-mismatch now produces a NaN-padded
  2-level MultiIndex (replaces M42's RangeIndex fallback);
  set_index_list unifies set_index + set_index_multi via length
  dispatch; pivot_table_aggfunc_list + pivot_table_margins
  extensions.
Phase D: time-series ops MultiIndex handling — resample +
  resample_index drop MultiIndex (reshape row dim); asof_merge +
  asof_merge_index preserve lhs MultiIndex.
Phase E: ~25 new tests + tabular_m46_extensions_demo.spy +
  LANGUAGE_GUIDE.md §11.32 rewrite + §11.33/§11.34 new + agent
  report.

NativeFn IDs 1033-1042 (10 new). Variable prefix m46_.
Tests: 934 → 934+N-K (N new, K flipped from M45).

After M46 the tabular v1 surface is functionally complete except
for v0.4 polish items (rolling Welford std, categorical column,
iloc 2-D, negative iloc, 1w/1M/1Y resample rules, desktop UI).
```
