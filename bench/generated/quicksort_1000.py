import sys
sys.setrecursionlimit(5000)

def partition(a, lo, hi):
    pivot = a[hi]
    i = lo - 1
    for j in range(lo, hi):
        if a[j] <= pivot:
            i += 1
            a[i], a[j] = a[j], a[i]
    a[i + 1], a[hi] = a[hi], a[i + 1]
    return i + 1

def quicksort(a, lo, hi):
    if lo < hi:
        p = partition(a, lo, hi)
        quicksort(a, lo, p - 1)
        quicksort(a, p + 1, hi)

def build(n):
    a = list(range(n))
    seed = 12345
    j = n - 1
    while j > 0:
        seed = (seed * 1103515245 + 12345) % 2147483648
        k = seed % (j + 1)
        a[j], a[k] = a[k], a[j]
        j -= 1
    return a

n = 1000
a = build(n)
quicksort(a, 0, n - 1)
print(f"first={a[0]} last={a[n - 1]}")
