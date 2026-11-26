use std::fmt;

/// Byte range in a source file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub const fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }
}

/// 1-based line and column location in a source file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Location {
    pub line: usize,
    pub column: usize,
}

impl fmt::Display for Location {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.line, self.column)
    }
}

/// Precomputed line-start byte offsets for fast span-to-location lookup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineIndex {
    line_starts: Vec<usize>,
}

impl LineIndex {
    /// Builds a `LineIndex` from source text.
    pub fn new(source: &str) -> Self {
        let mut line_starts = vec![0];
        for (idx, byte) in source.bytes().enumerate() {
            if byte == b'\n' {
                line_starts.push(idx + 1);
            }
        }
        Self { line_starts }
    }

    /// Resolves a byte offset to a 1-based line and column.
    pub fn location(&self, offset: usize) -> Location {
        let line_idx = self
            .line_starts
            .partition_point(|&start| start <= offset)
            .saturating_sub(1);
        let line_start = self.line_starts[line_idx];
        Location {
            line: line_idx + 1,
            column: offset.saturating_sub(line_start) + 1,
        }
    }
}

/// A parsed `use python::...` import declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PythonImport {
    pub module_path: Vec<String>,
    pub alias: String,
    pub span: Span,
}

/// Summary of a parsed `.rs` SPY source unit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedProgram {
    pub source: String,
    pub line_index: LineIndex,
    pub tokens: Vec<crate::Token>,
    pub python_imports: Vec<PythonImport>,
    pub rust_use_count: usize,
    pub has_main_fn: bool,
}

impl ParsedProgram {
    /// Resolves a span's starting position to a 1-based line and column.
    pub fn location_of(&self, span: Span) -> Location {
        self.line_index.location(span.start)
    }
}
