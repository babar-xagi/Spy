use logos::{Lexer, Logos};

use crate::ast::{LineIndex, Location, Span};
use crate::ParseError;

/// Lexical token kinds produced for SPY `.rs` source files.
#[derive(Logos, Debug, Clone, Copy, PartialEq, Eq)]
#[logos(skip r"[ \t\r\n\f]+")]
#[logos(skip r"//[^\n]*")]
pub enum TokenKind {
    // Nested block comments are skipped via callback.
    #[token("/*", skip_block_comment)]
    BlockCommentSkip,

    // Keywords
    #[token("as")]
    KwAs,
    #[token("fn")]
    KwFn,
    #[token("let")]
    KwLet,
    #[token("mut")]
    KwMut,
    #[token("pub")]
    KwPub,
    #[token("use")]
    KwUse,
    #[token("for")]
    KwFor,
    #[token("in")]
    KwIn,
    #[token("if")]
    KwIf,
    #[token("else")]
    KwElse,
    #[token("while")]
    KwWhile,
    #[token("loop")]
    KwLoop,
    #[token("match")]
    KwMatch,
    #[token("return")]
    KwReturn,
    #[token("struct")]
    KwStruct,
    #[token("enum")]
    KwEnum,
    #[token("impl")]
    KwImpl,
    #[token("mod")]
    KwMod,
    #[token("trait")]
    KwTrait,
    #[token("const")]
    KwConst,
    #[token("static")]
    KwStatic,
    #[token("type")]
    KwType,
    #[token("where")]
    KwWhere,
    #[token("true")]
    KwTrue,
    #[token("false")]
    KwFalse,

    // Identifiers and literals
    #[regex(r"(r#)?[A-Za-z_][A-Za-z0-9_]*")]
    Ident,

    #[regex(r"0[xX][0-9a-fA-F_]+(_?([iu](8|16|32|64|128|size)))?")]
    #[regex(r"0[bB][01_]+(_?([iu](8|16|32|64|128|size)))?")]
    #[regex(r"0[oO][0-7_]+(_?([iu](8|16|32|64|128|size)))?")]
    #[regex(r"[0-9][0-9_]*(\.[0-9][0-9_]*)?([eE][+-]?[0-9_]+)?(_?([iu](8|16|32|64|128|size)|f32|f64))?")]
    Number,

    #[regex(r#"[bc]?""#, lex_quoted_string)]
    StringLit,

    #[regex(r#"b?r#*""#, lex_raw_string)]
    RawStringLit,

    #[token("'", lex_single_quote)]
    #[token("b'", lex_byte_char)]
    CharOrLifetime(SingleQuoteKind),

    // Multi-character operators and punctuation
    #[token("::")]
    ColonColon,
    #[token("->")]
    Arrow,
    #[token("=>")]
    FatArrow,
    #[token("..=")]
    DotDotEq,
    #[token("..")]
    DotDot,
    #[token("==")]
    EqEq,
    #[token("!=")]
    BangEq,
    #[token("<=")]
    LtEq,
    #[token(">=")]
    GtEq,
    #[token("&&")]
    AndAnd,
    #[token("||")]
    OrOr,
    #[token("<<=")]
    ShlEq,
    #[token(">>=")]
    ShrEq,
    #[token("<<")]
    Shl,
    #[token(">>")]
    Shr,
    #[token("+=")]
    PlusEq,
    #[token("-=")]
    MinusEq,
    #[token("*=")]
    StarEq,
    #[token("/=")]
    SlashEq,
    #[token("%=")]
    PercentEq,
    #[token("&=")]
    AndEq,
    #[token("|=")]
    OrEq,
    #[token("^=")]
    CaretEq,

    // Single-character delimiters and operators
    #[token("{")]
    LBrace,
    #[token("}")]
    RBrace,
    #[token("(")]
    LParen,
    #[token(")")]
    RParen,
    #[token("[")]
    LBracket,
    #[token("]")]
    RBracket,
    #[token(";")]
    Semi,
    #[token(":")]
    Colon,
    #[token(",")]
    Comma,
    #[token(".")]
    Dot,
    #[token("=")]
    Eq,
    #[token("+")]
    Plus,
    #[token("-")]
    Minus,
    #[token("*")]
    Star,
    #[token("/")]
    Slash,
    #[token("%")]
    Percent,
    #[token("!")]
    Bang,
    #[token("<")]
    Lt,
    #[token(">")]
    Gt,
    #[token("&")]
    And,
    #[token("|")]
    Or,
    #[token("^")]
    Caret,
    #[token("~")]
    Tilde,
    #[token("@")]
    At,
    #[token("#")]
    Pound,
    #[token("$")]
    Dollar,
    #[token("?")]
    Question,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SingleQuoteKind {
    CharLit,
    Lifetime,
}

/// A token paired with its source text slice and byte span.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub kind: TokenKind,
    pub lexeme: String,
    pub span: Span,
}

