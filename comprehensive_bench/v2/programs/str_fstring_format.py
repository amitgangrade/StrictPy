# str_fstring_format: render N rows like f"row {name} -> {score} pts" with
# str() conversions; print total length.


def main() -> None:
    n = 600000
    total_len = 0
    last = ""
    for i in range(n):
        name = "user" + str(i % 1000)
        score = i * 7 % 10000
        row = f"row {name} -> {score} pts"
        total_len += len(row)
        last = row
    print(f"total_len={total_len}")
    print(f"last={last}")


if __name__ == "__main__":
    main()
