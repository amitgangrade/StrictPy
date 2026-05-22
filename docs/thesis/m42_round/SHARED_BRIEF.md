# M42 — `tabular` index propagation through existing methods

## Context

M41 shipped the minimum viable DatetimeIndex (optional `index: Column?` + `index_name: str?` fields on `DataFrame`; 6 index methods + 5 index-aware ops + `pivot_table`). The **explicit v1 scope-down**: every existing DataFrame method that returns a fresh frame **dropped the index** in v1 — only `sort_index`, `resample_index`, `asof_merge_index`, and `select_by_label_*` preserved it.

M42 closes that scope-down debt. The 11 existing methods listed below need to propagate the index through the row/column transformations they already perform. Per the M41 agent's analysis, this is ~600-800 LOC concentrated in 6 handlers in `vm/src/builtins.rs`, each gaining: **(a)** read the parent index + index_name, **(b)** permute the index by the same row-selection vector that produces the regular columns, **(c)** emit via `m41_build_df_with_index` instead of `m37_build_df`. The permutation logic is **already there** for the regular columns — the only new line per handler is the index-permute + emit call.

**This is a meaningfully smaller milestone than M37-M41** (~700-1000 LOC vs 2100-2800). It's also mostly **modifying existing handlers** rather than adding new ones — which means few new NativeFn IDs and minimal resolver/ir.rs changes. The dispatch tables are already wired; the work is in the handler bodies.

You are the **24th** of an unbroken Lesson-1-compliant agent streak (M28 → M41). M41 introduced the first per-phase-cadence nuance to the streak (combined Phases A+B+C because they shared infrastructure). M42 should be straightforwardly per-phase since each phase modifies disjoint handlers.

## Files to read FIRST (in order)

