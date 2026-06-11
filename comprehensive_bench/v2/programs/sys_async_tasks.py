# sys_async_tasks — CPython twin of sys_async_tasks.spy.
# 1000 tasks, each computing (i*i) % 1000; spawned and awaited in waves of 4.
# Output (byte-identical to the .spy): done=1000 / sum=461500.
import asyncio

N = 1000


async def task_value(i):
    return (i * i) % 1000


async def main_async():
    total = 0
    done = 0
    i = 0
    while i < N:
        fa = asyncio.create_task(task_value(i))
        fb = asyncio.create_task(task_value(i + 1))
        fc = asyncio.create_task(task_value(i + 2))
        fd = asyncio.create_task(task_value(i + 3))
        total = total + await fa + await fb + await fc + await fd
        done = done + 4
        i = i + 4
    print("done=" + str(done))
    print("sum=" + str(total))


asyncio.run(main_async())
