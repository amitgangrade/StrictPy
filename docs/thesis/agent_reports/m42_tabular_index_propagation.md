# M42 — `tabular` index propagation through existing methods

**Status:** complete (Phases A-E). Workspace builds clean; 19 new VM integration tests + 2 demo-runs tests pass. Closes the M41 v1 scope-down on the 11 most-used DataFrame methods.

## What shipped per phase

**Phase A** — Row-selection ops (`filter`, `sort_by`, `head`, `tail`, `iloc`). New helper `m42_permute_index_into_df(parent, names, cols, nrows, keep)` reads the parent's index + index_name, calls `m37_column_take(parent_index, keep)` to permute, and emits via `m41_build_df_with_index`. Falls back to `m37_build_df` when the parent has no index. Each of the 5 handlers gained exactly one new emit call. 6 new tests.

**Phase B** — Column-list ops (`select`, `drop`, `rename`). New sibling helper `m42_copy_index_into_df` skips the permute step (row order is unchanged) and clones the parent's index via the existing `m41_clone_column`. Same shape change: one new emit call per handler. 3 new tests.

**Phase C** — Null handling (`dropna`, `dropna_subset`, `fillna_*`). `dropna` (and `dropna_subset`, which delegates) uses `m42_permute_index_into_df` keyed on the existing `keep` vector. `fillna_*` is a pure row-pass-through and uses `m42_copy_index_into_df`. 5 new tests.

**Phase D** — `merge`. Two new helpers: `m42_merge_build_index` chooses the index strategy per `how`; `m42_merge_outer_index_column` materializes the per-cell stitch for outer joins. inner/left → permute lhs.index; right → permute rhs.index; outer → cell-wise lhs.index for `(Some(lr), _)` rows + rhs.index for `(None, rr)` rows, with a dtype-mismatch fallback to RangeIndex. index_name policy: lhs wins for inner/left/outer; rhs wins for right. 5 new tests.

**Phase E** — `examples/tabular_index_propagation_demo.spy` (~140 LOC) walks an indexed trades frame through filter → sort_by → dropna_subset → fillna_f64 → merge → select → sort_index. `compiler/tests/tabular_index_propagation_demo_runs.rs` asserts on every printed checkpoint. `LANGUAGE_GUIDE.md` §5 gains an "M42 additions" subsection + §11.26 is rewritten with the full propagation table; banner bumped to post-M42. This report + commit.

## STOP CRITERIA — what was cut

Nothing. All four phases (A-D) landed as separable per-phase commits, plus E for tests/demo/docs. Total budget usage well under the brief's target.

## LOC delta per touched file

| Path | Lines added | Purpose |
|---|---:|---|
| `vm/src/builtins.rs` | +280 | `m42_permute_index_into_df`, `m42_copy_index_into_df`, `m42_merge_build_index`, `m42_merge_outer_index_column` + 11 emit-call swaps. |
| `vm/tests/m42_tabular_index_propagation.rs` | +810 | 19 integration tests. |
| `vm/tests/m41_tabular_index.rs` | +5 / −5 | Flipped 1 test (see below). |
| `compiler/tests/tabular_index_propagation_demo_runs.rs` | +98 | 2 demo-runs tests. |
| `examples/tabular_index_propagation_demo.spy` | +175 | M42 demo. |
| `LANGUAGE_GUIDE.md` | +50 / −10 | §5 M42 subsection + §11.26 rewrite + banner. |
| `docs/thesis/agent_reports/m42_tabular_index_propagation.md` | +110 | This report. |

Total: ~1530 lines code + tests + docs. Meaningfully smaller than M41 (~2200) as predicted.

## Final test count

- M42 tests added: 19 in `vm/tests/m42_tabular_index_propagation.rs` + 2 in `compiler/tests/tabular_index_propagation_demo_runs.rs` = **21**.
- M41 tests flipped: **1** (see below).
- Pre-M42 baseline (per brief): 847 passing, 1 ignored.
- Post-M42: **passed: 868 failed: 0 ignored: 1** (verified via `cargo test --workspace --release --no-fail-fast`). The brief's `847 + N − K` target assumed K flipped tests are deleted; we renamed-and-flipped instead, so the M41 test still counts. Net: +21 (= 19 new VM tests + 2 demo-runs tests).

## M41 tests flipped

**Exactly one:** `vm/tests/m41_tabular_index.rs::filter_drops_index`.

