# ds_exceptions: hot loop where ~1/8 iterations raise+catch ValueError and
# ~1/13 raise a user-defined Exception subclass.


class BenchError(Exception):
    def __init__(self, message, code):
        super().__init__(message)
        self.code = code


def risky(i):
    if i % 8 == 0:
        raise ValueError("bad " + str(i % 100))
    if i % 13 == 0:
        raise BenchError("bench", i % 7)
    return i % 5


def main() -> None:
    n = 600000
    ok = 0
    val_errors = 0
    bench_errors = 0
    code_sum = 0
    total = 0
    for i in range(1, n + 1):
        try:
            total += risky(i)
            ok += 1
        except BenchError as e:
            bench_errors += 1
            code_sum += e.code
        except ValueError:
            val_errors += 1
    print(f"ok={ok}")
    print(f"val_errors={val_errors}")
    print(f"bench_errors={bench_errors}")
    print(f"code_sum={code_sum}")
    print(f"total={total}")


if __name__ == "__main__":
    main()
