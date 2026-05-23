# M45 — `tabular` full MultiIndex propagation through M42 + M43 ops

## Context

M44 (M44a) shipped MultiIndex storage + multi-column group_by promotion + **minimal** MultiIndex propagation through just 4 ops (filter / head / tail / iloc). The remaining ~14 row/column-transforming and reshape handlers **drop a MultiIndex back to RangeIndex** as an explicit v1 scope-down.

M45 lifts that scope-down. Every M42 op (`sort_by` / `dropna` / `dropna_subset` / `fillna_*` / `merge` / `select` / `drop` / `rename`) and every M43 reshape op (`pivot` / `melt` / `concat_rows` / `concat_cols` / `pivot_table`) now propagates MultiIndex through correctly — just like they propagate the single-col index today.

**M45 scope-down**: `stack` / `unstack`, `df.loc[label_list]` range-by-label, and the outer-merge MultiIndex fallback (replaces M42's current RangeIndex fallback for dtype-mismatched indexes) all stay deferred to M46.

After M45, the `tabular` package is **fully index-aware end-to-end for both single-column and multi-column indexes** — the v1 propagation story is complete.

You are the **27th** of an unbroken Lesson-1-compliant agent streak (M28 → M44). M44 returned cleanly per-phase after explicit shared-infra classification; M45 is **disjoint-handler** work (14 separate handlers, no shared new infrastructure), so expect a clean per-phase cadence (first commit at ~20% of budget).

## Files to read FIRST (in order)

1. `LANGUAGE_GUIDE.md` (project root) — §5 tabular subsection (M37-M44 additions) + §11.26 + §11.32 (the M44 v1 scope-down you're lifting)
2. `docs/thesis/agent_reports/m44_tabular_multiindex.md` — the recipe for `m44_permute_multiindex_into_df` + `m44_build_df_with_multiindex` (you'll extend the same recipe)
3. `docs/thesis/agent_reports/m42_tabular_index_propagation.md` — the M42 recipe pattern (`m42_permute_index_into_df` / `m42_copy_index_into_df`) — M45 either extends these helpers OR routes their callers through M44's auto-dispatching helper
4. `docs/thesis/agent_reports/m43_tabular_index_reshape.md` — the M43 reshape recipe + index-promotion patterns (for pivot_table / pivot / group_by / melt / concat)
5. `examples/tabular_multiindex_demo.spy` — M44's end-to-end demo
6. `vm/src/builtins.rs` — find:
   - `m44_permute_multiindex_into_df` — the auto-dispatching helper M44 introduced. **The simplest design for M45 is to make ALL the existing handlers call this helper instead of `m42_permute_index_into_df` / `m37_build_df`.** It already auto-dispatches on the parent's index state.
   - `m44_build_df_with_multiindex` — the constructor.
   - All the M42 handlers that currently drop MultiIndex: `m37_df_sort_by`, `m40_df_dropna`, `m40_df_dropna_subset`, `m40_df_fillna_*` (5 dtypes), `m39_df_merge`, `m37_df_select`, `m37_df_drop`, `m37_df_rename`.
   - All the M43 handlers that currently drop MultiIndex: `m39_df_pivot`, `m39_df_melt`, `m39_tabular_concat_rows`, `m39_tabular_concat_cols`, `m41_df_pivot_table`.
7. `compiler/src/resolver.rs` — no new method registrations needed; verify by grep
8. `vm/tests/m44_tabular_multiindex.rs` — search for any `*_drops_multiindex` tests; those WILL flip

## Constraints

- **Lesson 1**: first commit at ~20% of budget (this is disjoint-handler work — no shared-infra exception). 26-streak — don't break it.
- **Variable prefix `m45_`** for any new helpers / locals in shared files. Likely few new helpers — most work modifies existing handlers (or extends M42/M44 helpers).
- **NativeFn IDs**: likely none new in M45 (modifying existing handlers, not adding methods). If you discover a genuinely-needed new method, allocate from 1033-1064.
- **No new classes**, no new crate deps, no payload changes.
- **No changes to method signatures** — every existing method keeps its public surface. Behavior changes only for MultiIndex-input cases.
- The 192 existing tabular tests must continue to pass — EXCEPT any "drops MultiIndex" contract tests in M44 (and possibly M42/M43) that need to flip to "propagates MultiIndex". Search and list every flip.

### Edit-tool worktree leak — precautionary `cp` workaround

M44 confirmed the workaround eliminates the leak entirely. **At session start, run this `cp` block**:

```bash
for f in vm/src/builtins.rs compiler/src/resolver.rs compiler/src/ir.rs \
         shared/src/native.rs LANGUAGE_GUIDE.md; do
    cp /c/Users/AG/CascadeProjects/PythonCompiler/$f $f 2>/dev/null
done
```

Then proceed with edits as normal. M44 needed zero mid-session recoveries with this pattern.

## The shape — same recipe pattern that's worked since M42

Every affected handler today calls one of:

- `m37_build_df(interp, names, columns)` — emits a RangeIndex frame (no index propagation)
- `m42_permute_index_into_df(interp, parent, names, columns, &keep_indices)` — emits a frame whose single-col index is permuted by `keep_indices` (drops MultiIndex)
- `m42_copy_index_into_df(interp, parent, names, columns)` — emits a frame whose single-col index is copied unchanged (drops MultiIndex)
- `m43_concat_rows_index` / similar M43-specific helpers — drop MultiIndex by routing through `m37_build_df`

The M44 helper `m44_permute_multiindex_into_df` already auto-dispatches:
- No index → `m37_build_df` (RangeIndex)
- Single-col index → `m42_permute_index_into_df`
- MultiIndex → permute each level, emit via `m44_build_df_with_multiindex`

**The simplest path**: change every call site from `m42_permute_index_into_df` to `m44_permute_multiindex_into_df`. The single-col index case still works (the helper routes to the M42 path). The MultiIndex case now propagates correctly. **Zero behavior change for non-MultiIndex inputs.**

A sibling `m45_copy_multiindex_into_df` (or extending `m42_copy_index_into_df` with a MultiIndex branch) handles the column-list ops (select / drop / rename) that don't permute rows.

This is the **third application of the recipe pattern** (after M42's `m42_permute_index_into_df` and M44's `m44_permute_multiindex_into_df`). M45 is mostly call-site updates plus possibly one new helper for the copy case.

## Phase A — M42 row/column-transforming ops MultiIndex propagation (~400-500 LOC)

Eight handlers to update:

- **Row-selecting ops** (`m37_df_sort_by`, `m40_df_dropna`, `m40_df_dropna_subset`): route emit through `m44_permute_multiindex_into_df`. Each already builds `keep_indices`; the M44 helper does the rest.
- **Pass-through fill ops** (`m40_df_fillna_i64` / `_f64` / `_str` / `_bool` / `_datetime`, dispatched via shared `m40_df_fillna` body): same pattern — route through `m44_permute_multiindex_into_df` with `keep_indices = 0..nrows` (trivial pass-through), OR through a new `m45_copy_multiindex_into_df` that skips the permutation step entirely.
- **Merge** (`m39_df_merge`): the most architecturally substantial. M42's merge has per-`how` index policy (lhs wins inner/left/outer; rhs wins right; dtype-mismatch outer → RangeIndex fallback). M45 extends this: if lhs/rhs has MultiIndex, route the index through `m44_build_df_with_multiindex` instead of `m41_build_df_with_index`. **The outer-merge dtype-mismatch fallback stays RangeIndex** — replacing it with a NaN-padded MultiIndex is M46 work.
- **Column-list ops** (`m37_df_select`, `m37_df_drop`, `m37_df_rename`): pure pass-through. Either route through the new copy helper, OR through `m44_permute_multiindex_into_df` with trivial `keep_indices = 0..nrows`.

### Commit checkpoint after Phase A

`M45 A: tabular MultiIndex propagation through M42 ops`. Build clean + at least 6 tests verifying MultiIndex propagation on sort_by / dropna / fillna / merge (inner+left) / select / drop / rename.

## Phase B — M43 reshape ops MultiIndex propagation (~400-500 LOC)

Five handlers to update:

- **`m39_df_pivot`**: the M43 logic promotes the `index` value to the output's index. M45: if input has a MultiIndex, drop it (pivot reshapes the row dimension so the input MultiIndex doesn't have a clean target). Document. Single-col index case M43 already handles. Actually wait — let me reconsider. The user might WANT the pivot output to preserve the input MultiIndex... but pivot's output has different rows (one per unique `index` value), so the original index doesn't map. **OK to drop with explicit doc, same as today's RangeIndex fallback shape.**

- **`m39_df_melt`**: M43 repeats the input index per `value_var`. M45 extension: if input has MultiIndex, repeat each level per `value_var`. Same algorithm, applied to each level.

- **`m39_tabular_concat_rows`**: M43 concatenates compatible single-col indexes (same dtype + same name → concatenate; else RangeIndex fallback). M45 extension for MultiIndex: same dtype-per-level + same name-per-level + same number of levels → concatenate level-by-level; else fall back to RangeIndex (same strict-reconciliation policy as the single-col case).

- **`m39_tabular_concat_cols`**: M43 takes lhs's index. M45 extension: if lhs has MultiIndex, take it. (Same lhs-wins policy.)

- **`m41_df_pivot_table`**: M43 promotes `index_col` to the output's single-col index. M45: if input has a MultiIndex, **drop it** in the same way pivot does — pivot_table reshapes the row dimension. Document.

### Commit checkpoint after Phase B

`M45 B: tabular MultiIndex propagation through M43 reshape ops`. Build clean + tests for melt MultiIndex repetition, concat_rows MultiIndex concatenation, concat_cols MultiIndex inheritance, and the explicit pivot/pivot_table drop-MultiIndex behavior.

## Phase C — Tests + demo + LANGUAGE_GUIDE update + agent report (~250-300 LOC)

### Tests (`vm/tests/m45_tabular_multiindex_propagation.rs`)

Aim for 18-25 tests. Cover:
- Phase A: sort_by MultiIndex preserved; dropna MultiIndex restricted to non-null rows; fillna_* MultiIndex unchanged; merge inner/left/right/outer MultiIndex per `how`; select/drop/rename MultiIndex unchanged.
- Phase B: melt MultiIndex repetition (each level repeats); concat_rows happy path (compatible MultiIndexes concatenate); concat_rows mismatched-level-count fallback; concat_cols lhs MultiIndex wins; pivot drops MultiIndex (explicit contract test); pivot_table drops MultiIndex (explicit contract test).

### Tests to flip

Search `vm/tests/m44_tabular_multiindex.rs` for any "drops MultiIndex" contract tests on the M42/M43 ops. M44 likely has several (one per affected handler) — these all flip in M45. Also check M42/M43 test files for any explicit "MultiIndex input → RangeIndex output" assertions.

List every flip in your final report with old vs new assertion.

### Demo

Add `examples/tabular_multiindex_propagation_demo.spy` (~120 LOC) — a workflow showing end-to-end MultiIndex through both M42 and M43 ops:
1. Load sales CSV, group_by `["region", "category"]` → MultiIndex
2. `sort_by("total")` — MultiIndex preserved (per M45)
3. `dropna_subset(["total"])` — MultiIndex preserved
4. `merge(rates_df, ["region"], "left")` — MultiIndex preserved (lhs wins)
5. `select(["total", "rate"])` — MultiIndex preserved
6. `melt(["total"], ["rate"])` — MultiIndex repeats per value_var
7. Verify with `index_nlevels()` + `index_level(i)` accessors after each step

Testable via `compiler/tests/tabular_multiindex_propagation_demo_runs.rs`.

### LANGUAGE_GUIDE.md update

§5 tabular `M45 additions` subsection covering the propagation extension across M42 + M43 ops. §11.26 update: now "fully index-aware end-to-end" for both single-col AND MultiIndex. §11.32 rewrite: M44's "MultiIndex drops on these ops" list is now empty (or lists only pivot / pivot_table as explicit drops + the deferred-to-M46 items).

Bump banner to post-M45.

### Commit checkpoint after Phase C

`M45 C: tabular MultiIndex propagation — tests + demo + LANGUAGE_GUIDE update + agent report`.

## Acceptance criteria (machine-checkable)

1. `cargo build --workspace --release` — clean, no new warnings.
2. `cargo test --release -p strictpy-vm --test m45_tabular_multiindex_propagation` — all pass.
3. `cargo test --release -p strictpy-compiler --test tabular_multiindex_propagation_demo_runs` — passes.
4. **No M37/M38/M39/M40 regressions**: targeted sweeps pass byte-identically.
5. **M41/M42/M43/M44 mostly unchanged** except for flipped tests — list every flip in the report.
6. **Full sweep**: 915 + N - K passing (N new M45 tests, K flipped tests). Net should be at least 915 + 12.

## Constraints — files NOT to modify

- `seed_prelude` in `compiler/src/resolver.rs`.
- M37/M38/M39/M40 tests — must keep passing untouched.
- Only flip M44 (and possibly M42/M43) tests that explicitly assert "MultiIndex is dropped on [op]" — document every flip.
- The 8 existing tabular demos — add a separate `tabular_multiindex_propagation_demo.spy`.

## STOP CRITERIA — priority drops if budget runs out

Five priority drops, in order:

1. **Drop merge MultiIndex propagation** — keep all other Phase A ops. Merge is the bulkiest single piece (per-`how` index policy interacts with MultiIndex shape).
2. **Drop melt MultiIndex repetition** — keep concat_rows + concat_cols MultiIndex. Melt's per-level repetition is the bulkiest Phase B piece.
3. **Drop concat_rows MultiIndex level-by-level concatenation** — keep concat_cols MultiIndex inheritance. M46 picks up.
4. **Drop the LANGUAGE_GUIDE rewrite** — orchestrator finishes it.
5. **Drop the demo** — orchestrator extends an existing one.

After applying any drop, document what was cut with a "what M46 should pick up" list.

## Methodology discipline

1. **First commit at ~20% of budget** — Phase A's first 2-3 handler edits + 2-3 tests. M45 is disjoint-handler work (each phase modifies independent handlers), so per-phase commits are the natural cadence.
2. **Per-phase commits** — 3 commits (A, B, C). M45 has fewer phases than M44 because the work is more uniform (no big architectural change like the M44 payload bump).
3. **Variable prefix `m45_`** for any new helpers.
4. **No new IR opcodes** — pure handler-body updates routing emit calls through M44's existing helper.
5. **Edit-tool worktree leak**: run the precautionary `cp` block at session start (per M44's successful workaround).

## Final report

Write `docs/thesis/agent_reports/m45_tabular_multiindex_propagation.md` (under 500 words) covering:
- What shipped per phase (A-C)
- What was cut from STOP CRITERIA (if anything)
- LOC delta per touched file (mostly handler edits + maybe 1 new helper)
- Final test count + verification + list of M44 (and possibly M42/M43) tests flipped
- Surprises / design calls (e.g., did the merge per-`how` MultiIndex policy hit edge cases? did concat_rows level-by-level concatenation need an extra check?)
- "What M46 should pick up" — concrete list: stack/unstack, df.loc range-by-label, outer-merge MultiIndex fallback (replacing M42's RangeIndex fallback for dtype-mismatched indexes). Plus any items deferred from M45 STOP CRITERIA.
- LANGUAGE_GUIDE.md update status
- Whether the precautionary `cp` workaround held (yes/no, count of any leak recurrences)

Commit this report in Phase C's commit.

## Commit message shape (final)

```
M45: tabular full MultiIndex propagation through M42 + M43 ops

Lifts the M44 v1 scope-down. The 14 row/column-transforming and
reshape handlers that dropped MultiIndex back to RangeIndex now
propagate MultiIndex through correctly — same recipe pattern as
M42 (single-col index propagation), routed through M44's
auto-dispatching helper.

Phase A: M42 ops (sort_by / dropna / dropna_subset / fillna_* /
  merge / select / drop / rename) propagate MultiIndex via
  m44_permute_multiindex_into_df. Merge per-`how` policy extends
  to MultiIndex; outer-merge dtype-mismatch still falls back to
  RangeIndex (M46 anchor).
Phase B: M43 reshape ops (pivot / melt / concat_rows / concat_cols
  / pivot_table) propagate MultiIndex where it has a clean target:
  melt repeats each level per value_var; concat_rows concatenates
  level-by-level on dtype+name match; concat_cols takes lhs's
  MultiIndex. pivot and pivot_table explicitly drop MultiIndex
  (reshape the row dimension — no clean target).
Phase C: ~20 new tests + tabular_multiindex_propagation_demo.spy
  + LANGUAGE_GUIDE.md §11.26+§11.32 rewrite + agent report.

NativeFn IDs unchanged. Variable prefix m45_.
Tests: 915 → 915+N-K (N new, K flipped from M44).
```
