use crate::{
    ast::{Expression, Statement, Value},
    error::OrniError,
    token::{Token, TokenKind},
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

    /// Parses the next statement by inspecting its leading token.
    fn statement(&mut self) -> Result<Statement, OrniError> {
        if self.matches(|kind| matches!(kind, TokenKind::Fn)) {
            return self.function_statement();
        }
        if self.matches(|kind| matches!(kind, TokenKind::Let)) {
            return self.let_statement();
        }
        if self.matches(|kind| matches!(kind, TokenKind::Print)) {
            return self.print_statement();
        }
        if self.matches(|kind| matches!(kind, TokenKind::Return)) {
            return self.return_statement();
        }
        if self.matches(|kind| matches!(kind, TokenKind::If)) {
            return self.if_statement();
        }
        if self.matches(|kind| matches!(kind, TokenKind::LeftBrace)) {
            return Ok(Statement::Block(self.block()?));
        }

        self.expression_statement()
    }

    /// Parses a function name, parameters, and brace-delimited body.
    fn function_statement(&mut self) -> Result<Statement, OrniError> {
        let name = self.consume(
            |kind| matches!(kind, TokenKind::Identifier(_)),
            "expected a function name after 'fn'",
        )?;
        self.consume(
            |kind| matches!(kind, TokenKind::LeftParen),
            "expected '(' after the function name",
        )?;

        let mut parameters = Vec::new();
        if !matches!(self.peek().kind, TokenKind::RightParen) {
            loop {
                parameters.push(self.consume(
                    |kind| matches!(kind, TokenKind::Identifier(_)),
                    "expected a parameter name",
                )?);
                if !self.matches(|kind| matches!(kind, TokenKind::Comma)) {
                    break;
                }
            }
        }

        self.consume(
            |kind| matches!(kind, TokenKind::RightParen),
            "expected ')' after the parameters",
        )?;
        self.consume(
            |kind| matches!(kind, TokenKind::LeftBrace),
            "expected '{' before the function body",
        )?;
        let body = self.block()?;

        Ok(Statement::Function {
            name,
            parameters,
            body,
        })
    }

    /// Parses statements until a closing brace ends the current block.
    fn block(&mut self) -> Result<Vec<Statement>, OrniError> {
        let mut statements = Vec::new();
        while !matches!(self.peek().kind, TokenKind::RightBrace) && !self.is_at_end() {
            statements.push(self.statement()?);
        }
        self.consume(
            |kind| matches!(kind, TokenKind::RightBrace),
            "expected '}' after the function body",
        )?;
        Ok(statements)
    }

    /// Parses a variable declaration terminated by a semicolon.
    fn let_statement(&mut self) -> Result<Statement, OrniError> {
        let name = self.consume(
            |kind| matches!(kind, TokenKind::Identifier(_)),
            "expected a name after 'let'",
        )?;
        self.consume(
            |kind| matches!(kind, TokenKind::Equal),
            "expected '=' after the name",
        )?;
        let value = self.expression()?;
        self.consume(
            |kind| matches!(kind, TokenKind::Semicolon),
            "expected ';' after the declaration",
        )?;
        Ok(Statement::Let { name, value })
    }

    /// Parses a print statement terminated by a semicolon.
    fn print_statement(&mut self) -> Result<Statement, OrniError> {
        let expression = self.expression()?;
        self.consume(
            |kind| matches!(kind, TokenKind::Semicolon),
            "expected ';' after the value",
        )?;
        Ok(Statement::Print(expression))
    }

    /// Parses an optional return value followed by a semicolon.
    fn return_statement(&mut self) -> Result<Statement, OrniError> {
        let keyword = self.previous().clone();
        let value = if matches!(self.peek().kind, TokenKind::Semicolon) {
            None
        } else {
            Some(self.expression()?)
        };
        self.consume(
            |kind| matches!(kind, TokenKind::Semicolon),
            "expected ';' after the return value",
        )?;
        Ok(Statement::Return { keyword, value })
    }

    /// Parses an `if` statement with brace-delimited branches.
    fn if_statement(&mut self) -> Result<Statement, OrniError> {
        let keyword = self.previous().clone();
        let condition = self.expression()?;
        self.consume(
            |kind| matches!(kind, TokenKind::LeftBrace),
            "expected '{' after the if condition",
        )?;
        let then_branch = Box::new(Statement::Block(self.block()?));

        let else_branch = if self.matches(|kind| matches!(kind, TokenKind::Else)) {
            self.consume(
                |kind| matches!(kind, TokenKind::LeftBrace),
                "expected '{' after 'else'",
            )?;
            Some(Box::new(Statement::Block(self.block()?)))
        } else {
            None
        };

        Ok(Statement::If {
            keyword,
            condition,
            then_branch,
            else_branch,
        })
    }

    /// Parses an expression used as a statement, such as a function call.
    fn expression_statement(&mut self) -> Result<Statement, OrniError> {
        let expression = self.expression()?;
        self.consume(
            |kind| matches!(kind, TokenKind::Semicolon),
            "expected ';' after the expression",
        )?;
        Ok(Statement::Expression(expression))
    }

    /// Parses an expression starting at the lowest supported precedence.
    fn expression(&mut self) -> Result<Expression, OrniError> {
        self.equality()
    }

    /// Parses equality and inequality expressions.
    fn equality(&mut self) -> Result<Expression, OrniError> {
        let mut expression = self.comparison()?;
        while self.matches(|kind| matches!(kind, TokenKind::EqualEqual | TokenKind::BangEqual)) {
            let operator = self.previous().clone();
            let right = self.comparison()?;
            expression = Expression::Binary {
                left: Box::new(expression),
                operator,
                right: Box::new(right),
            };
        }
        Ok(expression)
    }

    /// Parses numeric ordering comparisons.
    fn comparison(&mut self) -> Result<Expression, OrniError> {
        let mut expression = self.term()?;
        while self.matches(|kind| {
            matches!(
                kind,
                TokenKind::Greater
                    | TokenKind::GreaterEqual
                    | TokenKind::Less
                    | TokenKind::LessEqual
            )
        }) {
            let operator = self.previous().clone();
            let right = self.term()?;
            expression = Expression::Binary {
                left: Box::new(expression),
                operator,
                right: Box::new(right),
            };
        }
        Ok(expression)
    }

    /// Parses addition and subtraction expressions.
    fn term(&mut self) -> Result<Expression, OrniError> {
        let mut expression = self.factor()?;
        while self.matches(|kind| matches!(kind, TokenKind::Plus | TokenKind::Minus)) {
            let operator = self.previous().clone();
            let right = self.factor()?;
            expression = Expression::Binary {
                left: Box::new(expression),
                operator,
                right: Box::new(right),
            };
        }
        Ok(expression)
    }

    /// Parses multiplication and division expressions.
    fn factor(&mut self) -> Result<Expression, OrniError> {
        let mut expression = self.unary()?;
        while self.matches(|kind| matches!(kind, TokenKind::Star | TokenKind::Slash)) {
            let operator = self.previous().clone();
            let right = self.unary()?;
            expression = Expression::Binary {
                left: Box::new(expression),
                operator,
                right: Box::new(right),
            };
        }
        Ok(expression)
    }

    /// Parses prefix operators before delegating to call expressions.
    fn unary(&mut self) -> Result<Expression, OrniError> {
        if self.matches(|kind| matches!(kind, TokenKind::Minus | TokenKind::Bang)) {
            let operator = self.previous().clone();
            let right = self.unary()?;
            return Ok(Expression::Unary {
                operator,
                right: Box::new(right),
            });
        }
        self.call()
    }

    /// Parses function calls and their comma-separated arguments.
    fn call(&mut self) -> Result<Expression, OrniError> {
        let mut expression = self.primary()?;

        while self.matches(|kind| matches!(kind, TokenKind::LeftParen)) {
            let mut arguments = Vec::new();
            if !matches!(self.peek().kind, TokenKind::RightParen) {
                loop {
                    arguments.push(self.expression()?);
                    if !self.matches(|kind| matches!(kind, TokenKind::Comma)) {
                        break;
                    }
                }
            }
            let closing_parenthesis = self.consume(
                |kind| matches!(kind, TokenKind::RightParen),
                "expected ')' after the arguments",
            )?;
            expression = Expression::Call {
                callee: Box::new(expression),
                closing_parenthesis,
                arguments,
            };
        }

        Ok(expression)
    }

    /// Parses literals, variable names, and parenthesized expressions.
    fn primary(&mut self) -> Result<Expression, OrniError> {
        let token = self.advance().clone();
        match token.kind.clone() {
            TokenKind::Number(value) => Ok(Expression::Literal(Value::Number(value))),
            TokenKind::String(value) => Ok(Expression::Literal(Value::String(value))),
            TokenKind::True => Ok(Expression::Literal(Value::Boolean(true))),
            TokenKind::False => Ok(Expression::Literal(Value::Boolean(false))),
            TokenKind::Identifier(_) => Ok(Expression::Variable(token)),
            TokenKind::LeftParen => {
                let expression = self.expression()?;
                self.consume(
                    |kind| matches!(kind, TokenKind::RightParen),
                    "expected ')' after the expression",
                )?;
                Ok(Expression::Grouping(Box::new(expression)))
            }
            _ => Err(OrniError::syntax(
                token.line,
                token.column,
                "expected an expression",
            )),
        }
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
