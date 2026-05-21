# M37 — `tabular` stdlib module (DataFrame + Column hierarchy + I/O + filter + sort)

## Context

StrictPy is a statically typed Python dialect compiled to bytecode + JIT'd via Cranelift (project root `C:\Users\AG\CascadeProjects\PythonCompiler`, Rust workspace). Status as of this brief: M0–M36 complete, 723 tests passing, 37 stdlib modules, 100 example programs.

This is the first milestone of a multi-phase Pandas-shaped data package. The user explicitly chose "from-scratch native" (real pandas can't import — see `LANGUAGE_GUIDE.md` §11.11) and picked **module name `tabular`** to avoid the confusion of `import pandas` not meaning real pandas. M37 covers what the design discussion called Phase 1 + Phase 2 — core types, I/O, filtering, and sorting.

**Why now**: M36 just landed the `StdlibItemKind::Class` infrastructure. New stdlib classes can now register module-scoped from the start (no prelude bloat). M37 is the first stdlib package to use this path natively — your work also validates the M36 refactor.

## Files to read FIRST (in order)

1. `LANGUAGE_GUIDE.md` (project root) — start with §6.2 Prelude classes, §5 stdlib reference structure, §11.11 (no NumPy/pandas import); skim §3-§4 to understand the language surface
2. `docs/thesis/agent_reports/m34_json_value.md` — the JsonValue sealed-class pattern your work mirrors
3. `docs/thesis/agent_reports/m35_p4b_sqlite_class.md` + `m35_p4c_hashlib_streaming.md` — Connection/Cursor (which you will reuse) + Hasher slot-table patterns
4. `docs/thesis/agent_reports/m36_stdlib_class_refactor.md` — the new `StdlibItemKind::Class` path you should use
5. `compiler/src/resolver.rs` — focus on:
   - `StdlibItemKind` enum + the `Class { class_id }` variant (recently added in M36)
   - `seed_stdlib_modules` for module registration shape (around line 388+)
   - `seed_prelude` lines 4329-4774 — read JsonValue / Pattern / Connection / Hasher class registrations as the template you will mirror (but you will register your classes in a stdlib module, NOT the prelude)
6. `compiler/src/ir.rs` — the M34/M35 class-method dispatch helpers (`m34_json_class_method_native_id_by_name`, `m35_p4b_sqlite_class_method_native_id_by_name`, `m35_re_pattern_method_native_id_by_name`) around lines 4397-4481. You will add `m37_tabular_class_method_native_id_by_name` here.
7. `vm/src/builtins.rs` — the M34/M35 handler bodies for `JsonJObject*` / `Pattern*` / `Sqlite3*` / `Hasher*`. Template for your `m37_*` handlers.
8. `examples/json_typed_demo.spy` + `examples/sqlite_class_demo.spy` + `examples/hashlib_streaming_demo.spy` — what an idiomatic StrictPy demo for a new class-shaped stdlib looks like

## Constraints you should know going in

- **Use the M36 `StdlibItemKind::Class` path. Do NOT register your classes in `seed_prelude`.** Register them as `StdlibItem { kind: Class { class_id }, … }` on the `tabular` stdlib module. The M34/M35 classes still live in the prelude for back-compat (the M36 honest-debt) — your work is the FIRST stdlib class set to use the post-M36 canonical path.
- **Lesson 1 streak is at 18**: commit before 60% of your time budget. Don't break the streak. Commit per phase below.
- **Lesson 2: variable prefix `m37_`** for all new helper functions / locals / slot tables in shared files.
- **NativeFn IDs: use 830-879** (50 slots reserved for M37). Existing usage stops at 829 (M35 P4-C Hasher).
- **No new crate deps** unless absolutely necessary. The base toolkit (List, Dict, csv, sqlite3, datetime) covers everything.
- **NA semantics: per-column null mask.** Every Column has `values: List[T]` and `nulls: List[bool]` of equal length; `nulls[i] == true` means the i-th cell is NA. NOT a NaN sentinel; NOT `T?` per cell.
- **DataFrame is row-major in API, column-major in storage.** Columns are first-class; row access stringifies for now.

## Phase A — Sealed Column hierarchy + DataFrame core (~400-500 LOC)

### Class layouts (register via M36 path)

```
sealed class Column                                   # base (open=false, sealed=true)
final class ColumnI64(Column)
final class ColumnF64(Column)
final class ColumnStr(Column)
final class ColumnBool(Column)
final class ColumnDateTime(Column)                    # see "scope-down option" below
final class DataFrame                                  # NOT a Column subclass
```

Each `Column*` subclass stores (registered as ClassLayout fields):
- `values: List[T]` (T = i64 / f64 / str / bool / i64 (datetime as epoch-ms))
- `nulls: List[bool]` (same length as values)
- `length: i64` (cached)

`DataFrame` stores:
- `names: List[str]` (column names in order)
- `columns: List[Column]` (parallel to names)
- `nrows: i64` (cached row count; all columns must have this length)

### Surface (methods on each class + module-level constructors)

```python
# Column construction helpers (module-level):
tabular.col_i64(values: List[i64], nulls: List[bool]) -> ColumnI64
tabular.col_i64_simple(values: List[i64]) -> ColumnI64     # nulls all false
tabular.col_f64(values: List[f64], nulls: List[bool]) -> ColumnF64
tabular.col_f64_simple(values: List[f64]) -> ColumnF64
tabular.col_str(values: List[str], nulls: List[bool]) -> ColumnStr
tabular.col_str_simple(values: List[str]) -> ColumnStr
tabular.col_bool(values: List[bool], nulls: List[bool]) -> ColumnBool
tabular.col_bool_simple(values: List[bool]) -> ColumnBool
tabular.col_datetime(values: List[i64], nulls: List[bool]) -> ColumnDateTime  # values: epoch ms

# DataFrame construction:
tabular.from_columns(names: List[str], cols: List[Column]) -> DataFrame
# (List[Column] works because of sealed dispatch — store as base, dispatch at use)

# Column shared methods (defined on each subclass; same signatures):
col.length() -> i64
col.dtype() -> str                # "i64" | "f64" | "str" | "bool" | "datetime"
col.is_null(i: i64) -> bool       # bounds-checked
col.null_count() -> i64

# Per-subclass typed accessors:
ColumnI64.get(i: i64) -> i64?     # none if null; raises IndexError if oob
ColumnF64.get(i: i64) -> f64?
ColumnStr.get(i: i64) -> str?
ColumnBool.get(i: i64) -> bool?
ColumnDateTime.get_ms(i: i64) -> i64?

# DataFrame inspection:
df.shape() -> Tuple[i64, i64]              # (nrows, ncols)
df.length() -> i64                          # nrows
df.ncols() -> i64
df.columns() -> List[str]
df.dtypes() -> List[str]
df.get_column(name: str) -> Column?         # none if absent
df.has_column(name: str) -> bool
df.show(n: i64) -> str                      # ASCII table; n = rows to show, -1 = all
```

### ASCII table for `show()`

Use M22 / M27 stdlib for formatting. Spec for a 2-col / 3-row DataFrame:
```
+-----+-------+
| age |  name |
+-----+-------+
|  30 | alice |
|  25 |   bob |
|  40 | carol |
+-----+-------+
[3 rows x 2 columns]
```
Right-align numeric, left-align string, "null" for null cells. Truncate long string cells at 20 chars with "...". Show only first `n` rows; if `n < nrows`, append `... (k more rows)` line.

### STOP CRITERIA for Phase A

If the sealed-class registration via `StdlibItemKind::Class` runs into M36-related issues you can't resolve in <30 min, drop `ColumnDateTime` (it's the most complex — needs M23 datetime integration). Ship 4 column types + DataFrame; flag the gap in your report.

