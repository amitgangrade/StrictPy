# str_search: repeated substring search (index_of / Python str.find) over a
# large haystack with hit and miss needles; print counts.


def main() -> None:
    parts = []
    for i in range(500):
        parts.append("block" + str(i) + " user" + str(i) + " data;")
    h = "".join(parts)

    queries = 60000
    hits = 0
    misses = 0
    idx_sum = 0
    for q in range(queries):
        needle = "user" + str(q % 1000)
        idx = h.find(needle)
        if idx >= 0:
            hits += 1
            idx_sum += idx
        else:
            misses += 1
    print(f"hits={hits}")
    print(f"misses={misses}")
    print(f"idx_sum={idx_sum}")
    print(f"haystack_len={len(h)}")


if __name__ == "__main__":
    main()
