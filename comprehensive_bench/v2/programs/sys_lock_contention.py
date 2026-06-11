"""Lock contention: 4 threads each perform N lock-protected increments of a
shared counter; print the final counter (after all joins)."""
import threading


def main():
    per_thread = 150000
    counter = [0]
    lock = threading.Lock()

    def worker():
        for _ in range(per_thread):
            with lock:
                counter[0] += 1

    threads = [threading.Thread(target=worker) for _ in range(4)]
    for t in threads:
        t.start()
    for t in threads:
        t.join()

    print("counter=" + str(counter[0]))


if __name__ == "__main__":
    main()
