use std::collections::HashSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Origin from which a Python interpreter candidate was discovered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PythonSource {
    Pyo3Env,
    VirtualEnv,
    CondaEnv,
    Path,
    UserLocal,
    UvManaged,
    System,
}

impl PythonSource {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Pyo3Env => "PYO3_PYTHON",
            Self::VirtualEnv => "VIRTUAL_ENV",
            Self::CondaEnv => "CONDA_PREFIX",
            Self::Path => "PATH",
            Self::UserLocal => "~/.local/bin",
            Self::UvManaged => "uv-managed",
            Self::System => "system",
        }
    }
}

/// Metadata collected from an installed CPython interpreter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PythonInstallation {
    pub source: PythonSource,
    pub invoked_path: PathBuf,
    pub executable: PathBuf,
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
    pub is_prerelease: bool,
    pub version_text: String,
    pub include_dir: Option<PathBuf>,
    pub lib_dir: Option<PathBuf>,
    pub lib_pl: Option<PathBuf>,
    pub ld_library: Option<String>,
    pub shared_enabled: bool,
    pub has_python_h: bool,
    pub linkable_libpython: Option<PathBuf>,
    pub runtime_libpython: Option<PathBuf>,
}

impl PythonInstallation {
    pub fn short_version(&self) -> String {
        format!("{}.{}.{}", self.major, self.minor, self.patch)
    }

    pub fn minor_tag(&self) -> String {
        format!("{}.{}", self.major, self.minor)
    }
}

const PROBE_SCRIPT: &str = r#"
import sys, sysconfig
vi = sys.version_info
print("EXE=" + (sys.executable or ""))
print(f"VER={vi.major}.{vi.minor}.{vi.micro}")
print("REL=" + str(vi.releaselevel))
print("INCLUDEPY=" + (sysconfig.get_config_var("INCLUDEPY") or ""))
print("LIBDIR=" + (sysconfig.get_config_var("LIBDIR") or ""))
print("LIBPL=" + (sysconfig.get_config_var("LIBPL") or ""))
print("LDLIBRARY=" + (sysconfig.get_config_var("LDLIBRARY") or ""))
print("SHARED=" + str(sysconfig.get_config_var("Py_ENABLE_SHARED") or 0))
"#;

/// Discovers Python installations available on the host system.
pub fn discover_python_installations() -> Vec<PythonInstallation> {
    let candidates = collect_python_candidates();
    let mut seen_canonical = HashSet::new();
    let mut discovered = Vec::new();

    for (source, path) in candidates {
        let Ok(canonical) = fs::canonicalize(&path) else {
            continue;
        };
        if !canonical.is_file() || !seen_canonical.insert(canonical.clone()) {
            continue;
        }
        if let Some(info) = probe_python_executable(source, path) {
            discovered.push(info);
        }
    }

    discovered
}

fn collect_python_candidates() -> Vec<(PythonSource, PathBuf)> {
    let mut candidates = Vec::new();
    let names = [
        "python3",
        "python",
        "python3.16",
        "python3.15",
        "python3.14",
        "python3.13",
        "python3.12",
        "python3.11",
        "python3.10",
        "python3.9",
        "python3.8",
    ];

    if let Some(pyo3_py) = env::var_os("PYO3_PYTHON") {
        candidates.push((PythonSource::Pyo3Env, PathBuf::from(pyo3_py)));
    }
    if let Some(venv) = env::var_os("VIRTUAL_ENV") {
        let bin = PathBuf::from(venv).join("bin");
        for name in ["python3", "python"] {
            candidates.push((PythonSource::VirtualEnv, bin.join(name)));
        }
    }
    if let Some(conda) = env::var_os("CONDA_PREFIX") {
        let bin = PathBuf::from(conda).join("bin");
        for name in ["python3", "python"] {
            candidates.push((PythonSource::CondaEnv, bin.join(name)));
        }
    }

    if let Some(path_var) = env::var_os("PATH") {
        for dir in env::split_paths(&path_var) {
            for name in names {
                candidates.push((PythonSource::Path, dir.join(name)));
            }
        }
    }

    if let Some(home) = env::var_os("HOME") {
        let home_dir = PathBuf::from(home);
        let local_bin = home_dir.join(".local/bin");
        for name in names {
            candidates.push((PythonSource::UserLocal, local_bin.join(name)));
        }

        let uv_root = env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home_dir.join(".local/share"))
            .join("uv/python");
        if let Ok(entries) = fs::read_dir(&uv_root) {
            let mut dirs: Vec<PathBuf> = entries
                .filter_map(|e| e.ok().map(|entry| entry.path()))
                .filter(|p| p.is_dir())
                .collect();
            dirs.sort();
            for dir in dirs {
                let bin = dir.join("bin");
                for name in names {
                    candidates.push((PythonSource::UvManaged, bin.join(name)));
                }
            }
        }
    }

    for sys_dir in ["/usr/local/bin", "/usr/bin"] {
        let dir = Path::new(sys_dir);
        for name in names {
            candidates.push((PythonSource::System, dir.join(name)));
        }
    }

    candidates
}

