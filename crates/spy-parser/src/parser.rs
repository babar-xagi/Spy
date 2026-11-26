use crate::ast::{ParsedProgram, PythonImport, Span};
use crate::lexer::{tokenize, Token, TokenKind};
use crate::ParseError;

/// Parses a SPY `.rs` source file into tokens, `use python::...` declarations, and metadata.
pub fn parse_source(source: &str) -> Result<ParsedProgram, ParseError> {
    let (tokens, line_index) = tokenize(source)?;
    let mut python_imports = Vec::new();
    let mut rust_use_count = 0usize;
    let mut has_main_fn = false;

    let mut i = 0usize;
    while i < tokens.len() {
        if tokens[i].kind == TokenKind::KwFn
            && let Some(name) = tokens.get(i + 1)
            && name.kind == TokenKind::Ident
            && name.lexeme == "main"
        {
            has_main_fn = true;
        }

        // Only inspect top-level / statement-level `use` declarations.
        if tokens[i].kind == TokenKind::KwUse {
            let start_span = tokens[i].span;
            if let Some((parsed_imports, next_idx)) =
                try_parse_python_use(&tokens, i, &line_index)?
            {
                python_imports.extend(parsed_imports);
                i = next_idx;
                continue;
            } else {
                let _ = start_span;
                rust_use_count += 1;
            }
        }

        i += 1;
    }

    Ok(ParsedProgram {
        source: source.to_owned(),
        line_index,
        tokens,
        python_imports,
        rust_use_count,
        has_main_fn,
    })
}

fn try_parse_python_use(
    tokens: &[Token],
    use_idx: usize,
    line_index: &crate::ast::LineIndex,
) -> Result<Option<(Vec<PythonImport>, usize)>, ParseError> {
    let Some(first) = tokens.get(use_idx + 1) else {
        return Ok(None);
    };
    if first.kind != TokenKind::Ident || first.lexeme != "python" {
        return Ok(None);
    }

    let Some(colon) = tokens.get(use_idx + 2) else {
        return Err(ParseError::SyntaxError {
            location: line_index.location(first.span.end),
            message: "expected `::` after `use python`".to_owned(),
        });
    };
    if colon.kind != TokenKind::ColonColon {
        return Err(ParseError::SyntaxError {
            location: line_index.location(colon.span.start),
            message: format!("expected `::` after `python`, found `{}`", colon.lexeme),
        });
    }

    let mut idx = use_idx + 3;
    let mut prefix: Vec<String> = Vec::new();
    let mut imports = Vec::new();

    loop {
        let Some(tok) = tokens.get(idx) else {
            return Err(ParseError::SyntaxError {
                location: line_index.location(tokens[use_idx].span.start),
                message: "unterminated `use python::...` import".to_owned(),
            });
        };

        if tok.kind == TokenKind::LBrace {
            idx += 1;
            while let Some(item) = tokens.get(idx) {
                if item.kind == TokenKind::RBrace {
                    idx += 1;
                    break;
                }
                if item.kind != TokenKind::Ident {
                    return Err(ParseError::SyntaxError {
                        location: line_index.location(item.span.start),
                        message: format!(
                            "expected Python module identifier inside `{{...}}`, found `{}`",
                            item.lexeme
                        ),
                    });
                }
                let mut module_path = prefix.clone();
                module_path.push(item.lexeme.clone());
                let mut alias = item.lexeme.clone();
                let mut end = item.span.end;
                idx += 1;

                if tokens.get(idx).map(|t| t.kind) == Some(TokenKind::KwAs) {
                    let Some(alias_tok) = tokens.get(idx + 1) else {
                        return Err(ParseError::SyntaxError {
                            location: line_index.location(tokens[idx].span.end),
                            message: "expected alias identifier after `as`".to_owned(),
                        });
                    };
                    if alias_tok.kind != TokenKind::Ident {
                        return Err(ParseError::SyntaxError {
                            location: line_index.location(alias_tok.span.start),
                            message: format!(
                                "expected alias identifier after `as`, found `{}`",
                                alias_tok.lexeme
                            ),
                        });
                    }
                    alias = alias_tok.lexeme.clone();
                    end = alias_tok.span.end;
                    idx += 2;
                }

                imports.push(PythonImport {
                    module_path,
                    alias,
                    span: Span::new(tokens[use_idx].span.start, end),
                });

                if tokens.get(idx).map(|t| t.kind) == Some(TokenKind::Comma) {
                    idx += 1;
                }
            }

            if tokens.get(idx).map(|t| t.kind) != Some(TokenKind::Semi) {
                let loc = tokens
                    .get(idx)
                    .map(|t| line_index.location(t.span.start))
                    .unwrap_or_else(|| line_index.location(tokens[use_idx].span.start));
                return Err(ParseError::SyntaxError {
                    location: loc,
                    message: "expected `;` after `use python::{...}` import".to_owned(),
                });
            }
            return Ok(Some((imports, idx + 1)));
        }

        if tok.kind != TokenKind::Ident {
            return Err(ParseError::SyntaxError {
                location: line_index.location(tok.span.start),
                message: format!(
                    "expected Python module name after `::`, found `{}`",
                    tok.lexeme
                ),
            });
        }
        prefix.push(tok.lexeme.clone());
        let mut end = tok.span.end;
        idx += 1;

        match tokens.get(idx).map(|t| t.kind) {
            Some(TokenKind::ColonColon) => {
                idx += 1;
            }
            Some(TokenKind::KwAs) => {
                let Some(alias_tok) = tokens.get(idx + 1) else {
                    return Err(ParseError::SyntaxError {
                        location: line_index.location(tokens[idx].span.end),
                        message: "expected alias identifier after `as`".to_owned(),
                    });
                };
                if alias_tok.kind != TokenKind::Ident {
                    return Err(ParseError::SyntaxError {
                        location: line_index.location(alias_tok.span.start),
                        message: format!(
                            "expected alias identifier after `as`, found `{}`",
                            alias_tok.lexeme
                        ),
                    });
                }
                end = alias_tok.span.end;
                idx += 2;

                if tokens.get(idx).map(|t| t.kind) != Some(TokenKind::Semi) {
                    let loc = tokens
                        .get(idx)
                        .map(|t| line_index.location(t.span.start))
                        .unwrap_or_else(|| line_index.location(end));
                    return Err(ParseError::SyntaxError {
                        location: loc,
                        message: "expected `;` at end of Python import".to_owned(),
                    });
                }

                imports.push(PythonImport {
                    module_path: prefix,
                    alias: alias_tok.lexeme.clone(),
                    span: Span::new(tokens[use_idx].span.start, tokens[idx].span.end),
                });
                return Ok(Some((imports, idx + 1)));
            }
            Some(TokenKind::Semi) => {
                let alias = prefix.last().cloned().unwrap_or_default();
                imports.push(PythonImport {
                    module_path: prefix,
                    alias,
                    span: Span::new(tokens[use_idx].span.start, tokens[idx].span.end),
                });
                return Ok(Some((imports, idx + 1)));
            }
            _ => {
                let loc = tokens
                    .get(idx)
                    .map(|t| line_index.location(t.span.start))
                    .unwrap_or_else(|| line_index.location(end));
                return Err(ParseError::SyntaxError {
                    location: loc,
                    message: "expected `::`, `as`, or `;` in `use python::...`".to_owned(),
                });
            }
        }
    }
}
