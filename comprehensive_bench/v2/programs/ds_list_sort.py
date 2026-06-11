# ds_list_sort: build deterministic pseudo-random lists (LCG), sort, print samples.


def main() -> None:
    n = 400000
    seed = 42
    nums = []
    for _ in range(n):
        seed = (seed * 1103515245 + 12345) % 2147483648
        nums.append(seed)
    nums.sort()
    print(f"first={nums[0]}")
    print(f"mid={nums[n // 2]}")
    print(f"last={nums[n - 1]}")

    m = 150000
    words = []
    for _ in range(m):
        seed = (seed * 1103515245 + 12345) % 2147483648
        words.append("w" + str(seed % 1000000))
    words.sort()
    print(f"wfirst={words[0]}")
    print(f"wmid={words[m // 2]}")
    print(f"wlast={words[m - 1]}")


if __name__ == "__main__":
    main()
