# str_methods_mix: hot loop over a list of messy strings applying the native
# string methods both languages share: strip, replace, in, startswith,
# endswith. (Case conversion is excluded: StrictPy has no str.lower()/upper();
# that gap is measured in str_http_parse.)


def process(msgs, reps):
    c_contains = 0
    c_starts = 0
    c_ends = 0
    total_len = 0
    for _ in range(reps):
        for m in msgs:
            t = m.strip()
            rep = t.replace("-", "_")
            if "Alpha" in t:
                c_contains += 1
            if t.startswith("MSG"):
                c_starts += 1
            if t.endswith("gamma"):
                c_ends += 1
            total_len += len(rep) + len(t)
    return c_contains, c_starts, c_ends, total_len


def main() -> None:
    msgs = []
    for i in range(200):
        suffix = "gamma" if i % 3 == 0 else "delta"
        msgs.append("  MSG-" + str(i) + " Alpha beta " + suffix + "  ")

    c_contains, c_starts, c_ends, total_len = process(msgs, 4000)
    print(f"contains={c_contains}")
    print(f"starts={c_starts}")
    print(f"ends={c_ends}")
    print(f"total_len={total_len}")


if __name__ == "__main__":
    main()
