def is_prime(n: int) -> bool:
    if n < 2:
        return False
    if n == 2:
        return True
    if n % 2 == 0:
        return False
    d = 3
    while d * d <= n:
        if n % d == 0:
            return False
        d += 2
    return True


def count_primes(limit: int) -> int:
    count = 0
    for n in range(2, limit):
        if is_prime(n):
            count += 1
    return count


def main() -> None:
    total = count_primes(150_000)
    print(f"primes={total}")


if __name__ == "__main__":
    main()
