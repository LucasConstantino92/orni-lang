use std::{error::Error, fmt};

/// An error produced while processing or executing Orni source code.
///
/// Every error identifies its phase and the one-based source position where
/// it occurred. Its [`Display`](fmt::Display) implementation is intended for
/// command-line diagnostics.
#[derive(Debug, Clone, PartialEq)]
pub struct OrniError {
    phase: &'static str,
    line: usize,
    column: usize,
    message: String,
}

impl OrniError {
    /// Creates an error reported by the lexer.
    pub(crate) fn lexical(line: usize, column: usize, message: impl Into<String>) -> Self {
        Self::new("Lexical error", line, column, message)
    }

    /// Creates an error reported by the parser.
    pub(crate) fn syntax(line: usize, column: usize, message: impl Into<String>) -> Self {
        Self::new("Syntax error", line, column, message)
    }

    /// Creates an error reported while evaluating a program.
    pub(crate) fn runtime(line: usize, column: usize, message: impl Into<String>) -> Self {
        Self::new("Runtime error", line, column, message)
    }

    /// Creates an error reported by semantic analysis.
    pub(crate) fn semantic(line: usize, column: usize, message: impl Into<String>) -> Self {
        Self::new("Semantic error", line, column, message)
    }

    fn new(phase: &'static str, line: usize, column: usize, message: impl Into<String>) -> Self {
        Self {
            phase,
            line,
            column,
            message: message.into(),
        }
    }
}

impl fmt::Display for OrniError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{} at line {}, column {}: {}",
            self.phase, self.line, self.column, self.message
        )
    }
}

impl Error for OrniError {}
