use python::math;
use python::collections as col;
use python::json as pyjson;
use python::os::path as ospath;

fn native_sum(n: u64) -> u64 {
    (0..n).sum()
}

fn main() -> spy_runtime::SpyResult<()> {
    let root: f64 = math.call("sqrt", (144.0,))?;
    let pi: f64 = math.attr("pi")?;

    let counter = col.instantiate("Counter", (vec!["rust", "python", "rust"],))?;
    let rust_hits: usize = counter.call_method("get", ("rust", 0))?;

    let encoded: String = pyjson.call("dumps", (vec![10, 20, 30],))?;
    let joined: String = ospath.call("join", ("spy", "phase2"))?;
    let native_total = native_sum(1_000_000);

    println!("math.sqrt(144) = {root}, pi = {pi:.4}");
    println!("Counter['rust'] = {rust_hits}");
    println!("json.dumps = {encoded}");
    println!("os.path.join = {joined}");
    println!("native_sum = {native_total}");
    println!("cached modules = {:?}", spy_runtime::cached_module_names());

    Ok(())
}
