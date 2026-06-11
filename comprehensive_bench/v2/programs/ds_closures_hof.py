# ds_closures_hof: idiomatic Python equivalents of map/filter/reduce —
# comprehensions + sum/max over a large list.


def main() -> None:
    n = 1000000
    base = [i % 1000 for i in range(n)]
    factor = 7
    scaled = [x * factor + 1 for x in base]
    threshold = 3500
    kept = [x for x in scaled if x > threshold]
    total = sum(kept)
    mx = max(kept)
    print(f"kept_len={len(kept)}")
    print(f"total={total}")
    print(f"max={mx}")


if __name__ == "__main__":
    main()
