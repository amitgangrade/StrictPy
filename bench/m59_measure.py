"""Generate a 1M-row x 8-col i64 CSV (once) and run the spy probe under an
RSS poller, reporting peak resident memory. Used to A/B the M59 null-mask
packing: run the same probe against the pre-M59 and post-M59 `spy` binary."""
import os, sys, time, subprocess, csv

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
CSV = os.path.join(ROOT, "bench", "data", "m59_probe.csv")
PROBE = os.path.join("bench", "m59_mem_probe.spy")
SPY = os.path.join(ROOT, "target", "release", "spy.exe")
ROWS = 1_000_000
COLS = 8

def gen_csv():
    if os.path.exists(CSV) and os.path.getsize(CSV) > 50_000_000:
        return
    os.makedirs(os.path.dirname(CSV), exist_ok=True)
    with open(CSV, "w", newline="") as f:
        w = csv.writer(f)
        w.writerow([f"c{j}" for j in range(COLS)])
        for i in range(ROWS):
            w.writerow([i + j for j in range(COLS)])
    print(f"generated {CSV} ({os.path.getsize(CSV)//1_000_000} MB)")

def measure():
    import psutil
    p = subprocess.Popen([SPY, PROBE], cwd=ROOT,
                         stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    proc = psutil.Process(p.pid)
    samples = []
    while p.poll() is None:
        try:
            rss = proc.memory_info().rss
            for c in proc.children(recursive=True):
                rss += c.memory_info().rss
            samples.append(rss)
        except Exception:
            pass
        time.sleep(0.02)
    out = p.stdout.read().decode(errors="replace")
    peak = max(samples) if samples else 0
    # "settled" = the held-frame footprint during the final hold (after the
    # churn loop forced GC) — the last ~1.5s of samples, taking the median.
    tail = samples[-75:] if len(samples) >= 75 else samples
    settled = sorted(tail)[len(tail)//2] if tail else 0
    print("  probe stdout:", out.strip().replace("\n", " | "))
    print(f"  PEAK RSS:    {peak/1_000_000:.1f} MB")
    print(f"  SETTLED RSS: {settled/1_000_000:.1f} MB  (held frame, post-GC)")

if __name__ == "__main__":
    gen_csv()
    measure()
