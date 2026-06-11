# core_matrix: NxN float matrix multiply (lists of lists), integer-valued contents.
# Checksum stays integral so output is an exact int in both languages.


def build(n, fa, fb):
    return [[float((i * fa + j * fb) % 10) for j in range(n)] for i in range(n)]


def main():
    n = 100
    a = build(n, 7, 3)
    b = build(n, 5, 11)
    c = [[0.0] * n for _ in range(n)]
    for i in range(n):
        ai = a[i]
        ci = c[i]
        for k in range(n):
            aik = ai[k]
            bk = b[k]
            for j in range(n):
                ci[j] += aik * bk[j]
    total = 0.0
    for row in c:
        for v in row:
            total += v
    print(f"matrix_checksum={int(total)}")


if __name__ == "__main__":
    main()
