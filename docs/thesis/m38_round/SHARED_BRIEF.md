# M38 — `tabular` round-out: typed accessors + aggregations + group-by

## Context

M37 shipped the `tabular` stdlib module (DataFrame + sealed Column hierarchy + I/O + filter + sort) as the first Pandas-shaped data package for StrictPy v0.3. The brief explicitly invoked STOP CRITERIA in Phase C, cutting several useful operations to ship in budget. M38 picks those up and adds aggregations + group-by — the foundation for v0.4 pivot/melt/join.

You are the **20th** of an unbroken streak of Lesson-1-compliant agents (M28 → M37). M37 set the per-phase-commit pattern across ~2800 LOC; M38 follows it but is somewhat smaller (~1400-1700 LOC estimated).

## Files to read FIRST (in order)

1. `LANGUAGE_GUIDE.md` (project root) — §5 `tabular (M37)` subsection + §6.2 + §11
2. `docs/thesis/agent_reports/m37_tabular.md` — the M37 design rationale, especially the "no `get_column(name) -> Column?` because sealed-class return type can't be cleanly chosen at NativeFn time" finding (you will fix this in Phase A with typed accessors)
3. `examples/tabular_demo.spy` — the current end-to-end demo
4. `compiler/src/resolver.rs::register_tabular_module` (search for `m37_register_tabular`) — the class-registration code you will extend
5. `compiler/src/ir.rs::m37_tabular_class_method_native_id_by_name` — the class-method dispatch table you will add to
6. `vm/src/builtins.rs` — search for `m37_alloc_col_i64` and `m37_col_i64_get` for handler shape templates; you will write similar `m38_*` handlers
7. `shared/src/native.rs` — search for `M37TabColI64` to see the NativeFn enum variant block; you will add an M38 block starting at id 880
8. `vm/tests/m37_tabular.rs` — test file shape for your new `vm/tests/m38_tabular_ops.rs`

## Constraints

- **Lesson 1**: first commit at ~20% of budget (Phase A green build + 1 smoke test). Don't proceed without this checkpoint. Don't break the 19-streak.
- **Variable prefix `m38_`** for all new helper functions / locals in shared files (resolver.rs, ir.rs, builtins.rs).
- **NativeFn IDs 880-929** (50 slots reserved). M37 used 830-877.
- **Use the M36 `StdlibItemKind::Class` path** for the new `GroupedDataFrame` class (Phase D). DO NOT touch `seed_prelude`. The existing M37 `register_tabular_module` function is where new module items go.
- **No new crate deps** unless absolutely needed. Hash maps from M5 / Dict are sufficient for group-by.
- **No changes to the M37 surface** other than additions. The 21 M37 tests must continue to pass byte-identically.

## Phase A — Typed accessors + restored Phase C ops (~250-300 LOC)

### Typed DataFrame accessors (resolves the M37 finding)

```python
df.get_column_i64(name: str) -> ColumnI64?         # none if absent OR wrong dtype
df.get_column_f64(name: str) -> ColumnF64?
df.get_column_str(name: str) -> ColumnStr?
df.get_column_bool(name: str) -> ColumnBool?
df.get_column_datetime(name: str) -> ColumnDateTime?
```

**Behavior**: returns `none` if the column doesn't exist OR if it exists with a different dtype. Each is its own NativeFn (5 handlers) because the return type is monomorphic per accessor.

### Restored Phase C comparison ops

```python
# On ColumnI64 + ColumnF64 (4 methods × 2 dtypes = 8 NativeFns):
col.ne(x) -> ColumnBool
col.ge(x) -> ColumnBool
col.le(x) -> ColumnBool
col.between(lo, hi) -> ColumnBool   # inclusive both ends

# On ColumnStr (2 NativeFns):
col.starts_with(prefix: str) -> ColumnBool
col.ends_with(suffix: str) -> ColumnBool

# On DataFrame (1 NativeFn):
df.rename(renames: List[Tuple[str, str]]) -> DataFrame
```

Null propagation: same rule as M37 — null on either side → null in result.

