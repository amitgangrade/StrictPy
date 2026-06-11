# ds_sort_by_key: sort N strings by (length, value) using sorted(key=...).


def main() -> None:
    n = 250000
    seed = 7
    words = []
    for _ in range(n):
        seed = (seed * 1103515245 + 12345) % 2147483648
        words.append("w" + str(seed % 1000000))
    out = sorted(words, key=lambda s: (len(s), s))
    print(f"first0={out[0]}")
    print(f"first1={out[1]}")
    print(f"first2={out[2]}")
    print(f"last2={out[n - 3]}")
    print(f"last1={out[n - 2]}")
    print(f"last0={out[n - 1]}")


if __name__ == "__main__":
    main()
