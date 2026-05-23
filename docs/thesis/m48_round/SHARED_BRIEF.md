# M48 — Comprehensive `tabular` vs pandas benchmark suite

## Context

After M47 the `tabular` package has 19 stdlib classes + ~50 user-facing methods spanning the common-95% of pandas workflows. We've made no quantitative claims about its performance against real pandas. **M48 closes that gap**: a comprehensive benchmark suite covering the surface across multiple dataset sizes, in the same shape as M26's extended benchmark (which compared pure-compute StrictPy vs CPython 3.12).

The benchmark establishes a baseline that the queued **M49** (optimized categorical codes paths + ordered categorical + more resample rules + rolling chainable) can measure before/after against. **Run M48 FIRST so M49 has a clean diff.**

**M48 ships ONLY benchmark infrastructure** — no language / compiler / codegen / VM changes. Pure measurement + reporting. This is the same shape as M26 (which added 0 LOC of language code, just bench harness + cell generators + JSON snapshots + rendered .md report).

You are the **30th** of an unbroken Lesson-1-compliant agent streak (M28 → M47). M48 is **disjoint-handler** classification (all phases are independent — generators / harness / runner / report rendering). First commit at ~20% of budget.

## Files to read FIRST (in order)

1. `bench/harness.py` (~1723 lines) — **the existing benchmark harness for the canonical 16-cell suite + the M26 extended 30-cell suite**. You will extend this (or write a sibling `bench/tabular_harness.py`) with the tabular comparison logic.
2. `bench/EXTENDED_REPORT.md` — the rendered output of M26's harness (the report shape you're matching).
3. `bench/history/m26_extended.json` — the JSON snapshot shape your output will mirror.
4. `LANGUAGE_GUIDE.md` §5 (tabular subsection M37-M47) — the StrictPy API surface you're benchmarking.
5. `examples/tabular_demo.spy` / `tabular_groupby_demo.spy` / `tabular_index_demo.spy` — examples of writing the operations in StrictPy.
6. `docs/thesis/agent_reports/m26_extended_bench.md` (if it exists) or `docs/thesis/milestones/m26_extended.md` — the M26 methodology for fair comparison.

## Constraints

- **Lesson 1**: first commit at ~20% of budget. 29-streak — don't break it.
- **Variable prefix `m48_`** for any new helper functions / locals in shared files. Most code is Python (the harness) — Python's lower-case-snake variable convention applies, so just prefix function names with `m48_` if needed for archaeology.
- **No new NativeFn IDs**, no language/compiler/codegen changes, no payload changes. This is bench infrastructure only.
- **No new crate deps** in the workspace.
- **Python deps**: pandas (already at 3.0.0 in the test environment), psutil (for memory measurement — install if not present, or document fallback if blocked). DO NOT add anything that requires admin permissions.
- All 248 existing tabular tests must keep passing (M48 makes no `.rs` changes; this should be trivially true).

### Edit-tool worktree leak — defensive measure

Per M44/M46: precautionary `cp` at session start as defensive measure even though this milestone is mostly Python files (which the leak hasn't hit historically since it's `Edit`/`Write` on Rust source under bulk-edit-of-shared-files conditions). Run anyway:

```bash
for f in vm/src/builtins.rs compiler/src/resolver.rs compiler/src/ir.rs \
         shared/src/native.rs LANGUAGE_GUIDE.md; do
    cp /c/Users/AG/CascadeProjects/PythonCompiler/$f $f 2>/dev/null
done
```

## Phase A — Bench infrastructure (~400-500 LOC)

### Shape

Mirror `bench/harness.py`'s structure, but for the tabular comparison:

```python
# In bench/tabular_harness.py:

# For each (op, size) cell:
# 1. Generate a deterministic CSV fixture at that size (cached on disk).
# 2. Write a .spy program that opens the CSV via tabular.read_csv,
#    runs the op N times in a loop, prints the total wall-clock.
# 3. Write a matching .py program that uses real pandas.
# 4. Compile the .spy once with spy --compile-only.
# 5. Run each program BEST_OF=3 times via spy.exe / python.exe.
# 6. Record min wall-clock and peak RSS per cell.
# 7. Compute the ratio (StrictPy_time / Pandas_time) per cell.

# Output:
#   - bench/history/m48_tabular.json — the raw measurements
#   - bench/TABULAR_BENCH_REPORT.md — the rendered comparison table
```

### CSV fixture generator

`m48_gen_csv(size: int, seed: int = 42) -> Path` — generates `bench/data/tabular_<size>.csv` if not already present. Columns:

