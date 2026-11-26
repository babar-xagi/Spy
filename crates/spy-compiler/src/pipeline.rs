use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use spy_codegen::{generate_rust, GeneratedArtifact};
use spy_parser::{parse_source, ParsedProgram};
use spy_runtime::CompatibilityStatus;

use crate::env::{detect_rust_toolchain, inspect_environment};
use crate::CompilerError;

/// Options controlling `spy build` and `spy run`.
#[derive(Debug, Clone, Default)]
pub struct BuildOptions {
    pub release: bool,
    pub output: Option<PathBuf>,
    pub edition: Option<String>,
}

/// Result of compiling a `.rs` file through SPY.
#[derive(Debug, Clone)]
pub struct CompileResult {
    pub input_path: PathBuf,
    pub output_binary: PathBuf,
    pub generated: GeneratedArtifact,
}

struct PythonBuildEnv {
    python_exe: PathBuf,
    lib_dir: Option<PathBuf>,
    needs_forward_compat: bool,
}

/// Parses and translates a `.rs` source file without invoking `rustc` or `cargo`.
pub fn parse_and_translate(
    input_path: &Path,
) -> Result<(ParsedProgram, GeneratedArtifact), CompilerError> {
    let source = fs::read_to_string(input_path).map_err(|err| CompilerError::ReadSource {
        path: input_path.to_path_buf(),
        source: err,
    })?;
    let parsed = parse_source(&source)?;
    let generated = generate_rust(&parsed)?;
    Ok((parsed, generated))
}

/// Type-checks a `.rs` file without leaving build artifacts behind.
pub fn check_file(
    input_path: &Path,
    edition: Option<&str>,
) -> Result<GeneratedArtifact, CompilerError> {
    let (_, generated) = parse_and_translate(input_path)?;
    let edition = edition.unwrap_or("2024");

    if !generated.requires_python_runtime {
        let rustc = require_rustc()?;
        let temp = TempDir::new("spy-check")?;
        let src_file = temp.path().join(source_filename(input_path));
        let out_meta = temp.path().join("libcheck.rmeta");
        fs::write(&src_file, &generated.rust_source)?;

        let output = Command::new(&rustc)
            .arg("--edition")
            .arg(edition)
            .arg("--crate-type")
            .arg("bin")
            .arg("--emit=metadata")
            .arg(&src_file)
            .arg("-o")
            .arg(&out_meta)
            .output()?;

        if !output.status.success() {
            return Err(CompilerError::RustcFailed {
                stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            });
        }
        return Ok(generated);
    }

    let runner = prepare_python_cargo_project(input_path, &generated, edition)?;
    let output = runner
        .cargo_command("check")
        .output()?;
    if !output.status.success() {
        return Err(CompilerError::CargoFailed {
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        });
    }

    Ok(generated)
}

/// Compiles a `.rs` file into an executable binary.
pub fn build_file(
    input_path: &Path,
    options: &BuildOptions,
) -> Result<CompileResult, CompilerError> {
    let (_, generated) = parse_and_translate(input_path)?;
    let edition = options.edition.as_deref().unwrap_or("2024");

    let output_binary = match &options.output {
        Some(path) => path.clone(),
        None => default_output_path(input_path)?,
    };
    if let Some(parent) = output_binary.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)?;
    }

    if !generated.requires_python_runtime {
        let rustc = require_rustc()?;
        let temp = TempDir::new("spy-build")?;
        let src_file = temp.path().join(source_filename(input_path));
        fs::write(&src_file, &generated.rust_source)?;
        compile_with_rustc(&rustc, &src_file, &output_binary, options.release, edition)?;
    } else {
        let runner = prepare_python_cargo_project(input_path, &generated, edition)?;
        let built_bin = runner.build(options.release)?;
        fs::copy(&built_bin, &output_binary)?;
    }

    Ok(CompileResult {
        input_path: input_path.to_path_buf(),
        output_binary,
        generated,
    })
}

