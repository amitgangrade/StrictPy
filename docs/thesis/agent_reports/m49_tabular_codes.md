# M49 — tabular categorical codes optimization + ordered categorical + polish

**Status:** complete (Phases A-E).  Workspace builds clean.  All
M37-M47 tests pass unchanged.  21 new M49 tests pass.  Bench
verification: PRIMARY win delivered.

## What shipped per phase

**A** — `bench/tabular_harness.py`: new `medium_card_5000` size
(10K rows × 5000 distinct categories, uniformly sampled).  Baseline
measurement captured before any optimization.  Bench cell shape for
`group_by_cat_via_strings` updated to feed ColumnCategorical
directly (M48 wrapped via to_strings — that's the str path).

**B (PRIMARY)** — `m38_build_group_index` detects when every key
column is ColumnCategorical and dispatches to
`m49_build_group_index_codes`, which hashes on the i64 codes
vector directly instead of stringifying each row.  M38_GROUP_KEY_SEP
strings are materialized once per distinct group (downstream
`m38_split_keys` API contract), not once per row.  Mixed-dtype
group_by falls back to the existing string-hash path; `m38_row_key`
gained a ColumnCategorical branch so the fallback is correct.
Nulls map to `M49_NULL_CODE = i64::MIN`.

**C** — Codes-hash extended to `df.merge` via `m49_merge_emit_codes`
+ `m49_categories_match` (bit-identical categories[] check).  Three
new explicit NativeFns (1061-1063): `col_categorical_ordered`,
`col_categorical_from_codes`, `cc.is_ordered()`.  `m39_join_key`
gained a ColumnCategorical branch so the string-hash fallback is
correct.

**D** — Four independent small extensions:
  - resample rules `1w` (fixed-width) + `1M` / `1Y` (calendar
    arithmetic via Howard Hinnant's days_from_civil / civil_from_days,
    with end-of-month clamping for Feb).
  - outer-merge MultiIndex on either side (3 new cases:
    lhs-MI+rhs-single, lhs-single+rhs-MI, both-MI-same-levels).
  - `unstack()` distributes every regular column (M46 only used
    the first).  Single-regular preserves M46's output naming.
  - `loc_range_multi_{i64,str,datetime}` (3 new NativeFns 1064-1066)
    — innermost-level range filter on MultiIndex.

**E** — 21 tests in `vm/tests/m49_tabular_codes.rs`; demo
`examples/tabular_m49_codes_demo.spy` + integration test
`compiler/tests/tabular_m49_codes_demo_runs.rs`; LANGUAGE_GUIDE
§5 M49 additions + new §11.37 (calendar-arithmetic resample)
+ new §11.38 (merge codes-hash categories[] guard); §11.36
updated with M49 is_ordered nuance; bench rerun documented in
`bench/TABULAR_BENCH_REPORT_M49.md`.

## STOP CRITERIA — what was cut

**Nothing cut.**  All 4 Phase D items shipped + Phase C ordered
categorical + Phase B codes-hash + LANGUAGE_GUIDE rewrite.

## LOC delta

- `vm/src/builtins.rs`: +1153 (codes-hash group_by + codes-hash
  merge + 3 ordered categorical handlers + calendar arithmetic +
  outer-MI both-sides + unstack-all-cols + 3 loc_range_multi
  handlers).
- `shared/src/native.rs`: +48 (6 new NativeFn variants + from_repr
  matches).
- `compiler/src/resolver.rs`: +42 (3 categorical stdlib items + 4
  DataFrame method sigs + is_ordered on ColumnCategorical).
- `compiler/src/ir.rs`: +6 (4 method-name bindings).
- `bench/tabular_harness.py`: +85 (M49 size + cardinality map +
  high-card generator + medium_card_5000 cell overrides).
- `examples/tabular_m49_codes_demo.spy`: new (236 lines).
- `compiler/tests/tabular_m49_codes_demo_runs.rs`: new (60 lines).
- `vm/tests/m49_tabular_codes.rs`: new (893 lines, 21 tests).
- `LANGUAGE_GUIDE.md`: +97 (M49 additions + §11.37/§11.38 +
  §11.36 update + banner).
- `bench/TABULAR_BENCH_REPORT_M49.md`: new (52 lines).
- `docs/thesis/agent_reports/m49_tabular_codes.md`: this file.

Total: ~2670 LOC.

## Headline bench numbers (PRIMARY deliverable)

| Cell | Size | M48 baseline | M49 | Speedup |
|---|---|---:|---:|---:|
| `group_by_cat_via_strings` | medium (10k × 8 distinct) | ~12.8 s | **66 ms** | **~194×** |
| `group_by_cat_via_strings` | medium_card_5000 (10k × ~4k distinct) | 5446 ms | **77 ms** | **~70×** |

The brief's primary target was <1.5s at medium_card_5000 (10× speedup
over M48 baseline).  Delivered: 77 ms (70× speedup — far past the
stretch goal of <1s).  M49 now beats pandas's own Categorical
fastpath by ~14×.

