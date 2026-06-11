"""Quick correctness pass: run each pair once, report status (no timing)."""
import subprocess
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
PROG = HERE / "programs"
ROOT = HERE.parent.parent
import os
SPY = Path(os.environ.get("SPY_BIN", ROOT / "target" / "release" / "spy.exe"))
SCRATCH = HERE / "build"

filt = sys.argv[1] if len(sys.argv) > 1 else ""
norm = lambda s: s.replace("\r\n", "\n").rstrip("\n")

for spy_path in sorted(PROG.glob(f"*{filt}*.spy" if filt else "*.spy")):
    py_path = spy_path.with_suffix(".py")
    name = spy_path.stem
    t0 = time.perf_counter()
    try:
        rs = subprocess.run([str(SPY), str(spy_path)], capture_output=True, text=True,
                            timeout=120, cwd=SCRATCH)
    except subprocess.TimeoutExpired:
        print(f"{name}: SPY TIMEOUT >120s")
        continue
    t_spy = time.perf_counter() - t0
    if rs.returncode != 0:
        print(f"{name}: SPY FAIL rc={rs.returncode}")
        print("  " + (rs.stderr.strip() or rs.stdout.strip())[:600].replace("\n", "\n  "))
        continue
    t0 = time.perf_counter()
    rp = subprocess.run([sys.executable, str(py_path)], capture_output=True, text=True,
                        timeout=120, cwd=SCRATCH)
    t_py = time.perf_counter() - t0
    if rp.returncode != 0:
        print(f"{name}: PY FAIL rc={rp.returncode}")
        print("  " + (rp.stderr.strip() or rp.stdout.strip())[:600].replace("\n", "\n  "))
        continue
    if norm(rs.stdout) != norm(rp.stdout):
        print(f"{name}: MISMATCH")
        print(f"  spy: {norm(rs.stdout)[:200]!r}")
        print(f"  py : {norm(rp.stdout)[:200]!r}")
    else:
        print(f"{name}: OK  spy={t_spy*1000:.0f}ms py={t_py*1000:.0f}ms")
