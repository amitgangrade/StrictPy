# ds_dict_ops: insert N str-keyed entries, lookup all, membership hit/miss,
# overwrite half, iterate keys in sorted order; print checksums.


def main() -> None:
    n = 250000
    d = {}
    for i in range(n):
        d["k" + str(i)] = i * 7 % 10000
    lookup_sum = 0
    for i in range(n):
        lookup_sum += d["k" + str(i)]
    hits = 0
    misses = 0
    for i in range(n):
        if "k" + str(i) in d:
            hits += 1
        if "x" + str(i) not in d:
            misses += 1
    for i in range(0, n, 2):
        d["k" + str(i)] = 1
    keys = sorted(d)
    val_sum = 0
    for k in keys:
        val_sum += d[k]
    print(f"len={len(d)}")
    print(f"lookup_sum={lookup_sum}")
    print(f"hits={hits}")
    print(f"misses={misses}")
    print(f"first_key={keys[0]}")
    print(f"val_sum={val_sum}")


if __name__ == "__main__":
    main()