### Commit checkpoint after Phase A

`M38 A: tabular typed accessors + restored Phase C ops`. Build clean + at least one smoke test exercising a typed accessor.

## Phase B — Per-column aggregations (~350-400 LOC)

Numeric aggregations skip null cells. The result is `none` only if ALL cells are null (except `count_*` methods, which always return a concrete i64).

### ColumnI64 (8 methods)

```python
ci.sum() -> i64?       # 0 if all null? No — none. Use 0 only for empty list.
ci.mean() -> f64?      # f64 even though col is i64; none if no non-null cells
ci.min() -> i64?
ci.max() -> i64?
ci.count() -> i64      # non-null cell count
ci.std() -> f64?       # sample std dev (n-1 denominator); none if <2 non-nulls
ci.var() -> f64?       # sample variance; none if <2 non-nulls
ci.median() -> f64?    # median of non-null values; none if all null
```

### ColumnF64 (same 8)

Same signatures but `sum / min / max` return `f64?` instead of `i64?`. NaN cells are NOT treated as null; they participate in computations and propagate normally (a sum containing NaN is NaN). Document this in LANGUAGE_GUIDE.md.

### ColumnStr (3 methods)

```python
cs.count() -> i64               # non-null cell count
cs.min() -> str?                # lexicographic min over non-null
cs.max() -> str?
```

### ColumnBool (4 methods, M37 has count_true/count_false already)

The M37 `count_true / count_false / count_null` are on **ColumnBool used as mask**. Add as proper Column aggregations:

```python
cb.count() -> i64               # non-null cell count
# count_true / count_false / count_null already exist from M37
```

Don't re-add count_true etc.; just `count()`.

### ColumnDateTime (3 methods)

```python
cd.count() -> i64
cd.min() -> i64?                # min epoch-ms
cd.max() -> i64?
```

### Commit checkpoint after Phase B

`M38 B: tabular aggregations — sum/mean/min/max/count/std/var/median per column`. Build clean + tests covering at least sum/mean/min/max on i64 and f64.

## Phase C — `df.describe()` + `Column.fill_null` + `tabular.from_dict` (~200-300 LOC)

### `df.describe() -> DataFrame`

Returns a 7-row × N-col DataFrame summarizing each numeric column. Rows (by index): "count", "mean", "std", "min", "25%", "50%", "max". Stringify all cells (the result DataFrame has all str columns). For non-numeric columns (str/bool/datetime): just the "count" row is populated; others are "".

For quantile rows (25%, 50%, 75%):
- Sort non-null values; linear-interpolated quantile using `(n-1) * q` index
- Drop the 75% row to keep this manageable (just 25% and 50%). Match pandas-lite. If easy to add 75% though, do it — describe is canonical.

Actually, simpler: ship "count / mean / std / min / max" only for v1, with median (50%) optional. Document the simplification.

### `Column.fill_null(value)` per subclass (5 methods)

```python
ci.fill_null(v: i64) -> ColumnI64       # nulls become v; non-nulls unchanged
cf.fill_null(v: f64) -> ColumnF64
cs.fill_null(v: str) -> ColumnStr
cb.fill_null(v: bool) -> ColumnBool
cd.fill_null(v: i64) -> ColumnDateTime  # v is epoch-ms
```

Result column has `nulls = [false; length]` and `values[i] = original_values[i]` if non-null else `v`.

### `tabular.from_dict(d: Dict[str, Column]) -> DataFrame`

Module-level constructor. Column order in the resulting DataFrame is dictionary insertion order (Dict in StrictPy preserves insertion order — verify). All columns must have equal length; raise ValueError otherwise.

### Commit checkpoint after Phase C

`M38 C: tabular describe + fill_null + from_dict`. Build clean + tests covering describe + fill_null.

## Phase D — Group-by (~400-500 LOC, the biggest piece)

### New class: `GroupedDataFrame`

Register as `StdlibItemKind::Class` in `register_tabular_module`. Layout:

```rust
// GroupedDataFrame:
//   parent: DataFrame             (i64 pointer to source frame)
//   group_keys: List[str]          (column names grouped by)
//   group_index_map: i64           (handle into a SharedVm slot table —
//                                   stores Dict<String, Vec<i64>>)
//   group_count: i64
```

Use a `SharedVm` slot table to hold the actual `HashMap<String, Vec<usize>>` (group-key string → row indices). The class instance carries an i64 handle into this table (mirrors how M35 P4-A Pattern stores a slot handle).

Multi-column group keys are serialized to a string via `\x01`-joined values of the grouped columns at each row. Null-keyed groups: nulls in any group-key column put that row into a synthesized "null-group" bucket — that's pandas's `dropna=False` behavior. (Pandas default is `dropna=True` but for v1 keeping null groups is simpler and correct.)

### Surface

```python
# Module-level on tabular (1 NativeFn):
df.group_by(cols: List[str]) -> GroupedDataFrame    # method on DataFrame

# Method on GroupedDataFrame:
gdf.size() -> DataFrame                 # group-key columns + a "size" i64 column
gdf.keys() -> DataFrame                 # just the group-key columns, one row per group

# Aggregation shortcuts (apply to all numeric columns NOT in group keys):
gdf.sum() -> DataFrame
gdf.mean() -> DataFrame
gdf.min() -> DataFrame
gdf.max() -> DataFrame
gdf.count() -> DataFrame

# Custom agg via spec list (1 NativeFn):
gdf.agg(specs: List[Tuple[str, str]]) -> DataFrame
# specs example: [("price", "sum"), ("qty", "mean")] applies sum to price and
# mean to qty; output DataFrame has group-key columns + one column per spec.
# Agg name strings: "sum" | "mean" | "min" | "max" | "count" | "std" | "var" | "median"
```

### Algorithm

1. Build group_index_map: iterate source frame rows, compute serialized key (`\x01`-join), append row index to the bucket.
2. For each output method, iterate the buckets in insertion order, build an aggregated DataFrame:
   - Output column for each group-key: take group key's serialized string, split on `\x01`, parse each part back to its original dtype (or just keep the values from one row in the bucket — simpler)
   - For shortcuts (`sum`/`mean`/...): apply per-column aggregation to non-key columns over the bucket's row indices
   - For `agg(specs)`: apply the named aggregation to the named column

### Commit checkpoint after Phase D

`M38 D: tabular group_by + GroupedDataFrame.{size,keys,sum,mean,min,max,count,agg}`. Build clean + tests for at least size/keys/sum.

## Phase E — Tests + demo + docs (~200-250 LOC)

### Tests (`vm/tests/m38_tabular_ops.rs`)

Aim for 22-30 tests. Cover at minimum:
- Phase A: each typed accessor (hit and miss), `between`, `ne`/`ge`/`le`, `starts_with`/`ends_with`, `rename`
- Phase B: sum / mean / min / max on i64 + f64; median; std/var sanity (specific input expected output); count semantics (null-skip)
- Phase C: describe shape (rows/cols match expectations), fill_null produces null-free column, from_dict round-trip
- Phase D: group_by single-column / multi-column; size; sum/mean shortcuts; agg with custom specs

### Demo

Add a new `examples/tabular_groupby_demo.spy` (~80-120 lines) — realistic walkthrough that:
- Loads a CSV with 4 columns (`category: str`, `qty: i64`, `price: f64`, `date: datetime`)
- Filters then groups by category
- Computes sum + mean + count
- Sorts by total
- Prints the result

The new demo should be testable via `compiler/tests/tabular_groupby_demo_runs.rs`.

### LANGUAGE_GUIDE.md update