- Old name: `filter_drops_index`. Old assertion: `assert!(out.contains("has=false"))` — confirmed v1 scope-down (filter returned a RangeIndex frame).
- New name: `filter_preserves_index_m42`. New assertion: `assert!(out.contains("has=true"))` — verifies M42 propagation (filter preserves the parent's index).

The body of the test (the `.spy` source) is byte-identical between old and new; only the test name + the asserted `has=` substring changed. The full M42 coverage (6 row-selection tests) lives in `vm/tests/m42_tabular_index_propagation.rs`; this kept-but-flipped M41 sanity test exists to make the regression of M42 obvious-by-failure should it ever land.

## Surprises / design calls

1. **Two helpers, not one.** The brief offered `m42_permute_index_into_df` with a trivial `0..nrows` keep vector for column-list ops. I chose to add a sibling `m42_copy_index_into_df` that skips the permutation entirely (calls `m41_clone_column` directly). Reasoning: a no-op `m37_column_take` allocates a fresh values + nulls list and copies every cell — for a 1M-row frame that's two 8MB allocations of pure waste per `rename` call. The sibling is 30 lines and keeps the fast path fast.

2. **Merge outer-join dtype-mismatch fallback handled at the helper level.** `m42_merge_build_index` checks `m38_col_class_name(l_index) != m38_col_class_name(r_index)` and returns `None`, causing the caller to emit a RangeIndex frame via `m37_build_df`. No special-case error path — pandas behavior here is a NaN-padded MultiIndex which is beyond v1's column model. Documented in §11.26.

3. **index_name policy for merge.** The brief left this implicit; I went with lhs wins for inner/left/outer (matches pandas's "self.index.name is preserved") and rhs wins for right (the symmetric mirror). The right-side choice avoids a confusing "right join carries other's labels but loses other's index_name."

4. **Edit-tool worktree leak recurred — narrower than M41.** The very first `Edit` on `builtins.rs` leaked silently (project root copy got the change; worktree copy did not). Detected immediately via `git status --short` showing a clean worktree. Recovered with a single `cp /c/Users/AG/CascadeProjects/PythonCompiler/vm/src/builtins.rs vm/src/builtins.rs`. The same pattern recurred at every "first edit of a session" boundary on each shared file (builtins.rs, m41_tabular_index.rs). `Write` calls (new test file, demo, this report) all landed in the worktree directly.

## What M43 should pick up

In priority order:

1. **Index propagation through `pivot_table`** — natural extension; `index_col` could become the result's index by default (or via a `set_index` boolean arg). Pandas does this automatically.
2. **Index propagation through `group_by` + agg** — the group-key column(s) become a MultiIndex in pandas. Single-column case is straightforward; MultiIndex is a separate big lift.
3. **`pivot` / `melt` / `concat_rows` / `concat_cols`** — the remaining row-reshaping ops. `concat_rows` is the easiest (stack the two indexes); `pivot` / `melt` need design (the index moves into or out of the column space).
4. **MultiIndex** — currently the index is a single column. `df.set_index([col1, col2])` for nested indices unlocks a lot of pandas-style workflows.
5. **`df.loc[label_list]` / range-by-label** — the M41 follow-up. `select_by_label_*` returns one row; range support mirrors pandas's `df.loc["a":"c"]`.
6. **Outer-join MultiIndex fallback** — replace M42's RangeIndex fallback for dtype-mismatch outer joins with a true MultiIndex (per pandas).
7. **`pivot_table(aggfunc=["sum", "mean"])`** + `margins=True` — quality-of-life features.

Items 1-3 are the index-propagation core for the remaining reshape ops; items 4-7 expand the index model itself. Cost estimate for items 1-3: ~400-500 LOC.

## LANGUAGE_GUIDE.md update status

Shipped:
- Banner bumped to "post-M42 (2026-05-22)".
- §5 `tabular` subheading extended to include M42.
- §5 "M41 additions" — scope-down paragraph updated to flag M42 as closing it for 11 methods.
- §5 new "M42 additions" subsection with per-phase summary + propagation rules + merge `how` table + demo pointer.
- §11.26 rewritten as the full propagation table (preserve vs. drop, per-merge-`how` rules, merge dtype-mismatch fallback).
- §11.27 reference to "drops the index in v1" softened to point at §11.26's post-M42 table.

## Edit-tool worktree leak recurrence

**Yes — recurred at every "first Edit on this shared file" boundary**, narrower than M41's burst leak. Specifically:
- First `Edit` on `vm/src/builtins.rs` (helper insertion): leaked. Recovered via `cp`.
- Second batch of `Edit`s on `vm/src/builtins.rs` (Phase A handlers): leaked. Recovered via `cp`.
- First `Edit` on `vm/tests/m41_tabular_index.rs` (flipping the M41 test): leaked. Recovered via `cp`.
- Third + fourth batches of `Edit`s on `vm/src/builtins.rs` (Phases B, C, D): leaked. Recovered via `cp`.

Pattern: the **first** Edit per file per session goes to the project-root copy; subsequent Edits on the same file in close succession ALSO go to project root (not just the first). Mitigation cost: ~5 `cp` operations totaling under 5 seconds. `Write` calls with absolute worktree paths all worked correctly — `m42_tabular_index_propagation.rs` (new file), `tabular_index_propagation_demo.spy` (new file), `tabular_index_propagation_demo_runs.rs` (new file), and this report all landed in the worktree directly.

Recommendation for M43: open the brief, identify the shared files that will receive Edits, and `cp` from project-root to worktree once at session start — would avoid the per-batch detection + recovery.

## Lesson 1 compliance

Lesson 1's letter is honored. Phase A's first commit (`M42 A: tabular index propagation through filter/sort_by/head/tail/iloc`) landed at ~20% of budget: helper insertion + 5 handler edits + 6 tests + 1 flipped M41 test, all building clean. Subsequent commits at the end of B, C, D, E. The streak at #23 should pass to #24 cleanly.

## Verdict

`tabular` index propagation ships per the brief. 11 existing DataFrame methods now carry the index through their row/column transformations. Two simple helpers (`m42_permute_index_into_df`, `m42_copy_index_into_df`) absorb the bulk of the work; merge's per-`how` index assembly is the only non-trivial bit. 19 new tests + 2 demo-runs tests pass. The M41 v1 scope-down debt is closed for the highest-traffic methods; reshape ops (pivot/melt/group_by) remain for M43+.
