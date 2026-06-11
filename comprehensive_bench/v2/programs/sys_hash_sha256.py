"""Streaming + one-shot hashing benchmark: feed many chunks through a
streaming hasher, then a sha256/hmac chain; print digest of digests."""
import hashlib
import hmac


def main():
    # Part 1: streaming hasher fed many small chunks.
    stream = hashlib.sha256()
    for i in range(80000):
        stream.update(f"chunk-{i}-abcdefghijklmnopqrstuvwxyz0123456789".encode())
    stream_digest = stream.hexdigest()

    # Part 2: one-shot sha256 chain + hmac, folded into a final hasher.
    final = hashlib.sha256()
    d = "seed"
    for j in range(25000):
        d = hashlib.sha256(f"{d}:{j}".encode()).hexdigest()
        hm = hmac.new(f"key-{j % 16}".encode(), d.encode(), hashlib.sha256).hexdigest()
        final.update(d.encode())
        final.update(hm.encode())

    print("stream=" + stream_digest)
    print("chain=" + d)
    print("final=" + final.hexdigest())


if __name__ == "__main__":
    main()
