# M49 — Categorical codes optimization + ordered categorical + polish (with bench-validated targets)

## Context

M47 shipped `ColumnCategorical` with v1 to_strings()-coercion semantics — every existing op that hits a categorical column routes through `to_strings()` for hashing. M48 measured this: at medium-size `group_by_cat_via_strings`, **StrictPy takes 12.8s vs pandas 1.04s (12.3× slower)**. The `to_strings()` coercion itself only adds ~11% on top of plain ColumnStr (12.8s vs 11.6s) — the real loss is the string-hash dominating both.

M49's primary win: **hash on `ColumnCategorical.codes` directly**. Codes are i64 indices into the categories table; hashing 8-byte ints is dramatically faster than hashing strings, and **at high cardinality** (where the categories table is large), the speedup is meaningful.

M48 also surfaced a **critical surprise**: pandas Categorical at 8 distinct values gave only 0.98× speedup vs str groupby — codes-hash shines at **high** cardinality, not low. **M49 must add a ~5000-value high-cardinality bench fixture before claiming victory.** This is Phase A of the brief.

After Phase A+B (the bench-validated PRIMARY win), Phase C extends codes-hash to merge + ships ordered categorical with `from_codes`, and Phase D picks up the smaller M46/M47 follow-ups (more resample rules, outer-merge MultiIndex on either side, unstack-all-columns, loc_range_* on MultiIndex).

**Deferred to M51**: RollingWindow chainable class + `center=True` rolling alignment. These add a new sealed-class subclass (cross-dispatch) which would push M49 past the single-agent ceiling. M50 sequence handles desktop UI; M51 picks up rolling polish.

You are the **31st** of an unbroken Lesson-1-compliant agent streak (M28 → M48). **Classification: disjoint-handler** — every phase modifies independent handlers / adds helpers; no new sealed-class subclass means no cross-dispatch ceremony. First commit at ~20% of budget.

## Files to read FIRST (in order)

1. `LANGUAGE_GUIDE.md` (project root) — §5 tabular subsection M37-M47
2. `docs/thesis/agent_reports/m48_tabular_bench.md` — the M48 findings + the M49 numeric target. The 0.98× pandas-Categorical-at-low-cardinality surprise is in here.
3. `bench/TABULAR_BENCH_REPORT.md` — the rendered baseline. M49 will re-run the harness with the same shape post-implementation.
4. `bench/tabular_harness.py` — you'll extend with a high-cardinality fixture in Phase A.
5. `docs/thesis/agent_reports/m47_tabular_polish.md` — ColumnCategorical's v1 design (codes + categories + nulls + length payload; to_strings coercion).
6. `vm/src/builtins.rs` — find:
   - `m38_groupby_*` family — the group_by handler chain you'll teach to detect categorical key columns and hash on codes.
   - `m45_merge_build_*` — the merge handler chain (Phase C extends codes-hash to here).
   - `m47_alloc_col_categorical` — the existing categorical constructor (Phase C adds the from_codes variant).
   - `m40_parse_rule_ms` — the resample rule parser (Phase D extends to 1w/1M/1Y).
   - `m46_merge_outer_dtype_mismatch_multiindex` — the M46 outer-merge fallback (Phase D extends to MultiIndex-on-either-side cases).
   - `m39_df_unstack` — Phase D fixes the "only distributes first column" behavior.
   - `m46_df_loc_range_*` — Phase D extends to MultiIndex inputs.
