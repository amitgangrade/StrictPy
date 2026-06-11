"""Cache-server pattern: bounded dict cache with two-generation (segmented)
eviction, 300k mixed get/put ops from an LCG key stream; print
hit/miss/rotation counts.

Mirrors the StrictPy twin: two generations `cur` and `prev`; when `cur`
fills to cap/2, `prev` is discarded wholesale and `cur` becomes `prev`.
Memory stays bounded at ~cap entries and old keys age out."""


def main():
    n = 300000
    half_cap = 2048
    cur = {}
    prev = {}
    state = 987654321
    hits = 0
    misses = 0
    rotations = 0

    for i in range(n):
        state = (state * 1103515245 + 12345) % 2147483648
        key = "k" + str(state % 30000)
        op = state % 10

        if op < 7:
            # get
            v = cur.get(key)
            if v is not None:
                hits += 1
            else:
                w = prev.get(key)
                if w is not None:
                    hits += 1
                    cur[key] = w        # promote into the live generation
                else:
                    misses += 1
                    cur[key] = i        # miss-fill
        else:
            # put (insert or overwrite)
            cur[key] = i

        # Rotate generations once the live one fills up.
        if len(cur) >= half_cap:
            prev = cur
            cur = {}
            rotations += 1

    print("hits=" + str(hits))
    print("misses=" + str(misses))
    print("rotations=" + str(rotations))
    print("resident=" + str(len(cur) + len(prev)))


if __name__ == "__main__":
    main()
