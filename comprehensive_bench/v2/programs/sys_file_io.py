"""File I/O: write 50k lines to a temp file (relative path), read it back,
count lines + length checksum + substring hits, then delete the file."""
import os


def main():
    n = 50000
    path = "bench_tmp_io.txt"

    with open(path, "w", newline="") as f:
        for i in range(n):
            f.write(f"line,{i},{(i * i) % 97}\n")

    with open(path, "r", newline="") as g:
        content = g.read()

    line_count = 0
    len_sum = 0
    field_sum = 0
    hits42 = 0
    for line in content.split("\n"):
        if line:
            line_count += 1
            len_sum += len(line)
            fields = line.split(",")
            field_sum += int(fields[1]) + int(fields[2])
            if ",42" in line:
                hits42 += 1

    os.remove(path)
    removed = 0 if os.path.exists(path) else 1

    print("lines=" + str(line_count))
    print("len_sum=" + str(len_sum))
    print("field_sum=" + str(field_sum))
    print("hits42=" + str(hits42))
    print("removed=" + str(removed))


if __name__ == "__main__":
    main()
