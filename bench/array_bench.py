import time

# Array (list) throughput benchmark: build by append, in-place transform,
# and index-sum over large sizes. Same algorithm as bench/array_bench.spy.
def run(n):
    # build
    xs = []
    i = 0
    while i < n:
        xs.append(i)
        i += 1
    # in-place transform
    i = 0
    while i < n:
        xs[i] = xs[i] * 2 + 1
        i += 1
    # index-sum
    total = 0
    i = 0
    while i < n:
        total += xs[i]
        i += 1
    return total


def main():
    for n in (1000000, 5000000, 10000000):
        t0 = time.perf_counter()
        checksum = run(n)
        t1 = time.perf_counter()
        print(f"n={n} checksum={checksum} ms={(t1 - t0) * 1000:.2f}")


main()
