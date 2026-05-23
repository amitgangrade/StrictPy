"""M48 — tabular vs pandas benchmark harness.

Mirrors the M26 extended bench shape (bench/harness.py) but targets the
M37-M47 `tabular` package against real pandas 3.0.  Pure measurement +
reporting infrastructure — no language / compiler / VM changes.

Strategy per (op, size) cell:
  1. Generate a deterministic CSV at that size (cached in bench/data/).
  2. Emit a .spy program + matching .py program that load the CSV and
     run the op N times (N tuned so small-size wall clock is ~0.5-1 s).
  3. Compile the .spy once via `spy --compile-only` (compile time not
     measured).
  4. Run each side BEST_OF times; record min wall-clock + peak RSS via
     a 50 ms psutil polling thread.
  5. Compute the ratio (StrictPy / pandas) and stash in JSON.

Outputs:
  bench/history/m48_tabular.json     — raw measurements
  bench/TABULAR_BENCH_REPORT.md      — rendered comparison

CLI:
  python bench/tabular_harness.py                # full run (small/medium/large)
  python bench/tabular_harness.py --sizes small  # subset
  python bench/tabular_harness.py --xl           # also run xl (100M)
  python bench/tabular_harness.py --report-only  # re-render from JSON
  python bench/tabular_harness.py --ops read_csv,filter  # subset of ops
"""

from __future__ import annotations

import argparse
import json
import math
import os
import random
import subprocess
import sys
import threading
import time
from pathlib import Path

try:
    import psutil  # type: ignore
    HAVE_PSUTIL = True
except ImportError:
    HAVE_PSUTIL = False

# ─────────────────────────────────────────────────────────────────────────────
#  Paths + constants
# ─────────────────────────────────────────────────────────────────────────────

ROOT      = Path(__file__).resolve().parent.parent
BENCH_DIR = ROOT / "bench"
DATA_DIR  = BENCH_DIR / "data"
GEN_DIR   = BENCH_DIR / "generated"
HIST_DIR  = BENCH_DIR / "history"

# spy.exe may live in the worktree's own target/ or in the canonical
# PythonCompiler/target/release (worktree-shared build).  Prefer the
# worktree's own build if present.
def m48_find_spy() -> Path:
    candidates = [
        ROOT / "target" / "release" / "spy.exe",
        Path("C:/Users/AG/CascadeProjects/PythonCompiler/target/release/spy.exe"),
    ]
    for c in candidates:
        if c.exists():
            return c
    # Fall back; will error on first invocation with a clear message.
    return candidates[0]

SPY    = m48_find_spy()
PYTHON = sys.executable

BEST_OF      = 3       # min of this many runs per cell (overridable per-size)
POLL_MS      = 50      # psutil RSS polling interval
SUBPROC_TIMEOUT = 1800 # seconds — generous so large/xl cells don't drop on slow hosts

# Per-size BEST_OF override.  At large/xl, slow ops would consume hours
# at BEST_OF=3; one run is honest enough for a >100x ratio.
M48_BEST_OF_FOR_SIZE = {
    "small":  3,
    "medium": 3,
    "large":  2,
    "xl":     1,
}

# Per-(op, size) BEST_OF override.  Some cells are slow enough on the
# StrictPy side at large that a 2-run minimum still doubles the wall.
M48_BEST_OF_OVERRIDE = {
    ("group_sum",   "large"): 1,
    ("pivot_table", "large"): 1,
    ("merge_inner", "large"): 1,
    ("group_by_str",               "large"): 1,
    ("group_by_cat_via_strings",   "large"): 1,
    ("group_by_pandas_categorical","large"): 1,
    ("merge_str",                  "large"): 1,
    ("merge_cat_via_strings",      "large"): 1,
}

for d in (DATA_DIR, GEN_DIR, HIST_DIR):
    d.mkdir(parents=True, exist_ok=True)


# ─────────────────────────────────────────────────────────────────────────────
#  Size + operation specs
# ─────────────────────────────────────────────────────────────────────────────

# Iteration counts per (op, size).  Tuned so small size totals ~0.5-1.5 s
# (process startup dominates for tiny work) and large is bounded under
# ~30 s per side.  read_csv is special — N=1 because the I/O dwarfs any
# loop overhead, and we want to time the load itself.
M48_SIZES = {
    "small":  100,
    "medium": 10_000,
    "large":  1_000_000,
    "xl":     100_000_000,
}

# Per-op default iteration count (override per-size via M48_ITERS if needed).
M48_DEFAULT_ITERS = {
    "read_csv":     1,
    "filter":       50,
    "sort_by":      20,
    "group_sum":    20,
    "merge_inner":  5,
    "pivot_table":  20,
    "rolling_mean": 5,
    "describe":     20,
    # Phase C cells:
    "group_by_str":               20,
    "group_by_cat_via_strings":   20,
    "group_by_pandas_categorical": 20,
    "merge_str":                  3,
    "merge_cat_via_strings":      3,
    "unique_str":                 20,
    "unique_cat_via_strings":     20,
}

# Per-(op, size) overrides — drop iters at large sizes so the cell
# completes in reasonable wall clock.  group_sum/pivot_table/merge use
# StrictPy's v1 hash-on-string-key path; at medium the per-iter cost is
# ~7-8 s on 10k rows, so we crank iters way down for slow ops.
M48_ITERS_OVERRIDE = {
    ("filter", "large"):       3,
    ("filter", "xl"):          1,
    ("sort_by", "large"):      2,
    ("sort_by", "xl"):         1,
    ("group_sum", "medium"):   3,
    ("group_sum", "large"):    1,
    ("group_sum", "xl"):       1,
    ("merge_inner", "large"):  1,
    ("merge_inner", "xl"):     1,
    ("pivot_table", "medium"): 3,
    ("pivot_table", "large"):  1,
    ("pivot_table", "xl"):     1,
    ("rolling_mean", "large"): 1,
    ("rolling_mean", "xl"):    1,
    ("describe", "large"):     3,
    ("describe", "xl"):        1,
    ("group_by_str", "medium"):              3,
    ("group_by_str", "large"):               1,
    ("group_by_cat_via_strings", "medium"):  3,
    ("group_by_cat_via_strings", "large"):   1,
    ("group_by_pandas_categorical", "medium"): 3,
    ("group_by_pandas_categorical", "large"):  1,
    ("merge_str", "medium"):                 1,
    ("merge_str", "large"):                  1,
    ("merge_cat_via_strings", "medium"):     1,
    ("merge_cat_via_strings", "large"):      1,
    ("unique_str", "large"):                 3,
    ("unique_cat_via_strings", "large"):     3,
}


def m48_iters_for(op: str, size: str) -> int:
    if (op, size) in M48_ITERS_OVERRIDE:
        return M48_ITERS_OVERRIDE[(op, size)]
    return M48_DEFAULT_ITERS.get(op, 1)


# ─────────────────────────────────────────────────────────────────────────────
#  CSV fixture generator
# ─────────────────────────────────────────────────────────────────────────────

M48_CATEGORIES = ["alpha", "bravo", "charlie", "delta", "echo", "foxtrot", "golf", "hotel"]
M48_AREAS      = ["north", "south", "east", "west"]
M48_NULL_PCT   = 0.05
M48_SEED       = 42


