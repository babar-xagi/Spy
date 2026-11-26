use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::discovery::PythonInstallation;
use crate::RuntimeError;

/// PyO3 compatibility verdict for a detected Python installation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CompatibilityStatus {
    Ready,
    ReadyWithForwardCompat,
    MissingPrerequisites,
    Incompatible,
}

impl CompatibilityStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::ReadyWithForwardCompat => "ready (forward-compat)",
            Self::MissingPrerequisites => "missing prerequisites",
            Self::Incompatible => "incompatible",
        }
    }
}

/// Detailed PyO3 compatibility report for a single Python installation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pyo3CompatibilityReport {
    pub python: PythonInstallation,
    pub status: CompatibilityStatus,
    pub notes: Vec<String>,
    pub recommended_env: Vec<(String, String)>,
}

/// Result of compiling and executing a live CPython embedding probe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CEmbedProbeResult {
    pub python_executable: PathBuf,
    pub python_version: String,
    pub compiler: PathBuf,
    pub embedded_version_output: String,
}

/// Evaluates whether a Python installation meets PyO3 embedding/linking requirements.
pub fn verify_pyo3_compatibility(
    python: &PythonInstallation,
    c_compiler: Option<&Path>,
) -> Pyo3CompatibilityReport {
    let mut notes = Vec::new();
    let mut recommended_env = Vec::new();
    let mut blocked = false;

    if python.major < 3 || (python.major == 3 && python.minor < 8) {
        notes.push(format!(
            "CPython {} is older than PyO3's supported baseline (>= 3.8)",
            python.short_version()
        ));
        return Pyo3CompatibilityReport {
            python: python.clone(),
            status: CompatibilityStatus::Incompatible,
            notes,
            recommended_env,
        };
    }

    if !python.has_python_h {
        blocked = true;
        let inc_hint = python
            .include_dir
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "<unknown>".to_owned());
        notes.push(format!("missing Python.h in `{inc_hint}` (install python3-dev or use a Python toolchain with headers)"));
    }

    if python.linkable_libpython.is_none() {
        blocked = true;
        if let Some(rt) = &python.runtime_libpython {
            notes.push(format!(
                "runtime library `{}` found, but unversioned linkable `libpython{}.so` / `.a` is missing",
                rt.display(),
                python.minor_tag()
            ));
        } else {
            notes.push(format!(
                "linkable `libpython{}.so` or `.a` was not found in LIBDIR/LIBPL",
                python.minor_tag()
            ));
        }
    }

    if c_compiler.is_none() {
        blocked = true;
        notes.push("no working C compiler/linker (`cc`, `gcc`, or `clang`) detected".to_owned());
    }

    if !python.shared_enabled {
        notes.push("Python built without `--enable-shared` (static linking may need extra linker flags)".to_owned());
    }

    if blocked {
        return Pyo3CompatibilityReport {
            python: python.clone(),
            status: CompatibilityStatus::MissingPrerequisites,
            notes,
            recommended_env,
        };
    }

    recommended_env.push((
        "PYO3_PYTHON".to_owned(),
        python.executable.display().to_string(),
    ));

    // PyO3 natively targets released CPython series (currently up to 3.14);
    // newer CPython releases (3.15+) work via ABI3 forward compatibility or newer PyO3.
    let status = if python.major == 3 && python.minor <= 14 {
        notes.push(format!(
            "CPython {} has Python.h, linkable libpython, and C toolchain support",
            python.short_version()
        ));
        CompatibilityStatus::Ready
    } else {
        recommended_env.push((
            "PYO3_USE_ABI3_FORWARD_COMPATIBILITY".to_owned(),
            "1".to_owned(),
        ));
        notes.push(format!(
            "CPython {} has Python.h and linkable libpython; set `PYO3_USE_ABI3_FORWARD_COMPATIBILITY=1` if using PyO3 releases predating Python {}",
            python.short_version(),
            python.minor_tag()
        ));
        CompatibilityStatus::ReadyWithForwardCompat
    };

    Pyo3CompatibilityReport {
        python: python.clone(),
        status,
        notes,
        recommended_env,
    }
}

