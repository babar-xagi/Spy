pub mod discovery;
pub mod pyo3_compat;
#[cfg(feature = "embedded")]
pub mod runtime;

#[cfg(feature = "embedded")]
pub use pyo3;

pub use discovery::{discover_python_installations, PythonInstallation, PythonSource};
pub use pyo3_compat::{
    probe_cpython_c_embedding, select_best_report, verify_pyo3_compatibility, CEmbedProbeResult,
    CompatibilityStatus, Pyo3CompatibilityReport,
};
#[cfg(feature = "embedded")]
pub use runtime::{
    cached_module_names, clear_module_cache, eval_module_code, import_module,
    import_module_uncached, preload_modules, preload_or_exit, LazyPyModule, RuntimeInfo,
    SpyModule, SpyObject, SpyResult, SpyRuntime,
};

#[cfg(feature = "embedded")]
use std::fmt;
use std::path::PathBuf;
use thiserror::Error;

/// Errors returned by Python discovery, runtime verification, and embedded execution.
#[derive(Debug, Error)]
pub enum RuntimeError {
    #[error("Python headers (`Python.h`) are missing for `{python}`")]
    MissingPythonHeaders { python: PathBuf },

    #[error("linkable `libpython` is missing for `{python}`")]
    MissingLibPython { python: PathBuf },

    #[error("CPython C embedding probe failed to compile: {stderr}")]
    ProbeCompileFailed { stderr: String },

    #[error("CPython C embedding probe exited with status {code:?}: {stderr}")]
    ProbeExecutionFailed { code: Option<i32>, stderr: String },

    #[error(
        "failed to import Python module `{module}` using `{python_executable}` (CPython {python_version}): {reason}\nhint: {hint}"
    )]
    ImportFailed {
        module: String,
        python_executable: String,
        python_version: String,
        reason: String,
        hint: String,
    },

    #[error("{}", format_python_exception(.exc_type, .message, .traceback.as_deref()))]
    PythonException {
        exc_type: String,
        message: String,
        traceback: Option<String>,
    },

    #[error("I/O error in runtime probe: {0}")]
    Io(#[from] std::io::Error),
}

impl RuntimeError {
    /// Converts a PyO3 `PyErr` into a structured `RuntimeError` while holding the GIL.
    #[cfg(feature = "embedded")]
    pub fn from_py_err(py: pyo3::Python<'_>, err: pyo3::PyErr) -> Self {
        let (exc_type, message, traceback) = runtime::extract_py_err_details(py, err);
        Self::PythonException {
            exc_type,
            message,
            traceback,
        }
    }
}

fn format_python_exception(exc_type: &str, message: &str, traceback: Option<&str>) -> String {
    match traceback {
        Some(tb) => format!("{exc_type}: {message}\n{tb}"),
        None => format!("{exc_type}: {message}"),
    }
}

#[cfg(feature = "embedded")]
impl From<pyo3::CastError<'_, '_>> for RuntimeError {
    fn from(err: pyo3::CastError<'_, '_>) -> Self {
        Self::PythonException {
            exc_type: "TypeError".to_owned(),
            message: err.to_string(),
            traceback: None,
        }
    }
}