7. `compiler/src/resolver.rs` — find existing ColumnCategorical class layout + tabular module registration (you'll add 3-4 new methods).
8. `vm/tests/m47_tabular_polish.rs` + `vm/tests/m48_*` — test file patterns.

## Constraints

- **Lesson 1**: first commit at ~20% of budget. 30-streak — don't break it.
- **Variable prefix `m49_`** for any new helpers / locals.
- **NativeFn IDs 1061-1080** reserved (M47 used 1043-1060; M48 added none). M49 expected to use ~3-5 new ones.
- **No payload changes** to DataFrame or Column subclasses.
- **No new sealed-class subclasses** — that's the M51 territory (RollingWindow). M49 stays disjoint-handler.
- **No new crate deps**.
- All 248+ existing tabular tests must keep passing — flips expected only on M48 bench numbers (which are rendered output, not test assertions).

### Edit-tool worktree leak — defensive measure

Per M44/M46: precautionary `cp` at session start as defensive measure. M48 was clean (mostly Python files); M49 is back to Rust source under bulk edits so the leak risk returns.

```bash
for f in vm/src/builtins.rs compiler/src/resolver.rs compiler/src/ir.rs \
         shared/src/native.rs LANGUAGE_GUIDE.md; do
    cp /c/Users/AG/CascadeProjects/PythonCompiler/$f $f 2>/dev/null
done
```

Per-file `cp` recovery if symptoms appear mid-session.

## Phase A — High-cardinality bench fixture + baseline measurement (~150-200 LOC)

### The verification gate

Before claiming codes-hash wins universally, we must measure it against a high-cardinality dataset. M48's existing fixture uses 8 distinct values for `category` — at that cardinality pandas Categorical got only 0.98× speedup. The win is at thousands.

### What to add to `bench/tabular_harness.py`

A new fixture size dimension parallel to the existing small/medium/large/xl: **cardinality variants of `medium`**:
- `medium_card_8` — current shape, 10K rows × 8 distinct category values (today's behavior; this becomes the named variant).
- **`medium_card_5000`** — 10K rows × 5000 distinct category values (high cardinality).

Plus regenerate fixtures with the new dimension. Only the categorical-related cells need both — non-categorical ops (filter / sort / read_csv) can stay on the existing `medium` (treated as `medium_card_8`).

### Run the baseline

Run `python bench/tabular_harness.py --sizes medium` (or whatever flag you add for cardinality) and capture the baseline `group_by_cat_via_strings` time on **medium_card_5000**. The brief expects:
- StrictPy `group_by_cat_via_strings` at medium_card_5000: likely 12-15s (similar order to medium_card_8, since to_strings overhead dominates).
- pandas Categorical at medium_card_5000: likely <1s (now the codes-hash actually wins).
- This sets up the M49 target: drive StrictPy → <1.5s for the codes-hash path.

If the baseline measurements surprise you (e.g. StrictPy somehow already faster, or pandas Categorical somehow doesn't speed up even at 5000), document it; it changes the calculus.

### Commit checkpoint after Phase A

`M49 A: bench fixture + baseline for high-cardinality group_by`. Build clean + the fixture generator works + the bench harness can run the new cell.

## Phase B — Categorical codes-hash for group_by (~500-600 LOC) — the PRIMARY M49 work

### The optimization

In `vm/src/builtins.rs::m38_groupby_*`, the group-key serialization currently joins per-cell-string-values with `\x01` and hashes the result. M49 changes this:

**When a group key column is `ColumnCategorical`, hash on `codes[i]` (i64) directly** instead of calling `to_strings()[i]` (str). The codes are 8-byte ints; hashing them is dramatically cheaper than hashing variable-length strings.

For **single-column** categorical group_by, this is the simple case: replace the str-hash with i64-hash. Multi-column group_by (M38) where one or more keys are categorical: build the composite key as `[code_for_lvl_0, code_for_lvl_1, ...]` and hash the i64 tuple. For mixed dtypes (str + categorical + i64 in the same group_by), fall back to the existing string-hash path — only fully-categorical or single-column-categorical groupings get the fast path.

### Behavioral correctness — categories must match

Codes-hash equality only works if all rows being compared share the same `categories[]` ordering. Within a single DataFrame, this is automatically true (ColumnCategorical's invariant). So **single-DataFrame group_by codes-hash is always safe**.

For merge (Phase C), if the lhs.category and rhs.category have different `categories[]` orderings, the codes don't correspond — must fall back to string-hash. Detect this in Phase C; not a concern in Phase B.

### Promotion to MultiIndex still works

M44's multi-col group_by promotion to MultiIndex still works the same — M49 just changes how the hash is computed. The output index columns (one per group-key) carry the original Categorical column data.

### Target verification

After Phase B, **re-run the bench**:
- `python bench/tabular_harness.py --ops group_by_cat_via_strings,group_by_pandas_categorical --sizes medium,medium_card_5000`
- Capture the new StrictPy time at medium_card_5000.
- **Target**: <1.5s. **Stretch**: <1s (within 2× of pandas).
- If the win isn't there, the implementation is wrong — debug before committing Phase B.

### Commit checkpoint after Phase B

`M49 B: tabular group_by codes-hash on ColumnCategorical (PRIMARY win)`. Build clean + 4-6 tests covering single-col + multi-col + mixed-dtype-fallback + the bench-rerun verification documented in the commit message.

## Phase C — Codes-hash for merge + ordered categorical + from_codes (~400-500 LOC)

### Codes-hash for merge

Extend Phase B's hashing optimization to `m39_df_merge` / `m45_merge_build_multiindex`:

- If both lhs.on_col and rhs.on_col are ColumnCategorical, **check that their `categories[]` arrays are bit-identical** (same length + same strings in same order).
- If categories match: hash on codes directly (the fast path).
- If categories differ (or one side isn't categorical): fall back to the existing string-hash path.

This is the "ordered categorical with matching ordering" promise from pandas; the typical workflow is `df1.set_index_cat(col)` + `df2.set_index_cat(col, categories=df1.col.categories())` to make the merge fast.

### Ordered categorical with shared categories

```python
tabular.col_categorical_ordered(values: List[str],
                                 categories: List[str]) -> ColumnCategorical
# Build a ColumnCategorical with categories pinned to the provided order.
# values must be a subset of categories; any value not in categories raises.
# The codes are 0..len(categories)-1.
```

```python
tabular.col_categorical_from_codes(codes: List[i64],
                                    categories: List[str]) -> ColumnCategorical
# Reverse constructor: build a ColumnCategorical from explicit codes + categories.
# Each codes[i] must be 0 <= code < len(categories) (else raises).
# Useful for round-tripping categorical data + for the merge-on-codes shape.
```

### Categorical sort ordering

Currently, `sort_by` on a ColumnCategorical sorts alphabetically (M47 v1). For ordered categoricals, **the categories[] ordering defines the sort order**. M49 adds: when sorting on a ColumnCategorical and the user wants ordered semantics, call `cc.codes()` first and sort by that. Document this v1 nuance — actual sort_by behavior change is the cleanest path but document carefully.

Actually, simpler: don't change `sort_by` semantics in M49. Add a new method `cc.is_ordered() -> bool` that returns true for ordered categoricals (those constructed with explicit categories). The agent's M49 report should document this v1 behavior. Pandas's full ordered-sort behavior is M51.

### NativeFn IDs

- `1061`: `col_categorical_ordered(values, categories)`
- `1062`: `col_categorical_from_codes(codes, categories)`
- `1063`: `cc.is_ordered()` — boolean predicate

### Commit checkpoint after Phase C

`M49 C: tabular merge codes-hash + ordered categorical + from_codes`. Build clean + tests for merge codes-hash with matching categories, merge fallback with mismatched categories, ordered categorical construction, from_codes round-trip.

## Phase D — Smaller M46/M47 follow-ups (~400-500 LOC)

Four small extensions. Each is independent.

### More resample rules: `1w` / `1M` / `1Y`

Extend `m40_parse_rule_ms` to accept `w` (weeks = 7 days × 86400000 ms), `M` (months — needs calendar arithmetic), and `Y` (years — needs calendar arithmetic).

**Calendar arithmetic**: months and years aren't fixed-width. Use M23's `datetime` module helpers. A `1M` bucket starting at epoch-ms `t` ends at the **same calendar day in the following month** (with end-of-month clamping for Feb/short months). A `1Y` bucket starts at `t` and ends at the same MM-DD in the following year (with Feb 29 → Feb 28 in non-leap-years).

If M23's datetime helpers don't expose month/year add, you'll need to add a small helper. Document the choice.

### Outer-merge with MultiIndex on either side

M46 added NaN-padded 2-level MultiIndex fallback only for **dtype-mismatched single-col** outer merges. M49 extends to:

- lhs has MultiIndex, rhs has single-col index: build NaN-padded result with lhs's MultiIndex levels intact + rhs's index as the last MultiIndex level (NaN for left-only rows).
- lhs has single-col, rhs has MultiIndex: symmetric.
- Both have MultiIndex: build the union shape — same level count is fine if names match; mismatched levels falls back to RangeIndex with a documented limitation.

### unstack distributing every regular column

M46's `unstack` only distributes the first regular column across the new wide columns. M49 fixes: **for each regular column, produce a set of wide output columns**. Output column names are `{innermost_level_value}_{original_col_name}`.

This is the pandas behavior. The implementation extends `m39_df_unstack` to loop over all regular columns instead of just the first.

### loc_range_* on MultiIndex

M46's `loc_range_*` works on single-col indexes only. M49 adds a per-dtype `loc_range_multi_*` variant that takes range bounds for each MultiIndex level (or just the innermost level — your call; document).

For v1, just support **innermost-level range** with outer levels left intact. Range bounds on outer levels can be M51.

### NativeFn IDs (Phase D)

Probably 1-2 new NativeFns for `loc_range_multi_*` variants. The other extensions modify existing handlers.

### Commit checkpoint after Phase D

`M49 D: tabular more resample rules + outer-merge MultiIndex on either side + unstack-all-columns + loc_range_multi`. Build clean + tests for each.

## Phase E — Tests + bench rerun + demo + LANGUAGE_GUIDE + agent report (~250-300 LOC)

### Tests (`vm/tests/m49_tabular_codes.rs`)

Aim for 20-28 tests. Cover:
- Phase B: codes-hash group_by single-col + multi-col + mixed-dtype-fallback + correctness vs str-hash on a small fixture.
- Phase C: ordered categorical happy path + from_codes round-trip + merge codes-hash with matching categories + merge fallback with mismatched categories + is_ordered() returns expected.
- Phase D: 1w/1M/1Y resample (M, Y need calendar arithmetic verification — test that a Feb 1 + 1M = Mar 1, Mar 31 + 1M clamps appropriately); outer-merge MultiIndex on either side (all 3 cases); unstack distributing >1 column; loc_range_multi_str on a 2-level MultiIndex.

### Bench rerun

After Phase B and C, re-run the bench harness with the new categorical optimization. Update `bench/TABULAR_BENCH_REPORT.md` with the post-M49 numbers (or add a `bench/TABULAR_BENCH_REPORT_M49.md` if cleaner). Show the before/after side by side:

```
group_by_cat_via_strings (medium):
  M48 baseline: 12.8s (StrictPy) vs 1.04s (pandas) → 12.3× slower
  M49:          ?     (StrictPy) vs 1.04s (pandas) → ?× ratio
```

Same for high-cardinality (medium_card_5000) — that's where the M48 surprise lives, and M49's win should be most visible.

### Demo

Add `examples/tabular_m49_codes_demo.spy` (~120 LOC) — a workflow that exercises:
1. Build a high-cardinality (~5000 unique values) DataFrame
2. group_by on the categorical column — fast (codes-hash)
3. Compare to group_by on a plain str column — slow (str-hash)
4. Merge two DataFrames on a categorical key with matching categories — fast
5. Merge with mismatched categories — falls back, document timing
6. Use from_codes to reconstruct
7. Use ordered categorical for a sort-by-categories-ordering test
8. Use resample `1M` for monthly aggregation
9. Use unstack on a multi-column frame to verify all columns distribute

Testable via `compiler/tests/tabular_m49_codes_demo_runs.rs`.

### LANGUAGE_GUIDE.md update

§5 tabular gets an "M49 additions" subsection covering codes-hash optimization (transparent — no API change) + ordered categorical + from_codes + new resample rules + outer-merge MultiIndex + unstack-all-columns + loc_range_multi_*. §11.36 update (M47 said sort uses string ordering): now there's `is_ordered()` to detect ordered-categorical (M51 will add sort-by-categories-ordering). New §11.37/§11.38 entries for calendar-arithmetic semantics of `1M`/`1Y` and the merge categories-must-match-for-codes-path.

Bump banner to post-M49.

### Commit checkpoint after Phase E

`M49 E: tabular codes optimization + polish — tests + bench rerun + demo + LANGUAGE_GUIDE update + agent report`.

## Acceptance criteria (machine-checkable)

1. `cargo build --workspace --release` — clean, no new warnings.
2. `cargo test --release -p strictpy-vm --test m49_tabular_codes` — all pass.
3. `cargo test --release -p strictpy-compiler --test tabular_m49_codes_demo_runs` — passes.
4. **No M37-M47 regressions**: targeted sweeps pass byte-identically.
5. **Bench verification**: `python bench/tabular_harness.py --ops group_by_cat_via_strings --sizes medium_card_5000` produces a StrictPy time that **beats the M48 baseline by at least 5×** (12.8s → <2.5s). Stretch target: <1.5s for ~10× speedup.
6. **Full sweep**: 993 + N - K passing. Net should be at least 993 + 18.

## Constraints — files NOT to modify

- `seed_prelude` in `compiler/src/resolver.rs`.
- M37-M47 tests — must keep passing untouched.
- The 11 existing tabular demos in `examples/` — add a separate `tabular_m49_codes_demo.spy`.
- DO NOT delete `bench/TABULAR_BENCH_REPORT.md` — append to it or add a new M49 section.

## STOP CRITERIA — priority drops if budget runs out

Six priority drops, in order. **Phase B (the codes-hash group_by win) is the must-ship core — never drop**.

1. **Drop Phase D `loc_range_multi_*`** — keep more_resample + outer-merge MultiIndex + unstack-all.
2. **Drop Phase D unstack-all-columns** — keep more_resample + outer-merge MultiIndex.
3. **Drop Phase D outer-merge MultiIndex on either side** — keep more_resample.
4. **Drop Phase D `1M` / `1Y` resample rules** — keep `1w` only (no calendar arithmetic needed).
5. **Drop Phase C ordered categorical + from_codes** — keep merge codes-hash only.
6. **Drop the LANGUAGE_GUIDE rewrite** — orchestrator finishes it.

After applying any drop, document what was cut with a "what M51 should pick up" list. (M51 is the next polish round; M50 sequence is the desktop UI track.)

## Methodology discipline

1. **First commit at ~20% of budget** — Phase A's bench fixture + 1 smoke run + Phase B's first codes-hash detection in `m38_groupby_*`.
2. **Per-phase commits** — 5 commits (A, B, C, D, E). M49 is disjoint-handler — clean per-phase cadence.
3. **Variable prefix `m49_`** for new helpers / locals in shared files.
4. **No new IR opcodes** — pure handler body changes + new constructors + a few new methods.
5. **Edit-tool worktree leak**: precautionary `cp` at session start (M49 is back to bulk-edit-of-shared-Rust-files territory after M48's Python-only round).
6. **Bench verification gate**: don't commit Phase B without re-running the bench and seeing the numeric win. If the codes-hash optimization doesn't beat the M48 baseline by ≥5× on the high-cardinality fixture, something is wrong — debug before committing.

## Final report

Write `docs/thesis/agent_reports/m49_tabular_codes.md` (under 600 words) covering:
- What shipped per phase (A-E)
- What was cut from STOP CRITERIA (if anything)
- LOC delta per touched file
- **Bench numbers (before/after)** for the categorical group_by + merge cells — this is the headline deliverable
- Surprises / design calls (e.g., did the high-cardinality fixture surface anything unexpected? did calendar arithmetic for 1M/1Y need new datetime helpers?)
- "What M51 should pick up" — concrete list (RollingWindow chainable + center=True + sort-by-categories-ordering + anything from M49 STOP CRITERIA)
- LANGUAGE_GUIDE.md update status
- Whether the Edit-tool worktree leak recurred + count + workaround effectiveness

Commit this report in Phase E's commit.

## Commit message shape (final)

```
M49: tabular categorical codes optimization + ordered categorical + polish

The PRIMARY M49 win — driven by the M48 numeric target — is
codes-hash for group_by on ColumnCategorical. M48 measured the
gap at medium-cardinality (12.8s vs pandas 1.04s, 12.3× slower).
M49 expects ~10× speedup (target <1.5s).

Phase A: high-cardinality (medium_card_5000) bench fixture added
  to bench/tabular_harness.py. Baseline measurement captured
  before any optimization.
Phase B: m38_groupby_* family now detects ColumnCategorical key
  columns and hashes on codes directly (i64) instead of routing
  through to_strings(). Single-col + multi-col + mixed-dtype-
  fallback all handled. PRIMARY M49 win.
Phase C: m39_df_merge / m45_merge_build_multiindex extend codes-
  hash to merge when both sides have matching categories[]
  orderings; fall back to string-hash otherwise. New constructors:
  tabular.col_categorical_ordered(values, categories) and
  tabular.col_categorical_from_codes(codes, categories) + new
  predicate cc.is_ordered().
Phase D: more resample rules (1w/1M/1Y with calendar arithmetic
  for month/year); outer-merge MultiIndex on either side
  (extends M46 dtype-mismatch fallback); unstack distributes
  every regular column (M46 only first); loc_range_multi_* on
  innermost MultiIndex level.
Phase E: ~25 new tests + bench re-run with before/after numbers
  + tabular_m49_codes_demo.spy + LANGUAGE_GUIDE updates +
  agent report.

NativeFn IDs 1061-... (3-5 new). Variable prefix m49_.
Tests: 993 → 993+N-K (N new, K M48-bench-update only if any).
```