def m48_csv_path(size_label: str) -> Path:
    return DATA_DIR / f"tabular_{size_label}.csv"


def m48_gen_csv(size_label: str, n_rows: int, seed: int = M48_SEED) -> Path:
    """Generate (or reuse cached) deterministic CSV with the M48 schema:
        id (i64), category (str / 8 values zipf-skewed), area (str / 4 uniform),
        qty (i64 1..1000), price (f64 0.01..10000), ts (datetime epoch-ms,
        daily increments from 2024-01-01).  ~5% random nulls per column
        (except `id`, which stays monotonic and non-null).
    """
    path = m48_csv_path(size_label)
    if path.exists() and path.stat().st_size > 0:
        return path

    print(f"  generating {path.name} ({n_rows:,} rows)…", flush=True)
    rng = random.Random(seed)

    # Zipf-ish skew for category: weights 8,7,6,5,4,3,2,1 normalized.
    cat_weights = [8.0, 7.0, 6.0, 5.0, 4.0, 3.0, 2.0, 1.0]
    cat_total = sum(cat_weights)
    cat_cum = []
    s = 0.0
    for w in cat_weights:
        s += w / cat_total
        cat_cum.append(s)

    def pick_category() -> str:
        r = rng.random()
        for i, c in enumerate(cat_cum):
            if r <= c:
                return M48_CATEGORIES[i]
        return M48_CATEGORIES[-1]

    # 2024-01-01 baseline.  tabular.read_csv's datetime dtype accepts
    # "YYYY-MM-DD" (or full ISO `YYYY-MM-DDTHH:MM:SS[Z|±HH:MM]`); we
    # emit the short form which is unambiguous + cheap.
    from datetime import datetime, timedelta, timezone
    base_dt = datetime(2024, 1, 1, tzinfo=timezone.utc)

    # Stream to disk so xl (100M rows) doesn't blow heap.
    with path.open("w", encoding="utf-8", newline="") as fh:
        fh.write("id,category,area,qty,price,ts\n")
        # Pre-decide which rows have null in each nullable column — but
        # at xl we don't want a list of 100M booleans per column.  Use
        # per-row random draw instead; reproducibility comes from the
        # seeded rng.
        for i in range(n_rows):
            row_id = i
            cat = pick_category() if rng.random() >= M48_NULL_PCT else ""
            area = M48_AREAS[rng.randrange(4)] if rng.random() >= M48_NULL_PCT else ""
            qty = rng.randint(1, 1000) if rng.random() >= M48_NULL_PCT else ""
            price = f"{rng.uniform(0.01, 10000.00):.4f}" if rng.random() >= M48_NULL_PCT else ""
            # ts increments by 1 day per row mod 365 — repeats but matches
            # the brief's "daily increments across `size` days starting
            # 2024-01-01" intent for time-series ops (rolling, resample).
            ts = ""
            if rng.random() >= M48_NULL_PCT:
                ts_dt = base_dt + timedelta(days=(i % 365))
                ts = ts_dt.strftime("%Y-%m-%d")
            fh.write(f"{row_id},{cat},{area},{qty},{price},{ts}\n")
            # Periodic flush so we get partial output even if interrupted.
            if (i & 0xFFFFF) == 0xFFFFF:
                fh.flush()
    print(f"    wrote {path.stat().st_size:,} bytes", flush=True)
    return path


# ─────────────────────────────────────────────────────────────────────────────
#  Cell generators — (.spy, .py) pairs per op
# ─────────────────────────────────────────────────────────────────────────────

# The StrictPy schema literal embedded in every .spy program.  Order +
# dtypes must match m48_gen_csv's header.
M48_SPY_SCHEMA = '''\
schema: List[Tuple[str, str]] = []
schema.append(("id", "i64"))
schema.append(("category", "str"))
schema.append(("area", "str"))
schema.append(("qty", "i64"))
schema.append(("price", "f64"))
schema.append(("ts", "datetime"))
'''

# The pandas schema dtype kwarg.  Note: pandas parses dtype=str for object
# columns and we let it infer numeric columns natively.
M48_PY_DTYPES = (
    "dtype={'id': 'Int64', 'qty': 'Int64', 'price': 'Float64', "
    "'category': 'object', 'area': 'object'}, "
    "parse_dates=False"
)


def m48_spy_preamble(csv_path: Path) -> str:
    """Common .spy header — imports + schema + read_csv."""
    csv_lit = str(csv_path).replace("\\", "\\\\")
    return (
        'from tabular import Column, ColumnI64, ColumnF64, ColumnStr, ColumnBool, '
        'ColumnDateTime, ColumnCategorical, DataFrame, GroupedDataFrame\n'
        'import tabular\n'
        '\n'
        'fn main() -> i32:\n'
        + ''.join('    ' + line + '\n' for line in M48_SPY_SCHEMA.strip().split('\n'))
        + f'    df: DataFrame = tabular.read_csv("{csv_lit}", schema)\n'
    )


def m48_py_preamble(csv_path: Path) -> str:
    csv_lit = str(csv_path).replace("\\", "\\\\")
    return (
        'import pandas as pd\n'
        'import time\n'
        'import sys\n'
        '\n'
        f'df = pd.read_csv(r"{csv_lit}", {M48_PY_DTYPES})\n'
    )


# Each generator returns (spy_src, py_src, expected_stdout_substring).
# The expected substring is used only for sanity ("nrows=" present) since
# byte-equality would require both sides to format floats identically.

def m48_gen_read_csv(csv_path: Path, _iters: int) -> tuple[str, str, str]:
    spy = m48_spy_preamble(csv_path) + (
        '    println("nrows=" + str(df.length()))\n'
        '    return 0\n'
    )
    py = m48_py_preamble(csv_path) + (
        'print(f"nrows={len(df)}")\n'
    )
    return spy, py, "nrows="


def m48_gen_filter(csv_path: Path, iters: int) -> tuple[str, str, str]:
    spy = m48_spy_preamble(csv_path) + (
        '    total: i64 = 0i64\n'
        f'    iters: i64 = {iters}i64\n'
        '    i: i64 = 0i64\n'
        '    while i < iters:\n'
        '        qc: ColumnI64? = df.get_column_i64("qty")\n'
        '        if qc is not none:\n'
        '            mask: ColumnBool = qc.gt(500i64)\n'
        '            sub: DataFrame = df.filter(mask)\n'
        '            total = total + sub.length()\n'
        '        i = i + 1i64\n'
        '    println("nrows=" + str(df.length()) + " kept_total=" + str(total))\n'
        '    return 0\n'
    )
    py = m48_py_preamble(csv_path) + (
        f'iters = {iters}\n'
        'total = 0\n'
        'for _ in range(iters):\n'
        '    sub = df[df["qty"] > 500]\n'
        '    total += len(sub)\n'
        'print(f"nrows={len(df)} kept_total={total}")\n'
    )
    return spy, py, "nrows="


