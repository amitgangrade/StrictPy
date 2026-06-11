# ds_comprehensions: list + dict comprehensions with filters over a large list.


def main() -> None:
    n = 500000
    base = [i * 7 % 9973 for i in range(n)]
    squares = [x * x % 100000 for x in base]
    evens = [x for x in base if x % 2 == 0]
    d = {"k" + str(x % 100): x for x in base if x % 17 == 0}
    sq_sum = sum(squares)
    ev_sum = sum(evens)
    dkeys = sorted(d)
    d_sum = sum(d[k] for k in dkeys)
    print(f"sq_len={len(squares)} sq_sum={sq_sum}")
    print(f"ev_len={len(evens)} ev_sum={ev_sum}")
    print(f"d_len={len(d)} d_sum={d_sum}")
    print(f"d_first={dkeys[0]} d_last={dkeys[-1]}")


if __name__ == "__main__":
    main()