1. `LANGUAGE_GUIDE.md` (project root) — §5 `tabular (M37, extended by M38, M39, M40, M41)` subsection + §11.26 (the v1 scope-down you're undoing)
2. `docs/thesis/agent_reports/m41_tabular_index.md` — especially the "What M42 should pick up" section which spells out the punch list + the implementation pattern (~600-800 LOC across 6 handlers, the index-permute + emit-with-index recipe)
3. `examples/tabular_index_demo.spy` — current demo; index gets re-set in user code after each transform; you're fixing that
4. `vm/src/builtins.rs` — find:
   - `m41_build_df_with_index` — the constructor you'll route through
   - `m37_df_filter` / `m37_df_select` / `m37_df_drop` / `m37_df_head` / `m37_df_tail` / `m37_df_iloc` (or similarly named) — the handlers you'll modify
   - `m39_df_merge` — the merge handler
   - `m40_df_dropna` / `m40_df_dropna_subset` / `m40_df_fillna_*` — the null-handling handlers
   - `m37_df_sort_by` — the sort handler
5. `compiler/src/resolver.rs` — no new method registrations needed; verify by searching for the existing method names
6. `vm/tests/m41_tabular_index.rs` — has a `filter_drops_index` test that demonstrates the current behavior. M42 will flip this test (you'll need to update it, see Acceptance criteria below).

## Constraints

- **Lesson 1**: first commit at ~20% of budget. 23-streak — don't break it.
- **Variable prefix `m42_`** for any new helper functions / locals. (Likely few — most work modifies existing `m37_` / `m38_` / `m39_` / `m40_` handlers; that's fine, no need to rename them.)
- **NativeFn IDs**: probably none new in M42 — you're modifying existing handlers, not adding new methods. If you discover a genuinely-needed new method (e.g. an "index-aware filter that takes an index-label predicate"), allocate from 1027-1040 and document why.
- **No new classes**, no new crate deps.
- **No changes to method signatures** — every existing method keeps its exact public surface. Only behavior changes: an indexed input frame now produces an indexed output frame, where before it produced a RangeIndex frame.
- All 124 existing tabular tests must continue passing — except `vm/tests/m41_tabular_index.rs::filter_drops_index` (and any analogous "verifies index gets dropped" tests), which you SHOULD update to verify the new behavior. List every test you flip in your final report.

### Edit-tool worktree leak — known recurring

Same pattern as M37-M41. **M40 narrowed the cause**: `Edit` on already-existing files leaks; `Write` with absolute worktree paths is unaffected. M41 burned ~30 seconds via `cp` recovery. Mitigation: after the first round of bulk Edits to shared files (`vm/src/builtins.rs` in M42's case — that's the main one), check `git status` once; if there are diffs in the project root, `cp` from project root to worktree.

## The shape — one shared helper does most of the work

Before touching any handler, write `m42_permute_index_into_df`:

```rust
// In vm/src/builtins.rs, near m41_build_df_with_index:

/// Permute the parent DataFrame's index by `keep_indices` (the row-selection
/// vector that produced the new column data) and emit the result DataFrame
/// via m41_build_df_with_index. If the parent has no index, emits via the
/// existing m37_build_df instead — the result is RangeIndex, same as today.
///
/// This is the common pattern: every row-transforming handler builds a
/// `Vec<usize>` of source-row indices (already happens), permutes each
/// regular column by it (already happens), and now permutes the index too.
fn m42_permute_index_into_df(
    interp: &mut Interpreter,
    parent_df_ptr: u64,
    new_names: ...,
    new_columns: ...,
    keep_indices: &[usize],
) -> Result<u64, VmError> {
    // 1. Read parent's index + index_name (zero if no index)
    // 2. If no parent index → emit via m37_build_df with no index (RangeIndex)
    // 3. Otherwise: permute the parent's index column by keep_indices
    //    (reuse the existing per-dtype column-permute helpers)
    // 4. Emit via m41_build_df_with_index with the permuted index + cloned index_name
}
```

For column-list ops (select / drop / rename) that don't drop rows, `keep_indices` is `0..nrows` — the index is copied unchanged. You can call the same helper with that trivial vector OR add a sibling `m42_copy_index_into_df` to avoid the no-op permutation. Your call.

Each of the 11 handlers below ends up with **one new call** to this helper instead of its current call to `m37_build_df`.

## Phase A — Row-selection ops: filter / sort_by / head / tail / iloc (~250-300 LOC)

These 5 handlers all build a row-selection vector (`keep_indices`) and use it to permute every regular column. Adding `m42_permute_index_into_df(parent, names, cols, &keep_indices)` at the emit site does the job.

- `df.filter(mask: ColumnBool)`: keep_indices = positions where mask is `true` (the existing code already builds this; nulls in mask treated as false per M37 semantics).
- `df.sort_by(col, ascending)`: keep_indices = the sort permutation (existing code already builds it).
- `df.head(n)`: keep_indices = `0..min(n, nrows)`.
- `df.tail(n)`: keep_indices = `max(0, nrows-n)..nrows`.
- `df.iloc(start, stop)`: keep_indices = `start..stop` (clamped per M40 semantics).

### Test coverage for Phase A

Update `vm/tests/m41_tabular_index.rs::filter_drops_index` to a new test name (e.g., `filter_preserves_index`) and flip its assertion. Add similar happy-path tests for sort_by / head / tail / iloc preserving the index of an indexed input.

### Commit checkpoint after Phase A

`M42 A: tabular index propagation through filter/sort_by/head/tail/iloc`. Build clean + at least 5 tests verifying index preservation on each method.

## Phase B — Column-list ops: select / drop / rename (~100-150 LOC)

These 3 handlers don't touch rows — they project / drop / rename columns. The index is unchanged in every case. Either:

- Call `m42_permute_index_into_df` with `keep_indices = 0..nrows` (extra no-op work but uniform), OR
- Add `m42_copy_index_into_df` that skips the permute step entirely.

Pick whichever fits the existing code patterns better. Same shape change at each handler: route the emit through the new helper.

### Test coverage for Phase B

Happy-path tests for select / drop / rename preserving an indexed frame's index.

### Commit checkpoint after Phase B

`M42 B: tabular index propagation through select/drop/rename`. Build clean + 3 tests.

## Phase C — Null-handling ops: dropna / dropna_subset / fillna_* (~200-300 LOC)

- `df.dropna()` and `df.dropna_subset(cols)`: row-selection vector is already built (rows without nulls). Same shape as Phase A — `m42_permute_index_into_df` at the emit site.
- `df.fillna_i64(v)`, `df.fillna_f64(v)`, `df.fillna_str(v)`, `df.fillna_bool(v)`, `df.fillna_datetime(v)`: pure row-pass-through, no row dropping. Use the trivial `0..nrows` keep_indices (or the copy variant) — index unchanged.

### Test coverage for Phase C

dropna preserves index (with some rows dropped, so the surviving index labels are a subset of input); fillna_* preserves index unchanged on a fully-indexed frame.

### Commit checkpoint after Phase C

`M42 C: tabular index propagation through dropna/dropna_subset/fillna_*`. Build clean + tests.

## Phase D — Merge index propagation (~200-300 LOC)

`df.merge(other, on, how)` is the most architecturally substantial M42 piece. Rules per pandas:

- **inner join**: result index = self's index, restricted to the rows that matched. If self has no index → RangeIndex result (today's behavior).
- **left join**: result index = self's index (all rows preserved).
- **right join**: result index = other's index (all rows preserved). If other has no index → RangeIndex result.
- **outer join**: result index = self's index for matched/left-only rows + other's index for right-only rows. Dtype mismatch (self has ColumnI64 index, other has ColumnStr) → fall back to RangeIndex with a v1 simplification (document); pandas's NaN-padded MultiIndex is M43+ territory.

The merge handler already builds row-selection vectors for both sides; M42 plumbs both indexes through.

### Test coverage for Phase D

Index preservation tests for each `how` value (inner / left / right / outer). The outer-with-dtype-mismatch fallback should be documented behavior, not a crash.

### Commit checkpoint after Phase D

`M42 D: tabular index propagation through merge`. Build clean + 4 tests.

## Phase E — Tests + demo + LANGUAGE_GUIDE update + agent report (~150-250 LOC)

### Tests (`vm/tests/m42_tabular_index_propagation.rs`)

Aim for 18-25 tests across all 11 affected methods. Many will be small "set_index → method → check the index is preserved (or permuted correctly)" tests.

### Demo

Add `examples/tabular_index_propagation_demo.spy` (~100 LOC) — a realistic workflow that exercises end-to-end index preservation. Suggested shape:

```
1. Load trades CSV; set_index("trade_id")
2. filter by amount > threshold (M42: index preserved)
3. sort_by("price", ascending=false) (M42: index preserved in sorted order)
4. dropna_subset(["counterparty"]) (M42: index restricted to non-null rows)
5. fillna_f64(0.0) (M42: index preserved)
6. merge with a small "trader" frame on "trader_id", how=left (M42: self's index preserved)
7. select(["price", "qty", "trader_name"]) (M42: index preserved)
8. Print, then sort_index(ascending=true) to verify the index threaded through
```

Testable via `compiler/tests/tabular_index_propagation_demo_runs.rs`.

### LANGUAGE_GUIDE.md update

Major §11.26 rewrite: the v1 scope-down is now closed. Document which methods preserve vs drop the index:

- **Preserve (now post-M42)**: filter, sort_by, head, tail, iloc, select, drop, rename, dropna, dropna_subset, fillna_*, merge (per `how` rules), and the 4 from M41 (sort_index, resample_index, asof_merge_index, select_by_label_*).
- **Still drop the index (v1 scope)**: pivot, melt, group_by/agg, pivot_table, concat_rows, concat_cols. These reshape the frame in ways that don't have an obvious index propagation. M43+ may revisit.
- **Column-returning ops** (unique_*, value_counts) trivially don't carry an index — they return Column or a 2-col DataFrame.

Bump banner to "post-M42". Add a brief §11.29 if the merge dtype-mismatch fallback warrants a gotcha note.

### Commit checkpoint after Phase E

`M42 E: tabular index propagation — tests + demo + LANGUAGE_GUIDE update + agent report`.

## Acceptance criteria (machine-checkable)

1. `cargo build --workspace --release` — clean, no new warnings.
2. `cargo test --release -p strictpy-vm --test m42_tabular_index_propagation` — all pass.
3. `cargo test --release -p strictpy-compiler --test tabular_index_propagation_demo_runs` — passes.
4. **No M37-M40 regressions**: M37 / M38 / M39 / M40 sweeps all pass byte-identically.
5. **M41 mostly unchanged**: M41's 23 VM tests + 2 demo-runs pass, except for any "verifies index gets dropped" tests you flip (list them in your report).
6. **Full sweep**: 847 + N - K where N = new M42 test count (target 18-25) and K = number of flipped M41 tests (likely 1-3). Net should be at least 847 + 15.

## Constraints — files NOT to modify

- `seed_prelude` in `compiler/src/resolver.rs`.
- Tests for M37/M38/M39/M40 — must keep passing untouched. Only flip M41 tests that explicitly assert "index is dropped" — and document every flip in your final report.
- The 5 existing tabular demos in `examples/` — add a separate `tabular_index_propagation_demo.spy`.

## STOP CRITERIA — priority drops if budget runs out

Five priority drops, in order:

1. **Drop Phase D (merge)** — index propagation through merge is the bulkiest single piece. M43 can pick it up. Ships A+B+C + most-used surface.
2. **Drop dropna / dropna_subset** in Phase C — keep fillna_* (which is trivial: pure pass-through). dropna's row-selection vector + index permute can be M43.
3. **Drop Phase B select/drop/rename** — these are the most-trivial; M43 picks up. Cuts ~100 LOC.
4. **Drop the LANGUAGE_GUIDE rewrite** — orchestrator finishes it.
5. **Drop the demo** — orchestrator can extend an existing one.

After applying any drop, document what was cut with a "what M43 should pick up" list.

## Methodology discipline

1. **First commit at ~20% of budget** — Phase A's helper + filter + one test exercising index preservation.
2. **Per-phase commits** — 4 phase commits expected (A, B, C, D), plus E for tests/demo/docs. M42 phases ARE separable because they modify disjoint handlers, unlike M41 where they shared infrastructure. Don't combine unless you discover an unexpected shared dependency.
3. **Variable prefix `m42_`** for any new helpers (likely just `m42_permute_index_into_df` and maybe `m42_copy_index_into_df`).
4. **Don't add new IR opcodes** — pure handler-body changes.
5. **Edit-tool worktree leak workaround**: bulk Edits to `builtins.rs` may leak. Check `git status` after the first round; `cp` if needed. `Write` for new files unaffected.

## Final report

Write `docs/thesis/agent_reports/m42_tabular_index_propagation.md` (under 500 words — M42 is a smaller milestone) covering:
- What shipped per phase (A-E)
- What was cut from STOP CRITERIA (if anything)
- LOC delta per touched file (which handlers grew by how much)
- Final test count + verification + the list of M41 tests you flipped (with old and new assertion)
- Surprises / design calls (e.g., did the merge outer-join dtype-mismatch fallback need special handling? did you go with `m42_copy_index_into_df` or the trivial-permute approach?)
- "What M43 should pick up" — concrete follow-up list
- LANGUAGE_GUIDE.md update status
- Whether the Edit-tool worktree leak recurred (yes/no, count, mitigation effectiveness)

Commit this report in Phase E's commit.

## Commit message shape (final)

```
M42: tabular index propagation through existing methods

Closes the M41 explicit v1 scope-down. The 11 existing DataFrame
methods that returned a fresh frame now PROPAGATE the index
instead of dropping it.

Phase A: row-selection ops — filter / sort_by / head / tail / iloc.
Phase B: column-list ops — select / drop / rename (no-op permute).
Phase C: null handling — dropna / dropna_subset / fillna_*.
Phase D: merge — index propagation per join `how` (left/right/outer
  handled per pandas rules; dtype-mismatch outer falls back to
  RangeIndex with v1 simplification).
Phase E: ~20 new tests + tabular_index_propagation_demo.spy +
  LANGUAGE_GUIDE.md §11.26 rewrite + agent report.

Pattern: single new helper m42_permute_index_into_df does the
read-parent-index + permute-by-keep-vector + emit-via-
m41_build_df_with_index work. Each affected handler gains one
new emit call; the row-selection vectors they already build feed
straight in.

NativeFn IDs unchanged (M42 modifies existing handlers; no new
methods). Variable prefix m42_.

Tests: 847 → 847 + N - K (N new, K M41 tests flipped to verify
new behavior).
```
