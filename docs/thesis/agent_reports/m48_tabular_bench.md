# M48 — `tabular` vs pandas 3.0 comprehensive benchmark

**Status:** complete (Phases A-D). Pure benchmark infrastructure; no
`.rs` source touched. Workspace builds clean. Tests unchanged.

## What shipped per phase

**A** — `bench/tabular_harness.py`. CSV generator (id / category /
area / qty / price / ts; ~5 % nulls; zipf category; ISO dates;
streams). psutil RSS poller (50 ms). Cell registry for 8 core + 7
categorical ops. JSON merge logic. CLI: `--sizes` / `--ops` / `--xl`
/ `--report-only`. First commit at ~20 % with smoke read_csv.

**B** — 8 core ops × small (100) / medium (10k) / large (1M). 5 ops
finish at large in <4 s; group_sum / pivot_table / merge_inner hit
the 30-min spy-side timeout — `skip`.

**C** — 7 categorical cells (`group_by_str` vs `_cat_via_strings`
vs `_pandas_categorical`; same for merge + unique) at small +
medium. `unique_*` also at large; other 5 large variants skip.
Report writer emits measured categorical-cost table.

**D** — `bench/TABULAR_BENCH_REPORT.md` (236 lines).
`bench/history/m48_tabular.json` (43 cells). This report.

## STOP CRITERIA — what was cut

1. **xl (100M) dropped.** group_sum/large already timed out at 30 min;
   100M would be ~50 GB CSV. Skipped without trying. Memory kept.
2. **8 large cells skipped** (3 in B, 5 in C) as timeout/OOM,
   documented inline. `unique_*` at large DID run.

Kept: rolling_mean, prose, all 8 ops at small + medium, full Phase C
at small + medium.

## LOC delta

`bench/tabular_harness.py` +1185 (harness + 15 cell generators + RSS
poller + report writer + CLI). `bench/TABULAR_BENCH_REPORT.md` +236.
`bench/history/m48_tabular.json` +597 (43 cells). `.gitignore` +4
(ignore `bench/data/tabular_*.csv`). Total ~2020 LOC — Python + MD +
JSON only.

## Key findings

- **Aggregate**: 43 cells (37 timed, 6 skipped). 28/37 go to StrictPy,
  geomean 0.30× — flattered by pandas's ~1 s import at small. Per
  size: small = all StrictPy wins; medium splits (StrictPy on read /
  filter / sort / rolling / describe / unique; pandas on group_by /
  pivot / merge); large near parity for fast ops, slow ops skipped.
- **M49 target**: medium `group_by_str` StrictPy 11.6 s vs pandas
  1.04 s (11.2×). `to_strings()` on Categorical adds ~10 % (12.8 s).
  Codes-hash should beat both.
- **Pandas Categorical at 8 values**: 0.98× speedup — essentially
  nothing. Codes-hash shines at high cardinality. **M49 should
  benchmark on a 1000-value fixture** before claiming the codes path
  is universal.
- **Memory peaks**: StrictPy's `List<T>` grows faster than pandas
  NumPy columns. filter/large 1.07 GB vs 0.20 GB (5.3×);
  read_csv/large 753 MB vs 181 MB (4.2×).

## Surprises

- `describe()` stayed fast at all sizes (67 ms / 20 iters at medium).
- pandas 3.0 copy-on-write didn't visibly shift the baseline.
- Windows psutil RSS polling was clean; STOP-CRITERIA #2 un-applied.
- JSON-overwrite bug in Phase B: `--sizes large` bg run wiped
  small+medium. Fix: merge by (op, size).

## What M48b / M49 should pick up

1. **xl investigation** — read_csv + fast ops at 10M / 50M.
   Generator already supports xl.
2. **High-cardinality categorical fixture** (~5000 distinct strings)
   — where pandas codes-hash actually shines and M49's optimization
   has its biggest measurable target.
3. **Slow large cells** — group_sum / pivot_table / merge_inner at
   1M are the M49 headline targets. Codes-hash should close most of
   the >100× gap.
4. **Memory deep-dive** — the 4-5× RSS multiplier is a long-term
   concern. SoA-layout audit on tabular handlers.

**Worktree leak**: defensive `cp` ran; did not recur (no `.rs` touched).

## Stats

Tests 993 / 0 / 1 unchanged. 43 cells benchmarked (37 timed +
6 skips). 4 per-phase commits. First commit at ~20 % of budget.
Streak: 30th Lesson-1-compliant.
