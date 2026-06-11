# str_wordcount: word frequency over a synthetic ~1MB text (deterministic
# generated words), split on space, count in dict, print top-10 sorted by
# count desc then word asc (identical tie-break both sides).


def main() -> None:
    nwords = 150000
    words_src = []
    for i in range(nwords):
        idx = (i * i + 17 * i) % 1200
        words_src.append("w" + str(idx))
    text = " ".join(words_src)

    reps = 4
    counts = {}
    for _ in range(reps):
        counts = {}
        words = text.split(" ")
        for w in words:
            if w in counts:
                counts[w] += 1
            else:
                counts[w] = 1

    top = sorted(counts.items(), key=lambda kv: (-kv[1], kv[0]))[:10]
    for j, (w, c) in enumerate(top):
        print(f"top{j}={w}:{c}")
    print(f"unique={len(counts)}")
    print(f"text_len={len(text)}")


if __name__ == "__main__":
    main()
