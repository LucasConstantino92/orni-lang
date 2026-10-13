use crate::{
    diagnostics::OrniError,
    syntax::{Expression, IntegerType, TokenKind, Value},
};

use super::Parser;

impl Parser {
    /// Parses an expression starting at the lowest supported precedence.
    pub(super) fn expression(&mut self) -> Result<Expression, OrniError> {
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
            TokenKind::Int(value) => Ok(Expression::Literal(Value::Integer {
                value,
                integer_type: IntegerType::smallest_signed(value),
            })),
            TokenKind::Double(value) => Ok(Expression::Literal(Value::Double(value))),
            TokenKind::String(value) => Ok(Expression::Literal(Value::String(value.into()))),
            TokenKind::Char(value) => Ok(Expression::Literal(Value::Char(value))),
            TokenKind::True => Ok(Expression::Literal(Value::Bool(true))),
            TokenKind::False => Ok(Expression::Literal(Value::Bool(false))),
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
}
