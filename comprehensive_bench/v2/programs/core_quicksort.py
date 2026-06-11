# core_quicksort: recursive in-place quicksort of an LCG-generated list of ints.


def quicksort(a, lo, hi):
    if lo < hi:
        mid = (lo + hi) // 2
        a[mid], a[hi] = a[hi], a[mid]
        pivot = a[hi]
        i = lo
        for j in range(lo, hi):
            if a[j] < pivot:
                a[i], a[j] = a[j], a[i]
                i += 1
        a[i], a[hi] = a[hi], a[i]
        quicksort(a, lo, i - 1)
        quicksort(a, i + 1, hi)


def main():
    n = 60000
    a = []
    seed = 1234567
    for _ in range(n):
        seed = (seed * 1103515245 + 12345) % 2147483648
        a.append(seed % 1000000)
    quicksort(a, 0, n - 1)
    checksum = 0
    for p in range(0, n, 600):
        checksum = (checksum * 31 + a[p]) % 1000000007
    print(f"quicksort_checksum={checksum}")
    print(f"quicksort_min={a[0]}")
    print(f"quicksort_max={a[n - 1]}")


if __name__ == "__main__":
    main()
