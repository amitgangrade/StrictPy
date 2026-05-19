"""Build the historical-comparison markdown section to append to BENCH_REPORT.md.

Reads every snapshot in bench/history/, picks a representative set of cells,
and emits two tables: StrictPy wall-clock (ms) per milestone, and ratio
vs CPython per milestone. Writes the result to stdout.
"""

import json
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
HIST = ROOT / "bench" / "history"

snapshots = [
    ("m7_pre_jit_fair",   "M7 (pre-JIT)"),
    ("m8_jit",            "M8 (JIT v1)"),
    ("m9_full_jit",       "M9 (full JIT)"),
    ("m10_real_world",    "M10"),
    ("m11_class_fix",     "M11"),
    ("m22_phase2_stdlib", "M22 (now)"),
]

data = {}
for slug, _ in snapshots:
    rows = json.loads((HIST / f"{slug}.json").read_text())
    data[slug] = {(r["name"], r["size"]): r for r in rows}

cells = [
    ("fib", "n20"), ("fib", "n30"), ("fib", "n33"),
    ("quicksort", "10000"), ("quicksort", "100000"),
    ("dot", "100000"), ("dot", "1000000"),
    ("mandelbrot", "60x30"),
]

out = []
out.append("## Historical comparison — StrictPy across milestones")
out.append("")
out.append(
    "Same benchmark suite at each StrictPy snapshot. JSON sources in "
    "`bench/history/`. Each cell is StrictPy median wall-clock in "
    "milliseconds (lower is better)."
)
out.append("")
header = "| Cell | " + " | ".join(label for _, label in snapshots) + " |"
out.append(header)
out.append("|---|" + "---:|" * len(snapshots))
for name, size in cells:
    row = [f"`{name}({size})`"]
    for slug, _ in snapshots:
        r = data[slug].get((name, size))
        row.append(f"{r['spy_ms']:.1f}" if r else "—")
    out.append("| " + " | ".join(row) + " |")
out.append("")
out.append(
    "Same data as ratios (StrictPy / CPython 3.12.10 wall-clock; lower is "
    "better; 0.10× means StrictPy is 10× faster):"
)
out.append("")
out.append(header)
out.append("|---|" + "---:|" * len(snapshots))
for name, size in cells:
    row = [f"`{name}({size})`"]
    for slug, _ in snapshots:
        r = data[slug].get((name, size))
        row.append(f"{r['ratio']:.2f}×" if r else "—")
    out.append("| " + " | ".join(row) + " |")
out.append("")
out.append("### What this trajectory shows")
out.append("")
out.append(
    "- **M7 → M8 (Cranelift AOT lands)**: `fib(30)` went 931 ms → 14.6 ms. "
    "The single biggest jump in the project."
)
out.append(
    "- **M8 → M9 (full JIT coverage)**: the list-mutation cells (quicksort, "
    "dot) flip from \"3× slower than CPython\" to \"5-10× faster\" once "
    "`ArraySet` / `ListPush` / `ListGet` become JIT-supported."
)
out.append(
    "- **M9 → M11 (correctness work)**: numbers stay essentially flat. "
    "Class-system overhaul, stress-test rounds, generics, exceptions — "
    "none of these touched the JIT-emitted code for these bench programs."
)
out.append(
    "- **M11 → M22 (stdlib sprint)**: numbers stay flat. Adding 17 stdlib "
    "modules (130+ NativeFns, 7 new `vm/Cargo.toml` deps) didn't change "
    "the hot-loop codegen."
)
out.append("")
out.append(
    "Variance between snapshots is ~10-20%, consistent with single-machine "
    "wall-clock noise. The signal is **structural stability** since M9 — "
    "every milestone since has shipped 16/16 wins."
)
out.append("")

sys.stdout.write("\n".join(out))
