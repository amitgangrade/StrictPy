"""Verify .spy/.py benchmark pairs: run both from inside a scratch cwd,
compare stdout byte-for-byte, report exit codes and wall times.

Usage: python verify_pairs.py [name ...]   (no args = all sys_* pairs)
"""
import os
import subprocess
import sys
import tempfile
import time

HERE = os.path.dirname(os.path.abspath(__file__))
SPY = r"C:\Users\AG\CascadeProjects\PythonCompiler\target\release\spy.exe"


def run(cmd, cwd, timeout=180):
    t0 = time.perf_counter()
    try:
        p = subprocess.run(cmd, cwd=cwd, capture_output=True, text=True, timeout=timeout)
        return p.returncode, p.stdout, p.stderr, time.perf_counter() - t0
    except subprocess.TimeoutExpired:
        return -999, "", "TIMEOUT", time.perf_counter() - t0


def verify(name, scratch):
    spy_file = os.path.join(HERE, name + ".spy")
    py_file = os.path.join(HERE, name + ".py")
    rc1, out1, err1, t1 = run([SPY, spy_file], scratch)
    rc2, out2, err2, t2 = run([sys.executable, py_file], scratch)
    ok = rc1 == 0 and rc2 == 0 and out1 == out2
    status = "OK   " if ok else "FAIL "
    print(f"{status}{name:26s} spy rc={rc1:>3} {t1:6.2f}s | py rc={rc2:>3} {t2:6.2f}s")
    if not ok:
        print("  spy stdout:", repr(out1[:400]))
        print("  py  stdout:", repr(out2[:400]))
        if err1.strip():
            print("  spy stderr:", err1[:400])
        if err2.strip():
            print("  py  stderr:", err2[:400])
    return ok


def main():
    names = sys.argv[1:]
    if not names:
        names = sorted(
            f[:-4] for f in os.listdir(HERE)
            if f.startswith("sys_") and f.endswith(".spy")
        )
    fails = 0
    with tempfile.TemporaryDirectory(prefix="bench_v2_verify_") as scratch:
        for name in names:
            if not verify(name, scratch):
                fails += 1
        leftover = os.listdir(scratch)
        if leftover:
            print("LEFTOVER FILES IN SCRATCH:", leftover)
    print(f"done: {len(names) - fails}/{len(names)} matched")
    sys.exit(1 if fails else 0)


if __name__ == "__main__":
    main()
