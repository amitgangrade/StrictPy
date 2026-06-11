# ds_generators: generator function yielding N values consumed by a for loop.


def lcg_stream(n):
    seed = 7
    for _ in range(n):
        seed = (seed * 1103515245 + 12345) % 2147483648
        yield seed % 1000


def main() -> None:
    n = 1500000
    total = 0
    cnt = 0
    for v in lcg_stream(n):
        total += v
        cnt += 1
    print(f"count={cnt}")
    print(f"total={total}")


if __name__ == "__main__":
    main()