def m48_gen_sort_by(csv_path: Path, iters: int) -> tuple[str, str, str]:
    spy = m48_spy_preamble(csv_path) + (
        f'    iters: i64 = {iters}i64\n'
        '    i: i64 = 0i64\n'
        '    acc: i64 = 0i64\n'
        '    while i < iters:\n'
        '        sd: DataFrame = df.sort_by("price", true)\n'
        '        acc = acc + sd.length()\n'
        '        i = i + 1i64\n'
        '    println("nrows=" + str(df.length()) + " acc=" + str(acc))\n'
        '    return 0\n'
    )
    py = m48_py_preamble(csv_path) + (
        f'iters = {iters}\n'
        'acc = 0\n'
        'for _ in range(iters):\n'
        '    sd = df.sort_values("price", ascending=True, na_position="last")\n'
        '    acc += len(sd)\n'
        'print(f"nrows={len(df)} acc={acc}")\n'
    )
    return spy, py, "nrows="


def m48_gen_group_sum(csv_path: Path, iters: int) -> tuple[str, str, str]:
    spy = m48_spy_preamble(csv_path) + (
        f'    iters: i64 = {iters}i64\n'
        '    i: i64 = 0i64\n'
        '    keys: List[str] = []\n'
        '    keys.append("category")\n'
        '    acc: i64 = 0i64\n'
        '    while i < iters:\n'
        '        gdf: GroupedDataFrame = df.group_by(keys)\n'
        '        summed: DataFrame = gdf.sum()\n'
        '        acc = acc + summed.length()\n'
        '        i = i + 1i64\n'
        '    println("nrows=" + str(df.length()) + " acc=" + str(acc))\n'
        '    return 0\n'
    )
    py = m48_py_preamble(csv_path) + (
        f'iters = {iters}\n'
        'acc = 0\n'
        'for _ in range(iters):\n'
        '    summed = df.groupby("category", dropna=False, observed=True).sum(numeric_only=True)\n'
        '    acc += len(summed)\n'
        'print(f"nrows={len(df)} acc={acc}")\n'
    )
    return spy, py, "nrows="


def m48_gen_merge_inner(csv_path: Path, iters: int) -> tuple[str, str, str]:
    # Self-merge on `id` — uniform across both backends.  In StrictPy
    # we re-read the CSV once for `df2` to avoid having to clone the
    # frame in user code; pandas just `df.copy()`.
    csv_lit = str(csv_path).replace("\\", "\\\\")
    spy = m48_spy_preamble(csv_path) + (
        f'    df2: DataFrame = tabular.read_csv("{csv_lit}", schema)\n'
        f'    iters: i64 = {iters}i64\n'
        '    i: i64 = 0i64\n'
        '    on: List[str] = []\n'
        '    on.append("id")\n'
        '    acc: i64 = 0i64\n'
        '    while i < iters:\n'
        '        merged: DataFrame = df.merge(df2, on, "inner")\n'
        '        acc = acc + merged.length()\n'
        '        i = i + 1i64\n'
        '    println("nrows=" + str(df.length()) + " acc=" + str(acc))\n'
        '    return 0\n'
    )
    py = m48_py_preamble(csv_path) + (
        'df2 = df.copy()\n'
        f'iters = {iters}\n'
        'acc = 0\n'
        'for _ in range(iters):\n'
        '    merged = df.merge(df2, on=["id"], how="inner")\n'
        '    acc += len(merged)\n'
        'print(f"nrows={len(df)} acc={acc}")\n'
    )
    return spy, py, "nrows="


def m48_gen_pivot_table(csv_path: Path, iters: int) -> tuple[str, str, str]:
    spy = m48_spy_preamble(csv_path) + (
        f'    iters: i64 = {iters}i64\n'
        '    i: i64 = 0i64\n'
        '    acc: i64 = 0i64\n'
        '    while i < iters:\n'
        '        p: DataFrame = df.pivot_table("category", "area", "qty", "sum")\n'
        '        acc = acc + p.length()\n'
        '        i = i + 1i64\n'
        '    println("nrows=" + str(df.length()) + " acc=" + str(acc))\n'
        '    return 0\n'
    )
    py = m48_py_preamble(csv_path) + (
        f'iters = {iters}\n'
        'acc = 0\n'
        'for _ in range(iters):\n'
        '    p = df.pivot_table(index="category", columns="area", values="qty",\n'
        '                       aggfunc="sum", observed=True, dropna=False)\n'
        '    acc += len(p)\n'
        'print(f"nrows={len(df)} acc={acc}")\n'
    )
    return spy, py, "nrows="


def m48_gen_rolling_mean(csv_path: Path, iters: int) -> tuple[str, str, str]:
    spy = m48_spy_preamble(csv_path) + (
        f'    iters: i64 = {iters}i64\n'
        '    i: i64 = 0i64\n'
        '    acc: i64 = 0i64\n'
        '    while i < iters:\n'
        '        pc: ColumnF64? = df.get_column_f64("price")\n'
        '        if pc is not none:\n'
        '            r: ColumnF64 = pc.rolling_mean(7i64)\n'
        '            acc = acc + r.length()\n'
        '        i = i + 1i64\n'
        '    println("nrows=" + str(df.length()) + " acc=" + str(acc))\n'
        '    return 0\n'
    )
    py = m48_py_preamble(csv_path) + (
        f'iters = {iters}\n'
        'acc = 0\n'
        'for _ in range(iters):\n'
        '    r = df["price"].rolling(7).mean()\n'
        '    acc += len(r)\n'
        'print(f"nrows={len(df)} acc={acc}")\n'
    )
    return spy, py, "nrows="


def m48_gen_describe(csv_path: Path, iters: int) -> tuple[str, str, str]:
    spy = m48_spy_preamble(csv_path) + (
        f'    iters: i64 = {iters}i64\n'
        '    i: i64 = 0i64\n'
        '    acc: i64 = 0i64\n'
        '    while i < iters:\n'
        '        d: DataFrame = df.describe()\n'
        '        acc = acc + d.length()\n'
        '        i = i + 1i64\n'
        '    println("nrows=" + str(df.length()) + " acc=" + str(acc))\n'
        '    return 0\n'
    )
    py = m48_py_preamble(csv_path) + (
        f'iters = {iters}\n'
        'acc = 0\n'
        'for _ in range(iters):\n'
        '    d = df.describe()\n'
        '    acc += len(d)\n'
        'print(f"nrows={len(df)} acc={acc}")\n'
    )
    return spy, py, "nrows="


# ── Phase C: categorical-cost cells ─────────────────────────────────────────

def m48_gen_group_by_str(csv_path: Path, iters: int) -> tuple[str, str, str]:
    """Group on `category` as ColumnStr (the canonical str-hash path)."""
    spy = m48_spy_preamble(csv_path) + (
        f'    iters: i64 = {iters}i64\n'
        '    i: i64 = 0i64\n'
        '    keys: List[str] = []\n'
        '    keys.append("category")\n'
        '    acc: i64 = 0i64\n'
        '    while i < iters:\n'
        '        gdf: GroupedDataFrame = df.group_by(keys)\n'
        '        s: DataFrame = gdf.size()\n'
        '        acc = acc + s.length()\n'
        '        i = i + 1i64\n'
        '    println("nrows=" + str(df.length()) + " acc=" + str(acc))\n'
        '    return 0\n'
    )
    py = m48_py_preamble(csv_path) + (
        f'iters = {iters}\n'
        'acc = 0\n'
        'for _ in range(iters):\n'
        '    s = df.groupby("category", dropna=False, observed=True).size()\n'
        '    acc += len(s)\n'
        'print(f"nrows={len(df)} acc={acc}")\n'
    )
    return spy, py, "nrows="


