import json


def collatz_steps(n: int) -> int:
    steps = 0
    while n != 1:
        if n % 2 == 0:
            n //= 2
        else:
            n = 3 * n + 1
        steps += 1
    return steps


def main() -> None:
    limit = 200_000
    total_steps = 0
    max_steps = 0
    for n in range(1, limit + 1):
        s = collatz_steps(n)
        total_steps += s
        if s > max_steps:
            max_steps = s

    payload = json.dumps(
        {
            "limit": limit,
            "max_steps": max_steps,
            "total_steps": total_steps,
        },
        sort_keys=True,
    )
    print(payload)


if __name__ == "__main__":
    main()
