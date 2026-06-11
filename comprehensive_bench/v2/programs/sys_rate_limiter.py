"""Token-bucket rate limiter simulation over 500k synthetic events with
integer timestamps; print allowed/rejected counts."""


def main():
    n = 500000
    capacity = 100
    refill_per_tick = 2
    tokens = capacity
    last_ts = 0
    state = 555555555
    allowed = 0
    rejected = 0
    clock = 0

    for _ in range(n):
        state = (state * 1103515245 + 12345) % 2147483648
        clock += state % 3                 # 0-2 ticks between events
        cost = 1 + state % 4               # request cost 1-4 tokens

        if clock > last_ts:
            tokens = min(capacity, tokens + (clock - last_ts) * refill_per_tick)
            last_ts = clock

        if tokens >= cost:
            tokens -= cost
            allowed += 1
        else:
            rejected += 1

    print("allowed=" + str(allowed))
    print("rejected=" + str(rejected))
    print("final_tokens=" + str(tokens))
    print("final_clock=" + str(clock))


if __name__ == "__main__":
    main()