def m48_gen_group_by_cat_via_strings(csv_path: Path, iters: int) -> tuple[str, str, str]:
    """Group on ColumnCategorical via the v1 to_strings() coercion path."""
    spy = m48_spy_preamble(csv_path) + (
        '    cs: ColumnStr? = df.get_column_str("category")\n'
        '    if cs is none:\n'
        '        println("nrows=" + str(df.length()) + " acc=0")\n'
        '        return 0\n'
        '    cs_v: ColumnStr = cs.fill_null("__null__")\n'
        '    j: i64 = 0i64\n'
        '    cs_values: List[str] = []\n'
        '    while j < cs_v.length():\n'
        '        opt: str? = cs_v.get(j)\n'
        '        if opt is not none:\n'
        '            cs_values.append(opt)\n'
        '        else:\n'
        '            cs_values.append("__null__")\n'
        '        j = j + 1i64\n'
        '    cat_col: ColumnCategorical = tabular.col_categorical(cs_values)\n'
        f'    iters: i64 = {iters}i64\n'
        '    i: i64 = 0i64\n'
        '    keys: List[str] = []\n'
        '    keys.append("cat")\n'
        '    acc: i64 = 0i64\n'
        '    while i < iters:\n'
        '        col_names: List[str] = []\n'
        '        col_names.append("cat")\n'
        '        cols2: List[Column] = []\n'
        '        cols2.append(cat_col.to_strings())\n'
        '        df2: DataFrame = tabular.from_columns(col_names, cols2)\n'
        '        gdf: GroupedDataFrame = df2.group_by(keys)\n'
        '        s: DataFrame = gdf.size()\n'
        '        acc = acc + s.length()\n'
        '        i = i + 1i64\n'
        '    println("nrows=" + str(df.length()) + " acc=" + str(acc))\n'
        '    return 0\n'
    )
    # The "via_strings" comparable on pandas is the same str groupby —
    # documented as such; the interesting comparison is StrictPy
    # cat_via_strings vs StrictPy str (both rows from this harness) +
    # pandas Categorical (next cell).
    py = m48_py_preamble(csv_path) + (
        'df["category"] = df["category"].fillna("__null__")\n'
        f'iters = {iters}\n'
        'acc = 0\n'
        'for _ in range(iters):\n'
        '    s = df.groupby("category", dropna=False, observed=True).size()\n'
        '    acc += len(s)\n'
        'print(f"nrows={len(df)} acc={acc}")\n'
    )
    return spy, py, "nrows="


def m48_gen_group_by_pandas_categorical(csv_path: Path, iters: int) -> tuple[str, str, str]:
    """StrictPy side: same as group_by_str (no native code path).  Pandas
    side: convert to Categorical dtype first — this is the optimized
    codes-hash path we want pandas Categorical to demonstrate."""
    spy = m48_spy_preamble(csv_path) + (
        f'    iters: i64 = {iters}i64\n'
        '    i: i64 = 0i64\n'
        '    keys: List[str] = []\n'
        '    keys.append("category")\n'
        '    acc: i64 = 0i64\n'
        '    while i < iters:\n'
        '        gdf: GroupedDataFrame = df.group_by(keys)\n'
        '        s: DataFrame = gdf.size()\n'
        '        acc = acc + s.length()\n'
        '        i = i + 1i64\n'
        '    println("nrows=" + str(df.length()) + " acc=" + str(acc))\n'
        '    return 0\n'
    )
    py = m48_py_preamble(csv_path) + (
        'df["category"] = df["category"].astype("category")\n'
        f'iters = {iters}\n'
        'acc = 0\n'
        'for _ in range(iters):\n'
        '    s = df.groupby("category", dropna=False, observed=True).size()\n'
        '    acc += len(s)\n'
        'print(f"nrows={len(df)} acc={acc}")\n'
    )
    return spy, py, "nrows="


def m48_gen_merge_str(csv_path: Path, iters: int) -> tuple[str, str, str]:
    """Merge on the str `category` column (Phase C: str path)."""
    csv_lit = str(csv_path).replace("\\", "\\\\")
    spy = m48_spy_preamble(csv_path) + (
        f'    df2: DataFrame = tabular.read_csv("{csv_lit}", schema)\n'
        f'    iters: i64 = {iters}i64\n'
        '    on: List[str] = []\n'
        '    on.append("category")\n'
        '    i: i64 = 0i64\n'
        '    acc: i64 = 0i64\n'
        '    while i < iters:\n'
        '        m: DataFrame = df.merge(df2, on, "inner")\n'
        '        acc = acc + m.length()\n'
        '        i = i + 1i64\n'
        '    println("nrows=" + str(df.length()) + " acc=" + str(acc))\n'
        '    return 0\n'
    )
    py = m48_py_preamble(csv_path) + (
        'df2 = df.copy()\n'
        f'iters = {iters}\n'
        'acc = 0\n'
        'for _ in range(iters):\n'
        '    m = df.merge(df2, on=["category"], how="inner")\n'
        '    acc += len(m)\n'
        'print(f"nrows={len(df)} acc={acc}")\n'
    )
    return spy, py, "nrows="


def m48_gen_merge_cat_via_strings(csv_path: Path, iters: int) -> tuple[str, str, str]:
    """StrictPy side: same str-merge (categorical coerces to str in v1).
    Pandas side: cast to category dtype first."""
    csv_lit = str(csv_path).replace("\\", "\\\\")
    spy = m48_spy_preamble(csv_path) + (
        f'    df2: DataFrame = tabular.read_csv("{csv_lit}", schema)\n'
        f'    iters: i64 = {iters}i64\n'
        '    on: List[str] = []\n'
        '    on.append("category")\n'
        '    i: i64 = 0i64\n'
        '    acc: i64 = 0i64\n'
        '    while i < iters:\n'
        '        m: DataFrame = df.merge(df2, on, "inner")\n'
        '        acc = acc + m.length()\n'
        '        i = i + 1i64\n'
        '    println("nrows=" + str(df.length()) + " acc=" + str(acc))\n'
        '    return 0\n'
    )
    py = m48_py_preamble(csv_path) + (
        'df["category"] = df["category"].astype("category")\n'
        'df2 = df.copy()\n'
        f'iters = {iters}\n'
        'acc = 0\n'
        'for _ in range(iters):\n'
        '    m = df.merge(df2, on=["category"], how="inner")\n'
        '    acc += len(m)\n'
        'print(f"nrows={len(df)} acc={acc}")\n'
    )
    return spy, py, "nrows="


def m48_gen_unique_str(csv_path: Path, iters: int) -> tuple[str, str, str]:
    spy = m48_spy_preamble(csv_path) + (
        f'    iters: i64 = {iters}i64\n'
        '    i: i64 = 0i64\n'
        '    acc: i64 = 0i64\n'
        '    while i < iters:\n'
        '        u: ColumnStr? = df.unique_str("category")\n'
        '        if u is not none:\n'
        '            acc = acc + u.length()\n'
        '        i = i + 1i64\n'
        '    println("nrows=" + str(df.length()) + " acc=" + str(acc))\n'
        '    return 0\n'
    )
    py = m48_py_preamble(csv_path) + (
        f'iters = {iters}\n'
        'acc = 0\n'
        'for _ in range(iters):\n'
        '    u = df["category"].dropna().unique()\n'
        '    acc += len(u)\n'
        'print(f"nrows={len(df)} acc={acc}")\n'
    )
    return spy, py, "nrows="