fn skip_block_comment(lex: &mut Lexer<'_, TokenKind>) -> logos::Skip {
    let bytes = lex.remainder().as_bytes();
    let mut depth = 1usize;
    let mut i = 0usize;
    while i + 1 < bytes.len() {
        if bytes[i] == b'/' && bytes[i + 1] == b'*' {
            depth += 1;
            i += 2;
        } else if bytes[i] == b'*' && bytes[i + 1] == b'/' {
            depth -= 1;
            i += 2;
            if depth == 0 {
                lex.bump(i);
                return logos::Skip;
            }
        } else {
            i += 1;
        }
    }
    lex.bump(bytes.len());
    logos::Skip
}

fn lex_quoted_string(lex: &mut Lexer<'_, TokenKind>) -> bool {
    let bytes = lex.remainder().as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => {
                i += 2;
            }
            b'"' => {
                lex.bump(i + 1);
                return true;
            }
            _ => {
                i += 1;
            }
        }
    }
    false
}

fn lex_raw_string(lex: &mut Lexer<'_, TokenKind>) -> bool {
    let hashes = lex.slice().bytes().filter(|&b| b == b'#').count();
    let remainder = lex.remainder().as_bytes();
    let mut i = 0usize;
    while i < remainder.len() {
        if remainder[i] == b'"' {
            let close_start = i + 1;
            if close_start + hashes <= remainder.len()
                && remainder[close_start..close_start + hashes]
                    .iter()
                    .all(|&b| b == b'#')
            {
                lex.bump(close_start + hashes);
                return true;
            }
        }
        i += 1;
    }
    false
}

fn try_consume_char_body(remainder: &str) -> Option<usize> {
    let bytes = remainder.as_bytes();
    if bytes.is_empty() {
        return None;
    }
    if bytes[0] == b'\\' {
        // Simple escape or hex/unicode escape ending in closing quote.
        let mut i = 2usize;
        if bytes.len() < 3 {
            return None;
        }
        if bytes[1] == b'u' && bytes.get(2) == Some(&b'{') {
            i = 3;
            while i < bytes.len() && bytes[i] != b'}' {
                i += 1;
            }
            if i >= bytes.len() {
                return None;
            }
            i += 1;
        } else if bytes[1] == b'x' {
            if bytes.len() < 5 {
                return None;
            }
            i = 4;
        }
        if bytes.get(i) == Some(&b'\'') {
            return Some(i + 1);
        }
        return None;
    }

    let mut chars = remainder.char_indices();
    let (_, ch) = chars.next()?;
    if ch == '\'' || ch == '\n' || ch == '\r' {
        return None;
    }
    let (next_idx, next_ch) = chars.next()?;
    if next_ch == '\'' {
        Some(next_idx + 1)
    } else {
        None
    }
}

fn lex_single_quote(lex: &mut Lexer<'_, TokenKind>) -> Option<SingleQuoteKind> {
    let rem = lex.remainder();
    if let Some(len) = try_consume_char_body(rem) {
        lex.bump(len);
        return Some(SingleQuoteKind::CharLit);
    }

    let bytes = rem.as_bytes();
    if let Some(&first) = bytes.first()
        && (first.is_ascii_alphabetic() || first == b'_')
    {
        let mut len = 1usize;
        while len < bytes.len() && (bytes[len].is_ascii_alphanumeric() || bytes[len] == b'_') {
            len += 1;
        }
        lex.bump(len);
        return Some(SingleQuoteKind::Lifetime);
    }

    None
}

fn lex_byte_char(lex: &mut Lexer<'_, TokenKind>) -> Option<SingleQuoteKind> {
    let len = try_consume_char_body(lex.remainder())?;
    lex.bump(len);
    Some(SingleQuoteKind::CharLit)
}

/// Tokenizes a `.rs` source string into tokens with spans.
pub fn tokenize(source: &str) -> Result<(Vec<Token>, LineIndex), ParseError> {
    let line_index = LineIndex::new(source);
    let mut tokens = Vec::new();
    let mut lexer = TokenKind::lexer(source);

    while let Some(next) = lexer.next() {
        let range = lexer.span();
        let span = Span::new(range.start, range.end);
        match next {
            Ok(TokenKind::BlockCommentSkip) => {}
            Ok(kind) => {
                tokens.push(Token {
                    kind,
                    lexeme: lexer.slice().to_owned(),
                    span,
                });
            }
            Err(()) => {
                let location: Location = line_index.location(span.start);
                return Err(ParseError::LexError {
                    location,
                    snippet: lexer.slice().to_owned(),
                });
            }
        }
    }

    Ok((tokens, line_index))
}
