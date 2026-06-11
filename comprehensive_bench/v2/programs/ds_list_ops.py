# ds_list_ops: append N items, index-sum them, pop all.


def main() -> None:
    n = 1500000
    xs = []
    for i in range(n):
        xs.append(i * 3 % 1000)
    index_sum = 0
    for i in range(n):
        index_sum += xs[i]
    pop_sum = 0
    while xs:
        pop_sum += xs.pop()
    print(f"n={n}")
    print(f"index_sum={index_sum}")
    print(f"pop_sum={pop_sum}")


if __name__ == "__main__":
    main()
