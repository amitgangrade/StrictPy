# ds_string_keys_aggregation: group-by aggregation. Synthetic records mapped
# into dict counters; print sorted top entries (count desc, key asc).


def main() -> None:
    n = 200000
    seed = 99
    counts = {}
    sums = {}
    for _ in range(n):
        seed = (seed * 1103515245 + 12345) % 2147483648
        key = "u" + str(seed % 1000)
        val = seed % 100
        if key in counts:
            counts[key] += 1
            sums[key] += val
        else:
            counts[key] = 1
            sums[key] = val
    top = sorted(counts.items(), key=lambda kv: (-kv[1], kv[0]))[:3]
    print(f"records={n}")
    print(f"distinct={len(counts)}")
    for t, (k, c) in enumerate(top):
        print(f"top{t}={k}:{c}:{sums[k]}")


if __name__ == "__main__":
    main()