### Commit checkpoint after Phase A

Commit: "M37 A: tabular module — sealed Column hierarchy + DataFrame core". Code must build clean and at least one smoke test must pass.

## Phase B — I/O (~300-400 LOC)

```python
# Schema = List[Tuple[col_name, dtype_string]] where dtype_string in {"i64", "f64", "str", "bool", "datetime"}
tabular.read_csv(path: str, schema: List[Tuple[str, str]]) -> DataFrame
tabular.write_csv(path: str, df: DataFrame) -> None
tabular.from_sql(cur: Cursor, schema: List[Tuple[str, str]]) -> DataFrame    # reuses M35 Cursor

# Row construction (the building block for the above):
tabular.from_rows(rows: List[List[str]], schema: List[Tuple[str, str]]) -> DataFrame
```

**`read_csv` behavior**:
- Read whole file via M22 `csv.read_file(path)` returning `List[List[str]]`
- First row is the header; assert it matches `schema` column names (order-sensitive)
- Parse each subsequent row per `schema` dtype: `i64` via `int_parse` or equivalent; `f64` via `float_parse`; `str` as-is; `bool` accepts "true"/"True"/"TRUE"/"1" → true and same for false; `datetime` accepts ISO-8601 (use M23 datetime parser) → epoch ms
- Empty string in any cell → null in that column (set `nulls[i] = true`, push 0/0.0/""/false/0 placeholder into `values`)
- Returns a `DataFrame` with columns in schema order

**`write_csv` behavior**:
- First row is column names from `df.columns()`
- Subsequent rows: stringify each cell, "" for nulls, ISO-8601 for datetime
- Use M22 `csv.format_row` for proper escaping

