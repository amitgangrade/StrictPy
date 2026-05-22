# M44 — `tabular` MultiIndex (storage + multi-col group_by promotion + minimal propagation)

**Status:** complete (Phases A-D). Workspace builds clean; 25 new VM integration tests + 2 new demo-runs tests pass. Ships the headline missing piece from M41-M43's v1 single-index story: nested indices for multi-column `group_by` results.

## What shipped per phase

**Phase A** — MultiIndex storage + accessors + `sort_index_multi`. The DataFrame payload grew 40 → 56 bytes to carry an optional `index_levels: List[Column]? + index_names: List[str]?` alongside the existing M41 `index + index_name`. The two index representations are **mutually exclusive**: a frame has one OR the other OR neither (RangeIndex), enforced by zeroing one set of slots in every constructor when the other is written. New helper `m44_build_df_with_multiindex` mirrors `m41_build_df_with_index`. Six new methods (NativeFns 1027-1032): `set_index_multi(cols)`, `reset_index_multi()`, `index_nlevels()`, `index_level(i)`, `index_level_name(i)`, `sort_index_multi(ascending)`. `sort_index_multi` does a stable lexicographic sort (level 0 primary, level 1 secondary, etc.) via a single comparator that walks the per-level dtype snapshots inline. 11 Phase A tests.

**Phase B** — Multi-column `group_by` promotion. All 8 group_by aggregation methods (`size`, `keys`, `sum`, `mean`, `min`, `max`, `count`, `agg`) now dispatch by `key_col_indices.len()`: 1 → M41 single-col index path (M43 contract); ≥2 → M44 MultiIndex via `m44_build_df_with_multiindex`. The existing `m38_build_key_columns` already extracts per-level columns — no new helper. `keys()` with ≥2 keys returns a 0-regular-column DataFrame whose MultiIndex is the unique (col1, col2, ...) tuples. 7 Phase B tests.

**Phase C** — Minimal MultiIndex propagation through `filter` / `head` / `tail` / `iloc`. New helper `m44_permute_multiindex_into_df` auto-dispatches on the parent's index state: MultiIndex → permute every level by `keep_indices`; single-col → delegate to `m42_permute_index_into_df`; none → emit RangeIndex. The 4 handlers swap their emit call from `m42_permute_index_into_df` to the new helper. Existing single-col-index and RangeIndex callers see no behavior change. Every OTHER row-transforming or column-list op (`sort_by`, `dropna`, `fillna_*`, `merge`, `select`, `drop`, `rename`, `pivot`, `melt`, `concat_*`, `pivot_table`, `resample_index`, `asof_merge_index`) **drops a MultiIndex back to RangeIndex** — explicit M44b anchor (documented in `LANGUAGE_GUIDE.md` §11.32). 7 Phase C tests.

**Phase D** — `examples/tabular_multiindex_demo.spy` (~165 LOC) walks an 8-row sales frame through 2-level `group_by` sum → `sort_index_multi(true)` → `filter` (preserves MultiIndex) → `index_level(0)/(1)` access → `reset_index_multi` round-trip. `compiler/tests/tabular_multiindex_demo_runs.rs` asserts on every printed checkpoint. `LANGUAGE_GUIDE.md` §5 gains an "M44 additions" subsection; §11.26 is updated to reference §11.32; new §11.32 documents the M44a → M44b drop table. Banner bumped to "post-M44 (2026-05-22)". This report + the Phase D commit.

## STOP CRITERIA — what was cut

**Nothing.** All four phases (A-D) landed as separable per-phase commits, plus the dispatch-time work in A. Total budget usage well under the brief's target (~1500 LOC vs. the brief's 1500-2000 estimate).

## LOC delta per touched file

| Path | Lines added | Purpose |
|---|---:|---|
| `compiler/src/resolver.rs` | +44 | DataFrame layout: added `index_levels` + `index_names` fields; payload_size 40 → 56; 6 new method signatures. |
| `compiler/src/ir.rs` | +8 | 6 new dispatcher entries (M44TabDf* family). |
| `shared/src/native.rs` | +44 | 6 NativeFn entries (1027-1032) + from_u32 arms + doc comments. |
| `vm/src/builtins.rs` | +400 | `m44_build_df_with_multiindex` + `m44_df_multiindex_fields` + `m44_df_nlevels` + `m44_read_index_levels` + `m44_read_index_level_names` + `m44_sort_multiindex_perm` + 6 handler functions + `m44_permute_multiindex_into_df` + 4 emit-swap edits + 4 group_by-handler dispatch rewrites + 3 constructor bumps (40→56). |
| `vm/tests/m44_tabular_multiindex.rs` | +470 | 25 integration tests across all 3 phases. |
| `vm/tests/m43_tabular_index_reshape.rs` | +5 / −5 | 1 test flipped (renamed + assertions flipped — see below). |
| `compiler/tests/tabular_multiindex_demo_runs.rs` | +95 | 2 demo-runs tests. |
| `examples/tabular_multiindex_demo.spy` | +165 | M44 demo. |
| `LANGUAGE_GUIDE.md` | +75 / −10 | §5 M44 subsection + §11.26 rewrite + §11.32 new + §5 tabular subheading + banner. |
| `docs/thesis/agent_reports/m44_tabular_multiindex.md` | +120 | This report. |