/// Compiles a `.rs` file and runs it, returning its exit code.
pub fn run_file(
    input_path: &Path,
    options: &BuildOptions,
    program_args: &[String],
) -> Result<i32, CompilerError> {
    let (_, generated) = parse_and_translate(input_path)?;
    let edition = options.edition.as_deref().unwrap_or("2024");

    if !generated.requires_python_runtime {
        let rustc = require_rustc()?;
        let temp = TempDir::new("spy-run")?;
        let src_file = temp.path().join(source_filename(input_path));
        let bin_name = input_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("spy_app");
        let bin_path = temp.path().join(bin_name);
        fs::write(&src_file, &generated.rust_source)?;

        compile_with_rustc(&rustc, &src_file, &bin_path, options.release, edition)?;
        let status = Command::new(&bin_path).args(program_args).status()?;
        return Ok(status.code().unwrap_or(1));
    }

    let runner = prepare_python_cargo_project(input_path, &generated, edition)?;
    let built_bin = runner.build(options.release)?;
    let local_bin = runner.temp_dir.path().join(&runner.pkg_name);
    fs::copy(&built_bin, &local_bin)?;

    let mut cmd = Command::new(&local_bin);
    cmd.args(program_args);
    if let Some(lib_dir) = &runner.py_env.lib_dir {
        let joined = prepend_ld_library_path(lib_dir);
        cmd.env("LD_LIBRARY_PATH", joined);
    }
    let status = cmd.status()?;
    Ok(status.code().unwrap_or(1))
}

struct PreparedCargoRunner {
    temp_dir: TempDir,
    cargo_path: PathBuf,
    manifest_path: PathBuf,
    target_dir: PathBuf,
    pkg_name: String,
    py_env: PythonBuildEnv,
}

impl PreparedCargoRunner {
    fn cargo_command(&self, subcommand: &str) -> Command {
        let mut cmd = Command::new(&self.cargo_path);
        if let Some(manifest_dir) = self.manifest_path.parent() {
            cmd.current_dir(manifest_dir);
        }
        cmd.arg(subcommand)
            .arg("--offline")
            .arg("--manifest-path")
            .arg(&self.manifest_path)
            .env("CARGO_TARGET_DIR", &self.target_dir)
            .env("PYO3_PYTHON", &self.py_env.python_exe);
        if self.py_env.needs_forward_compat {
            cmd.env("PYO3_USE_ABI3_FORWARD_COMPATIBILITY", "1");
        }
        cmd
    }

    fn build(&self, release: bool) -> Result<PathBuf, CompilerError> {
        let mut cmd = self.cargo_command("build");
        if release {
            cmd.arg("--release");
        }
        let output = cmd.output()?;
        if !output.status.success() {
            return Err(CompilerError::CargoFailed {
                stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            });
        }
        let profile = if release { "release" } else { "debug" };
        Ok(self.target_dir.join(profile).join(&self.pkg_name))
    }
}

fn prepare_python_cargo_project(
    input_path: &Path,
    generated: &GeneratedArtifact,
    edition: &str,
) -> Result<PreparedCargoRunner, CompilerError> {
    let cargo_path = require_cargo()?;
    let py_env = resolve_python_env()?;
    let spy_runtime_dir = locate_spy_runtime_dir()?;
    let target_dir = spy_target_cache_dir()?;

    let temp_dir = TempDir::new("spy-cargo")?;
    let src_dir = temp_dir.path().join("src");
    fs::create_dir_all(&src_dir)?;
    fs::write(src_dir.join("main.rs"), &generated.rust_source)?;

    let pkg_name = sanitize_package_name(input_path);
    let runtime_escaped = toml_string(&spy_runtime_dir.display().to_string());
    let manifest_text = format!(
        "[workspace]\n\n[package]\nname = {pkg:?}\nversion = \"0.1.0\"\nedition = {edition:?}\n\n[dependencies]\nspy-runtime = {{ path = {runtime_escaped} }}\n",
        pkg = pkg_name
    );
    let manifest_path = temp_dir.path().join("Cargo.toml");
    fs::write(&manifest_path, manifest_text)?;

    if let Some(workspace_lock) = spy_runtime_dir
        .parent()
        .and_then(|p| p.parent())
        .map(|root| root.join("Cargo.lock"))
        .filter(|p| p.is_file())
    {
        let _ = fs::copy(workspace_lock, temp_dir.path().join("Cargo.lock"));
    }

    if cfg!(unix)
        && let Some(lib_dir) = &py_env.lib_dir
    {
        let dot_cargo = temp_dir.path().join(".cargo");
        fs::create_dir_all(&dot_cargo)?;
        let link_arg = toml_string(&format!("link-arg=-Wl,-rpath,{}", lib_dir.display()));
        let cfg_text = format!("[build]\nrustflags = [\"-C\", {link_arg}]\n");
        fs::write(dot_cargo.join("config.toml"), cfg_text)?;
    }

    Ok(PreparedCargoRunner {
        temp_dir,
        cargo_path,
        manifest_path,
        target_dir,
        pkg_name,
        py_env,
    })
}

