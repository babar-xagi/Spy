import math


def main() -> None:
    acc = 0.0
    sqrt = math.sqrt
    for i in range(1, 100_001):
        acc += sqrt(i)
    print(f"sum={acc:.4f}")


if __name__ == "__main__":
    main()
