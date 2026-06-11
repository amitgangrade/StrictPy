# core_int_arith: tight int loop mixing + - * // % and comparisons.
# Collatz-style iteration over many seeds, accumulating a checksum.


def collatz_steps(n):
    steps = 0
    while n != 1:
        if n % 2 == 0:
            n //= 2
        else:
            n = 3 * n + 1
        steps += 1
    return steps


def main():
    total = 0
    best = 0
    best_seed = 0
    checksum = 0
    for seed in range(1, 20001):
        s = collatz_steps(seed)
        total += s
        if s > best:
            best = s
            best_seed = seed
        checksum = (checksum * 31 + s) % 1000000007
    print(f"int_arith_total={total}")
    print(f"int_arith_best_seed={best_seed}")
    print(f"int_arith_checksum={checksum}")


if __name__ == "__main__":
    main()
