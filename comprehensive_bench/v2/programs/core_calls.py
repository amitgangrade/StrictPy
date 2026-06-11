# core_calls: small-function call overhead — tiny functions called in a hot loop.


def add(a, b):
    return a + b


def mul(a, b):
    return a * b


def clamp(x, lo, hi):
    if x < lo:
        return lo
    if x > hi:
        return hi
    return x


def main():
    acc = 0
    for i in range(1000000):
        s = add(i % 97, i % 31)
        p = mul(s, 3)
        c = clamp(p - 150, 0, 200)
        acc += c
    print(f"calls_acc={acc}")


if __name__ == "__main__":
    main()
