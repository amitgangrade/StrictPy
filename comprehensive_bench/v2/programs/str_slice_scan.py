# str_slice_scan: char-by-char scanning + slicing windows of a large string;
# print checksum of char codes and window stats.


def main() -> None:
    parts = []
    for i in range(4000):
        parts.append("seg" + str(i) + ":payload-" + str((i * 7) % 100) + ";")
    s = "".join(parts)
    n = len(s)

    reps = 40
    checksum = 0
    windows = 0
    wlen_sum = 0
    whits = 0
    for _ in range(reps):
        for j in range(n):
            checksum += ord(s[j])
        k = 0
        while k < n:
            end = min(k + 16, n)
            w = s[k:end]
            windows += 1
            wlen_sum += len(w)
            if "pay" in w:
                whits += 1
            k += 997
    print(f"checksum={checksum}")
    print(f"windows={windows}")
    print(f"wlen_sum={wlen_sum}")
    print(f"whits={whits}")
    print(f"n={n}")


if __name__ == "__main__":
    main()
