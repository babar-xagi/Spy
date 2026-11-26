use std::collections::HashSet;

use spy_parser::{Location, ParsedProgram, PythonImport, Span};
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
    #[error("duplicate Python import binding `{alias}` at {location}")]
    DuplicatePythonAlias { alias: String, location: Location },
}

/// Translates a parsed SPY unit into valid Rust code backed by `spy_runtime`.
///
/// Pure Rust programs are preserved verbatim so `rustc` diagnostics align 1:1.
/// Programs with `use python::...;` declarations lower each import into a cached
/// `spy_runtime::LazyPyModule` constant while preserving line numbers.
pub fn generate_rust(program: &ParsedProgram) -> Result<GeneratedArtifact, CodegenError> {
    if program.python_imports.is_empty() {
        return Ok(GeneratedArtifact {
            rust_source: program.source.clone(),
            requires_python_runtime: false,
            python_imports: Vec::new(),
        });
    }

    let mut seen_aliases = HashSet::new();
    for imp in &program.python_imports {
        if !seen_aliases.insert(imp.alias.clone()) {
            return Err(CodegenError::DuplicatePythonAlias {
                alias: imp.alias.clone(),
                location: program.location_of(imp.span),
            });
        }
    }

    let mut edits: Vec<(Span, String)> = Vec::new();
    for stmt in &program.python_import_statements {
        let original = &program.source[stmt.span.start..stmt.span.end];
        let original_newlines = original.bytes().filter(|&b| b == b'\n').count();
        let mut emitted = stmt
            .imports
            .iter()
            .map(|imp| {
                let dotted = imp.dotted_name();
                format!(
                    "#[allow(non_upper_case_globals)] const {alias}: spy_runtime::LazyPyModule = spy_runtime::LazyPyModule::new({dotted:?});",
                    alias = imp.alias,
                )
            })
            .collect::<Vec<_>>()
            .join(" ");
        for _ in 0..original_newlines {
            emitted.push('\n');
        }
        edits.push((stmt.span, emitted));
    }

    if let Some(brace_end) = program.main_body_open_brace {
        let mut unique_modules = Vec::new();
        let mut seen_modules = HashSet::new();
        for imp in &program.python_imports {
            let dotted = imp.dotted_name();
            if seen_modules.insert(dotted.clone()) {
                unique_modules.push(format!("{dotted:?}"));
            }
        }
        let preload = format!(
            " spy_runtime::preload_or_exit(&[{}]);",
            unique_modules.join(", ")
        );
        edits.push((Span::new(brace_end, brace_end), preload));
    }

    let rust_source = apply_edits(&program.source, edits);

    Ok(GeneratedArtifact {
        rust_source,
        requires_python_runtime: true,
        python_imports: program.python_imports.clone(),
    })
}

fn apply_edits(source: &str, mut edits: Vec<(Span, String)>) -> String {
    edits.sort_by_key(|(span, _)| span.start);
    let mut out = String::with_capacity(source.len() + 256);
    let mut cursor = 0usize;
    for (span, replacement) in edits {
        if span.start > cursor {
            out.push_str(&source[cursor..span.start]);
        }
        out.push_str(&replacement);
        cursor = span.end;
    }
    if cursor < source.len() {
        out.push_str(&source[cursor..]);
    }
    out
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
    fn lowers_python_imports_and_preserves_line_numbers() {
        let src = "\
use python::math;
use python::{
    json,
    os::path as ospath,
};

fn main() {
    let _ = (math, json, ospath);
}
";
        let parsed = parse_source(src).unwrap();
        let artifact = generate_rust(&parsed).unwrap();
        assert!(artifact.requires_python_runtime);
        assert!(artifact
            .rust_source
            .contains("const math: spy_runtime::LazyPyModule = spy_runtime::LazyPyModule::new(\"math\");"));
        assert!(artifact
            .rust_source
            .contains("const ospath: spy_runtime::LazyPyModule = spy_runtime::LazyPyModule::new(\"os.path\");"));
        assert!(artifact
            .rust_source
            .contains("spy_runtime::preload_or_exit(&[\"math\", \"json\", \"os.path\"]);"));
        assert_eq!(
            src.lines().count(),
            artifact.rust_source.lines().count(),
            "line numbers must align 1:1 after codegen"
        );
    }

    #[test]
    fn rejects_duplicate_python_aliases() {
        let src = "use python::math as m;\nuse python::json as m;\nfn main() {}\n";
        let parsed = parse_source(src).unwrap();
        let err = generate_rust(&parsed).unwrap_err();
        assert!(err.to_string().contains("duplicate Python import binding `m`"));
    }
}
