"""Render results_v2.json into REPORT_V2.md tables (analysis prose is appended
manually/by the author after a run)."""
import json
from pathlib import Path

HERE = Path(__file__).resolve().parent

TRACKS = {
    "core": "Core compute (JIT-friendly numeric / control flow)",
    "ds": "Data structures & language features",
    "str": "Strings, text & serialization",
    "sys": "Systems, concurrency & stdlib",
}


def fmt_ms(v):
    return f"{v:,.1f}" if v is not None else "—"


def marker(ratio, correct):
    if ratio is None:
        return "💥 FAIL"
    if correct is False:
        return "⚠ WRONG OUTPUT"
    speedup = 1.0 / ratio
    if speedup >= 1.15:
        return f"✅ {speedup:.2f}× faster"
    if speedup <= 0.87:
        return f"❌ {1/speedup:.2f}× SLOWER"
    return "➖ tie"


def main():
    data = json.loads((HERE / "results_v2.json").read_text())
    meta, results = data["meta"], data["results"]

    by_track = {}
    for r in results:
        track = r["name"].split("_", 1)[0]
        by_track.setdefault(track, []).append(r)

    ok = [r for r in results if r["ratio"] is not None and r["correct"]]
    wins = sum(1 for r in ok if 1 / r["ratio"] >= 1.15)
    losses = sum(1 for r in ok if 1 / r["ratio"] <= 0.87)
    ties = len(ok) - wins - losses
    mismatches = [r for r in results if r["correct"] is False]
    failures = [r for r in results if r["ratio"] is None]
    import statistics as st
    geomean = None
    if ok:
        import math
        geomean = math.exp(st.mean(math.log(1 / r["ratio"]) for r in ok))

    L = []
    L.append("# StrictPy vs CPython — Comprehensive Benchmark v2")
    L.append("")
    L.append(f"_Generated {meta['timestamp']} · StrictPy `{meta['spy']}` vs CPython {meta['python']} · "
             f"Windows 11 · best-of-{meta['repeats']} full-process runs, interleaved_")
    L.append("")
    L.append(f"Process startup floor: StrictPy {meta['spy_startup_ms']:.0f} ms · "
             f"CPython {meta['py_startup_ms']:.0f} ms (included in every number below).")
    L.append("")
    L.append("## Scoreboard")
    L.append("")
    L.append(f"| Benchmarks | StrictPy wins (≥1.15×) | Ties | CPython wins (≥1.15×) | Wrong output | Failed to run |")
    L.append(f"|---|---|---|---|---|---|")
    L.append(f"| **{len(results)}** | **{wins}** | {ties} | **{losses}** | {len(mismatches)} | {len(failures)} |")
    L.append("")
    if geomean:
        L.append(f"**Geometric-mean speedup across all passing benchmarks: {geomean:.2f}× vs CPython.**")
        L.append("")

    for track, title in TRACKS.items():
        rows = by_track.get(track)
        if not rows:
            continue
        L.append(f"## {title}")
        L.append("")
        L.append("| Benchmark | StrictPy (ms) | CPython (ms) | Verdict |")
        L.append("|---|---:|---:|---|")
        for r in sorted(rows, key=lambda x: (x["ratio"] is None, -(1/x["ratio"]) if x["ratio"] else 0)):
            L.append(f"| `{r['name']}` | {fmt_ms(r['spy_ms'])} | {fmt_ms(r['py_ms'])} | "
                     f"{marker(r['ratio'], r['correct'])} |")
        L.append("")

    if mismatches or failures:
        L.append("## Correctness & stability issues")
        L.append("")
        for r in mismatches + failures:
            L.append(f"- **{r['name']}**: {r['note'][:500]}")
        L.append("")

    (HERE / "REPORT_V2_tables.md").write_text("\n".join(L), encoding="utf-8")
    print("wrote REPORT_V2_tables.md (auto-generated tables; REPORT_V2.md is the authored report)")


if __name__ == "__main__":
    main()
