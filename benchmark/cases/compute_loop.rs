use python::math;

fn is_prime(n: u32) -> bool {
    if n < 2 {
        return false;
    }
    if n == 2 {
        return true;
    }
    if n.is_multiple_of(2) {
        return false;
    }
    let mut d = 3u32;
    while d * d <= n {
        if n.is_multiple_of(d) {
            return false;
        }
        d += 2;
    }
    true
}

fn count_primes(limit: u32) -> u32 {
    let mut count = 0u32;
    for n in 2..limit {
        if is_prime(n) {
            count += 1;
        }
    }
    count
}

fn main() -> spy_runtime::SpyResult<()> {
    let _ = math.name();
    let total = count_primes(150_000);
    println!("primes={total}");
    Ok(())
}
