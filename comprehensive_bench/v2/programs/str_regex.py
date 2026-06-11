# str_regex: compiled regex — findall of a word+digits pattern plus a
# sub over synthetic text, N iterations; print match counts and
# replaced length. (Pattern subset valid in both Rust regex and Python re.)

import re


def main() -> None:
    parts = []
    for i in range(1500):
        parts.append("alpha" + str(i % 100) + " beta gamma" + str(i % 10) + " ")
    text = "".join(parts)

    p1 = re.compile("[a-z]+[0-9]+")
    p2 = re.compile("[0-9]+")
    reps = 100
    match_count = 0
    replaced_len = 0
    for _ in range(reps):
        found = p1.findall(text)
        match_count += len(found)
        repl = p2.sub("#", text)
        replaced_len += len(repl)
    print(f"match_count={match_count}")
    print(f"replaced_len={replaced_len}")
    print(f"text_len={len(text)}")


if __name__ == "__main__":
    main()
