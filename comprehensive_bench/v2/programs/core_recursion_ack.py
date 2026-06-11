# core_recursion_ack: Ackermann function, deep recursion.

import sys


def ack(m, n):
    if m == 0:
        return n + 1
    if n == 0:
        return ack(m - 1, 1)
    return ack(m - 1, ack(m, n - 1))


def main():
    print(f"ack={ack(3, 6)}")


if __name__ == "__main__":
    sys.setrecursionlimit(100000)
    main()
