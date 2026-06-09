import time

# Idiomatic CPython: build+transform via a list comprehension over range(n),
# then sum() it. Same result (sum == n^2) as bench/array_bench.py.
def run(n):
    xs = [i * 2 + 1 for i in range(n)]
    return sum(xs)


def main():
    for n in (1000000, 5000000, 10000000):
        t0 = time.perf_counter()
        checksum = run(n)
        t1 = time.perf_counter()
        print(f"n={n} checksum={checksum} ms={(t1 - t0) * 1000:.2f}")


main()