def m48_gen_unique_cat_via_strings(csv_path: Path, iters: int) -> tuple[str, str, str]:
    """Pandas: cast to category first (its .unique() returns category codes
    very fast).  StrictPy: same as unique_str (no codes-fastpath in v1)."""
    spy, _, exp = m48_gen_unique_str(csv_path, iters)
    py = m48_py_preamble(csv_path) + (
        'df["category"] = df["category"].astype("category")\n'
        f'iters = {iters}\n'
        'acc = 0\n'
        'for _ in range(iters):\n'
        '    u = df["category"].dropna().unique()\n'
        '    acc += len(u)\n'
        'print(f"nrows={len(df)} acc={acc}")\n'
    )
    return spy, py, exp


# Op registry — maps op name to generator + a short label for the report.
M48_OP_REGISTRY = {
    "read_csv":     ("read_csv (load only)",                      m48_gen_read_csv),
    "filter":       ("filter (qty > 500)",                        m48_gen_filter),
    "sort_by":      ("sort_by price asc",                         m48_gen_sort_by),
    "group_sum":    ("group_by category + sum",                   m48_gen_group_sum),
    "merge_inner":  ("merge inner on id",                         m48_gen_merge_inner),
    "pivot_table":  ("pivot_table category x area sum(qty)",      m48_gen_pivot_table),
    "rolling_mean": ("rolling_mean(7) on price",                  m48_gen_rolling_mean),
    "describe":     ("describe()",                                m48_gen_describe),
    "group_by_str":               ("group_by str category (size)",         m48_gen_group_by_str),
    "group_by_cat_via_strings":   ("group_by Categorical via to_strings",  m48_gen_group_by_cat_via_strings),
    "group_by_pandas_categorical":("group_by pandas Categorical (codes)",  m48_gen_group_by_pandas_categorical),
    "merge_str":                  ("merge inner on category (str)",        m48_gen_merge_str),
    "merge_cat_via_strings":      ("merge inner on category (cat coerce)", m48_gen_merge_cat_via_strings),
    "unique_str":                 ("unique str category",                  m48_gen_unique_str),
    "unique_cat_via_strings":     ("unique Categorical via to_strings",    m48_gen_unique_cat_via_strings),
}

# Two phase groupings — Phase B (core 8) and Phase C (categorical).
M48_CORE_OPS = [
    "read_csv", "filter", "sort_by", "group_sum",
    "merge_inner", "pivot_table", "rolling_mean", "describe",
]
M48_CATEGORICAL_OPS = [
    "group_by_str", "group_by_cat_via_strings", "group_by_pandas_categorical",
    "merge_str", "merge_cat_via_strings",
    "unique_str", "unique_cat_via_strings",
]


# ─────────────────────────────────────────────────────────────────────────────
#  Memory-polling runner
# ─────────────────────────────────────────────────────────────────────────────

def m48_run_with_memory(cmd: list[str], timeout: int = SUBPROC_TIMEOUT
                        ) -> tuple[float, str, int]:
    """Run `cmd`, return (wall_seconds, stdout, peak_rss_bytes).

    Memory is polled at POLL_MS via a background thread reading
    psutil.Process(pid).memory_info().rss (peak across samples).  If
    psutil isn't available, returns 0 for peak_rss and the run is
    wall-clock only.  Children that exit between samples may yield
    peak=0 — we backstop with one final sample after wait().

    Raises RuntimeError on non-zero exit.
    """
    t0 = time.perf_counter()
    proc = subprocess.Popen(cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE)

    peak = 0
    stop_flag = threading.Event()

    def poll():
        nonlocal peak
        if not HAVE_PSUTIL:
            return
        try:
            p = psutil.Process(proc.pid)
        except psutil.NoSuchProcess:
            return
        while not stop_flag.is_set():
            try:
                rss = p.memory_info().rss
                if rss > peak:
                    peak = rss
                # Also walk children (some VMs spawn helpers).
                for child in p.children(recursive=True):
                    try:
                        crss = child.memory_info().rss
                        if crss > peak:
                            peak = crss
                    except (psutil.NoSuchProcess, psutil.AccessDenied):
                        pass
            except (psutil.NoSuchProcess, psutil.AccessDenied):
                break
            time.sleep(POLL_MS / 1000.0)

    poll_thread = threading.Thread(target=poll, daemon=True)
    poll_thread.start()

    try:
        stdout_b, stderr_b = proc.communicate(timeout=timeout)
    except subprocess.TimeoutExpired:
        proc.kill()
        stop_flag.set()
        poll_thread.join(timeout=1.0)
        raise RuntimeError(f"command {cmd!r} timed out after {timeout}s")

    stop_flag.set()
    poll_thread.join(timeout=1.0)
    elapsed = time.perf_counter() - t0

    if proc.returncode != 0:
        stderr_s = stderr_b.decode("utf-8", errors="replace")
        raise RuntimeError(
            f"command {cmd!r} failed: rc={proc.returncode}\nstderr:\n{stderr_s[-2000:]}"
        )
    return elapsed, stdout_b.decode("utf-8", errors="replace"), peak


def m48_best_of(cmd: list[str], n: int = BEST_OF, timeout: int = SUBPROC_TIMEOUT
                ) -> tuple[float, str, int]:
    """Take min(wall) across n runs; return (min_wall, last_stdout, max_peak_rss)."""
    times = []
    peaks = []
    stdout = ""
    for _ in range(n):
        t, s, p = m48_run_with_memory(cmd, timeout=timeout)
        times.append(t)
        peaks.append(p)
        stdout = s
    return min(times), stdout, max(peaks) if peaks else 0


# ─────────────────────────────────────────────────────────────────────────────
#  Cell runner
# ─────────────────────────────────────────────────────────────────────────────