/// Picks the best PyO3-compatible Python report, preferring compatibility status,
/// explicit environment overrides, and stable releases over prereleases.
pub fn select_best_report(reports: &[Pyo3CompatibilityReport]) -> Option<&Pyo3CompatibilityReport> {
    reports.iter().min_by_key(|r| {
        let explicit_env = match r.python.source {
            crate::PythonSource::Pyo3Env => 0u8,
            crate::PythonSource::VirtualEnv | crate::PythonSource::CondaEnv => 1,
            _ => 2,
        };
        (r.status, explicit_env, r.python.is_prerelease)
    })
}

/// Compiles and executes a minimal CPython C-API embedding test to verify headers and `libpython`.
pub fn probe_cpython_c_embedding(
    python: &PythonInstallation,
    c_compiler: &Path,
) -> Result<CEmbedProbeResult, RuntimeError> {
    let include_dir = python
        .include_dir
        .as_ref()
        .filter(|_| python.has_python_h)
        .ok_or_else(|| RuntimeError::MissingPythonHeaders {
            python: python.executable.clone(),
        })?;
    let libpython = python
        .linkable_libpython
        .as_ref()
        .ok_or_else(|| RuntimeError::MissingLibPython {
            python: python.executable.clone(),
        })?;
    let lib_dir = libpython
        .parent()
        .ok_or_else(|| RuntimeError::MissingLibPython {
            python: python.executable.clone(),
        })?;

    let temp_dir = TempProbeDir::new("spy-cpython-probe")?;
    let c_path = temp_dir.path().join("probe.c");
    let bin_path = temp_dir.path().join("probe");

    let probe_c = r#"
#include <Python.h>
#include <stdio.h>

int main(void) {
    Py_Initialize();
    if (!Py_IsInitialized()) {
        return 2;
    }
    PyObject *sys = PyImport_ImportModule("sys");
    if (!sys) {
        PyErr_Print();
        return 3;
    }
    PyObject *ver = PyObject_GetAttrString(sys, "version");
    if (!ver) {
        Py_DECREF(sys);
        PyErr_Print();
        return 4;
    }
    const char *ver_utf8 = PyUnicode_AsUTF8(ver);
    if (ver_utf8) {
        printf("%s\n", ver_utf8);
    }
    Py_DECREF(ver);
    Py_DECREF(sys);
    if (Py_FinalizeEx() < 0) {
        return 5;
    }
    return 0;
}
"#;
    fs::write(&c_path, probe_c)?;

    let link_flag = format!("-lpython{}", python.minor_tag());
    let rpath_flag = format!("-Wl,-rpath,{}", lib_dir.display());
    let compile_out = Command::new(c_compiler)
        .arg(&c_path)
        .arg(format!("-I{}", include_dir.display()))
        .arg(format!("-L{}", lib_dir.display()))
        .arg(&rpath_flag)
        .arg(&link_flag)
        .arg("-o")
        .arg(&bin_path)
        .output()?;

    if !compile_out.status.success() {
        return Err(RuntimeError::ProbeCompileFailed {
            stderr: String::from_utf8_lossy(&compile_out.stderr).into_owned(),
        });
    }

    let run_out = Command::new(&bin_path).output()?;
    if !run_out.status.success() {
        return Err(RuntimeError::ProbeExecutionFailed {
            code: run_out.status.code(),
            stderr: String::from_utf8_lossy(&run_out.stderr).into_owned(),
        });
    }

    let first_line = String::from_utf8_lossy(&run_out.stdout)
        .lines()
        .next()
        .unwrap_or("")
        .trim()
        .to_owned();

    Ok(CEmbedProbeResult {
        python_executable: python.executable.clone(),
        python_version: python.short_version(),
        compiler: c_compiler.to_path_buf(),
        embedded_version_output: first_line,
    })
}

struct TempProbeDir(PathBuf);

impl TempProbeDir {
    fn new(prefix: &str) -> Result<Self, RuntimeError> {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = env::temp_dir().join(format!("{prefix}-{}-{nanos}", std::process::id()));
        fs::create_dir_all(&dir)?;
        Ok(Self(dir))
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempProbeDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