- `id: i64` (monotonic 0..size)
- `category: str` (one of 8 values, distribution skewed via Zipf)
- `region: str` (one of 4 values, uniform)  — note: see footnote about `region` being a VSCode-folding keyword in the M47 agent report; consider naming this `area` instead
- `qty: i64` (uniform 1..1000)
- `price: f64` (uniform 0.01..10000.00)
- `ts: datetime` (epoch-ms; daily increments across `size` days starting 2024-01-01)
- ~5% random nulls per column (use a fixed seed for reproducibility)

For `100M` size, generating the CSV may take a while — cache aggressively + only regenerate if the seed changes.

### Memory measurement

Use `psutil.Process(child_pid).memory_info().rss` polled every 50ms while the child runs; record peak. Document the approach in the methodology section of the report.

### Commit checkpoint after Phase A

`M48 A: tabular bench infrastructure + CSV generator + memory measurement`. Build clean + 1 smoke run on small-size read_csv producing a JSON output + a parseable report row.

## Phase B — Core 8 ops × 4 sizes = 32 cells (~500-700 LOC)

### Operations to benchmark

For each op, write a .spy + .py program pair that loads the CSV (via read_csv / pandas.read_csv) and runs the op N times (where N is tuned so total wall-clock for `small` is ~1 sec):

1. **read_csv** — measure load time only (no loop; just `read_csv` once + `len(df)` check)
2. **filter** — `df[df["qty"] > 500]` (pandas) / `df.filter(df.get_column_i64("qty").gt(500))` (StrictPy)
3. **sort_by** — sort by `price` ascending
4. **group_by + sum** — `df.groupby("category")["qty"].sum()` / equivalent
5. **merge inner** — self-merge on `id` after splitting df in half (silly but uniform; matches pandas behavior)
6. **pivot_table** — `df.pivot_table("category", "region", "qty", "sum")`
7. **rolling_mean** — 7-day rolling mean on `price`
8. **describe** — `df.describe()`

### Sizes

- `small`: 100 rows
- `medium`: 10,000 rows
- `large`: 1,000,000 rows
- `xl`: 100,000,000 rows (100M)

