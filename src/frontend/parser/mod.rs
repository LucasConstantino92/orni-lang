mod expressions;
mod statements;

use crate::{
    diagnostics::OrniError,
    syntax::{Statement, Token, TokenKind},
};

/// Builds an abstract syntax tree from a sequence of lexical tokens.
pub(crate) struct Parser {
    tokens: Vec<Token>,
    current: usize,
}

impl Parser {
    /// Creates a parser positioned at the first token.
    pub fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, current: 0 }
    }

    /// Parses all statements until the end-of-file marker is reached.
    pub fn parse(mut self) -> Result<Vec<Statement>, OrniError> {
        let mut statements = Vec::new();
        while !self.is_at_end() {
            statements.push(self.statement()?);
        }
        Ok(statements)
    }

    /// Consumes a required token or reports a syntax error at the current one.
    fn consume(
        &mut self,
        predicate: impl FnOnce(&TokenKind) -> bool,
        message: &str,
    ) -> Result<Token, OrniError> {
        if predicate(&self.peek().kind) {
            return Ok(self.advance().clone());
        }
        Err(self.error_at_current(message))
    }

    /// Consumes the current token only when it satisfies `predicate`.
    fn matches(&mut self, predicate: impl FnOnce(&TokenKind) -> bool) -> bool {
        if predicate(&self.peek().kind) {
            self.advance();
            true
        } else {
            false
        }
    }

    /// Moves to the next token and returns the token just consumed.
    fn advance(&mut self) -> &Token {
        if !self.is_at_end() {
            self.current += 1;
        }
        self.previous()
    }

    /// Returns the current token without consuming it.
    fn peek(&self) -> &Token {
        &self.tokens[self.current]
    }

    /// Returns the most recently consumed token.
    fn previous(&self) -> &Token {
        &self.tokens[self.current.saturating_sub(1)]
    }

    /// Reports whether the current token is the end-of-file marker.
    fn is_at_end(&self) -> bool {
        matches!(self.peek().kind, TokenKind::Eof)
    }

    /// Creates a syntax error positioned at the current token.
    fn error_at_current(&self, message: impl Into<String>) -> OrniError {
        let token = self.peek();
        OrniError::syntax(token.line, token.column, message)
    }
}
