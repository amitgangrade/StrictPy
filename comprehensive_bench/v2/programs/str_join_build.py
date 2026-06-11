# str_join_build: build a comma-separated string of 100k ints, repeated.
# Python uses ",".join(...); StrictPy has no str.join, so it uses its best
# idiom: an accumulator concat loop. Print len + a slice sample.


def main() -> None:
    n = 100000
    reps = 20
    total = 0
    sample = ""
    for _ in range(reps):
        s = ",".join(str(i) for i in range(n))
        total += len(s)
        sample = s[100:120]
    print(f"total_len={total}")
    print(f"one_len={total // reps}")
    print(f"sample={sample}")


if __name__ == "__main__":
    main()
