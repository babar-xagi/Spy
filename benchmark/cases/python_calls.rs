use python::math;

use spy_runtime::pyo3::prelude::*;
use spy_runtime::SpyRuntime;

fn main() -> spy_runtime::SpyResult<()> {
    let mode = std::env::args().nth(1).unwrap_or_else(|| "per-call".to_owned());
    let n = 100_000u64;
    let total: f64 = match mode.as_str() {
        "batched" => {
            let math_mod = math.module()?;
            SpyRuntime::try_with_gil(|py| {
                let sqrt_fn = math_mod.as_py().bind(py).getattr("sqrt")?;
                let mut acc = 0.0f64;
                for i in 1..=n {
                    acc += sqrt_fn.call1((i,))?.extract::<f64>()?;
                }
                Ok(acc)
            })?
        }
        _ => {
            let mut acc = 0.0f64;
            for i in 1..=n {
                acc += math.call::<_, f64>("sqrt", (i,))?;
            }
            acc
        }
    };
    println!("sum={total:.4}");
    Ok(())
}
