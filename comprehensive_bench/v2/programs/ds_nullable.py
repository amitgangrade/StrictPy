# ds_nullable: functions returning Optional[int], hot loop with is-None
# checks and coalescing.


def lookup(i):
    if i % 3 == 0:
        return None
    return i % 97


def main() -> None:
    n = 1000000
    none_count = 0
    total = 0
    coalesced = 0
    for i in range(n):
        v = lookup(i)
        if v is None:
            none_count += 1
        else:
            total += v
        w = lookup(i + 1)
        coalesced += w if w is not None else 5
    print(f"none_count={none_count}")
    print(f"total={total}")
    print(f"coalesced={coalesced}")


if __name__ == "__main__":
    main()
