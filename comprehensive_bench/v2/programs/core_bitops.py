# core_bitops: xorshift-style PRNG + popcount via shifts/masks.
# Mirror of the StrictPy version: every bitwise operand and result stays below
# 2^31 because StrictPy evaluates i64 bitwise ops in 32-bit width (see .spy).


def popcount31(v):
    c = 0
    while v != 0:
        c += v & 1
        v >>= 1
    return c


def main():
    x = 123456789
    acc = 0
    for _ in range(400000):
        x = x ^ ((x & 262143) << 13)
        x = x ^ (x >> 17)
        x = x ^ ((x & 67108863) << 5)
        acc = (acc + popcount31(x) * 31 + (x & 255)) % 1000000007
    print(f"bitops_acc={acc}")
    print(f"bitops_final={x}")


if __name__ == "__main__":
    main()