For `xl`, expect:
- Memory pressure (StrictPy's List[T] per-cell overhead vs pandas/NumPy contiguous buffers)
- StrictPy potentially OOMing on read_csv or filter — document if so; the failure is data
- pandas should handle 100M without crashing if RAM is sufficient

If `xl` results in OOM or unrealistic wall-clock (>10 minutes per cell), **skip that cell and document the failure mode**. The report's value is the honest comparison.

### Commit checkpoint after Phase B

`M48 B: tabular bench core 8 ops × 4 sizes`. Build clean + initial JSON snapshot + initial rendered report (may be skeletal).

## Phase C — Categorical-specific benchmarks + memory comparison (~400-500 LOC)

### Categorical-specific cells

M47 shipped `ColumnCategorical` but the v1 implementation coerces via `to_strings()` for all ops. M48 measures the **cost of this coercion** vs:
- Native string column (no categorical wrapper)
- Pandas Categorical dtype (which has optimized codes-based ops)

Three cells:
- **group_by_str**: group on `category` as ColumnStr → str hash
- **group_by_cat_via_strings**: group on ColumnCategorical (v1 coerce path) → str hash via to_strings()
- **group_by_pandas_categorical**: pandas with `df["category"] = df["category"].astype("category")` → codes hash

This sets up the M49 baseline: the expected speedup when M49 optimizes the codes paths.

Same comparison for `merge` and `unique`.

### Memory peak comparison

For each (op, size) cell, record:
- StrictPy peak RSS during run
- Pandas peak RSS during run
- Ratio (StrictPy / Pandas)

This will exposes the per-cell `List<T>` overhead vs NumPy contiguous buffer cost.

### Commit checkpoint after Phase C

`M48 C: tabular bench categorical + memory comparison`. Build clean + full JSON snapshot + draft report with all cells.

## Phase D — Report rendering + agent report (~200-300 LOC)

### Render `bench/TABULAR_BENCH_REPORT.md`

Mirror `bench/EXTENDED_REPORT.md`'s shape. Sections:

1. **Headline summary**: total wins / ties / losses across all cells; aggregate geomean ratio.
2. **Per-op breakdown**: table with rows = sizes, columns = StrictPy / Pandas / Ratio. One table per op (8 + 3 categorical = 11 tables).
3. **Memory comparison table**: same structure but RSS instead of wall-clock.
4. **Categorical cost analysis**: highlights the v1 to_strings() overhead vs ColumnStr vs pandas Categorical.
5. **Reproducibility**: exact `cargo build` + `python bench/tabular_harness.py` invocations.
6. **Methodology**: BEST_OF=3, deterministic CSV seeds, memory polling approach, what was excluded (xl size if OOM).
7. **Honest findings**: where StrictPy wins, where pandas wins, where the v0.4 polish from M49 should help.

### Snapshot

`bench/history/m48_tabular.json` — the raw measurements. Same shape as `bench/history/m26_extended.json`.

### Commit checkpoint after Phase D

`M48 D: tabular bench report + agent report`.

## Acceptance criteria (machine-checkable)

1. `cargo build --workspace --release` — clean (no Rust changes; should be trivially clean).
2. **All 248 existing tabular tests still pass** (M48 makes no `.rs` changes).
3. **`python bench/tabular_harness.py` runs to completion** producing both the JSON snapshot and the rendered report.
4. **The report's headline table is complete** for at least the small + medium + large sizes; xl-failures documented inline.
5. **Reproducibility verification**: deleting the cached CSVs + JSON and re-running produces consistent results within ±10% (variance is expected; wild swings indicate a methodology bug).
6. **No regressions in `cargo test --workspace --release --no-fail-fast`** — should still be **993 / 0 / 1**.

## Constraints — files NOT to modify

- ANY `.rs` source file in `compiler/`, `vm/`, `shared/` — M48 is pure benchmark infrastructure.
- The 11 existing tabular tests/demos in `examples/` — DO NOT modify.
- `seed_prelude` etc. — same reason.

## STOP CRITERIA — priority drops if budget runs out

Five priority drops, in order:

1. **Drop xl (100M) size** entirely if it requires more than ~10 minutes per cell or OOMs StrictPy. Document the failure in the report. Keep small/medium/large.
2. **Drop memory measurement** if `psutil` polling proves too noisy/slow on Windows. Keep wall-clock only.
3. **Drop categorical-specific cells** (Phase C) — keep the 8 × 4 = 32 core cells. Categorical comparison can be M49's deliverable.
4. **Drop the rolling_mean cell** — it's the most CPU-bound op and tabular's may underperform significantly; not essential to the headline picture.
5. **Drop the rendered report's prose sections** — keep just the data tables + JSON snapshot. Orchestrator finishes the prose.

After applying any drop, document what was cut with a "what M48b should pick up" list (memory deep-dive, xl-size investigation, etc.).

## Methodology discipline

1. **First commit at ~20% of budget** — Phase A's bench infrastructure + 1 smoke cell on small read_csv.
2. **Per-phase commits** — 4 commits (A, B, C, D). M48 is disjoint-handler (each phase is independent).
3. **Variable prefix `m48_`** for new helper functions in Python (mostly cosmetic — Python doesn't have the same archaeology need as Rust).
4. **No new IR opcodes / no Rust source changes** — pure bench infrastructure.
5. **Edit-tool worktree leak**: precautionary `cp` at session start. Even though this milestone is mostly Python, the leak has been intermittent enough that defensive copy is cheap.
6. **Honest findings only**: if pandas wins by 10× on a cell, report it. The benchmark's value is its honesty, not StrictPy's vanity.

## Final report

Write `docs/thesis/agent_reports/m48_tabular_bench.md` (under 600 words) covering:
- What shipped per phase (A-D)
- What was cut from STOP CRITERIA (if anything)
- LOC delta per touched file
- Key findings — where StrictPy wins, where pandas wins, the categorical to_strings() overhead measurement, memory comparison summary
- Surprises / design calls (e.g., did `xl` OOM? how noisy was memory polling on Windows? did pandas 3.0's new copy-on-write change any baselines?)
- "What M48b / M49 should pick up" — concrete list (memory deep-dive, xl investigation, what M49's categorical-codes optimization should aim to beat)
- Whether the Edit-tool worktree leak recurred (yes/no — it's likely not relevant since this milestone is Python-heavy)

Commit this report in Phase D's commit.

## Commit message shape (final)

```
M48: tabular vs pandas benchmark suite (comprehensive)

Pure benchmark infrastructure — no language / compiler / codegen
changes. Mirrors M26's extended-bench shape but for the M37-M47
tabular surface against real pandas 3.0.

Phase A: bench/tabular_harness.py infrastructure + deterministic
  CSV fixture generator (small/medium/large/xl sizes) + memory
  measurement via psutil RSS polling.
Phase B: 8 core ops × 4 sizes = 32 cells. read_csv / filter /
  sort_by / group_by+sum / merge inner / pivot_table /
  rolling_mean / describe.
Phase C: categorical-specific cells (str vs categorical-via-strings
  vs pandas Categorical) + memory peak comparison per cell.
Phase D: bench/TABULAR_BENCH_REPORT.md rendered output +
  bench/history/m48_tabular.json snapshot + agent report.

Tests: 993 → 993 (no Rust changes). Examples: unchanged.
Establishes the baseline for M49's optimized categorical codes
paths.
```
