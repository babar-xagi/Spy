use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use spy_codegen::{generate_rust, GeneratedArtifact};
use spy_parser::{parse_source, ParsedProgram};

use crate::env::detect_rust_toolchain;
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

/// Parses and translates a `.rs` source file without invoking `rustc`.
pub fn parse_and_translate(input_path: &Path) -> Result<(ParsedProgram, GeneratedArtifact), CompilerError> {
    let source = fs::read_to_string(input_path).map_err(|err| CompilerError::ReadSource {
        path: input_path.to_path_buf(),
        source: err,
    })?;
    let parsed = parse_source(&source)?;
    let generated = generate_rust(&parsed)?;
    Ok((parsed, generated))
}

/// Type-checks a `.rs` file using `rustc --emit=metadata` in a temporary directory.
pub fn check_file(input_path: &Path, edition: Option<&str>) -> Result<GeneratedArtifact, CompilerError> {
    let (_, generated) = parse_and_translate(input_path)?;
    let rustc = require_rustc()?;
    let temp = TempDir::new("spy-check")?;
    let src_file = temp.path().join(source_filename(input_path));
    let out_meta = temp.path().join("libcheck.rmeta");
    fs::write(&src_file, &generated.rust_source)?;

    let edition = edition.unwrap_or("2024");
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

    Ok(generated)
}

/// Compiles a `.rs` file into an executable binary.
pub fn build_file(input_path: &Path, options: &BuildOptions) -> Result<CompileResult, CompilerError> {
    let (_, generated) = parse_and_translate(input_path)?;
    let rustc = require_rustc()?;
    let temp = TempDir::new("spy-build")?;
    let src_file = temp.path().join(source_filename(input_path));
    fs::write(&src_file, &generated.rust_source)?;

    let output_binary = match &options.output {
        Some(path) => path.clone(),
        None => default_output_path(input_path)?,
    };
    if let Some(parent) = output_binary.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)?;
    }

    compile_with_rustc(
        &rustc,
        &src_file,
        &output_binary,
        options.release,
        options.edition.as_deref().unwrap_or("2024"),
    )?;

    Ok(CompileResult {
        input_path: input_path.to_path_buf(),
        output_binary,
        generated,
    })
}

/// Compiles a `.rs` file to a temporary executable and runs it, returning its exit code.
pub fn run_file(
    input_path: &Path,
    options: &BuildOptions,
    program_args: &[String],
) -> Result<i32, CompilerError> {
    let (_, generated) = parse_and_translate(input_path)?;
    let rustc = require_rustc()?;
    let temp = TempDir::new("spy-run")?;
    let src_file = temp.path().join(source_filename(input_path));
    let bin_name = input_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("spy_app");
    let bin_path = temp.path().join(bin_name);
    fs::write(&src_file, &generated.rust_source)?;

    compile_with_rustc(
        &rustc,
        &src_file,
        &bin_path,
        options.release,
        options.edition.as_deref().unwrap_or("2024"),
    )?;

    let status = Command::new(&bin_path).args(program_args).status()?;
    Ok(status.code().unwrap_or(1))
}

fn require_rustc() -> Result<PathBuf, CompilerError> {
    let rust = detect_rust_toolchain();
    rust.rustc
        .map(|tool| tool.path)
        .ok_or(CompilerError::MissingRustc)
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
