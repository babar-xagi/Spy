use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use spy_runtime::{
    discover_python_installations, select_best_report, verify_pyo3_compatibility,
    Pyo3CompatibilityReport,
};

/// Operating system and WSL environment details.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlatformInfo {
    pub os: &'static str,
    pub arch: &'static str,
    pub is_wsl: bool,
    pub wsl_distro: Option<String>,
    pub kernel_release: Option<String>,
}

/// Detected executable path paired with its reported version line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolInfo {
    pub path: PathBuf,
    pub version: String,
}

/// Detected Rust toolchain (`rustc`, `cargo`, and optional `rustup` active toolchain).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RustToolchain {
    pub rustc: Option<ToolInfo>,
    pub cargo: Option<ToolInfo>,
    pub active_toolchain: Option<String>,
}

/// Detected C/C++ and Zig native toolchains.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeToolchains {
    pub c_compiler: Option<ToolInfo>,
    pub cpp_compiler: Option<ToolInfo>,
    pub zig: Option<ToolInfo>,
    pub zig_note: Option<String>,
}

/// Full environment report surfaced by `spy doctor`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvironmentReport {
    pub platform: PlatformInfo,
    pub rust: RustToolchain,
    pub native: NativeToolchains,
    pub python_reports: Vec<Pyo3CompatibilityReport>,
    pub preferred_python_index: Option<usize>,
}

/// Collects platform, Rust, C/C++/Zig, Python, and PyO3 compatibility diagnostics.
pub fn inspect_environment() -> EnvironmentReport {
    let platform = detect_platform();
    let rust = detect_rust_toolchain();
    let native = detect_native_toolchains();
    let pythons = discover_python_installations();
    let c_path = native.c_compiler.as_ref().map(|t| t.path.as_path());
    let python_reports: Vec<Pyo3CompatibilityReport> = pythons
        .iter()
        .map(|py| verify_pyo3_compatibility(py, c_path))
        .collect();
    let preferred_python_index = select_best_report(&python_reports).and_then(|best| {
        python_reports
            .iter()
            .position(|r| r.python.executable == best.python.executable)
    });

    EnvironmentReport {
        platform,
        rust,
        native,
        python_reports,
        preferred_python_index,
    }
}

pub fn detect_platform() -> PlatformInfo {
    let proc_version = fs::read_to_string("/proc/version").ok();
    let wsl_distro = env::var("WSL_DISTRO_NAME").ok().filter(|s| !s.is_empty());
    let has_wsl_interop = env::var_os("WSL_INTEROP").is_some();
    let kernel_mentions_wsl = proc_version.as_deref().is_some_and(|v| {
        let lower = v.to_ascii_lowercase();
        lower.contains("microsoft") || lower.contains("wsl")
    });
    let is_wsl = wsl_distro.is_some() || has_wsl_interop || kernel_mentions_wsl;
    let kernel_release = proc_version
        .as_deref()
        .and_then(|v| v.split_whitespace().nth(2))
        .map(str::to_owned);

    PlatformInfo {
        os: env::consts::OS,
        arch: env::consts::ARCH,
        is_wsl,
        wsl_distro,
        kernel_release,
    }
}

pub fn detect_rust_toolchain() -> RustToolchain {
    let mut extra_dirs = Vec::new();
    if let Some(cargo_home) = env::var_os("CARGO_HOME") {
        extra_dirs.push(PathBuf::from(cargo_home).join("bin"));
    }
    if let Some(home) = env::var_os("HOME") {
        extra_dirs.push(PathBuf::from(home).join(".cargo/bin"));
    }

    let rustc = find_working_tool(&["rustc"], &extra_dirs, &["--version"]);
    let cargo = find_working_tool(&["cargo"], &extra_dirs, &["--version"]);
    let active_toolchain = find_working_tool(&["rustup"], &extra_dirs, &["show", "active-toolchain"])
        .map(|info| info.version);

    RustToolchain {
        rustc,
        cargo,
        active_toolchain,
    }
}

pub fn detect_native_toolchains() -> NativeToolchains {
    let mut extra_dirs = Vec::new();
    if let Some(home) = env::var_os("HOME") {
        extra_dirs.push(PathBuf::from(home).join(".local/bin"));
    }
    extra_dirs.push(PathBuf::from("/usr/local/bin"));
    extra_dirs.push(PathBuf::from("/usr/bin"));

    let c_compiler = find_working_tool(&["gcc", "clang", "cc"], &extra_dirs, &["--version"]);
    let cpp_compiler = find_working_tool(&["g++", "clang++", "c++"], &extra_dirs, &["--version"]);
    let zig = find_working_tool(&["zig"], &extra_dirs, &["version"]);
    let zig_note = if zig.is_none() {
        detect_broken_candidate(&["zig"], &extra_dirs)
    } else {
        None
    };

    NativeToolchains {
        c_compiler,
        cpp_compiler,
        zig,
        zig_note,
    }
}

fn candidate_dirs(extra_dirs: &[PathBuf]) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(path_var) = env::var_os("PATH") {
        dirs.extend(env::split_paths(&path_var));
    }
    for dir in extra_dirs {
        if !dirs.contains(dir) {
            dirs.push(dir.clone());
        }
    }
    dirs
}

fn find_working_tool(names: &[&str], extra_dirs: &[PathBuf], version_args: &[&str]) -> Option<ToolInfo> {
    let dirs = candidate_dirs(extra_dirs);
    for name in names {
        for dir in &dirs {
            let candidate = dir.join(name);
            if !is_regular_executable(&candidate) {
                continue;
            }
            if let Some(version) = query_version(&candidate, version_args) {
                return Some(ToolInfo {
                    path: candidate,
                    version,
                });
            }
        }
    }
    None
}

fn detect_broken_candidate(names: &[&str], extra_dirs: &[PathBuf]) -> Option<String> {
    let dirs = candidate_dirs(extra_dirs);
    for name in names {
        for dir in &dirs {
            let candidate = dir.join(name);
            if let Ok(meta) = fs::symlink_metadata(&candidate) {
                if meta.file_type().is_symlink() && !candidate.exists() {
                    let target = fs::read_link(&candidate)
                        .map(|p| p.display().to_string())
                        .unwrap_or_else(|_| "<unknown>".to_owned());
                    return Some(format!(
                        "`{}` is a broken symlink pointing to `{}`",
                        candidate.display(),
                        target
                    ));
                }
            }
        }
    }
    None
}

fn is_regular_executable(path: &Path) -> bool {
    let Ok(meta) = fs::metadata(path) else {
        return false;
    };
    if !meta.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

fn query_version(exe: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new(exe).args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let first_line = String::from_utf8_lossy(&out.stdout)
        .lines()
        .next()
        .unwrap_or("")
        .trim()
        .to_owned();
    if first_line.is_empty() {
        None
    } else {
        Some(first_line)
    }
}
