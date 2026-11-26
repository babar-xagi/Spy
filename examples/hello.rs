fn fib(n: u64) -> u64 {
    let mut a = 0;
    let mut b = 1;
    for _ in 0..n {
        let next = a + b;
        a = b;
        b = next;
    }
    a
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    println!("Hello from SPY Phase 0!");
    println!("fib(10) = {}", fib(10));
    if !args.is_empty() {
        println!("args = {:?}", args);
    }
}
