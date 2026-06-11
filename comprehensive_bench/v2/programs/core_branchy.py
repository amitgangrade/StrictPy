# core_branchy: branch-heavy loop — nested if/elif chains plus and/or
# short-circuit logic over an LCG stream; counts bucket hits.


def main():
    seed = 42
    b0 = b1 = b2 = b3 = b4 = 0
    for _ in range(800000):
        seed = (seed * 1103515245 + 12345) % 2147483648
        v = seed % 1000
        if v < 100 and seed % 7 == 0:
            b0 += 1
        elif v < 300 or v > 950:
            b1 += 1
        elif v % 2 == 0 and (v % 3 == 0 or v % 5 == 0):
            b2 += 1
        elif v < 600:
            b3 += 1
        else:
            b4 += 1
    print(f"branchy_b0={b0}")
    print(f"branchy_b1={b1}")
    print(f"branchy_b2={b2}")
    print(f"branchy_b3={b3}")
    print(f"branchy_b4={b4}")


if __name__ == "__main__":
    main()