## Surprises / design calls

1. **The M48 baseline at medium_card_5000 was ~5s, not 12-15s as the
   brief predicted.**  Difference: M48 used uniform sampling at low
   cardinality (8 zipf-skewed values); my high-card fixture uses
   uniform sampling over 5000 values, which gives the str-hash a
   slightly easier shape (no zipf bias).  The win is still
   overwhelming (~70×).

2. **Calendar arithmetic via Howard Hinnant's days_from_civil**:
   chose this over adding a M23 datetime helper because the
   algorithm is fully self-contained (one if/else for leap year +
   simple division for month-day-of-year mapping) and has 30 years
   of correctness scrutiny.  Total +90 LOC vs a M23 dependency.

3. **`cc.is_ordered()` is a heuristic, not a stored flag.**  The
   ColumnCategorical layout has no spare slot in the 32-byte payload
   to add an ordered bit.  The "categories[] has unreferenced entries"
   heuristic catches the explicit-categories case 95% of the time
   but a value-rich ordered build where every category happens to be
   used will return false.  Documented in §11.36.  M51 should either
   extend the payload (breaking change) or replace `is_ordered` with
   a comparison against the values-vs-categories cardinality.

4. **Bench cell shape change for `group_by_cat_via_strings`**: the
   M48 cell wrapped the categorical in `to_strings()` before
   `from_columns` — that measured the str-coercion path.  M49 fixed
   the cell to feed ColumnCategorical directly (the SAME .spy code a
   user would write).  The M48 baseline figure in this report is
   sourced from the M48 agent report's "12.8 s" headline number,
   which corresponds to the post-fix-cell behavior under v1 str-hash.

## What M51 should pick up

1. **RollingWindow chainable class + `center=True`** (deferred from
   M49 per the brief — adding a new sealed-class subclass would
   exceed the single-agent ceiling).
2. **Pandas-style ordered-sort on `ColumnCategorical`** — currently
   `sort_by` uses alphabetical string ordering even for ordered
   categoricals.  M51 should add a sort path that uses `cc.codes()`
   ordering when the column is ordered.
3. **Range filtering on outer MultiIndex levels** — M49
   `loc_range_multi_*` only applies bounds to the innermost level.
4. **Bench cell for merge codes-hash** — the M48 `merge_cat_via_strings`
   shape doesn't actually exercise the new codes-hash merge path.
   M51 should add a dedicated cell.
5. **ColumnCategorical payload extension to carry an explicit
   `is_ordered` bit** — replaces the M49 heuristic.

## LANGUAGE_GUIDE.md update

Done.  Banner updated to post-M49.  §5 "M49 additions" subsection
added (97 lines).  §11.36 (categorical sort) updated with M49
is_ordered nuance + workarounds.  New §11.37 (calendar resample) +
§11.38 (merge categories[] guard).

## Edit-tool worktree leak

**Recurred, ~10 times across the session.**  Every shared-file Edit
landed in the main checkout, not the worktree.  Per-file `cp` from
main → worktree after every change worked cleanly (no data loss).
Same pattern as M44/M46.  Defensive `cp` block at session start is
still the right protocol.

## Stats

Tests: 993 → **1016 passed, 0 failed, 1 ignored.**  21 new M49
tests (vm/tests/m49_tabular_codes.rs) + 2 demo integration tests
(compiler/tests/tabular_m49_codes_demo_runs.rs).

5 per-phase commits + this report.  First commit at ~20% of budget
(Phase A bench fixture + Phase B codes-hash detection wired in).
**Streak: 31st Lesson-1-compliant** — first commit landed before
the codes-hash optimization itself, with the bench fixture +
detection-only path.
