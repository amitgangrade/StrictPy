"""StrictPy vs CPython comprehensive benchmark harness (v2).

Discovers paired programs in programs/ (<name>.spy + <name>.py), precompiles
both, verifies byte-identical stdout (correctness), and times both
(performance, best-of-N + median wall-clock of full process runs).

Usage:
    python run_v2.py                  # run everything
    python run_v2.py --filter str_    # run subset by name substring
    python run_v2.py --repeats 5      # override repeat count
    python run_v2.py --list           # list discovered pairs

Output: results_v2.json (raw data). Report generation is separate.
"""
import argparse
import json
import statistics
import subprocess
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent.parent
PROG_DIR = HERE / "programs"
BUILD_DIR = HERE / "build"
import os
SPY = Path(os.environ.get("SPY_BIN", ROOT / "target" / "release" / "spy.exe"))
PYTHON = sys.executable
DEFAULT_REPEATS = 5
TIMEOUT = 300

BUILD_DIR.mkdir(parents=True, exist_ok=True)


def discover(filter_sub: str | None):
    pairs = []
    for spy_path in sorted(PROG_DIR.glob("*.spy")):
        py_path = spy_path.with_suffix(".py")
        if not py_path.exists():
            print(f"WARNING: {spy_path.name} has no .py twin, skipping", file=sys.stderr)
            continue
        if filter_sub and filter_sub not in spy_path.stem:
            continue
        pairs.append((spy_path.stem, spy_path, py_path))
    return pairs


def compile_pair(name: str, spy_path: Path, py_path: Path):
    """Returns (spyc_path, pyc_path) or raises RuntimeError."""
    spyc = BUILD_DIR / f"{name}.spyc"
    res = subprocess.run(
        [str(SPY), "--compile-only", str(spy_path), "-o", str(spyc)],
        capture_output=True, text=True, timeout=TIMEOUT,
    )
    if res.returncode != 0:
        raise RuntimeError(f"spy compile failed:\n{res.stderr.strip()[:2000]}")

    res = subprocess.run(
        [PYTHON, "-m", "py_compile", str(py_path)],
        capture_output=True, text=True, timeout=TIMEOUT,
    )
    if res.returncode != 0:
        raise RuntimeError(f"py compile failed:\n{res.stderr.strip()[:2000]}")
    tag = f"cpython-{sys.version_info.major}{sys.version_info.minor}"
    pyc = py_path.parent / "__pycache__" / f"{name}.{tag}.pyc"
    if not pyc.exists():
        raise RuntimeError("pyc not found after py_compile")
    return spyc, pyc


def run_once(cmd: list[str], cwd: Path) -> tuple[float, str]:
    t0 = time.perf_counter()
    res = subprocess.run(cmd, capture_output=True, text=True, timeout=TIMEOUT, cwd=cwd)
    dt = time.perf_counter() - t0
    if res.returncode != 0:
        raise RuntimeError(
            f"rc={res.returncode}\nstdout:\n{res.stdout[:1000]}\nstderr:\n{res.stderr[:2000]}")
    return dt, res.stdout


def bench_pair(name: str, spy_path: Path, py_path: Path, repeats: int):
    rec = {"name": name, "spy_ms": None, "py_ms": None, "spy_median_ms": None,
           "py_median_ms": None, "ratio": None, "correct": None, "note": ""}
    try:
        spyc, pyc = compile_pair(name, spy_path, py_path)
    except Exception as e:
        rec["note"] = f"COMPILE FAIL: {e}"
        return rec

    spy_cmd = [str(SPY), str(spyc)]
    py_cmd = [PYTHON, str(pyc)]
    spy_times, py_times = [], []
    spy_out = py_out = None
    try:
        # warmup, one each (also captures output for correctness check)
        _, spy_out = run_once(spy_cmd, BUILD_DIR)
    except Exception as e:
        rec["note"] = f"SPY RUN FAIL: {e}"
        return rec
    try:
        _, py_out = run_once(py_cmd, BUILD_DIR)
    except Exception as e:
        rec["note"] = f"PY RUN FAIL: {e}"
        return rec

    # correctness: byte-identical stdout (normalize line endings)
    norm = lambda s: s.replace("\r\n", "\n").rstrip("\n")
    rec["correct"] = norm(spy_out) == norm(py_out)
    if not rec["correct"]:
        rec["note"] = (f"OUTPUT MISMATCH spy={norm(spy_out)[:300]!r} "
                       f"py={norm(py_out)[:300]!r}")

    # timed runs, interleaved to spread thermal/cache effects fairly
    try:
        for _ in range(repeats):
            t, _ = run_once(spy_cmd, BUILD_DIR)
            spy_times.append(t)
            t, _ = run_once(py_cmd, BUILD_DIR)
            py_times.append(t)
    except Exception as e:
        rec["note"] = (rec["note"] + f" | TIMED RUN FAIL: {e}").strip(" |")
        return rec

    rec["spy_ms"] = min(spy_times) * 1000
    rec["py_ms"] = min(py_times) * 1000
    rec["spy_median_ms"] = statistics.median(spy_times) * 1000
    rec["py_median_ms"] = statistics.median(py_times) * 1000
    rec["ratio"] = rec["spy_ms"] / rec["py_ms"] if rec["py_ms"] > 0 else None
    return rec


