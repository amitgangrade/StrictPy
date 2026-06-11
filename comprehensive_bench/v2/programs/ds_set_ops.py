# ds_set_ops: set-shaped workload (dedupe + membership).
# Python uses a real set of ints — the idiomatic tool. StrictPy has no usable
# Set, so its twin emulates one with Dict[str, i64] (see .spy notes).

def main():
    n = 300000
    members = set()
    for i in range(n):
        members.add(i * 2)
    hits = 0
    misses = 0
    for i in range(2 * n):
        if i in members:
            hits += 1
        else:
            misses += 1
    print(f"size={len(members)}")
    print(f"hits={hits}")
    print(f"misses={misses}")

main()
