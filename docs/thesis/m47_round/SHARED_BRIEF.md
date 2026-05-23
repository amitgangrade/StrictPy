# M47 — `tabular` v0.4 polish: iloc 2-D + negative iloc + rolling Welford/min_periods + ColumnCategorical

## Context

M46 closed the `tabular` v1 surface. M47 is the v0.4 polish round — the smaller items that didn't fit M46's scope. After M47 the only remaining `tabular` work is:

- **Desktop UI** (M37-design Phase 6) — its own substantial milestone, deferred separately.
- **More resample rules** (`1w` / `1M` / `1Y`) — needs a calendar arithmetic layer, deferred.
- **Categorical optimized codes paths** — M47 ships ColumnCategorical via to_strings coercion; optimized codes-based hashing for group_by/merge is M48 work.

M47's actual scope:

1. **`df.iloc[rows, cols]` 2-D indexing** — extends M40's row-range `iloc(start, stop)` with an optional column slice.
2. **Negative-index support for `iloc`** — pandas accepts `-1` to mean "last row"; M47 lifts the v1 rejection.
3. **Rolling Welford std (internal)** — replaces the M40 sum + sum-of-squares formula for `rolling_std` with Welford's incremental algorithm. No API change; better numerical stability for large windows / large values.
4. **Rolling `min_periods` variants** — `rolling_{sum,mean,min,max,std}_min_periods(window, min_periods)` per dtype. Returns null only when the window has fewer than `min_periods` non-null values (instead of always-null for the first `window-1` cells).
5. **`ColumnCategorical` dtype** — new sealed Column subclass: stores `codes: List[i64]` + `categories: List[str]`. v1 implementation: every operation that doesn't have a specific categorical handler coerces to ColumnStr via `to_strings()`. Optimized codes-based paths for group_by hashing, merge equality, etc. are M48 follow-up.

You are the **29th** of an unbroken Lesson-1-compliant agent streak (M28 → M46). M47 phases are disjoint — first commit at ~20% of budget.

## Files to read FIRST (in order)

