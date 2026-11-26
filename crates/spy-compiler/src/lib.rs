pub mod env;
pub mod pipeline;

pub use env::{
    detect_native_toolchains, detect_platform, detect_rust_toolchain, inspect_environment,
    EnvironmentReport, NativeToolchains, PlatformInfo, RustToolchain, ToolInfo,
};
pub use pipeline::{
    build_file, check_file, parse_and_translate, run_file, BuildOptions, CompileResult,
};

use std::path::PathBuf;
use thiserror::Error;

/// Top-level compiler pipeline errors.
#[derive(Debug, Error)]
pub enum CompilerError {
    #[error("failed to read source file `{path}`: {source}")]
    ReadSource {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("invalid input path `{path}`")]
    InvalidInputPath { path: PathBuf },

    #[error("could not find a working `rustc` executable in PATH or ~/.cargo/bin")]
    MissingRustc,

    #[error("{0}")]
    Parse(#[from] spy_parser::ParseError),

    #[error("{0}")]
    Codegen(#[from] spy_codegen::CodegenError),

    #[error("rustc compilation failed:\n{stderr}")]
    RustcFailed { stderr: String },

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}
