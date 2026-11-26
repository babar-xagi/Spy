use crate::ast::{ParsedProgram, PythonImport, PythonImportStatement, Span};
use crate::lexer::{tokenize, Token, TokenKind};
use crate::ParseError;

/// Parses a SPY `.rs` source file into tokens, `use python::...` declarations, and metadata.
pub fn parse_source(source: &str) -> Result<ParsedProgram, ParseError> {
    let (tokens, line_index) = tokenize(source)?;
    let mut python_imports = Vec::new();
    let mut python_import_statements = Vec::new();
    let mut rust_use_count = 0usize;
    let mut has_main_fn = false;
    let mut main_body_open_brace = None;

    let mut i = 0usize;
    while i < tokens.len() {
        if tokens[i].kind == TokenKind::KwFn
            && let Some(name) = tokens.get(i + 1)
            && name.kind == TokenKind::Ident
            && name.lexeme == "main"
        {
            has_main_fn = true;
            if main_body_open_brace.is_none() {
                main_body_open_brace = find_fn_body_open_brace(&tokens, i + 2);
            }
        }

        if tokens[i].kind == TokenKind::KwUse {
            if let Some((stmt, next_idx)) = try_parse_python_use(&tokens, i, &line_index)? {
                python_imports.extend(stmt.imports.clone());
                python_import_statements.push(stmt);
                i = next_idx;
                continue;
            } else {
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
        python_import_statements,
        rust_use_count,
        has_main_fn,
        main_body_open_brace,
    })
}

fn find_fn_body_open_brace(tokens: &[Token], mut idx: usize) -> Option<usize> {
    let mut paren_depth = 0usize;
    let mut seen_params = false;

    while let Some(tok) = tokens.get(idx) {
        match tok.kind {
            TokenKind::LParen => {
                seen_params = true;
                paren_depth += 1;
            }
            TokenKind::RParen => {
                paren_depth = paren_depth.saturating_sub(1);
            }
            TokenKind::LBrace if seen_params && paren_depth == 0 => {
                return Some(tok.span.end);
            }
            TokenKind::Semi if !seen_params || paren_depth == 0 => {
                return None;
            }
            _ => {}
        }
        idx += 1;
    }
    None
}

fn try_parse_python_use(
    tokens: &[Token],
    use_idx: usize,
    line_index: &crate::ast::LineIndex,
) -> Result<Option<(PythonImportStatement, usize)>, ParseError> {
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
                let mut module_path = prefix.clone();
                let mut item_end;
                loop {
                    let Some(seg) = tokens.get(idx) else {
                        return Err(ParseError::SyntaxError {
                            location: line_index.location(tokens[use_idx].span.start),
                            message: "unterminated `use python::{...}` import".to_owned(),
                        });
                    };
                    if seg.kind != TokenKind::Ident {
                        return Err(ParseError::SyntaxError {
                            location: line_index.location(seg.span.start),
                            message: format!(
                                "expected Python module identifier inside `{{...}}`, found `{}`",
                                seg.lexeme
                            ),
                        });
                    }
                    module_path.push(seg.lexeme.clone());
                    item_end = seg.span.end;
                    idx += 1;
                    if tokens.get(idx).map(|t| t.kind) == Some(TokenKind::ColonColon) {
                        idx += 1;
                    } else {
                        break;
                    }
                }

                let mut alias = module_path.last().cloned().unwrap_or_default();
                if tokens.get(idx).map(|t| t.kind) == Some(TokenKind::KwAs) {
                    let (alias_name, alias_end, next) = parse_alias(tokens, idx, line_index)?;
                    alias = alias_name;
                    item_end = alias_end;
                    idx = next;
                }

                imports.push(PythonImport {
                    module_path,
                    alias,
                    span: Span::new(tokens[use_idx].span.start, item_end),
                });

                if tokens.get(idx).map(|t| t.kind) == Some(TokenKind::Comma) {
                    idx += 1;
                }
            }

            let Some(semi) = tokens.get(idx).filter(|t| t.kind == TokenKind::Semi) else {
                let loc = tokens
                    .get(idx)
                    .map(|t| line_index.location(t.span.start))
                    .unwrap_or_else(|| line_index.location(tokens[use_idx].span.start));
                return Err(ParseError::SyntaxError {
                    location: loc,
                    message: "expected `;` after `use python::{...}` import".to_owned(),
                });
            };
            if imports.is_empty() {
                return Err(ParseError::SyntaxError {
                    location: line_index.location(tokens[use_idx].span.start),
                    message: "empty `use python::{}` import list".to_owned(),
                });
            }
            let stmt = PythonImportStatement {
                span: Span::new(tokens[use_idx].span.start, semi.span.end),
                imports,
            };
            return Ok(Some((stmt, idx + 1)));
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
        let end = tok.span.end;
        idx += 1;

        match tokens.get(idx).map(|t| t.kind) {
            Some(TokenKind::ColonColon) => {
                idx += 1;
            }
            Some(TokenKind::KwAs) => {
                let (alias, alias_end, next) = parse_alias(tokens, idx, line_index)?;
                idx = next;

                let Some(semi) = tokens.get(idx).filter(|t| t.kind == TokenKind::Semi) else {
                    let loc = tokens
                        .get(idx)
                        .map(|t| line_index.location(t.span.start))
                        .unwrap_or_else(|| line_index.location(alias_end));
                    return Err(ParseError::SyntaxError {
                        location: loc,
                        message: "expected `;` at end of Python import".to_owned(),
                    });
                };

                let span = Span::new(tokens[use_idx].span.start, semi.span.end);
                imports.push(PythonImport {
                    module_path: prefix,
                    alias,
                    span,
                });
                return Ok(Some((PythonImportStatement { span, imports }, idx + 1)));
            }
            Some(TokenKind::Semi) => {
                let alias = prefix.last().cloned().unwrap_or_default();
                let span = Span::new(tokens[use_idx].span.start, tokens[idx].span.end);
                imports.push(PythonImport {
                    module_path: prefix,
                    alias,
                    span,
                });
                return Ok(Some((PythonImportStatement { span, imports }, idx + 1)));
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

fn parse_alias(
    tokens: &[Token],
    as_idx: usize,
    line_index: &crate::ast::LineIndex,
) -> Result<(String, usize, usize), ParseError> {
    let Some(alias_tok) = tokens.get(as_idx + 1) else {
        return Err(ParseError::SyntaxError {
            location: line_index.location(tokens[as_idx].span.end),
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
    Ok((alias_tok.lexeme.clone(), alias_tok.span.end, as_idx + 2))
}
