use python::json as pyjson;

use spy_runtime::pyo3::prelude::*;
use spy_runtime::pyo3::types::PyDict;
use spy_runtime::SpyRuntime;

fn collatz_steps(mut n: u64) -> u64 {
    let mut steps = 0u64;
    while n != 1 {
        if n.is_multiple_of(2) {
            n /= 2;
        } else {
            n = 3 * n + 1;
        }
        steps += 1;
    }
    steps
}

fn main() -> spy_runtime::SpyResult<()> {
    let limit = 200_000u64;
    let mut total_steps = 0u64;
    let mut max_steps = 0u64;
    for n in 1..=limit {
        let s = collatz_steps(n);
        total_steps += s;
        if s > max_steps {
            max_steps = s;
        }
    }

    let json_mod = pyjson.module()?;
    let payload = SpyRuntime::try_with_gil(|py| {
        let dict = PyDict::new(py);
        dict.set_item("limit", limit)?;
        dict.set_item("max_steps", max_steps)?;
        dict.set_item("total_steps", total_steps)?;
        let kwargs = PyDict::new(py);
        kwargs.set_item("sort_keys", true)?;
        let dumps = json_mod.as_py().bind(py).getattr("dumps")?;
        Ok(dumps.call((dict,), Some(&kwargs))?.extract::<String>()?)
    })?;

    println!("{payload}");
    Ok(())
}
