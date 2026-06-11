"""Deterministic math kernel: sqrt/sin/cos/log/exp over N values derived from
an in-language LCG (no random module — implementations differ). Each result is
quantized per element to int(f(x)*1000) so float accumulation stays stable."""
import math


def main():
    n = 300000
    state = 42
    sqrt_sum = 0
    sin_sum = 0
    cos_sum = 0
    log_sum = 0
    exp_sum = 0

    for _ in range(n):
        state = (state * 1103515245 + 12345) % 2147483648
        x = state / 2147483648.0 * 10.0

        sqrt_sum += int(math.sqrt(x) * 1000.0)
        sin_sum += int(math.sin(x) * 1000.0)
        cos_sum += int(math.cos(x) * 1000.0)
        log_sum += int(math.log(x + 1.0) * 1000.0)
        exp_sum += int(math.exp(x / 10.0) * 1000.0)

    print("sqrt_sum=" + str(sqrt_sum))
    print("sin_sum=" + str(sin_sum))
    print("cos_sum=" + str(cos_sum))
    print("log_sum=" + str(log_sum))
    print("exp_sum=" + str(exp_sum))


if __name__ == "__main__":
    main()