**`from_sql` behavior**:
- Loop over `cur.fetchone()` until none
- Each row is `List[str]` (sqlite3 stringifies)
- Apply same schema-driven parsing as `read_csv`
- Returns a `DataFrame`

### STOP CRITERIA for Phase B

If `csv.read_file` integration has issues (unlikely — it's an existing stable function), ship `read_csv` + `from_rows` alone; drop `write_csv` + `from_sql`. The user can re-add those in M38.

### Commit checkpoint after Phase B

Commit: "M37 B: tabular I/O — read_csv + write_csv + from_sql".

## Phase C — Filtering, projection, row ops (~300-400 LOC)

### Per-Column comparison methods (return ColumnBool)

```python
# On ColumnI64:
ci.eq(x: i64) -> ColumnBool
ci.ne(x: i64) -> ColumnBool
ci.gt(x: i64) -> ColumnBool
ci.lt(x: i64) -> ColumnBool
ci.ge(x: i64) -> ColumnBool
ci.le(x: i64) -> ColumnBool
ci.between(lo: i64, hi: i64) -> ColumnBool       # inclusive both ends

# On ColumnF64 — same 7 methods, f64 args
# On ColumnStr — eq / ne / starts_with(prefix) / contains(needle)
# On ColumnBool — eq(x: bool) only
# On ColumnDateTime — eq / gt / lt / between (i64 epoch-ms args)
```

**Null propagation rule**: if either side of a comparison involves a null cell, the resulting `ColumnBool` cell is null (not false). The `nulls` mask of the result is the OR of input nulls.

### ColumnBool combinators

```python
mask.and_(other: ColumnBool) -> ColumnBool
mask.or_(other: ColumnBool) -> ColumnBool
mask.not_() -> ColumnBool
mask.count_true() -> i64                          # nulls treated as false
mask.count_false() -> i64
mask.count_null() -> i64
```

### DataFrame filter / projection / row ops

```python
df.filter(mask: ColumnBool) -> DataFrame          # keep rows where mask is true (drop nulls)
df.select(cols: List[str]) -> DataFrame           # error if any col absent
df.drop(cols: List[str]) -> DataFrame             # no-op if col absent
df.rename(renames: List[Tuple[str, str]]) -> DataFrame
df.head(n: i64) -> DataFrame
df.tail(n: i64) -> DataFrame
df.row(i: i64) -> List[str]                       # i-th row stringified; "null" for nulls
```

### STOP CRITERIA for Phase C

If the per-column comparison methods balloon (5 columns × 7 ops × NativeFn = 35 handlers + dispatch), ship the smallest useful set: `eq` and `gt`/`lt` on i64/f64/str, plus `and_` / `or_` / `not_` on ColumnBool. Drop `between`, `starts_with`, `contains`, the f64 monotone ops if needed. Flag what was cut.

### Commit checkpoint after Phase C

Commit: "M37 C: tabular filtering — comparisons + masks + select/drop/head/tail".

## Phase D — Sort (~150-200 LOC)

```python
df.sort_by(col_name: str, ascending: bool) -> DataFrame
```

- Stable sort. Build an index permutation by sorting `(value, original_index)` pairs of the named column (dispatch on Column subclass for type-correct comparator)
- Nulls go to the END regardless of ascending/descending (pandas default)
- Apply permutation to all columns (rebuild values + nulls lists per column)

### STOP CRITERIA for Phase D

If sort dispatch by column type becomes tangled, ship sort for i64 + f64 + str only; drop bool + datetime sort. Flag in report.

### Commit checkpoint after Phase D

Commit: "M37 D: tabular sort_by".

## Phase E — Tests + demo + docs (~200-300 LOC)

### Tests (`vm/tests/m37_tabular.rs`)

Aim for 18-25 tests covering:
- Construction: each col_* builder, from_columns, mismatched-length error
- Inspection: shape/columns/dtypes/get_column/has_column
- Display: show() output format on a 3×2 frame, show() with nulls
- I/O: round-trip a 3-row CSV (write_csv → read_csv → compare), from_sql with a SQLite cursor
- Filter: i64 eq mask, mask combinators, filter result shape
- Sort: ascending + descending, nulls-at-end behavior

### Compiler integration test

`compiler/tests/tabular_demo_runs.rs` — compile + run `examples/tabular_demo.spy`.

### Demo (`examples/tabular_demo.spy`)

~100-150 lines: realistic walkthrough — construct a frame inline, write to CSV, read back, filter for ages > 25, sort by name ascending, print. Output should be deterministic so the demo_runs test can assert on it.

### LANGUAGE_GUIDE.md update

Add §5 entry for `tabular (M37)` after the existing stdlib subsections. Document the surface concisely with code blocks (mirror the `re (M20c, extended by M35)` style). Bump the version banner at top to "post-M37". Add a §11 entry if there's a gotcha worth flagging (e.g., "null-propagation in comparisons" or "schema-driven parsing").

### STOP CRITERIA for Phase E

If you hit 80% of budget before Phase E, ship the minimum viable: ~10 tests + a 50-line demo + a one-paragraph §5 entry in LANGUAGE_GUIDE.md. The orchestrator will round out docs.

### Commit checkpoint after Phase E

Commit: "M37 E: tabular tests + demo + LANGUAGE_GUIDE.md update + agent report".

## Acceptance criteria (machine-checkable)

1. `cargo build --workspace --release` — clean, no warnings on touched code.
2. `cargo test --release -p strictpy-vm --test m37_tabular` — all tests pass.
3. `cargo test --release -p strictpy-compiler --test tabular_demo_runs` — passes.
4. **No regressions**: `cargo test --workspace --release --no-fail-fast` reports 723 + N passing where N = the new M37 test count, 0 failing, 1 ignored.
5. `examples/tabular_demo.spy` runs to completion and produces the expected output.
6. The 11 existing stdlib classes (JsonValue + 6 subclasses, Pattern, Connection, Cursor, Hasher) and 39 M34/M35 tests continue to pass — your work does NOT touch the M36 honest-debt prelude bindings.

## Constraints — files NOT to modify

- `compiler/src/resolver.rs::seed_prelude` for class registration — use `seed_stdlib_modules` + `StdlibItemKind::Class` instead. The only exception: if you discover a genuinely-needed prelude binding (very unlikely), DOCUMENT IT in your report.
- The 39 M34/M35 test files — they must continue passing untouched.
- Any existing example in `examples/*.spy` except the new `tabular_demo.spy`.

## STOP CRITERIA — when to ship a smaller working version

Five priority drops, in order:

1. **Drop `ColumnDateTime`** — keep ColumnI64/F64/Str/Bool only. Cuts ~15% of code.
2. **Drop Phase D (sort)** — ships A+B+C without sort. Cuts another ~10%.
3. **Drop write_csv + from_sql** — keeps read_csv only. Cuts another ~10%.
4. **Drop sub-methods of Phase C** — keep eq + gt + lt + and_ + or_ + not_ only; drop between/starts_with/contains/etc.
5. **Drop the LANGUAGE_GUIDE update** — orchestrator will write it in 5 min.

After applying any drop, your final report must document what was cut and why, with a "what M38 should pick up" list. The Lesson 1 streak is more important than feature completeness — commit what works.

## Methodology discipline

1. **First commit at ~25% of budget** — Phase A green build + 1 smoke test. Don't proceed to Phase B without this checkpoint.
2. **Per-phase commits** — 5 commits expected (A, B, C, D, E). Each commit must build clean and have its targeted tests passing.
3. **Variable prefix `m37_`** for all new helper functions / locals in shared files (resolver.rs, ir.rs, builtins.rs).
4. **No exhaustive-match breakage**: if you add new variants to existing enums, you'll break match statements scattered across the codebase. AVOID. Use the M34/M35 pattern: name-based dispatch in ir.rs (mirrors `m34_json_class_method_native_id_by_name`), not new IR opcodes.

## Final report

Write `docs/thesis/agent_reports/m37_tabular.md` (under 600 words) covering:
- What shipped per phase (A-E)
- What was cut from STOP CRITERIA (if anything)
- LOC delta per touched file
- Final test count + verification
- Surprises / design calls worth recording (e.g., what's the i64-vs-Tuple representation for sort permutations)
- "What M38 should pick up" — concrete follow-up list
- LANGUAGE_GUIDE.md update status

Commit this report as part of Phase E's commit.

## Commit message shape (final)

```
M37: tabular stdlib module — DataFrame + sealed Column hierarchy + I/O + filter + sort

First Pandas-shaped data package for v0.3. Native Rust impl (real pandas
can't import — see LANGUAGE_GUIDE.md §11.11). Uses the post-M36
StdlibItemKind::Class path — first stdlib package to register
classes module-scoped from the start (no prelude bloat).

Surface (`tabular` module):
- Sealed Column + 5 subclasses (ColumnI64/F64/Str/Bool/DateTime)
- DataFrame: named columns, RangeIndex(i64)
- I/O: read_csv / write_csv / from_sql (reuses M22 csv + M35 sqlite3)
- Filter: per-Column comparison methods → ColumnBool masks;
  mask combinators (and_/or_/not_); df.filter / select / drop / rename
- Sort: df.sort_by(col, ascending) — stable, nulls-at-end

NA semantics: per-column null mask (List[bool] parallel to values).
Uniform across dtypes; no NaN payload-bit games.

NativeFn IDs 830-879. Variable prefix m37_.

Tests: 723 → 723+N (M37 added N tests).
Examples: 100 → 101 (examples/tabular_demo.spy).
```
