"""Spawn/join 200 threads; each does a small local computation and reports
its result over a queue. Print completion count + total (after all joins)."""
import queue
import threading


def worker(idx, results):
    acc = 0
    for j in range(5000):
        acc += (idx * 31 + j) % 97
    results.put(acc)


def main():
    n = 200
    results = queue.Queue()
    threads = []

    for i in range(n):
        t = threading.Thread(target=worker, args=(i, results))
        threads.append(t)
        t.start()

    for t in threads:
        t.join()

    completed = 0
    total = 0
    for _ in range(n):
        total += results.get()
        completed += 1

    print("completed=" + str(completed))
    print("total=" + str(total))


if __name__ == "__main__":
    main()
