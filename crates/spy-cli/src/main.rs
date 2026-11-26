use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use spy_compiler::{
    build_file, check_file, inspect_environment, parse_and_translate, run_file, BuildOptions,
    EnvironmentReport,
};
use spy_runtime::{probe_cpython_c_embedding, CompatibilityStatus};

#[derive(Parser)]
#[command(
    name = "spy",
    version,
    about = "SPY (Super Python) — Rust and Python in one .rs file"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Compile and run a .rs source file
    Run {
        /// Path to the .rs source file
        file: PathBuf,
        /// Build with optimizations enabled (-C opt-level=3)
        #[arg(long)]
        release: bool,
        /// Rust edition to compile with
        #[arg(long)]
        edition: Option<String>,
        /// Arguments forwarded to the compiled program
        #[arg(last = true)]
        args: Vec<String>,
    },
    /// Compile a .rs source file into an executable
    Build {
        /// Path to the .rs source file
        file: PathBuf,
        /// Build with optimizations enabled (-C opt-level=3)
        #[arg(long)]
        release: bool,
        /// Output executable path
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Rust edition to compile with
        #[arg(long)]
        edition: Option<String>,
        /// Print generated Rust code before compiling
        #[arg(long)]
        emit_rust: bool,
    },
    /// Parse and type-check a .rs source file without emitting a binary
    Check {
        /// Path to the .rs source file
        file: PathBuf,
        /// Rust edition to check with
        #[arg(long)]
        edition: Option<String>,
    },
    /// Inspect Rust, C/C++/Zig, Python, Linux/WSL, and PyO3 compatibility
    Doctor {
        /// Compile and execute a live CPython C-API embedding probe
        #[arg(long)]
        probe: bool,
    },
    /// Inspect lexed tokens and detected Python imports in a .rs file
    Tokens {
        /// Path to the .rs source file
        file: PathBuf,
    },
    /// Emit generated Rust source for a .rs SPY file without compiling
    Emit {
        /// Path to the .rs source file
        file: PathBuf,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match execute(cli) {
        Ok(code) => code,
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::FAILURE
        }
    }
}

fn execute(cli: Cli) -> Result<ExitCode, Box<dyn std::error::Error>> {
    match cli.command {
        Commands::Run {
            file,
            release,
            edition,
            args,
        } => {
            let options = BuildOptions {
                release,
                output: None,
                edition,
            };
            let code = run_file(&file, &options, &args)?;
            Ok(ExitCode::from(u8::try_from(code).unwrap_or(1)))
        }
        Commands::Build {
            file,
            release,
            output,
            edition,
            emit_rust,
        } => {
            let options = BuildOptions {
                release,
                output,
                edition,
            };
            let res = build_file(&file, &options)?;
            if emit_rust {
                println!("{}", res.generated.rust_source);
            }
            println!(
                "built `{}` -> `{}`",
                res.input_path.display(),
                res.output_binary.display()
            );
            Ok(ExitCode::SUCCESS)
        }
        Commands::Check { file, edition } => {
            let artifact = check_file(&file, edition.as_deref())?;
            if artifact.requires_python_runtime {
                println!(
                    "ok: `{}` passed SPY check ({} Python import(s))",
                    file.display(),
                    artifact.python_imports.len()
                );
            } else {
                println!("ok: `{}` passed SPY check", file.display());
            }
            Ok(ExitCode::SUCCESS)
        }
        Commands::Doctor { probe } => {
            let report = inspect_environment();
            print_doctor_report(&report);
            if probe {
                run_c_probe(&report)?;
            }
            Ok(ExitCode::SUCCESS)
        }
        Commands::Tokens { file } => {
            let src = std::fs::read_to_string(&file)?;
            let parsed = spy_parser::parse_source(&src)?;
            println!("tokens: {}", parsed.tokens.len());
            for imp in &parsed.python_imports {
                let loc = parsed.location_of(imp.span);
                println!(
                    "python import @ {loc}: {} as {}",
                    imp.module_path.join("::"),
                    imp.alias
                );
            }
            for tok in &parsed.tokens {
                let loc = parsed.location_of(tok.span);
                println!("  {loc:<8} {:?} `{}`", tok.kind, tok.lexeme);
            }
            Ok(ExitCode::SUCCESS)
        }
        Commands::Emit { file } => {
            let (_, generated) = parse_and_translate(&file)?;
            print!("{}", generated.rust_source);
            Ok(ExitCode::SUCCESS)
        }
    }
}

