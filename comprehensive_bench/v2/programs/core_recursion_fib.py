# core_recursion_fib: naive recursive fibonacci.


def fib(n):
    if n < 2:
        return n
    return fib(n - 1) + fib(n - 2)


def main():
    print(f"fib={fib(31)}")


if __name__ == "__main__":
    main()
