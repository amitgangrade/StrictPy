import time

# Pure iteration benchmark: build a list once (UNtimed), then time only
# repeated for-loop traversal that sums every element. Same shape as
# bench/iter_bench.spy.


def build(n):
    return list(range(n))


def iterate(xs, reps):
    total = 0
    for _ in range(reps):
        for x in xs:
            total += x
    return total


def main():
    reps = 5
    for n in (1000000, 5000000, 10000000):
        xs = build(n)  # not timed
        t0 = time.perf_counter()
        checksum = iterate(xs, reps)
        t1 = time.perf_counter()
        total_ms = (t1 - t0) * 1000.0
        print(f"n={n} reps={reps} checksum={checksum} per_pass_ms={total_ms / reps:.2f}")


main()
