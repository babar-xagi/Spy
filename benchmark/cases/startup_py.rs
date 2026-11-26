use python::math;

fn main() -> spy_runtime::SpyResult<()> {
    let val: i64 = math.call("floor", (1.5,))?;
    println!("{val}");
    Ok(())
}