def measure_startup(repeats: int = 7):
    """Process startup floor for both runtimes (empty main)."""
    spy_src = BUILD_DIR / "_startup.spy"
    spy_src.write_text("fn main() -> i32:\n    return 0\n")
    spyc = BUILD_DIR / "_startup.spyc"
    subprocess.run([str(SPY), "--compile-only", str(spy_src), "-o", str(spyc)],
                   capture_output=True, timeout=60)
    py_src = BUILD_DIR / "_startup.py"
    py_src.write_text("pass\n")
    spy_t, py_t = [], []
    for _ in range(repeats):
        t0 = time.perf_counter()
        subprocess.run([str(SPY), str(spyc)], capture_output=True, timeout=60)
        spy_t.append(time.perf_counter() - t0)
        t0 = time.perf_counter()
        subprocess.run([PYTHON, str(py_src)], capture_output=True, timeout=60)
        py_t.append(time.perf_counter() - t0)
    return min(spy_t) * 1000, min(py_t) * 1000


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--filter", default=None)
    ap.add_argument("--repeats", type=int, default=DEFAULT_REPEATS)
    ap.add_argument("--list", action="store_true")
    args = ap.parse_args()

    if not SPY.exists():
        print(f"spy.exe not found at {SPY}; run cargo build --release", file=sys.stderr)
        sys.exit(1)

    pairs = discover(args.filter)
    if args.list:
        for name, *_ in pairs:
            print(name)
        return
    if not pairs:
        print("no benchmark pairs found", file=sys.stderr)
        sys.exit(1)

    spy_startup, py_startup = measure_startup()
    print(f"startup floor: spy={spy_startup:.1f}ms  python={py_startup:.1f}ms")

    results = []
    for i, (name, spy_path, py_path) in enumerate(pairs, 1):
        print(f"[{i}/{len(pairs)}] {name} ...", end="", flush=True)
        rec = bench_pair(name, spy_path, py_path, args.repeats)
        results.append(rec)
        if rec["ratio"] is not None:
            mark = "OK" if rec["correct"] else "MISMATCH"
            print(f" spy={rec['spy_ms']:.1f}ms py={rec['py_ms']:.1f}ms "
                  f"ratio={rec['ratio']:.2f}x [{mark}]")
        else:
            print(f" FAIL: {rec['note'][:160]}")

    out = {
        "meta": {
            "timestamp": time.strftime("%Y-%m-%d %H:%M:%S"),
            "python": sys.version.split()[0],
            "spy": subprocess.run([str(SPY), "--version"], capture_output=True,
                                  text=True).stdout.strip(),
            "repeats": args.repeats,
            "spy_startup_ms": spy_startup,
            "py_startup_ms": py_startup,
        },
        "results": results,
    }
    path = HERE / "results_v2.json"
    if args.filter:
        # merge into existing results instead of clobbering
        if path.exists():
            old = json.loads(path.read_text())
            merged = {r["name"]: r for r in old.get("results", [])}
            for r in results:
                merged[r["name"]] = r
            out["results"] = sorted(merged.values(), key=lambda r: r["name"])
    path.write_text(json.dumps(out, indent=2))
    print(f"wrote {path}")


if __name__ == "__main__":
    main()
