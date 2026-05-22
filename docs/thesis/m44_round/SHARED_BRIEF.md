# M44 — `tabular` MultiIndex (storage + multi-col group_by promotion + minimal propagation)

## Context

M41 added an optional single-column index to `DataFrame` (`index: Column? + index_name: str?`). M42 propagated it through 11 row/column-transforming methods. M43 finished the v1 single-index story through the reshape side (pivot_table, single-column group_by, pivot, melt, concat).

**The headline missing piece is MultiIndex** — pandas's nested indices that let you `group_by([col1, col2])` and get the group keys as a structured row label rather than as regular columns. M43's multi-column `group_by` retains today's keys-as-columns shape because there was no MultiIndex to promote to.

M44 ships MultiIndex. **Scope-down for this milestone (M44a)**:

- **Storage + accessors**: DataFrame gains optional `index_levels: List[Column]? + index_names: List[str]?` alongside the existing `index: Column? + index_name: str?` from M41. A frame can have one or the other or neither (RangeIndex), but not both — `set_index` clears any MultiIndex, `set_index_multi` clears any single-col index.
- **Multi-column group_by promotion**: `df.group_by([col1, col2]).{sum/mean/min/max/count/agg/size/keys}` now promotes ALL group-key columns to a MultiIndex on the result. (Single-column group_by still promotes to single-col index per M43.)
- **Minimal propagation**: only through `filter` / `head` / `tail` / `iloc` — the most commonly-chained ops after group_by. Other ops (sort_by / dropna / fillna_* / merge / select / drop / rename / pivot / melt / concat) **drop the MultiIndex** in M44a, reverting to RangeIndex. **Document this explicitly as the M44b anchor.**

