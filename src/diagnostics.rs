use std::{error::Error, fmt, path::Path};

use crate::syntax::Token;

/// The impact of a diagnostic reported by the Orni compiler.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticSeverity {
    /// Compilation cannot continue safely until the problem is fixed.
    Error,
    /// Compilation may continue, but the source can be improved.
    Warning,
}

impl fmt::Display for DiagnosticSeverity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Error => write!(formatter, "error"),
            Self::Warning => write!(formatter, "warning"),
        }
    }
}

/// One structured compiler diagnostic with a stable code and source span.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrniDiagnostic {
    code: &'static str,
    severity: DiagnosticSeverity,
    phase: &'static str,
    source: Option<String>,
    line: usize,
    column: usize,
    end_line: usize,
    end_column: usize,
    message: String,
}

impl OrniDiagnostic {
    /// Returns the stable machine-readable diagnostic code.
    pub fn code(&self) -> &'static str {
        self.code
    }

    /// Returns whether this diagnostic is an error or warning.
    pub fn severity(&self) -> DiagnosticSeverity {
        self.severity
    }

    /// Returns the compiler phase that produced the diagnostic.
    pub fn phase(&self) -> &'static str {
        self.phase
    }

    /// Returns the source path, when the diagnostic came from a file.
    pub fn source(&self) -> Option<&str> {
        self.source.as_deref()
    }

    /// Returns the one-based start line.
    pub fn line(&self) -> usize {
        self.line
    }

    /// Returns the one-based start column.
    pub fn column(&self) -> usize {
        self.column
    }

    /// Returns the one-based end line.
    pub fn end_line(&self) -> usize {
        self.end_line
    }

    /// Returns the one-based exclusive end column.
    pub fn end_column(&self) -> usize {
        self.end_column
    }

    /// Returns the human-readable explanation.
    pub fn message(&self) -> &str {
        &self.message
    }

    pub(crate) fn warning(token: &Token, code: &'static str, message: impl Into<String>) -> Self {
        Self::from_token(
            token,
            code,
            DiagnosticSeverity::Warning,
            "Module warning",
            message,
        )
    }

    pub(crate) fn semantic_warning(
        token: &Token,
        code: &'static str,
        message: impl Into<String>,
    ) -> Self {
        Self::from_token(
            token,
            code,
            DiagnosticSeverity::Warning,
            "Semantic warning",
            message,
        )
    }

    fn from_token(
        token: &Token,
        code: &'static str,
        severity: DiagnosticSeverity,
        phase: &'static str,
        message: impl Into<String>,
    ) -> Self {
        Self {
            code,
            severity,
            phase,
            source: token.source.as_deref().map(str::to_owned),
            line: token.line,
            column: token.column,
            end_line: token.end_line,
            end_column: token.end_column,
            message: message.into(),
        }
    }
}

impl fmt::Display for OrniDiagnostic {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} [{}]", self.phase, self.code)?;
        if let Some(source) = &self.source {
            write!(formatter, " in '{source}'")?;
        }
        write!(
            formatter,
            " at line {}, column {} to line {}, column {}: {}",
            self.line, self.column, self.end_line, self.end_column, self.message
        )
    }
}

/// One or more errors produced while processing or executing Orni source code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrniError {
    diagnostics: Vec<OrniDiagnostic>,
}

impl OrniError {
    /// Attaches a specific stable diagnostic identity at the error's creation site.
    ///
    /// Help consumers must select guidance by this code, not by English wording.
    pub(crate) fn with_code(mut self, code: &'static str) -> Self {
        for diagnostic in &mut self.diagnostics {
            diagnostic.code = code;
        }
        self
    }

    /// Returns every error collected during the failed operation.
    pub fn diagnostics(&self) -> &[OrniDiagnostic] {
        &self.diagnostics
    }

    pub(crate) fn lexical(line: usize, column: usize, message: impl Into<String>) -> Self {
        Self::new("ORNI-L001", "Lexical error", line, column, message)
    }

    pub(crate) fn syntax(line: usize, column: usize, message: impl Into<String>) -> Self {
        Self::new("ORNI-S001", "Syntax error", line, column, message)
    }

    pub(crate) fn runtime(line: usize, column: usize, message: impl Into<String>) -> Self {
        Self::new("ORNI-R001", "Runtime error", line, column, message)
    }

    pub(crate) fn semantic(line: usize, column: usize, message: impl Into<String>) -> Self {
        Self::new("ORNI-T001", "Semantic error", line, column, message)
    }

    pub(crate) fn module(line: usize, column: usize, message: impl Into<String>) -> Self {
        Self::new("ORNI-M001", "Module error", line, column, message)
    }

    pub(crate) fn combine(errors: Vec<Self>) -> Self {
        let diagnostics = errors
            .into_iter()
            .flat_map(|error| error.diagnostics)
            .collect();
        Self { diagnostics }
    }

    pub(crate) fn with_source(mut self, path: &Path) -> Self {
        for diagnostic in &mut self.diagnostics {
            diagnostic.source = Some(path.display().to_string());
        }
        self
    }

    pub(crate) fn with_token(mut self, token: &Token) -> Self {
        for diagnostic in &mut self.diagnostics {
            diagnostic.source = token.source.as_deref().map(str::to_owned);
            diagnostic.line = token.line;
            diagnostic.column = token.column;
            diagnostic.end_line = token.end_line;
            diagnostic.end_column = token.end_column;
        }
        self
    }

    fn new(
        code: &'static str,
        phase: &'static str,
        line: usize,
        column: usize,
        message: impl Into<String>,
    ) -> Self {
        Self {
            diagnostics: vec![OrniDiagnostic {
                code,
                severity: DiagnosticSeverity::Error,
                phase,
                source: None,
                line,
                column,
                end_line: line,
                end_column: column.saturating_add(1),
                message: message.into(),
            }],
        }
    }
}

impl fmt::Display for OrniError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, diagnostic) in self.diagnostics.iter().enumerate() {
            if index > 0 {
                writeln!(formatter)?;
            }
            write!(formatter, "{diagnostic}")?;
        }
        Ok(())
    }
}

impl Error for OrniError {}
