import sys
sys.setrecursionlimit(40100)

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

n = 20000
a = [n - i for i in range(n)]
quicksort(a, 0, n - 1)
print(f"first={a[0]} last={a[n - 1]}")