fn probe_python_executable(source: PythonSource, path: PathBuf) -> Option<PythonInstallation> {
    let output = Command::new(&path).arg("-c").arg(PROBE_SCRIPT).output().ok()?;
    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut exe: Option<PathBuf> = None;
    let mut ver_str = String::new();
    let mut is_prerelease = false;
    let mut include_dir: Option<PathBuf> = None;
    let mut lib_dir: Option<PathBuf> = None;
    let mut lib_pl: Option<PathBuf> = None;
    let mut ld_library: Option<String> = None;
    let mut shared_enabled = false;

    for line in stdout.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim();
        match key {
            "EXE" if !value.is_empty() => exe = Some(PathBuf::from(value)),
            "VER" => ver_str = value.to_owned(),
            "REL" => is_prerelease = value != "final",
            "INCLUDEPY" if !value.is_empty() => include_dir = Some(PathBuf::from(value)),
            "LIBDIR" if !value.is_empty() => lib_dir = Some(PathBuf::from(value)),
            "LIBPL" if !value.is_empty() => lib_pl = Some(PathBuf::from(value)),
            "LDLIBRARY" if !value.is_empty() => ld_library = Some(value.to_owned()),
            "SHARED" => shared_enabled = value == "1",
            _ => {}
        }
    }

    let (major, minor, patch) = parse_version_triple(&ver_str)?;
    let executable = exe.unwrap_or_else(|| path.clone());
    let has_python_h = include_dir
        .as_ref()
        .is_some_and(|dir| dir.join("Python.h").is_file());

    let (linkable_libpython, runtime_libpython) = locate_libpython(
        major,
        minor,
        lib_dir.as_deref(),
        lib_pl.as_deref(),
        ld_library.as_deref(),
    );

    Some(PythonInstallation {
        source,
        invoked_path: path,
        executable,
        major,
        minor,
        patch,
        is_prerelease,
        version_text: ver_str,
        include_dir,
        lib_dir,
        lib_pl,
        ld_library,
        shared_enabled,
        has_python_h,
        linkable_libpython,
        runtime_libpython,
    })
}

fn parse_version_triple(text: &str) -> Option<(u32, u32, u32)> {
    let mut parts = text.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next()?.parse().ok()?;
    Some((major, minor, patch))
}

fn locate_libpython(
    major: u32,
    minor: u32,
    lib_dir: Option<&Path>,
    lib_pl: Option<&Path>,
    ld_library: Option<&str>,
) -> (Option<PathBuf>, Option<PathBuf>) {
    // PyO3's build script searches `LIBDIR` (`-L <lib_dir>`); libraries located solely
    // inside `LIBPL` without a `LIBDIR` symlink (e.g. Debian/Ubuntu without python3-dev)
    // fail `-lpython3.x` linking unless `python3-dev` is installed.
    let mut link_dirs = Vec::new();
    if let Some(dir) = lib_dir {
        link_dirs.push(dir);
    }

    let mut runtime_dirs = Vec::new();
    if let Some(dir) = lib_dir {
        runtime_dirs.push(dir);
    }
    if let Some(dir) = lib_pl {
        runtime_dirs.push(dir);
    }

    let linkable_names = [
        ld_library.unwrap_or("").to_owned(),
        format!("libpython{major}.{minor}.so"),
        format!("libpython{major}.{minor}.a"),
    ];
    let runtime_names = [
        format!("libpython{major}.{minor}.so.1.0"),
        format!("libpython{major}.{minor}.so.1"),
        format!("libpython{major}.{minor}.so"),
    ];

    let linkable = find_first_file(&link_dirs, &linkable_names);
    let runtime = find_first_file(&runtime_dirs, &runtime_names);
    (linkable, runtime)
}

fn find_first_file(dirs: &[&Path], names: &[String]) -> Option<PathBuf> {
    for dir in dirs {
        for name in names {
            if name.is_empty() {
                continue;
            }
            let candidate = dir.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}
