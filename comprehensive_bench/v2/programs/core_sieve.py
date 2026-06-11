# core_sieve: sieve of Eratosthenes with a list of bools; count primes below N.


def main():
    n = 2000000
    sieve = [True] * n
    sieve[0] = False
    sieve[1] = False
    i = 2
    while i * i < n:
        if sieve[i]:
            for j in range(i * i, n, i):
                sieve[j] = False
        i += 1
    count = 0
    for p in range(2, n):
        if sieve[p]:
            count += 1
    print(f"sieve_count={count}")


if __name__ == "__main__":
    main()
