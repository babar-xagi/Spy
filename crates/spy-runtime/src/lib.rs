pub mod discovery;
pub mod pyo3_compat;

pub use discovery::{discover_python_installations, PythonInstallation, PythonSource};
pub use pyo3_compat::{
    probe_cpython_c_embedding, select_best_report, verify_pyo3_compatibility, CEmbedProbeResult,
    CompatibilityStatus, Pyo3CompatibilityReport,
};

use std::path::PathBuf;
use thiserror::Error;

/// Errors returned by Python discovery and runtime verification.
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

    #[error("I/O error in runtime probe: {0}")]
    Io(#[from] std::io::Error),
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
            linkable_libpython: has_lib.then(|| PathBuf::from(format!("/usr/lib/libpython{major}.{minor}.so"))),
            runtime_libpython: None,
        }
    }

    #[test]
    fn prefers_stable_over_prerelease_for_same_status() {
        let cc = Path::new("/usr/bin/gcc");
        let alpha_316 = verify_pyo3_compatibility(&fake_python(3, 16, true, true, true), Some(cc));
        let stable_315 = verify_pyo3_compatibility(&fake_python(3, 15, false, true, true), Some(cc));
        let missing_314 =
            verify_pyo3_compatibility(&fake_python(3, 14, false, false, false), Some(cc));

        assert_eq!(missing_314.status, CompatibilityStatus::MissingPrerequisites);
        let reports = vec![missing_314, alpha_316, stable_315];
        let best = select_best_report(&reports).unwrap();
        assert_eq!(best.python.minor, 15);
    }
}
