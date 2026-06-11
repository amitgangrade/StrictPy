# str_concat_build: build a large string by repeated `s = s + piece`;
# both languages optimize the unique-accumulator concat pattern. Print len.


def main() -> None:
    n = 800000
    s = ""
    for i in range(n):
        piece = "ab" + str(i % 10)
        s = s + piece
    print(f"len={len(s)}")
    print(f"head={s[0:12]}")
    print(f"tail={s[len(s) - 12:len(s)]}")


if __name__ == "__main__":
    main()