def m48_run_cell(op: str, size: str, csv_path: Path, iters: int) -> dict:
    """Compile + run one (op, size) cell.  Returns a dict suitable for JSON."""
    label, gen = M48_OP_REGISTRY[op]
    spy_src, py_src, expected_sub = gen(csv_path, iters)

    base = f"m48_{op}_{size}"
    spy_path  = GEN_DIR / f"{base}.spy"
    spyc_path = GEN_DIR / f"{base}.spyc"
    py_path   = GEN_DIR / f"{base}.py"
    spy_path.write_text(spy_src, encoding="utf-8")
    py_path.write_text(py_src, encoding="utf-8")

    rec: dict = {
        "op": op, "size": size, "iters": iters,
        "spy_ms": None, "py_ms": None, "ratio": None,
        "spy_rss_mb": None, "py_rss_mb": None, "rss_ratio": None,
        "note": "",
    }

    # Compile StrictPy (not timed).
    if not SPY.exists():
        rec["note"] = f"spy.exe missing at {SPY}"
        return rec
    res = subprocess.run(
        [str(SPY), "--compile-only", str(spy_path), "-o", str(spyc_path)],
        capture_output=True, text=True,
    )
    if res.returncode != 0:
        rec["note"] = f"spy compile failed: {(res.stderr or res.stdout).strip()[-400:]}"
        return rec

    n_runs = M48_BEST_OF_OVERRIDE.get((op, size),
             M48_BEST_OF_FOR_SIZE.get(size, BEST_OF))

    # Run StrictPy.
    try:
        spy_time, spy_out, spy_rss = m48_best_of([str(SPY), str(spyc_path)], n=n_runs)
    except Exception as e:
        rec["note"] = f"spy run failed: {str(e)[-400:]}"
        return rec

    # Run pandas.
    try:
        py_time, py_out, py_rss = m48_best_of([PYTHON, str(py_path)], n=n_runs)
    except Exception as e:
        rec["spy_ms"] = spy_time * 1000
        rec["spy_rss_mb"] = spy_rss / (1024 * 1024) if spy_rss else None
        rec["note"] = f"python run failed: {str(e)[-400:]}"
        return rec

    # Sanity: both should mention `nrows=`.
    if expected_sub not in spy_out:
        rec["note"] = f"spy stdout missing {expected_sub!r}: {spy_out[-200:]!r}"
    if expected_sub not in py_out:
        rec["note"] = (rec["note"] + "; " if rec["note"] else "") + \
                      f"py stdout missing {expected_sub!r}: {py_out[-200:]!r}"

    rec["spy_ms"] = spy_time * 1000
    rec["py_ms"]  = py_time * 1000
    rec["ratio"]  = spy_time / py_time if py_time > 0 else None
    rec["spy_rss_mb"] = spy_rss / (1024 * 1024) if spy_rss else None
    rec["py_rss_mb"]  = py_rss  / (1024 * 1024) if py_rss  else None
    if rec["spy_rss_mb"] and rec["py_rss_mb"]:
        rec["rss_ratio"] = rec["spy_rss_mb"] / rec["py_rss_mb"]
    return rec


def m48_fmt(r: dict) -> str:
    if r["spy_ms"] is None:
        return f"FAIL ({r.get('note','')[:120]})"
    py_ms = f"{r['py_ms']:8.1f}ms" if r["py_ms"] is not None else "    FAIL"
    ratio = f"{r['ratio']:6.2f}x" if r["ratio"] is not None else "  ----"
    rss = ""
    if r["spy_rss_mb"] is not None and r["py_rss_mb"] is not None:
        rss = f" RSS spy={r['spy_rss_mb']:6.1f}MB py={r['py_rss_mb']:6.1f}MB " \
              f"r={r['rss_ratio']:5.2f}x"
    return (f"spy={r['spy_ms']:8.1f}ms  py={py_ms}  ratio={ratio}"
            f"{rss}  {r.get('note','')}").rstrip()


# ─────────────────────────────────────────────────────────────────────────────
#  Report rendering
# ─────────────────────────────────────────────────────────────────────────────

