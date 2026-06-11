"""Async tasks: spawn 1000 small coroutines and gather them all, summing the
results; print the sum (only after every task resolved)."""
import asyncio


async def task_value(k):
    return (k * k) % 1000


async def async_main():
    tasks = [asyncio.create_task(task_value(i)) for i in range(1000)]
    results = await asyncio.gather(*tasks)

    total = sum(results)
    done = len(results)

    print("done=" + str(done))
    print("sum=" + str(total))


def main():
    asyncio.run(async_main())


if __name__ == "__main__":
    main()