Total: ~1430 lines code + tests + docs + demo.

## Final test count + verification

- M44 tests added: **25** in `vm/tests/m44_tabular_multiindex.rs` + **2** in `compiler/tests/tabular_multiindex_demo_runs.rs` = **27 new tests**.
- M38/M43 tests flipped: **1** (in M43 — renamed + assertion flipped, not deleted, so it still counts toward the total).
- Pre-M44 baseline (per brief): 888 passing.
- **Post-M44 sweep: `passed: 915 failed: 0 ignored: 1`** (`cargo test --workspace --release --no-fail-fast` summed across all crates). Net delta: +27 (= 25 new M44 VM tests + 2 new demo-runs tests). The 1 flipped test was renamed in place, so it counts toward the total.
- All targeted M37-M44 tabular sweeps pass: 176 VM tests total (19 + 23 + 23 + 26 + 23 + 19 + 18 + 25). All 8 tabular demo-runs files pass (16 demo tests).
- `cargo build --workspace --release` clean — no new warnings.

## M38/M43 tests flipped (with old → new assertion)

The brief said "list every flip in your final report." Only **one** test had to flip:

**M43 (1 flip):**

- `vm/tests/m43_tabular_index_reshape.rs::multi_col_group_by_does_not_promote_to_index` → renamed to `multi_col_group_by_promotes_to_multiindex_m44`.
  - Old assertions: `ncols=3, has=false` (keys retained as 3 regular columns, RangeIndex output).
  - New assertions: `ncols=1, nlev=2` (keys promoted to a 2-level MultiIndex; only `qty` remains as a regular column).
  - Body unchanged except for the `println` calls — the input frame + group_by call sequence is byte-identical.

**M38: zero flips.** The only M38 test exercising multi-col group_by (`group_by_multi_column`) checks only `sz.length()` — the number of groups, not column shape — so it kept passing without modification. Other M38 group_by tests use single-column keys (already promoted under M43) and continued passing.

**M37 / M39 / M40 / M41 / M42:** zero flips. All targeted sweeps for those milestones pass byte-identically.

## Surprises / design calls

1. **Mutually-exclusive invariant via zeroed slots.** Every constructor — `m37_build_df` / `m37_from_columns` / `m37_from_rows` / `m41_build_df_with_index` / `m44_build_df_with_multiindex` — zeros the slots it doesn't own. `m41_build_df_with_index` writes to `index + index_name` and zeros `index_levels + index_names`; `m44_build_df_with_multiindex` mirrors. This makes the invariant "at most one of (single-col, multi) is non-null" structural rather than convention-only, and the accessors (`has_index`, `index_nlevels`, `index_level`) trivially check both slots without needing a discriminator tag.

2. **`index_level(0)` falls through to M41 single-col index.** When `nlevels()==1` the frame uses the M41 path (single-col index, `index_levels` is null). I chose to have `index_level(0)` return the same column as `index()` rather than `none` in that case — it makes the accessor uniform across single-col and multi-col indexes and matches the brief's "for a single-col index, index_level(0) returns the same column as index()" wording.

3. **Lexicographic sort via per-level dtype snapshots.** `m44_sort_multiindex_perm` snapshots every level's value-list once before sorting (as a `LevelVals` enum), then the comparator walks `m44_snapshots` in order until it finds a non-Equal level. This avoids per-comparison heap reads via `m37_col_fields` (those would also touch the GC barrier on every call). The closure-friendly enum is local to the function — no public surface.

4. **`m44_permute_multiindex_into_df` is the only Phase C touchpoint.** Rather than threading "does this op preserve MultiIndex?" through 13 different handlers, Phase C added one new helper and rewrote only 4 emit calls. The remaining 9+ handlers (sort_by, dropna, fillna_*, merge, select, drop, rename, pivot, melt, concat_*, pivot_table) automatically drop the MultiIndex because they call `m42_permute_index_into_df` / `m42_copy_index_into_df`, which only read the M41 single-col slots — `m44_df_multiindex_fields` is never reached. Clean separation of M44a (4 ops carry MI) from M44b (the rest will lift).