fn resolve_python_env() -> Result<PythonBuildEnv, CompilerError> {
    let report = inspect_environment();
    let Some(idx) = report.preferred_python_index else {
        return Err(CompilerError::NoCompatiblePython {
            detail: "no Python 3 installation found (run `spy doctor` for details)".to_owned(),
        });
    };
    let best = &report.python_reports[idx];
    if !matches!(
        best.status,
        CompatibilityStatus::Ready | CompatibilityStatus::ReadyWithForwardCompat
    ) {
        return Err(CompilerError::NoCompatiblePython {
            detail: format!(
                "preferred Python `{}` is `{}` ({}); run `spy doctor` to inspect prerequisites",
                best.python.executable.display(),
                best.status.label(),
                best.notes.join("; ")
            ),
        });
    }

    Ok(PythonBuildEnv {
        python_exe: best.python.executable.clone(),
        lib_dir: best.python.lib_dir.clone(),
        needs_forward_compat: best.status == CompatibilityStatus::ReadyWithForwardCompat,
    })
}

fn locate_spy_runtime_dir() -> Result<PathBuf, CompilerError> {
    if let Some(env_path) = env::var_os("SPY_RUNTIME_PATH") {
        let p = PathBuf::from(env_path);
        if p.join("Cargo.toml").is_file() {
            return Ok(p);
        }
    }

    if let Ok(cwd) = env::current_dir() {
        for ancestor in cwd.ancestors() {
            let candidate = ancestor.join("crates/spy-runtime");
            if candidate.join("Cargo.toml").is_file() {
                return Ok(candidate);
            }
        }
    }

    let manifest_fallback = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(|p| p.join("spy-runtime"));
    if let Some(p) = manifest_fallback
        && p.join("Cargo.toml").is_file()
    {
        return Ok(p);
    }

    Err(CompilerError::MissingSpyRuntimeCrate)
}

fn spy_target_cache_dir() -> Result<PathBuf, CompilerError> {
    if let Some(dir) = env::var_os("SPY_TARGET_DIR") {
        let path = PathBuf::from(dir);
        fs::create_dir_all(&path)?;
        return Ok(path);
    }

    let cache_base = env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")));
    if let Some(base) = cache_base {
        let candidate = base.join("spy/target");
        if fs::create_dir_all(&candidate).is_ok() {
            return Ok(candidate);
        }
    }

    let fallback = env::temp_dir().join("spy-cargo-target");
    fs::create_dir_all(&fallback)?;
    Ok(fallback)
}

fn sanitize_package_name(input_path: &Path) -> String {
    let stem = input_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("spy_app");
    let mut cleaned = String::from("spy_");
    for ch in stem.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            cleaned.push(ch.to_ascii_lowercase());
        } else {
            cleaned.push('_');
        }
    }
    cleaned
}

fn toml_string(value: &str) -> String {
    format!("{value:?}")
}

fn prepend_ld_library_path(dir: &Path) -> std::ffi::OsString {
    let mut paths = vec![dir.to_path_buf()];
    if let Some(existing) = env::var_os("LD_LIBRARY_PATH") {
        paths.extend(env::split_paths(&existing));
    }
    env::join_paths(paths).unwrap_or_else(|_| dir.as_os_str().to_os_string())
}

fn require_rustc() -> Result<PathBuf, CompilerError> {
    let rust = detect_rust_toolchain();
    rust.rustc
        .map(|tool| tool.path)
        .ok_or(CompilerError::MissingRustc)
}

fn require_cargo() -> Result<PathBuf, CompilerError> {
    let rust = detect_rust_toolchain();
    rust.cargo
        .map(|tool| tool.path)
        .ok_or(CompilerError::MissingCargo)
}

fn compile_with_rustc(
    rustc: &Path,
    source_path: &Path,
    output_path: &Path,
    release: bool,
    edition: &str,
) -> Result<(), CompilerError> {
    let mut cmd = Command::new(rustc);
    cmd.arg("--edition").arg(edition);
    if release {
        cmd.arg("-C").arg("opt-level=3");
    }
    let output = cmd.arg(source_path).arg("-o").arg(output_path).output()?;
    if !output.status.success() {
        return Err(CompilerError::RustcFailed {
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        });
    }
    Ok(())
}

fn source_filename(path: &Path) -> String {
    path.file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("main.rs")
        .to_owned()
}

fn default_output_path(input_path: &Path) -> Result<PathBuf, CompilerError> {
    let stem = input_path
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| CompilerError::InvalidInputPath {
            path: input_path.to_path_buf(),
        })?;
    Ok(std::env::current_dir()?.join(stem))
}

struct TempDir(PathBuf);

impl TempDir {
    fn new(prefix: &str) -> Result<Self, CompilerError> {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!("{prefix}-{}-{nanos}", std::process::id()));
        fs::create_dir_all(&dir)?;
        Ok(Self(dir))
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