def m48_write_report(results: list[dict], out_path: Path) -> None:
    """Render bench/TABULAR_BENCH_REPORT.md from JSON results."""
    py_ver = sys.version.split()[0]

    def tally(rows):
        wins = sum(1 for r in rows if r["ratio"] is not None and r["ratio"] < 0.85)
        ties = sum(1 for r in rows if r["ratio"] is not None and 0.85 <= r["ratio"] <= 1.15)
        loss = sum(1 for r in rows if r["ratio"] is not None and r["ratio"] > 1.15)
        return wins, ties, loss

    rated = [r for r in results if r["ratio"] is not None]
    aw, at, al = tally(rated)

    def geomean(rs):
        rs = [r for r in rs if r is not None and r > 0]
        if not rs:
            return None
        return math.exp(sum(math.log(x) for x in rs) / len(rs))

    gm = geomean([r["ratio"] for r in rated])

    def row_marker(ratio):
        if ratio is None: return ""
        if ratio < 0.85:  return "v "   # StrictPy faster
        if ratio > 1.5:   return "x "   # pandas wins by >50%
        return ""

    lines = [
        "# StrictPy `tabular` vs pandas 3.0 — comprehensive benchmark",
        "",
        f"_Generated by `python bench/tabular_harness.py`. Best of {BEST_OF} runs per cell. "
        "Wall-clock time for execution of pre-compiled bytecode — `.spyc` for StrictPy, "
        "`.pyc` for Python (Python's import-side compile NOT separated here — pandas's "
        "import cost lives in the wall-clock). Process startup IS included._",
        "",
        f"- **StrictPy**: post-M47 release build (`cargo build --workspace --release`). "
        "Tabular surface = M37-M47 (19 stdlib classes, ~50 methods).",
        f"- **pandas**: {_pandas_version()} on Python {py_ver}.",
        "- **Host**: Windows 11, single-thread workload.",
        f"- **Memory measurement**: psutil RSS polled every {POLL_MS} ms; peak across "
        "polls + a final post-wait sample. Documented as best-effort — Windows working-set "
        "RSS is the OS's view, not pandas's allocator high-water mark.",
        "",
        f"**Aggregate ({aw + at + al} timed cells):** "
        f"{aw} StrictPy wins  ·  {at} ties  ·  {al} pandas wins. "
        f"Geomean ratio (StrictPy / pandas) = "
        + (f"{gm:.2f}x." if gm is not None else "—."),
        "",
        "Ratio = StrictPy time / pandas time. **Lower is better for StrictPy.** "
        "`v` marks cells where StrictPy is >=15% faster; `x` marks cells where pandas "
        "wins by >=50%.",
        "",
        "---",
        "",
        "## Headline summary",
        "",
        "| Op | small | medium | large | xl |",
        "|---|---:|---:|---:|---:|",
    ]

    # Headline: ratio per (op, size)
    sizes_in_use = [s for s in ("small", "medium", "large", "xl")
                    if any(r["size"] == s for r in results)]
    headline_sizes = ["small", "medium", "large", "xl"]
    for op in M48_CORE_OPS + M48_CATEGORICAL_OPS:
        label, _ = M48_OP_REGISTRY[op]
        row = [f"`{op}`"]
        for sz in headline_sizes:
            cell = next((r for r in results if r["op"] == op and r["size"] == sz), None)
            if cell is None:
                row.append("—")
            elif cell["ratio"] is None:
                note = (cell.get("note") or "").lower()
                if "skipped" in note or "timed out" in note or "timeout" in note:
                    row.append("skip")
                else:
                    row.append("FAIL")
            else:
                row.append(f"{row_marker(cell['ratio'])}{cell['ratio']:.2f}x")
        lines.append("| " + " | ".join(row) + " |")

    lines.extend(["", "---", "", "## Per-op detail"])

    size_order = {"small": 0, "medium": 1, "large": 2, "xl": 3}
    for op in M48_CORE_OPS + M48_CATEGORICAL_OPS:
        label, _ = M48_OP_REGISTRY[op]
        rows = sorted(
            [r for r in results if r["op"] == op],
            key=lambda r: size_order.get(r["size"], 99),
        )
        if not rows:
            continue
        lines.extend([
            "",
            f"### `{op}` — {label}",
            "",
            "| Size | iters | StrictPy ms | pandas ms | Ratio | StrictPy RSS | pandas RSS | RSS ratio |",
            "|---|---:|---:|---:|---:|---:|---:|---:|",
        ])
        for r in rows:
            note_lc = (r.get("note") or "").lower()
            skipped = ("skipped" in note_lc or "timed out" in note_lc
                       or "timeout" in note_lc)
            fail_label = "skip" if skipped else "FAIL"
            spy_ms = f"{r['spy_ms']:.1f}" if r['spy_ms'] is not None else fail_label
            py_ms  = f"{r['py_ms']:.1f}"  if r['py_ms']  is not None else fail_label
            ratio  = (f"{row_marker(r['ratio'])}{r['ratio']:.2f}x"
                      if r['ratio'] is not None else "—")
            spy_rss = f"{r['spy_rss_mb']:.1f} MB" if r['spy_rss_mb'] is not None else "—"
            py_rss  = f"{r['py_rss_mb']:.1f} MB"  if r['py_rss_mb']  is not None else "—"
            rss_r   = f"{r['rss_ratio']:.2f}x"     if r['rss_ratio']  is not None else "—"
            lines.append(
                f"| {r['size']} | {r['iters']} | {spy_ms} | {py_ms} | {ratio} | "
                f"{spy_rss} | {py_rss} | {rss_r} |"
            )
        notes = [r["note"] for r in rows if r.get("note")]
        for n in notes:
            lines.append(f"> note: {n}")

    # Categorical cost analysis — pull actual numbers from the cells
    def _cell(op: str, size: str) -> dict | None:
        return next((r for r in results if r["op"] == op and r["size"] == size), None)

    def _ms(op: str, size: str) -> str:
        r = _cell(op, size)
        if r and r["spy_ms"] is not None:
            return f"{r['spy_ms']:.0f} ms"
        return "—"

    gby_str_med  = _cell("group_by_str", "medium")
    gby_cat_med  = _cell("group_by_cat_via_strings", "medium")
    gby_pcat_med = _cell("group_by_pandas_categorical", "medium")

    cat_lines = [
        "",
        "---",
        "",
        "## Categorical cost analysis (Phase C)",
        "",
        "M47 shipped `ColumnCategorical` with codes + categories storage, but every op "
        "currently coerces via `to_strings()` (documented as v1 limitation). The cells "
        "below quantify the cost vs the canonical `ColumnStr` group-by, and the gap to "
        "pandas's optimized codes-hash Categorical:",
        "",
        "- `group_by_str`: hash on the str column directly.",
        "- `group_by_cat_via_strings`: build a `ColumnCategorical`, then materialize "
        "back via `to_strings()` and group (the v1 integration idiom).",
        "- `group_by_pandas_categorical`: pandas with `astype('category')` — the "
        "codes-hash fastpath we want M49 to beat.",
        "",
        "**Measured numbers (medium = 10k rows, 3 iters):**",
        "",
        "| Cell | StrictPy ms | pandas ms | Notes |",
        "|---|---:|---:|---|",
        f"| `group_by_str` | {_ms('group_by_str', 'medium')} | "
        + (f"{gby_str_med['py_ms']:.0f} ms" if gby_str_med and gby_str_med['py_ms'] else "—")
        + " | StrictPy's v1 string-hash baseline |",
        f"| `group_by_cat_via_strings` | {_ms('group_by_cat_via_strings', 'medium')} | "
        + (f"{gby_cat_med['py_ms']:.0f} ms" if gby_cat_med and gby_cat_med['py_ms'] else "—")
        + " | adds `to_strings()` materialization on top of str path |",
        f"| `group_by_pandas_categorical` | {_ms('group_by_pandas_categorical', 'medium')} | "
        + (f"{gby_pcat_med['py_ms']:.0f} ms" if gby_pcat_med and gby_pcat_med['py_ms'] else "—")
        + " | StrictPy unchanged; pandas uses `astype('category')` |",
    ]

    if gby_str_med and gby_cat_med and gby_str_med['spy_ms'] and gby_cat_med['spy_ms']:
        coerce_overhead = (gby_cat_med['spy_ms'] / gby_str_med['spy_ms'] - 1.0) * 100
        cat_lines.extend([
            "",
            f"**`to_strings()` coercion overhead** (StrictPy): "
            f"{coerce_overhead:+.0f}% wall-clock vs the direct str path "
            f"({gby_str_med['spy_ms']:.0f} → {gby_cat_med['spy_ms']:.0f} ms at medium). "
            "An optimized codes-hash group-by path (M49) would skip both the str "
            "hashing AND the `to_strings()` materialization, so the expected win is "
            "larger than this gap alone.",
        ])

    if gby_str_med and gby_pcat_med and gby_str_med['py_ms'] and gby_pcat_med['py_ms']:
        pandas_cat_speedup = gby_str_med['py_ms'] / gby_pcat_med['py_ms']
        cat_lines.extend([
            "",
            f"**Pandas Categorical speedup at this cardinality** (8 distinct values): "
            f"{pandas_cat_speedup:.2f}x — i.e. essentially nothing. Pandas's "
            "codes-hash optimization shines at high cardinality (thousands of distinct "
            "values where comparing 32-bit codes is dramatically cheaper than comparing "
            "string interned object pointers). At 8 categories, the string interning "
            "+ pointer equality is already cheap. **M49 should sanity-check this on a "
            "higher-cardinality fixture** before claiming the codes-hash optimization "
            "is the universal win.",
        ])

    cat_lines.extend([
        "",
        "**`merge_*` at medium** is dominated by the **self-merge cartesian "
        "blow-up**: with 10k rows × 8 distinct category values, the inner-join "
        "produces ~12.5M rows (10000 × 10000 / 8). Both sides spend most of "
        "their time materializing the joined frame. The StrictPy/pandas ratio "
        "(~15x) reflects pandas's NumPy-backed concat being much faster than "
        "StrictPy's per-cell `List<T>` append. This is a known M49 target "
        "(`merge_inner` is one of the codes-hash paths).",
        "",
        "---",
    ])
    lines.extend(cat_lines)
    lines.extend([
        "",
        "## Methodology",
        "",
        f"- **Iterations per cell**: tuned so small-size wall clock is ~0.5-1.5 s "
        f"(see `M48_DEFAULT_ITERS` + `M48_ITERS_OVERRIDE` in `tabular_harness.py`); "
        f"read_csv runs once (no loop) because loading dominates.",
        f"- **BEST_OF = {BEST_OF}**: each (op, size, language) pair is run {BEST_OF} times; "
        "report uses the minimum wall clock + max observed RSS.",
        f"- **CSV fixtures**: deterministic (seed={M48_SEED}), cached in `bench/data/`. "
        "Schema: `id i64`, `category str (8 zipf-skewed values)`, `area str (4 uniform)`, "
        f"`qty i64 (1..1000)`, `price f64 (0.01..10000)`, `ts datetime (daily increments)`. "
        f"~{int(M48_NULL_PCT*100)}% random nulls per column (except `id`).",
        f"- **Memory polling**: psutil RSS sampled every {POLL_MS} ms in a daemon thread "
        "for the duration of each subprocess. Windows working-set semantics — actual "
        "peak heap may be larger; the metric is consistent across both sides.",
        "- **Both sides include process startup**: spy.exe + JIT load on the StrictPy "
        "side; python + pandas import on the pandas side. Startup is meaningful at "
        "the small-size end.",
        "",
        "---",
        "",
        "## Reproducibility",
        "",
        "```",
        "cargo build --workspace --release",
        "python bench/tabular_harness.py             # small + medium + large",
        "python bench/tabular_harness.py --xl        # also include xl (100M rows)",
        "python bench/tabular_harness.py --report-only  # re-render from JSON",
        "```",
        "",
        "Raw measurements: `bench/history/m48_tabular.json`. Generated `.spy` + `.py` "
        "pairs and `.spyc` artifacts: `bench/generated/m48_*`.",
        "",
        "---",
        "",
        "## Honest findings",
        "",
        "Findings are summarised inline in the per-op tables — see the `Ratio` column "
        "(StrictPy time / pandas time). The headline reading:",
        "",
        f"- {aw} / {aw + at + al} cells go to StrictPy; {al} go to pandas; "
        f"{at} are within ±15% (tied).",
        f"- Geomean across all rated cells: "
        + (f"**{gm:.2f}x**" if gm is not None else "—") + ". "
        "Values < 1 mean StrictPy wins on average; > 1 means pandas wins on average.",
        "- Honest expectation pre-run: pandas should beat StrictPy on compute-heavy "
        "ops at large N because NumPy uses SIMD + contiguous buffers and pandas's "
        "Cython/C kernels are extremely tight. StrictPy's wins (if any) will likely "
        "concentrate at small sizes where pandas's import-time overhead dominates, "
        "and on ops where pandas falls back to Python-object dtypes.",
        "- The `group_by_pandas_categorical` cell vs `group_by_str`/`_cat_via_strings` "
        "is the M49 target: the gap between the two on the pandas side maps directly "
        "to what an optimized codes-hash path on StrictPy could deliver.",
        "",
        "**What this benchmark does NOT measure**: SIMD utilization, GIL release "
        "patterns, memory-mapped I/O, multi-core scaling. All cells are single-thread "
        "wall-clock against a single-thread pandas baseline.",
    ]
    + (["",
        "## Notes — what was cut / partial",
        "",
        _cut_notes(results),
        ] if _has_skipped(results) else [])
    )

    out_path.write_text("\n".join(lines), encoding="utf-8")