#[cfg(feature = "embedded")]
impl From<fmt::Error> for RuntimeError {
    fn from(err: fmt::Error) -> Self {
        Self::PythonException {
            exc_type: "FormatError".to_owned(),
            message: err.to_string(),
            traceback: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn fake_python(
        major: u32,
        minor: u32,
        is_prerelease: bool,
        has_python_h: bool,
        has_lib: bool,
    ) -> PythonInstallation {
        PythonInstallation {
            source: PythonSource::Path,
            invoked_path: PathBuf::from("/usr/bin/python3"),
            executable: PathBuf::from(format!("/usr/bin/python{major}.{minor}")),
            major,
            minor,
            patch: 0,
            is_prerelease,
            version_text: format!("{major}.{minor}.0"),
            include_dir: Some(PathBuf::from("/usr/include/python")),
            lib_dir: Some(PathBuf::from("/usr/lib")),
            lib_pl: None,
            ld_library: Some(format!("libpython{major}.{minor}.so")),
            shared_enabled: true,
            has_python_h,
            linkable_libpython: has_lib
                .then(|| PathBuf::from(format!("/usr/lib/libpython{major}.{minor}.so"))),
            runtime_libpython: None,
        }
    }

    #[test]
    fn prefers_stable_over_prerelease_and_headers_for_same_status() {
        let cc = Path::new("/usr/bin/gcc");
        let alpha_316 = verify_pyo3_compatibility(&fake_python(3, 16, true, true, true), Some(cc));
        let stable_315 = verify_pyo3_compatibility(&fake_python(3, 15, false, true, true), Some(cc));
        let stable_314_no_h =
            verify_pyo3_compatibility(&fake_python(3, 14, false, false, true), Some(cc));
        let missing_lib =
            verify_pyo3_compatibility(&fake_python(3, 14, false, false, false), Some(cc));

        assert_eq!(alpha_316.status, CompatibilityStatus::ReadyWithForwardCompat);
        assert_eq!(stable_315.status, CompatibilityStatus::Ready);
        assert_eq!(stable_314_no_h.status, CompatibilityStatus::Ready);
        assert_eq!(missing_lib.status, CompatibilityStatus::MissingPrerequisites);

        let reports = vec![missing_lib, alpha_316, stable_314_no_h, stable_315];
        let best = select_best_report(&reports).unwrap();
        assert_eq!(best.python.minor, 15);
    }

    #[cfg(feature = "embedded")]
    #[test]
    fn initializes_python_imports_modules_and_caches_handles() {
        clear_module_cache();
        let info = SpyRuntime::initialize().expect("CPython should initialize");
        assert!(SpyRuntime::is_initialized());
        assert!(info.version.starts_with('3'));

        let math = import_module("math").expect("math module should import");
        let sqrt: f64 = math.call("sqrt", (25,)).expect("sqrt(25) should succeed");
        assert!((sqrt - 5.0).abs() < f64::EPSILON);

        let pi: f64 = math.attr("pi").expect("math.pi should extract");
        assert!((pi - std::f64::consts::PI).abs() < 1e-12);

        let math_again = import_module("math").expect("cached import should succeed");
        assert_eq!(math_again.name(), "math");
        assert!(cached_module_names().contains(&"math".to_owned()));
    }

    #[cfg(feature = "embedded")]
    #[test]
    fn instantiates_classes_handles_objects_and_exceptions() {
        let collections = import_module("collections").expect("collections should import");
        let counter = collections
            .instantiate("Counter", (vec!["apple", "banana", "apple"],))
            .expect("Counter instantiation should work");
        let count_apple: usize = counter
            .call_method("get", ("apple", 0))
            .expect("Counter.get should work");
        assert_eq!(count_apple, 2);

        let mod_code = c"
class Greeter:
    def __init__(self, prefix):
        self.prefix = prefix
    def greet(self, name):
        if not name:
            raise ValueError('name cannot be empty')
        return f'{self.prefix}, {name}!'
";
        let test_mod = eval_module_code(mod_code, c"greeter.py", "spy_test_greeter")
            .expect("inline Python class module should evaluate");
        let greeter = test_mod
            .instantiate("Greeter", ("Hello",))
            .expect("Greeter should instantiate");
        assert_eq!(greeter.get_attr::<String>("prefix").unwrap(), "Hello");
        greeter.set_attr("prefix", "Hi").unwrap();
        let msg: String = greeter.call_method("greet", ("SPY",)).unwrap();
        assert_eq!(msg, "Hi, SPY!");

        let err = greeter
            .call_method::<_, String>("greet", ("",))
            .expect_err("empty name should raise Python ValueError");
        match err {
            RuntimeError::PythonException {
                exc_type, message, ..
            } => {
                assert_eq!(exc_type, "ValueError");
                assert!(message.contains("name cannot be empty"));
            }
            other => panic!("expected PythonException, got {other:?}"),
        }
    }

    #[cfg(feature = "embedded")]
    #[test]
    fn formats_actionable_import_errors_and_supports_gil_detach() {
        let err = import_module("spy_definitely_nonexistent_module_12345")
            .expect_err("missing module should return ImportFailed");
        let text = err.to_string();
        assert!(text.contains("spy_definitely_nonexistent_module_12345"));
        assert!(text.contains("hint:"));

        let total = SpyRuntime::with_gil(|py| {
            SpyRuntime::detach(py, || (0u64..1000).sum::<u64>())
        });
        assert_eq!(total, 499_500);
    }
}
