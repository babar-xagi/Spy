use spy_parser::{Location, ParsedProgram, PythonImport};
use thiserror::Error;

/// Generated Rust output from a parsed SPY program.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedArtifact {
    pub rust_source: String,
    pub requires_python_runtime: bool,
    pub python_imports: Vec<PythonImport>,
}

/// Errors emitted when translating SPY syntax into valid Rust code.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum CodegenError {
    #[error(
        "Python import `use python::{module}` at {location} requires SPY Phases 1-5 (PyO3 runtime & translator); Phase 0 currently compiles pure Rust `.rs` files"
    )]
    PythonTranslationNotYetImplemented { module: String, location: Location },
}

/// Translates a parsed SPY unit into Rust source code ready for `rustc`.
///
/// In Phase 0, pure Rust programs are preserved verbatim so `rustc` error spans
/// correspond 1:1 with the input `.rs` file.
pub fn generate_rust(program: &ParsedProgram) -> Result<GeneratedArtifact, CodegenError> {
    if let Some(first_import) = program.python_imports.first() {
        return Err(CodegenError::PythonTranslationNotYetImplemented {
            module: first_import.module_path.join("::"),
            location: program.location_of(first_import.span),
        });
    }

    Ok(GeneratedArtifact {
        rust_source: program.source.clone(),
        requires_python_runtime: false,
        python_imports: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use spy_parser::parse_source;

    #[test]
    fn preserves_pure_rust_source() {
        let src = "fn main() { println!(\"ok\"); }\n";
        let parsed = parse_source(src).unwrap();
        let artifact = generate_rust(&parsed).unwrap();
        assert_eq!(artifact.rust_source, src);
        assert!(!artifact.requires_python_runtime);
    }

    #[test]
    fn rejects_python_imports_with_helpful_phase_diagnostic() {
        let src = "use python::pandas as pd;\nfn main() {}\n";
        let parsed = parse_source(src).unwrap();
        let err = generate_rust(&parsed).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("pandas"));
        assert!(msg.contains("1:1"));
    }
}
