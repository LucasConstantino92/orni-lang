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
    nesting: usize,
}

impl Parser {
    /// Parses a public declaration reference such as `logging.Logger`.
    fn qualified_name(&mut self, message: &str) -> Result<Token, OrniError> {
        let mut token = self.consume(|kind| matches!(kind, TokenKind::Identifier(_)), message)?;
        while self.matches(|kind| matches!(kind, TokenKind::Dot)) {
            let member = self.consume(
                |kind| matches!(kind, TokenKind::Identifier(_)),
                "expected a name after '.'",
            )?;
            if let (TokenKind::Identifier(name), TokenKind::Identifier(member_name)) =
                (&mut token.kind, member.kind)
            {
                if member_name.starts_with('_') {
                    return Err(syntax_error(
                        &token,
                        "private members cannot be accessed through a module namespace",
                    )
                    .with_code("ORNI-E124"));
                }
                name.push('.');
                name.push_str(&member_name);
            }
            token.end_line = member.end_line;
            token.end_column = member.end_column;
        }
        Ok(token)
    }

    /// Creates a parser positioned at the first token.
    pub fn new(tokens: Vec<Token>) -> Self {
        Self {
            tokens,
            current: 0,
            nesting: 0,
        }
    }

    /// Parses all statements until the end-of-file marker is reached.
    pub fn parse(mut self) -> Result<Vec<Statement>, OrniError> {
        let mut statements = Vec::new();
        while !self.is_at_end() {
            statements.push(self.statement()?);
        }
        Ok(statements)
    }

    /// Bounds recursive syntax before exhausting the native or WASI stack.
    fn with_nesting<T>(
        &mut self,
        parse: impl FnOnce(&mut Self) -> Result<T, OrniError>,
    ) -> Result<T, OrniError> {
        if self.nesting >= 64 {
            return Err(self
                .error_at_current("syntax nesting exceeds the limit of 64 levels")
                .with_code("ORNI-E142"));
        }
        self.nesting += 1;
        let result = parse(self);
        self.nesting -= 1;
        result
    }

    fn check_chain(&self, length: &mut usize) -> Result<(), OrniError> {
        *length += 1;
        if *length > 128 {
            return Err(self
                .error_at_current("expression chain exceeds the limit of 128 operations")
                .with_code("ORNI-E142"));
        }
        Ok(())
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
        syntax_error(self.peek(), message)
    }
}

fn syntax_error(token: &Token, message: impl Into<String>) -> OrniError {
    OrniError::syntax(token.line, token.column, message).with_token(token)
}
