pub mod ast;
pub mod lexer;
pub mod parser;

pub use ast::{LineIndex, Location, ParsedProgram, PythonImport, Span};
pub use lexer::{tokenize, SingleQuoteKind, Token, TokenKind};
pub use parser::parse_source;

use thiserror::Error;

/// Errors emitted during lexing or parsing a SPY `.rs` source file.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ParseError {
    #[error("lexical error at {location}: unexpected token `{snippet}`")]
    LexError { location: Location, snippet: String },

    #[error("syntax error at {location}: {message}")]
    SyntaxError { location: Location, message: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lexes_rust_lifetimes_and_chars_distinctly() {
        let src = r#"
            fn pick<'a>(s: &'a str, c: char) -> char {
                let quote = '\'';
                let nl = '\n';
                let _ = quote;
                let _ = nl;
                let _ = s;
                c
            }
        "#;
        let (tokens, _) = tokenize(src).expect("should tokenize cleanly");
        assert!(tokens.iter().any(|t| {
            t.kind == TokenKind::CharOrLifetime(SingleQuoteKind::Lifetime) && t.lexeme == "'a"
        }));
        assert!(tokens.iter().any(|t| {
            t.kind == TokenKind::CharOrLifetime(SingleQuoteKind::CharLit) && t.lexeme == "'\\''"
        }));
    }

    #[test]
    fn lexes_raw_strings_nested_comments_and_ranges() {
        let src = r##"
            /* outer /* inner */ comment */
            fn main() {
                let raw = r#"hello "world""#;
                for i in 0..10 {
                    let _ = (raw, i);
                }
            }
        "##;
        let program = parse_source(src).expect("should parse cleanly");
        assert!(program.has_main_fn);
        assert!(program
            .tokens
            .iter()
            .any(|t| t.kind == TokenKind::RawStringLit));
        assert!(program.tokens.iter().any(|t| t.kind == TokenKind::DotDot));
    }

    #[test]
    fn detects_python_imports_and_aliases() {
        let src = r#"
            use std::collections::HashMap;
            use python::pandas as pd;
            use python::os::path as ospath;
            use python::math;
            use python::{json, sys as pysys};

            fn main() {}
        "#;
        let program = parse_source(src).expect("should parse python imports");
        assert_eq!(program.rust_use_count, 1);
        assert_eq!(program.python_imports.len(), 5);
        assert_eq!(program.python_imports[0].module_path, vec!["pandas"]);
        assert_eq!(program.python_imports[0].alias, "pd");
        assert_eq!(program.python_imports[1].module_path, vec!["os", "path"]);
        assert_eq!(program.python_imports[1].alias, "ospath");
        assert_eq!(program.python_imports[2].module_path, vec!["math"]);
        assert_eq!(program.python_imports[2].alias, "math");
        assert_eq!(program.python_imports[3].alias, "json");
        assert_eq!(program.python_imports[4].alias, "pysys");
    }
}