**Out of scope (M44b later)**:
- Full MultiIndex propagation through sort_by / dropna_subset / fillna_* / merge / select / drop / rename / pivot / melt / concat_*.
- `stack` / `unstack` — pandas's MultiIndex bread-and-butter.
- `df.loc[label_list]` / range-by-label.
- Outer-merge MultiIndex fallback (replaces M42's current RangeIndex fallback for dtype-mismatched indexes).

You are the **26th** of an unbroken Lesson-1-compliant agent streak (M28 → M43). M44 is **shared-infrastructure-heavy** (the new payload field + new helper functions are used by every phase). Per the M41/M42/M43 cadence trilogy: this is a **shared-infra** milestone, so expect Phase A to combine into one larger commit at ~30-50% of budget rather than ~20%. That's fine — the Lesson 1 SPIRIT (commit before orchestrator intervenes, green build + tests passing at each commit) is what matters.

## Files to read FIRST (in order)

1. `LANGUAGE_GUIDE.md` (project root) — §5 `tabular` subsection (M37-M43 additions) + §6.2 + §11.26-§11.31
2. `docs/thesis/agent_reports/m43_tabular_index_reshape.md` — especially the "two methodology data points" (test-flip cascade is 9; Edit-tool leak is broader than M40 narrowing claimed)
3. `docs/thesis/agent_reports/m41_tabular_index.md` — the 24→40 byte DataFrame payload growth + GC implications (you're doing 40→~56)
4. `docs/thesis/agent_reports/m42_tabular_index_propagation.md` — the `m42_permute_index_into_df` recipe pattern (you'll write a sibling `m44_permute_multiindex_into_df` for the 4 critical-propagation handlers)
5. `examples/tabular_index_reshape_demo.spy` — M43's end-to-end demo
6. `compiler/src/resolver.rs` — find DataFrame's class layout; you'll add `index_levels: List[Column]? + index_names: List[str]?` fields
7. `vm/src/builtins.rs` — find:
   - `m37_build_df` + `m41_build_df_with_index` — the constructors. You'll add `m44_build_df_with_multiindex`.
   - `m42_permute_index_into_df` + `m42_copy_index_into_df` — the recipe pattern.
   - The 4 row-selection handlers you'll modify in Phase C: `m37_df_filter`, `m37_df_head`, `m37_df_tail`, `m40_df_iloc`.
   - The group_by family in `m38_*` — you'll modify the 5 aggregation handlers + agg + size + keys for multi-column key promotion.
8. `shared/src/native.rs` — find the M43 NativeFn range; M44 likely needs a few new methods (set_index_multi / reset_index_multi / index_nlevels / index_level / index_level_name / sort_index_multi = 6 NativeFns).
9. `vm/tests/m41_tabular_index.rs` + `vm/tests/m43_tabular_index_reshape.rs` — test file patterns. Especially the M43 multi-column-group_by-NOT-promoted contract tests (those will need to flip).

## Constraints

- **Lesson 1**: first commit at ~30-50% of budget is acceptable for this shared-infra milestone — see "Methodology discipline" below.
- **Variable prefix `m44_`** for all new helpers / locals in shared files.
- **NativeFn IDs `1027–1050`** reserved (50 slots from the M40-era reserve). M44a expected to use ~10-15 (6 new methods + a few internal).
- **No new classes**, no new crate deps.
- **No changes to existing method signatures** — every existing method keeps its public surface. Behavior may change for MultiIndex-input cases.
- The 144 existing tabular tests must continue to pass — EXCEPT:
  - M43's multi-column-group_by-NOT-promoted contract test will flip (now promoted).
  - Any M38 multi-column group_by tests that assert keys-as-columns will flip.
  - List every flipped test in the final report with old vs new assertion.

### Edit-tool worktree leak — broader than M40 claimed

M43 confirmed the leak hits `Write` of new files too, not just `Edit` of existing. Total recovery time scaled with edit volume: M40 ~2 min, M41 ~30s, M42 ~5s, **M43 ~90s across ~15 cp recoveries**. **M44 mitigation**: at session start, do a **precautionary `cp` of all shared files** from project root to worktree:

```bash
# Worktree mitigation — run once at the start of your session:
for f in vm/src/builtins.rs compiler/src/resolver.rs compiler/src/ir.rs \
         shared/src/native.rs LANGUAGE_GUIDE.md; do
    cp /c/Users/AG/CascadeProjects/PythonCompiler/$f $f 2>/dev/null
done
```

Then after each phase's bulk Edits, check `git status`. If any project-root copies differ from your worktree, `cp` them again. Defensive copy is cheap; per-phase discovery is not.

## Phase A — MultiIndex storage + accessors + sort_index_multi (~500-700 LOC)

### DataFrame layout change

Grow the DataFrame payload to carry MultiIndex storage:

```
DataFrame:
  names: List[str]
  columns: List[Column]
  nrows: i64
  index: Column?              # M41 — single-col index
  index_name: str?            # M41
  index_levels: List[Column]?  # M44 — NEW. None = no MultiIndex.
  index_names: List[str]?     # M44 — NEW. None = no MultiIndex.
```

The two index representations are **mutually exclusive** at any given moment: a frame can have a single-col index (M41 path), or a MultiIndex (M44 path), or neither (RangeIndex). Constructors and transforms enforce this invariant.

Payload size guidance: today 40 bytes. Adding 2 pointer slots (16 bytes) → 56 bytes. GC implications same as M41 (zero slots treat as non-pointers via M11 false-positive analysis; benign).

You'll need to update the same 3 constructors M41 updated (`m37_from_columns`, `m37_from_rows`, `m37_build_df`) to allocate 56 bytes and zero the new trailing slots.

### New surface

```python
# Multi-column index manipulation:
df.set_index_multi(cols: List[str]) -> DataFrame
# Removes col1..colN from columns, makes them the new MultiIndex levels.
# Raises ValueError if any col is absent, if cols is empty, or if df
# already has an index (single-col OR multi).

df.reset_index_multi() -> DataFrame
# Removes the MultiIndex; re-inserts each level as a regular column at
# the appropriate position, named by index_names[i]. Returns RangeIndex.
# No-op if no MultiIndex.

# Accessors:
df.index_nlevels() -> i64
# 0 = RangeIndex. 1 = single-col index (M41). N = MultiIndex with N levels.
# Replaces / supplements df.has_index() — keep has_index() working as
# "any kind of index" (returns true if nlevels >= 1).

df.index_level(i: i64) -> Column?
# Returns the i-th index level as a Column. None if i out of range or
# if df has no index. For a single-col index (nlevels=1), index_level(0)
# returns the same column as index().

df.index_level_name(i: i64) -> str?
# Returns the i-th level's name. None if out of range or no index.

# Sort by MultiIndex (lexicographic across levels):
df.sort_index_multi(ascending: bool) -> DataFrame
# Stable sort. ascending=true sorts by level 0, then level 1, etc., all
# ascending. ascending=false reverses lexicographic order. Raises if df
# has no MultiIndex (use sort_index for single-col).
```

### Commit checkpoint after Phase A

`M44 A: tabular MultiIndex — storage + set_index_multi + reset_index_multi + accessors + sort_index_multi`. Build clean + at least 3 smoke tests (round-trip set_index_multi/reset_index_multi; index_nlevels = N after set; sort_index_multi orders rows correctly).

## Phase B — Multi-column group_by promotion (~400-500 LOC)

The biggest user-visible M44a win. Today, `df.group_by([col1, col2]).sum()` returns a DataFrame with `[col1, col2, "...sum"]` columns and RangeIndex. After M44a it returns a DataFrame with the aggregated columns + a **MultiIndex** built from the (col1, col2) keys per group, in encounter order.

### Detection mechanism

Read `group_keys.length()` at the top of each aggregation handler (same pattern M43 used for single-col promotion):
- length 0 → impossible (group_by requires ≥1 key)
- length 1 → single-col index path (M43)
- length ≥ 2 → MultiIndex path (M44a) — promote all keys to index levels

### Affected handlers

All 5 group_by aggregation methods + `agg(specs)` + `size()` + `keys()`. Each gains a multi-col branch that builds the MultiIndex's level columns by extracting the group-key values per bucket in encounter order.

### Implementation pattern

For each unique group key (from M38's hash-bucketed group order), extract the per-level values into N parallel `Column` vectors. Wrap into the MultiIndex via `m44_build_df_with_multiindex`.

For `keys()` and `size()`:
- `keys()`: 0-column DataFrame with the MultiIndex (matches single-col `keys()` shape).
- `size()`: 1-column DataFrame ("size", `ColumnI64`) with the MultiIndex.

### Commit checkpoint after Phase B

`M44 B: tabular multi-column group_by promotes to MultiIndex`. Build clean + tests for 2-level and 3-level group_by sum/mean/agg/size/keys.

## Phase C — Minimal MultiIndex propagation through filter / head / tail / iloc (~250-300 LOC)

These 4 handlers all build a `keep_indices: Vec<usize>` row-selection vector. The M42 recipe applies — permute each index level by the same vector, emit via the MultiIndex constructor.

### New helper

```rust
// In vm/src/builtins.rs, near m42_permute_index_into_df:
fn m44_permute_multiindex_into_df(
    interp: &mut Interpreter,
    parent_df_ptr: u64,
    new_names: ...,
    new_columns: ...,
    keep_indices: &[usize],
) -> Result<u64, VmError> {
    // 1. If parent has neither single-col index nor MultiIndex → m37_build_df (RangeIndex)
    // 2. If parent has single-col index → m42_permute_index_into_df (M41/M42 path)
    // 3. If parent has MultiIndex → permute each level by keep_indices, emit via m44_build_df_with_multiindex
}
```

Then change the 4 handlers' emit calls from `m42_permute_index_into_df` to `m44_permute_multiindex_into_df`. The helper auto-dispatches to the right path based on the parent's index state. This means existing single-col-index callers see no behavior change.

### EXPLICIT scope-down for other ops (M44b anchor)

Every OTHER existing handler that propagates the single-col index (M42's: sort_by / dropna / dropna_subset / fillna_* / merge / select / drop / rename; M43's: pivot / melt / concat_rows / concat_cols / pivot_table) **drops a MultiIndex in M44a**, returning a RangeIndex frame. Document this explicitly in LANGUAGE_GUIDE.md §11.32; M44b's scope is to lift it.

The `m42_permute_index_into_df` and `m42_copy_index_into_df` helpers stay unchanged — they still handle the single-col case correctly. Add a third helper `m44_drop_multiindex_into_df` (or just call `m37_build_df` directly) that the other handlers use when they detect a MultiIndex on the input.

Actually — simpler design: rename the M42 helpers to handle the "single-col index ok; MultiIndex → drop" behavior internally. Then the M42 callers don't need to change. Add `m44_permute_multiindex_into_df` as a separate helper used only by the 4 in-scope handlers (filter/head/tail/iloc).

### Commit checkpoint after Phase C

`M44 C: tabular MultiIndex propagation through filter / head / tail / iloc`. Build clean + tests for each (group_by → filter preserves MultiIndex; group_by → iloc preserves the right slice of it).

## Phase D — Tests + demo + LANGUAGE_GUIDE update + agent report (~250-300 LOC)

### Tests (`vm/tests/m44_tabular_multiindex.rs`)

Aim for 18-25 tests. Cover:
- Phase A: set_index_multi round-trip; ValueError on empty cols / absent col / already-indexed frame; index_nlevels = 0/1/N; index_level(i) returns correct column; sort_index_multi orders lexicographically; sort_index_multi with descending; sort_index_multi raises without MultiIndex.
- Phase B: 2-level group_by sum promotes to MultiIndex; 2-level mean returns ColumnF64 values + MultiIndex; 3-level group_by; agg with specs; keys() returns 0-column with MultiIndex; size() returns 1-column "size" + MultiIndex; **multi-col group_by no longer keeps keys-as-columns** (this is the contract flip).
- Phase C: filter preserves MultiIndex; head/tail/iloc preserve the right slice; **other ops (sort_by, dropna, merge, pivot, etc.) drop the MultiIndex back to RangeIndex** (M44a contract test).

### Demo

Add `examples/tabular_multiindex_demo.spy` (~120 LOC) — a workflow:
1. Load sales CSV
2. `group_by(["region", "category"]).sum()` — produces a 2-level MultiIndex result
3. `sort_index_multi(true)` — order by region then category
4. `filter` on a column condition — MultiIndex preserved
5. `index_level(0)` and `index_level(1)` access
6. `reset_index_multi()` to verify round-trip

Testable via `compiler/tests/tabular_multiindex_demo_runs.rs`.

### Tests to flip

Search `vm/tests/m38_tabular_ops.rs` + `vm/tests/m43_tabular_index_reshape.rs` for:
- M38: any "multi-column group_by keeps keys as columns" assertion (the M43 contract that's now flipping)
- M43: the `multi_col_group_by_does_not_promote_to_index` test — this WILL flip in M44a.

List every flip in your final report.

### LANGUAGE_GUIDE.md update

§5 `tabular` gets an "M44 additions" subsection covering MultiIndex storage, set_index_multi/reset_index_multi/index_nlevels/index_level/index_level_name/sort_index_multi, and multi-column group_by promotion.

§11.32 (new): MultiIndex is dropped by ops other than filter/head/tail/iloc in M44a; M44b expansion list.

§11.26 update: the "fully index-aware" claim needs nuance — single-col indexes propagate broadly; MultiIndex only through 4 ops in M44a.

Banner bumps to post-M44.

### Commit checkpoint after Phase D

`M44 D: tabular MultiIndex — tests + demo + LANGUAGE_GUIDE update + agent report`.

## Acceptance criteria (machine-checkable)

1. `cargo build --workspace --release` — clean, no new warnings.
2. `cargo test --release -p strictpy-vm --test m44_tabular_multiindex` — all pass.
3. `cargo test --release -p strictpy-compiler --test tabular_multiindex_demo_runs` — passes.
4. **No M37/M39/M40 regressions**: targeted M37/M39/M40 sweeps pass byte-identically (those don't touch group_by or the index).
5. **M38/M41/M42/M43 mostly unchanged** except for flipped tests; list every flip in the report.
6. **Full sweep**: 888 + N - K passing (N new M44 tests, K flipped tests). Net should be at least 888 + 12.

## Constraints — files NOT to modify

- `seed_prelude` in `compiler/src/resolver.rs`.
- M37 / M39 / M40 tests — must keep passing untouched.
- Only flip M38 + M43 tests that explicitly assert the old multi-col-group_by-not-promoted contract.
- The 7 existing tabular demos — add a separate `tabular_multiindex_demo.spy`.

## STOP CRITERIA — priority drops if budget runs out

Five priority drops, in order:

1. **Drop Phase C entirely** — ship A+B without minimal propagation. Users would set up a MultiIndex via group_by but couldn't chain `filter` on the result without losing the index. Acceptable v1 cut; M44b picks up.
2. **Drop `agg(specs)` multi-col promotion in Phase B** — keep the simple shortcuts (sum/mean/min/max/count + size + keys). agg's spec parsing is the bulkiest single piece.
3. **Drop `sort_index_multi` in Phase A** — keep set_index_multi + reset_index_multi + accessors. Users can sort_by(level_col) as a workaround until sort_index_multi ships.
4. **Drop the demo** — orchestrator extends an existing one.
5. **Drop the LANGUAGE_GUIDE rewrite** — orchestrator finishes it.

After applying any drop, document what was cut with a "what M44b should pick up" list.

## Methodology discipline

1. **First commit at ~30-50% of budget is acceptable** for this milestone — it's shared-infra-heavy (the new payload field + the new constructor are used by every phase). M41 set the precedent for shared-infra exception; M44 fits the same pattern. The Lesson 1 SPIRIT (commit before orchestrator intervenes, green build + tests passing at each commit) is what matters.

2. **Per-phase commits expected** — 4 commits (A, B, C, D). Phase A may bundle several conceptual sub-pieces (the payload bump + constructor + the 6 new methods); that's expected.

3. **Variable prefix `m44_`** for all new helpers / locals in shared files.

4. **Precautionary `cp` at session start** — see "Edit-tool worktree leak" above. Skip the per-phase discovery loop; defensive copy is cheaper.

5. **No new IR opcodes** — pure handler-body + new helper + new constructor. Class method dispatch goes through the existing `m37_tabular_class_method_native_id_by_name` (or you may need a new `m44_*` dispatcher if there are method-name collisions — unlikely but check).

## Final report

Write `docs/thesis/agent_reports/m44_tabular_multiindex.md` (under 600 words) covering:
- What shipped per phase (A-D)
- What was cut from STOP CRITERIA (if anything)
- LOC delta per touched file (especially the payload bump + the new constructor + the helper)
- Final test count + verification + list of M38/M43 tests flipped (old → new assertion)
- Surprises / design calls (e.g., how did you handle the "mutually exclusive single-col vs MultiIndex" invariant in the constructors? did `sort_index_multi` lexicographic ordering across mixed-dtype levels need any cleverness?)
- "What M44b should pick up" — full propagation list (all the ops M44a explicitly drops MultiIndex for) + stack/unstack + outer-merge fallback + loc range
- LANGUAGE_GUIDE.md update status
- Whether the Edit-tool worktree leak recurred (yes/no, count, did the precautionary-cp workaround help?)

Commit this report in Phase D's commit.

## Commit message shape (final)

```
M44: tabular MultiIndex — storage + multi-col group_by promotion + minimal propagation

Adds the headline missing piece from the v1 single-index story:
nested indices for multi-column group_by results.

Phase A: DataFrame payload 40 → 56 bytes (optional
  index_levels: List[Column] + index_names: List[str]); 6 new
  methods (set_index_multi / reset_index_multi / index_nlevels /
  index_level / index_level_name / sort_index_multi).
Phase B: multi-column group_by (length ≥ 2) promotes all group-key
  columns to a MultiIndex on the result. All 8 group_by aggregation
  methods (sum/mean/min/max/count/agg/size/keys) participate.
Phase C: minimal MultiIndex propagation through filter / head /
  tail / iloc via new helper m44_permute_multiindex_into_df.
  EXPLICIT v1 scope-down (M44b anchor): all other ops (sort_by /
  dropna / fillna_* / merge / select / drop / rename / pivot /
  melt / concat_*) drop a MultiIndex back to RangeIndex.
Phase D: ~20 new tests + tabular_multiindex_demo.spy +
  LANGUAGE_GUIDE.md §11.32 + agent report.

NativeFn IDs 1027-... (6+ new). Variable prefix m44_.
Tests: 888 → 888+N-K (N new M44, K flipped M38/M43).
```