fn print_doctor_report(report: &EnvironmentReport) {
    println!("SPY Environment Doctor (Phase 0)");
    println!("================================");
    println!(
        "Platform : {}/{} (WSL: {}{}{})",
        report.platform.os,
        report.platform.arch,
        if report.platform.is_wsl { "yes" } else { "no" },
        report
            .platform
            .wsl_distro
            .as_deref()
            .map(|d| format!(", distro: {d}"))
            .unwrap_or_default(),
        report
            .platform
            .kernel_release
            .as_deref()
            .map(|k| format!(", kernel: {k}"))
            .unwrap_or_default(),
    );

    println!("\nRust Toolchain:");
    print_tool("rustc", report.rust.rustc.as_ref());
    print_tool("cargo", report.rust.cargo.as_ref());
    if let Some(tc) = &report.rust.active_toolchain {
        println!("  rustup  : {tc}");
    }

    println!("\nNative Toolchains:");
    print_tool("C", report.native.c_compiler.as_ref());
    print_tool("C++", report.native.cpp_compiler.as_ref());
    if let Some(zig) = &report.native.zig {
        println!("  zig     : {} ({})", zig.version, zig.path.display());
    } else if let Some(note) = &report.native.zig_note {
        println!("  zig     : unavailable ({note})");
    } else {
        println!("  zig     : not found");
    }

    println!("\nPython & PyO3 Compatibility:");
    if report.python_reports.is_empty() {
        println!("  no Python 3 installations detected");
        return;
    }

    for (idx, py_report) in report.python_reports.iter().enumerate() {
        let marker = if report.preferred_python_index == Some(idx) {
            "*"
        } else {
            " "
        };
        let py = &py_report.python;
        println!(
            "  {marker} Python {} [{}] ({}) -> {}",
            py.short_version(),
            py.source.label(),
            py.executable.display(),
            py_report.status.label()
        );
        println!(
            "      headers: {} | libpython: {} | shared: {}",
            if py.has_python_h { "yes" } else { "no" },
            py.linkable_libpython
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "missing".to_owned()),
            if py.shared_enabled { "yes" } else { "no" }
        );
        for note in &py_report.notes {
            println!("      note: {note}");
        }
        if !py_report.recommended_env.is_empty() {
            let envs: Vec<String> = py_report
                .recommended_env
                .iter()
                .map(|(k, v)| format!("{k}={v}"))
                .collect();
            println!("      env : {}", envs.join(" "));
        }
    }
}

fn print_tool(label: &str, tool: Option<&spy_compiler::ToolInfo>) {
    match tool {
        Some(t) => println!("  {label:<7} : {} ({})", t.version, t.path.display()),
        None => println!("  {label:<7} : not found"),
    }
}

fn run_c_probe(report: &EnvironmentReport) -> Result<(), Box<dyn std::error::Error>> {
    println!("\nCPython Embedding Probe:");
    let Some(c_tool) = &report.native.c_compiler else {
        return Err("cannot run CPython probe without a working C compiler".into());
    };
    let Some(idx) = report.preferred_python_index else {
        return Err("no Python installation detected for probing".into());
    };
    let best = &report.python_reports[idx];
    if matches!(
        best.status,
        CompatibilityStatus::MissingPrerequisites | CompatibilityStatus::Incompatible
    ) {
        return Err(format!(
            "preferred Python `{}` is `{}` and cannot run embedding probe",
            best.python.executable.display(),
            best.status.label()
        )
        .into());
    }

    let result = probe_cpython_c_embedding(&best.python, &c_tool.path)?;
    println!(
        "  ok: embedded CPython {} using {} ({})",
        result.python_version,
        result.compiler.display(),
        result.embedded_version_output
    );
    Ok(())
}