Extend §5 `tabular (M37, extended by M38)` to document the new surface. Add a sub-block per phase. Update §6.2 prelude table if you add new class names (GroupedDataFrame). Add a §11 entry on f64 NaN semantics in aggregations (NaN propagates, doesn't skip). Bump banner to "post-M38".

### Commit checkpoint after Phase E

`M38 E: tabular round-out — tests + group-by demo + LANGUAGE_GUIDE update + agent report`.

## Acceptance criteria (machine-checkable)

1. `cargo build --workspace --release` — clean, no warnings.
2. `cargo test --release -p strictpy-vm --test m38_tabular_ops` — all tests pass.
3. `cargo test --release -p strictpy-compiler --test tabular_groupby_demo_runs` — passes.
4. **No M37 regressions**: `cargo test --release -p strictpy-vm --test m37_tabular` (the 19 tests) all pass byte-identically. `cargo test --release -p strictpy-compiler --test tabular_demo_runs` (the 2 tests) pass byte-identically.
5. **Full sweep**: `cargo test --workspace --release --no-fail-fast` reports 744 + N passing where N is the new M38 test count (target 22-30 tests), 0 failing, 1 ignored.
6. `examples/tabular_groupby_demo.spy` runs to completion with deterministic output.

## Constraints — files NOT to modify

- `seed_prelude` in `compiler/src/resolver.rs` — use the M37 `register_tabular_module` (or its equivalent) instead.
- The 21 M37 test cases — they must keep passing untouched.
- `examples/tabular_demo.spy` — leave the M37 demo alone; add a separate `tabular_groupby_demo.spy`.

## STOP CRITERIA — priority drops if budget runs out

Five priority drops, in order:

1. **Drop Phase D `gdf.agg(specs)`** — keep the shortcuts (sum/mean/min/max/count). The shortcut surface covers ~80% of group-by use cases.
2. **Drop Phase D entirely** — ship A + B + C without group-by. Big cut (~30%) but most-used features remain. M39 picks group-by up.
3. **Drop `df.describe()`** in Phase C — it's the most code-heavy non-essential piece.
4. **Drop std / var / median** in Phase B — keep sum/mean/min/max/count. Cuts ~15%.
5. **Drop `tabular.from_dict`** — smallest, easiest to defer.

After applying any drop, your final report must document what was cut with a "what M39 should pick up" list. The Lesson 1 streak is more important than feature completeness.

## Methodology discipline

1. **First commit at ~20% of budget** (Phase A green build + 1 smoke). Don't proceed without this checkpoint.
2. **Per-phase commits** — 5 commits expected. Each builds clean with targeted tests passing.
3. **Variable prefix `m38_`** for all new helper functions / locals in shared files.
4. **No exhaustive-match breakage**: name-based dispatch in `ir.rs` (mirror `m37_tabular_class_method_native_id_by_name`). Do NOT add new IR opcodes.

## Final report

Write `docs/thesis/agent_reports/m38_tabular_ops.md` (under 600 words) covering:
- What shipped per phase (A-E)
- What was cut from STOP CRITERIA (if anything)
- LOC delta per touched file
- Final test count + verification
- Surprises / design calls (e.g., null-group key handling in group_by, NaN propagation in f64 sums)
- "What M39 should pick up" — concrete follow-up list (pivot? joins? more aggregations?)
- LANGUAGE_GUIDE.md update status

Commit this report in Phase E's commit.

## Commit message shape (final)

```
M38: tabular round-out — typed accessors + aggregations + group-by

Picks up the M37 STOP CRITERIA debt and adds aggregations +
hash-based group-by. Builds on the M37 sealed-class layout; no
prelude additions (uses M36 StdlibItemKind::Class path).

Phase A: typed get_column_* accessors (5) + restored Phase C ops
(between/ne/ge/le on numeric; starts_with/ends_with on str;
df.rename).
Phase B: per-column aggregations — sum/mean/min/max/count on all
types; std/var/median on numeric. NaN propagation on f64
(documented).
Phase C: df.describe() + Column.fill_null per subclass +
tabular.from_dict.
Phase D: new GroupedDataFrame class + df.group_by(cols) + size/
keys/sum/mean/min/max/count shortcuts + agg(specs).

NativeFn IDs 880-929. Variable prefix m38_.

Tests: 744 → 744+N. Examples: +1 (tabular_groupby_demo.spy).
```
