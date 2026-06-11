"""Priority queue: push N tasks with synthetic priorities, pop all in
priority order; print integer priority/item sums.

heapq is the idiomatic single-threaded choice (queue.PriorityQueue adds
locking overhead this workload doesn't need)."""
import heapq


def main():
    n = 200000
    heap = []

    for i in range(n):
        prio = float((i * 37) % 1000)
        heapq.heappush(heap, (prio, i))

    prio_sum = 0
    item_sum = 0
    popped = 0
    while heap:
        prio, item = heapq.heappop(heap)
        prio_sum += int(prio)
        item_sum += item
        popped += 1

    print("popped=" + str(popped))
    print("prio_sum=" + str(prio_sum))
    print("item_sum=" + str(item_sum))


if __name__ == "__main__":
    main()