5. **Test-flipping was minimal (1 flip).** The brief flagged M38 multi-column-key-as-columns tests as flipping candidates. M38's `group_by_multi_column` only asserts on group count; the M38 single-col tests had already been updated in M43. So only one M43 contract test needed flipping. Much smaller blast radius than M43's 9-flip cascade.

## What M44b should pick up

In priority order:

1. **Full MultiIndex propagation through the remaining row-transforming ops** (M44a explicitly drops): `sort_by`, `dropna`, `dropna_subset`, `fillna_i64/f64/str/bool/datetime`, `merge`. Per-handler change is exactly the M42 → M44 emit-call swap that Phase C already did for filter/head/tail/iloc. Estimated ~60 LOC each = ~400 LOC across 9 handlers.
2. **Full MultiIndex propagation through column-list ops**: `select`, `drop`, `rename`. Sibling helper `m44_copy_multiindex_into_df` (mirrors `m42_copy_index_into_df`). ~50 LOC each = ~150 LOC.
3. **Full MultiIndex propagation through reshape ops**: `pivot`, `melt`, `concat_rows`, `concat_cols`, `pivot_table`. Each needs design (the levels move into or out of the column space, or stack/repeat as melt does).
4. **`stack` / `unstack`** — pandas's MultiIndex bread-and-butter. Pivots a level into/out of the column space.
5. **Outer-merge MultiIndex fallback** — replace M42's RangeIndex fallback for dtype-mismatched indexes with a true NaN-padded MultiIndex (per pandas).
6. **`df.loc[label_list]`** / range-by-label across single-col AND MultiIndex.
7. **`set_index(col)` accepting `List[str]` of length 1** — harmonize the single/multi APIs.
8. **`pivot_table(aggfunc=["sum", "mean"])`** + `margins=True` — quality-of-life from M41 deferral.

Items 1-2 are mechanical (~550 LOC) and unlock the most-common MI workflows; items 3-4 are the design-heavy ones.

## LANGUAGE_GUIDE.md update status

Shipped:
- Banner bumped to "post-M44 (2026-05-22)".
- §5 `tabular` subheading extended to include M43, M44.
- §5 "M41 scope-down" paragraph updated to flag M44 as adding MultiIndex on top.
- §5 new "M44 additions" subsection covering all 6 new methods, multi-col `group_by` promotion, and the minimal-propagation contract.
- §11.26 rewritten as the post-M44 single-col + MultiIndex propagation summary; references §11.32.
- §11.32 new: the M44a/M44b MultiIndex drop table.

## Edit-tool worktree leak recurrence

**No recurrence in this session.** The precautionary `cp` block from the brief was run at session start and synced `vm/src/builtins.rs`, `compiler/src/resolver.rs`, `compiler/src/ir.rs`, `shared/src/native.rs`, `LANGUAGE_GUIDE.md` from project root (those files were already identical because the worktree started at the M43 archive commit — `git status` was clean post-cp). Every subsequent `Edit` and `Write` call landed in the worktree directly. **The precautionary-cp workaround completely eliminated the leak this session** — zero `cp` recoveries needed, zero time burned on leak diagnosis. Net win vs. the per-phase discovery loop M40-M43 used (which averaged 5-90s per session).

## Lesson 1 compliance

The first commit (`M44 A: tabular MultiIndex storage + accessors + sort_index_multi`) landed at ~35% of budget — squarely inside the brief's "30-50% acceptable for shared-infra-heavy milestones" exception. The Phase A commit shipped the payload bump + new constructor + all 6 new methods + 11 passing tests + clean workspace build. Subsequent per-phase commits at the end of B (group_by promotion + 1 M43 flip + 7 new tests), C (4-op propagation + 7 new tests), and D (demo + LANGUAGE_GUIDE + this report). The streak at #25 passes to #26 cleanly with all 4 phases as separable commits.

## Verdict

`tabular` MultiIndex ships in v1 form. After M44 the package has both index representations (single-col M41/M42/M43 and multi M44) coexisting in the same payload; multi-col `group_by` promotes to MultiIndex (the M43 anchor); the 4 most-common row-selection ops carry the MultiIndex through. Every other op currently drops the MultiIndex — that's the explicit M44b anchor. 25 new VM tests + 2 demo-runs tests pass, plus 1 M43 test flipped. The Edit-tool worktree leak did not recur thanks to the precautionary `cp` workaround.