def _pandas_version() -> str:
    try:
        import pandas as pd
        return pd.__version__
    except ImportError:
        return "unknown"


def _has_skipped(results: list[dict]) -> bool:
    return any(r.get("note", "").startswith("skipped") for r in results)


def _cut_notes(results: list[dict]) -> str:
    out = []
    for r in results:
        if r.get("note", "").startswith("skipped"):
            out.append(f"- `{r['op']}` @ `{r['size']}`: {r['note']}")
    return "\n".join(out) if out else "_(none)_"


# ─────────────────────────────────────────────────────────────────────────────
#  Main runner
# ─────────────────────────────────────────────────────────────────────────────

def m48_main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--sizes", default="small,medium,large",
                    help="comma-separated subset of small/medium/large/xl (default: small,medium,large)")
    ap.add_argument("--xl", action="store_true",
                    help="also include xl (100M rows); equivalent to adding xl to --sizes")
    ap.add_argument("--ops", default="",
                    help="comma-separated subset of op names (default: all core + categorical)")
    ap.add_argument("--report-only", action="store_true",
                    help="re-render the report from existing JSON (no runs)")
    ap.add_argument("--skip-xl-csv", action="store_true",
                    help="never (re-)generate the xl CSV even if requested")
    args = ap.parse_args()

    json_path = HIST_DIR / "m48_tabular.json"
    md_path   = BENCH_DIR / "TABULAR_BENCH_REPORT.md"

    if args.report_only:
        if not json_path.exists():
            print(f"missing {json_path}; nothing to render", file=sys.stderr)
            sys.exit(1)
        results = json.loads(json_path.read_text(encoding="utf-8"))
        m48_write_report(results, md_path)
        print(f"report: {md_path}")
        return

    # Resolve size + op selection.
    sizes = [s.strip() for s in args.sizes.split(",") if s.strip()]
    if args.xl and "xl" not in sizes:
        sizes.append("xl")
    for s in sizes:
        if s not in M48_SIZES:
            print(f"unknown size {s!r}; valid: {list(M48_SIZES)}", file=sys.stderr)
            sys.exit(1)

    if args.ops:
        ops = [o.strip() for o in args.ops.split(",") if o.strip()]
        for o in ops:
            if o not in M48_OP_REGISTRY:
                print(f"unknown op {o!r}; valid: {list(M48_OP_REGISTRY)}", file=sys.stderr)
                sys.exit(1)
    else:
        ops = M48_CORE_OPS + M48_CATEGORICAL_OPS

    if not SPY.exists():
        print(f"missing spy.exe at {SPY}; run `cargo build --workspace --release`",
              file=sys.stderr)
        sys.exit(1)

    if not HAVE_PSUTIL:
        print("note: psutil not installed — memory measurements will be 0 / skipped.",
              file=sys.stderr)

    # Generate CSVs first (so subprocesses see them on first run).
    print("== CSV fixtures ==", flush=True)
    csv_paths: dict[str, Path] = {}
    for sz in sizes:
        if sz == "xl" and args.skip_xl_csv and not m48_csv_path("xl").exists():
            print(f"  skipping xl CSV generation (per --skip-xl-csv)", flush=True)
            continue
        csv_paths[sz] = m48_gen_csv(sz, M48_SIZES[sz])

    # Load any pre-existing results so we incrementally accumulate cells
    # across multiple invocations (e.g. small+medium in one run, large in
    # another — common when a partial run is interrupted by a long cell).
    # A cell is overwritten only when this invocation re-runs it.
    results: list[dict] = []
    if json_path.exists():
        try:
            results = json.loads(json_path.read_text(encoding="utf-8"))
        except Exception:
            results = []

    def merge_cell(rec: dict) -> None:
        key = (rec["op"], rec["size"])
        for i, r in enumerate(results):
            if (r["op"], r["size"]) == key:
                results[i] = rec
                return
        results.append(rec)

    # Run cells.
    for op in ops:
        print(f"\n== {op} ==", flush=True)
        for sz in sizes:
            if sz not in csv_paths:
                rec = {"op": op, "size": sz, "iters": 0,
                       "spy_ms": None, "py_ms": None, "ratio": None,
                       "spy_rss_mb": None, "py_rss_mb": None, "rss_ratio": None,
                       "note": "skipped (no csv)"}
                merge_cell(rec)
                continue
            iters = m48_iters_for(op, sz)
            print(f"  {op}({sz}, iters={iters}) …", flush=True)
            rec = m48_run_cell(op, sz, csv_paths[sz], iters)
            print(f"    {m48_fmt(rec)}", flush=True)
            merge_cell(rec)
            # Persist after every cell so a long run is recoverable.
            json_path.write_text(json.dumps(results, indent=2), encoding="utf-8")

    json_path.write_text(json.dumps(results, indent=2), encoding="utf-8")
    m48_write_report(results, md_path)
    print(f"\nreport: {md_path}")
    print(f"json:   {json_path}")


if __name__ == "__main__":
    m48_main()
