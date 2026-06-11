"""Producer/consumer: one producer thread sends 200k ints through a bounded
queue, main thread consumes them all; print the sum."""
import queue
import threading


def producer(q, n):
    for i in range(n):
        q.put(i % 1000)


def main():
    n = 200000
    q = queue.Queue(maxsize=1024)

    t = threading.Thread(target=producer, args=(q, n))
    t.start()

    total = 0
    received = 0
    while received < n:
        total += q.get()
        received += 1

    t.join()
    print("received=" + str(received))
    print("sum=" + str(total))


if __name__ == "__main__":
    main()