1. `LANGUAGE_GUIDE.md` (project root) — §5 tabular subsection M37-M46 + §11.26-§11.34
2. `docs/thesis/agent_reports/m46_tabular_extensions.md` — most recent recipe pattern + the leak hypothesis-refutation
3. `docs/thesis/agent_reports/m40_tabular_timeseries.md` — the original rolling-window + iloc implementations you're extending
4. `docs/thesis/agent_reports/m44_tabular_multiindex.md` — Column hierarchy (you're adding a new sealed Column subclass)
5. `examples/tabular_m46_extensions_demo.spy` — M46's end-to-end demo
6. `vm/src/builtins.rs` — find:
   - `m40_df_iloc` — you'll extend with column slice + negative-index support
   - `m40_col_rolling_*` — find the existing rolling implementations (sum/mean/min/max/std); you'll add `_min_periods` variants + refactor std to Welford internally
   - `m37_alloc_col_i64` / similar — the existing Column allocation pattern (you'll add `m47_alloc_col_categorical`)
   - The `m44_permute_multiindex_into_df` / `m45_copy_multiindex_into_df` helpers — your new ColumnCategorical column has to route through these correctly
   - `m37_build_df` / `m41_build_df_with_index` / `m44_build_df_with_multiindex` — constructors that need to handle categorical columns
7. `compiler/src/resolver.rs` — find the existing Column hierarchy sealed-class registration in `register_tabular_module`; add `ColumnCategorical` as a new sealed subclass
8. `shared/src/native.rs` — find the M46 NativeFn range (ends at 1042); add M47 block from 1043

## Constraints

- **Lesson 1**: first commit at ~20% of budget. 28-streak — don't break it.
- **Variable prefix `m47_`** for new helpers / locals.
- **NativeFn IDs `1043-1064`** reserved (22 slots). M47 expected to use ~16-18.
- **No payload changes** to DataFrame (the 56-byte M44 layout still works for all M47 cases).
- **One new sealed-class subclass** (`ColumnCategorical`). The existing Column hierarchy registration in `resolver.rs` needs to be extended. The match-arm dispatch in every per-dtype handler (`get_column_*`, comparison ops, `is_null`, `get`, etc.) needs to include `ColumnCategorical` — most arms route through `to_strings()` for v1 simplicity.
- All 248 existing tabular tests must keep passing — except any tests that explicitly assert iloc rejects negative indices (those flip). List flips.

### Edit-tool worktree leak — defensive measure

Per M44/M46: precautionary `cp` at session start, per-file `cp` recovery mid-session if `git status` shows project-root diffs. M45/M46 cycle showed the leak is intermittent — defensive copy is cheap insurance, do it.

```bash
for f in vm/src/builtins.rs compiler/src/resolver.rs compiler/src/ir.rs \
         shared/src/native.rs LANGUAGE_GUIDE.md; do
    cp /c/Users/AG/CascadeProjects/PythonCompiler/$f $f 2>/dev/null
done
```

## Phase A — `df.iloc` 2-D indexing + negative iloc (~200-300 LOC)

### New 2-D iloc

```python
df.iloc_2d(row_start: i64, row_stop: i64,
           col_start: i64, col_stop: i64) -> DataFrame
```

Half-open `[row_start, row_stop) × [col_start, col_stop)` slice. Both clamped to bounds. Returns a DataFrame with the row slice AND the column slice applied. Index propagation: same as M40 iloc (preserves index per the M42/M44/M45 recipe).

Use sentinel values `-1` for "no slicing on this axis" → take all rows / all columns. (E.g., `iloc_2d(0, 10, -1, -1)` = `iloc(0, 10)` on all columns.) Actually no, that's confusing. Better: distinct method that always slices both dimensions. Users wanting "row slice only" still call `iloc(start, stop)`.

### Negative-index support for `iloc(start, stop)`

Extend M40's `iloc` (existing NativeFn) to accept negative indices via Python semantics: `-1` = last row, `-N` = `nrows - N`. Negative + positive can mix (`iloc(-3, -1)` = last 2 rows). Out-of-range still raises ValueError as today, but `iloc(-3, nrows)` is valid.

This extends an EXISTING handler — flips any test that asserts negative-iloc rejects (likely 1 test from M40).

### Negative-index support for `iloc_2d`

Same semantics on both axes.

### NativeFn IDs

- `1043`: `iloc_2d`

### Commit checkpoint after Phase A

`M47 A: tabular iloc 2-D + negative iloc`. Build clean + 4-6 tests covering iloc_2d happy path, both axes negative, row-only negative iloc, ValueError on truly-OOB inputs.

## Phase B — Rolling Welford std + min_periods variants (~400-500 LOC)

### Welford std (internal — no API change)

M40's `rolling_std` uses the simple "sum of values + sum of squares → variance" formula. For large windows / large values this loses precision catastrophically (Wikipedia: "naive variance" is the bad version). Replace with **Welford's online algorithm**:

```
mean_n = mean_{n-1} + (x - mean_{n-1}) / n
M2_n   = M2_{n-1} + (x - mean_{n-1}) * (x - mean_n)
variance = M2_n / (n - 1)  // sample
```

For rolling-window: maintain Welford state over the window. When the window slides, you need to "remove" the leaving cell + "add" the entering cell. This is the tricky part — Welford doesn't have an exact remove operation. Two options:

**Option 1**: Recompute Welford state from scratch over the current window each step. O(window) per output cell vs M40's O(1). Acceptable for small windows; slow for large.

**Option 2**: Use the West & Welford "running variance for windowed data" formula (1979) which has the proper remove op. ~20 lines of math, better asymptotic complexity.

**Recommendation**: go with Option 1 for v1 — simpler, more obviously correct. Document the O(n·w) cost in agent report. Option 2 can be M48 if it matters.

Apply Welford internally in `rolling_std` (i64 + f64). Existing tests should still pass with bit-identical (or numerically-close-enough) results for small inputs.

### `rolling_*_min_periods(window, min_periods)` variants

For each of 5 rolling methods × 2 dtypes = 10 NativeFns:

```python
ci.rolling_sum_min_periods(window: i64, min_periods: i64) -> ColumnI64
ci.rolling_mean_min_periods(window: i64, min_periods: i64) -> ColumnF64
ci.rolling_min_min_periods(window: i64, min_periods: i64) -> ColumnI64
ci.rolling_max_min_periods(window: i64, min_periods: i64) -> ColumnI64
ci.rolling_std_min_periods(window: i64, min_periods: i64) -> ColumnF64
# Same on ColumnF64
```

Behavior: at each output position `i`, count non-null cells in the window `[i-window+1, i]` (or `[0, i]` if `i < window-1`). If the count is `>= min_periods`, emit the aggregate; else emit null.

Valid `min_periods` range: `1 <= min_periods <= window`. Out of range raises ValueError.

### NativeFn IDs

- `1044-1053`: 10 `rolling_*_min_periods` methods (i64 + f64 × 5 ops).

### Commit checkpoint after Phase B

`M47 B: tabular rolling Welford std + min_periods variants`. Build clean + tests covering: rolling_std numerical stability (small input, bit-equivalent to M40); each rolling_*_min_periods on i64 + f64 with min_periods=1, =window/2, =window, > window (ValueError).

## Phase C — `ColumnCategorical` dtype (~500-600 LOC)

### Class design

New sealed Column subclass:

```rust
final class ColumnCategorical extends Column {
    codes: List[i64]            // per-cell integer index into categories[]
    categories: List[str]        // distinct values, ordered by first appearance
    nulls: List[bool]            // standard null-mask (codes[i] is undefined when nulls[i])
    length: i64                  // cached
}
```

Register in `register_tabular_module` as a sealed subclass of `Column`, alongside ColumnI64/F64/Str/Bool/DateTime.

### Construction

```python
tabular.col_categorical(values: List[str]) -> ColumnCategorical
# Builds categories by first-appearance order, populates codes accordingly.
# All inputs treated as non-null.

tabular.col_categorical_with_nulls(values: List[str],
                                    nulls: List[bool]) -> ColumnCategorical
# Null cells get codes[i] = 0 (don't-care; nulls mask controls).
```

### Surface

```python
cc.length() -> i64
cc.dtype() -> str                  # "categorical"
cc.is_null(i: i64) -> bool
cc.null_count() -> i64
cc.get(i: i64) -> str?             # returns the category string (or none)
cc.codes() -> ColumnI64            # the underlying code column
cc.categories() -> ColumnStr       # the distinct values (ordered)
cc.to_strings() -> ColumnStr       # full materialization to a string column
```

### DataFrame accessor

```python
df.get_column_categorical(name: str) -> ColumnCategorical?
```

### Existing-ops integration via `to_strings()` coercion

Every existing Column-dispatching op that hits a `ColumnCategorical` instance routes through `to_strings()` internally as the v1 simplification. Specifically:

- **group_by**: when a key column is categorical, hash on the materialized strings (slow but correct). M48 optimizes to hash on codes.
- **merge**: same — categorical join keys compared via strings.
- **filter**: ColumnBool masks work unchanged (categorical is just str-shaped to the consumer).
- **sort_by**: comparator-on-categorical uses string ordering (alphabetical), NOT the categories[] declaration order. Document this v1 behavior; pandas's ordered-categorical sort is M48.
- **show()**: prints the category string per cell.
- **pivot / pivot_table / melt / concat**: handle categorical inputs by treating as ColumnStr.
- **unique / value_counts**: trivially work via the string view.

### What NOT to ship (M48 work)

- Optimized group_by hashing on codes.
- Optimized merge using codes (only valid when both sides have the same categories ordering).
- Ordered categorical (categories ordering matters for sort).
- `pandas.Categorical.from_codes` reverse constructor.
- Categorical group_by promotion (currently single-col promotes — categorical key column promotes to ColumnCategorical index; M48 may want it as ColumnStr index for compatibility).

### NativeFn IDs

- `1054`: `col_categorical(values)`
- `1055`: `col_categorical_with_nulls(values, nulls)`
- `1056`: `cc.codes() -> ColumnI64`
- `1057`: `cc.categories() -> ColumnStr`
- `1058`: `cc.to_strings() -> ColumnStr`
- `1059`: `df.get_column_categorical(name)`

(Existing per-Column methods like length/dtype/is_null/null_count/get extend via dispatch in the existing match arms — no new NativeFns for these.)

### Commit checkpoint after Phase C

`M47 C: tabular ColumnCategorical dtype (via to_strings coercion in v1)`. Build clean + tests covering: col_categorical happy path; to_strings round-trip; categorical column in a DataFrame; group_by + merge work on categorical (string-coerced). df.get_column_categorical hit/miss/wrong-dtype.

## Phase D — Tests + demo + LANGUAGE_GUIDE + agent report (~250-300 LOC)

### Tests (`vm/tests/m47_tabular_polish.rs`)

Aim for 22-28 tests. Cover:
- Phase A: iloc_2d happy path; both axes negative; negative iloc(start, stop) extended; ValueError on truly-OOB.
- Phase B: rolling_std Welford produces bit-identical (or near-bit-identical) results to M40 on small inputs; each rolling_*_min_periods method × 2 dtypes (smoke); min_periods=1 emits even on incomplete windows; min_periods=window matches today's M40 behavior.
- Phase C: col_categorical construction; codes() returns expected i64 column; categories() returns distinct-value str column; to_strings() round-trips; ColumnCategorical in a DataFrame's columns; group_by on categorical column works; merge on categorical join key works; sort_by on categorical (alphabetical by string); df.get_column_categorical.

### Tests to flip

Search M40 iloc tests for any "negative start raises" assertion. M47 lifts this — flip the test to verify negative semantics work.

### Demo

Add `examples/tabular_m47_polish_demo.spy` (~100-130 LOC) — a workflow exercising the new pieces:
1. Load CSV with a "region" column
2. `tabular.col_categorical(...)` to wrap region as categorical
3. group_by(["region"]).sum() — works via string coercion
4. rolling_mean_min_periods(window=5, min_periods=3) on a numeric column — see leading partial windows fill in
5. iloc_2d(-5, -1, 0, 3) — last 4 rows × first 3 columns
6. Print with index_nlevels checks

Testable via `compiler/tests/tabular_m47_polish_demo_runs.rs`.

### LANGUAGE_GUIDE.md update

§5 tabular gets an "M47 additions" subsection covering iloc_2d + negative iloc + rolling_*_min_periods + ColumnCategorical (with the v1-coercion note). §11.35 (new): negative iloc semantics; §11.36 (new): ColumnCategorical sort uses string ordering (M48 will add ordered-categorical).

Bump banner to post-M47.

### Commit checkpoint after Phase D

`M47 D: tabular polish — tests + demo + LANGUAGE_GUIDE update + agent report`.

## Acceptance criteria (machine-checkable)

1. `cargo build --workspace --release` — clean, no new warnings.
2. `cargo test --release -p strictpy-vm --test m47_tabular_polish` — all pass.
3. `cargo test --release -p strictpy-compiler --test tabular_m47_polish_demo_runs` — passes.
4. **No M37-M45 regressions**: targeted sweeps pass byte-identically.
5. **M40 mostly unchanged** except the 1 expected iloc-negative test flip.
6. **M46 unchanged**: all 25 + 2 demo tests pass.
7. **Full sweep**: 961 + N - K passing (N new M47, K flipped tests, likely K=1). Net should be at least 961 + 18.

## Constraints — files NOT to modify

- `seed_prelude` in `compiler/src/resolver.rs`.
- M37-M46 tests — must keep passing untouched.
- Only flip M40 tests that explicitly assert iloc rejects negative indices — document.
- The 10 existing tabular demos — add a separate `tabular_m47_polish_demo.spy`.

## STOP CRITERIA — priority drops if budget runs out

Five priority drops, in order:

1. **Drop `ColumnCategorical`** (Phase C) — biggest single piece, mostly net-new class machinery. M48 can take both this + the optimized paths.
2. **Drop `rolling_*_min_periods` for std** — keep sum/mean/min/max (8 methods instead of 10). std with min_periods is the least-used variant.
3. **Drop `iloc_2d`** — keep negative iloc on existing `iloc(start, stop)`. iloc_2d alone is the next slice up.
4. **Drop Welford std internal refactor** — keep the M40 naive formula; document the precision risk for large inputs.
5. **Drop the LANGUAGE_GUIDE rewrite** — orchestrator finishes it.

After applying any drop, document what was cut with a "what M48 should pick up" list.

## Methodology discipline

1. **First commit at ~20% of budget** (M47 is disjoint-phase work).
2. **Per-phase commits** — 4 commits (A, B, C, D).
3. **Variable prefix `m47_`** for new helpers.
4. **No new IR opcodes** — handler bodies + new NativeFn registrations + 1 new sealed-class subclass registration.
5. **Edit-tool worktree leak**: precautionary `cp` at session start + per-file recovery if symptoms appear. Leak is intermittent per M45/M46.

## Final report

Write `docs/thesis/agent_reports/m47_tabular_polish.md` (under 600 words) covering:
- What shipped per phase (A-D)
- What was cut from STOP CRITERIA (if anything)
- LOC delta per touched file
- Final test count + verification + list of M40 tests flipped (iloc-rejects-negative pattern)
- Surprises / design calls (e.g., how did the new ColumnCategorical match-arm extension fan out? Did Welford-vs-naive produce different results on any existing tests?)
- "What M48 should pick up" — concrete list (categorical optimized codes paths, more resample rules with calendar layer, df.rolling chainable, desktop UI, anything from M47 STOP CRITERIA)
- LANGUAGE_GUIDE.md update status
- Whether the Edit-tool worktree leak recurred + count + workaround effectiveness

Commit this report in Phase D's commit.

## Commit message shape (final)

```
M47: tabular polish — iloc 2-D + negative iloc + rolling Welford/min_periods + ColumnCategorical

The v0.4 polish round after M46 closed the v1 surface. Smaller
items that didn't fit M46's scope.

Phase A: df.iloc_2d(row_start, row_stop, col_start, col_stop)
  half-open 2-D slice; negative iloc semantics on both
  iloc(start, stop) and iloc_2d (extends M40 v1 reject-negative).
Phase B: rolling_std now uses Welford's online algorithm
  internally for numerical stability (Option 1 — recompute over
  window each step; Option 2 windowed remove deferred). 10 new
  rolling_*_min_periods variants (sum/mean/min/max/std × i64+f64)
  emit null when window has < min_periods non-nulls.
Phase C: ColumnCategorical sealed subclass — codes: List[i64] +
  categories: List[str] + standard nulls mask. All existing ops
  coerce via to_strings() in v1; optimized codes paths deferred
  to M48.
Phase D: ~25 new tests + tabular_m47_polish_demo.spy +
  LANGUAGE_GUIDE.md §11.35/§11.36 new + agent report.

NativeFn IDs 1043-1059 (17 new). Variable prefix m47_.
Tests: 961 → 961+N-K (N new, K flipped from M40).

After M47 the tabular polish list is largely complete. Remaining
v0.4 items: optimized categorical codes paths (M48), more
resample rules (1w/1M/1Y — needs calendar layer), df.rolling
chainable, desktop UI Phase 6 (its own milestone).
```
