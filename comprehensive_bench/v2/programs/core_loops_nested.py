# core_loops_nested: triple-nested for-range loops with minimal body work
# (loop overhead microbenchmark).


def main():
    acc = 0
    for i in range(150):
        for j in range(150):
            for k in range(200):
                acc += k
            acc += j
        acc += i
    print(f"loops_nested_acc={acc}")


if __name__ == "__main__":
    main()
