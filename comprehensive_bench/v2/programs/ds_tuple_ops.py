# ds_tuple_ops: create/destructure tuples in a hot loop (divmod-style),
# tuple as multi-return.


def main() -> None:
    n = 1000000
    acc_q = 0
    acc_r = 0
    acc_swap = 0
    for i in range(n):
        q, r = divmod(i * 13 % 100000, 10)
        acc_q += q
        acc_r += r
        p = (r, q)
        acc_swap += p[1] % 3
    print(f"acc_q={acc_q}")
    print(f"acc_r={acc_r}")
    print(f"acc_swap={acc_swap}")


if __name__ == "__main__":
    main()
