use crate::{
    diagnostics::OrniError,
    syntax::{Expression, IntegerType, TokenKind, Value},
};

use super::{Parser, syntax_error};

impl Parser {
    /// Parses an expression starting at the lowest supported precedence.
    pub(super) fn expression(&mut self) -> Result<Expression, OrniError> {
        let expression = self.with_nesting(|parser| parser.equality())?;
        let mut pending = vec![(&expression, 0)];
        while let Some((node, depth)) = pending.pop() {
            if depth > 64 {
                return Err(self
                    .error_at_current("expression tree exceeds the limit of 64 levels")
                    .with_code("ORNI-E142"));
            }
            match node {
                Expression::Binary { left, right, .. } => {
                    pending.push((left, depth + 1));
                    pending.push((right, depth + 1));
                }
                Expression::Unary { right, .. } | Expression::Grouping(right) => {
                    pending.push((right, depth + 1))
                }
                Expression::Call {
                    callee, arguments, ..
                } => {
                    pending.push((callee, depth + 1));
                    pending.extend(arguments.iter().map(|argument| (argument, depth + 1)));
                }
                _ => {}
            }
        }
        Ok(expression)
    }

    /// Parses equality and inequality expressions.
    fn equality(&mut self) -> Result<Expression, OrniError> {
        let mut expression = self.comparison()?;
        let mut chain_length = 0;
        while self.matches(|kind| matches!(kind, TokenKind::EqualEqual | TokenKind::BangEqual)) {
            let operator = self.previous().clone();
            let right = self.comparison()?;
            self.check_chain(&mut chain_length)?;
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
        let mut chain_length = 0;
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
            self.check_chain(&mut chain_length)?;
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
        let mut chain_length = 0;
        while self.matches(|kind| matches!(kind, TokenKind::Plus | TokenKind::Minus)) {
            let operator = self.previous().clone();
            let right = self.factor()?;
            self.check_chain(&mut chain_length)?;
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
        let mut chain_length = 0;
        while self.matches(|kind| matches!(kind, TokenKind::Star | TokenKind::Slash)) {
            let operator = self.previous().clone();
            let right = self.unary()?;
            self.check_chain(&mut chain_length)?;
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
            if matches!(operator.kind, TokenKind::Minus)
                && let TokenKind::Int(value) = self.peek().kind
            {
                self.advance();
                let value = -value;
                if !IntegerType::Int64.contains(value) {
                    return Err(syntax_error(
                        &operator,
                        "negative integer literal is out of range for int64",
                    )
                    .with_code("ORNI-E104"));
                }
                return Ok(Expression::Literal(Value::Integer {
                    value,
                    integer_type: IntegerType::smallest_signed(value),
                }));
            }
            let right = self.with_nesting(|parser| parser.unary())?;
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
        let mut chain_length = 0;

        loop {
            self.check_chain(&mut chain_length)?;
            if self.matches(|kind| matches!(kind, TokenKind::Dot)) {
                let member = self.consume(
                    |kind| {
                        matches!(
                            kind,
                            TokenKind::Identifier(_) | TokenKind::Patch | TokenKind::Delete
                        )
                    },
                    "expected a member name after '.'",
                )?;
                let Expression::Variable(mut namespace) = expression else {
                    return Err(syntax_error(
                        &member,
                        "only module namespaces support member access",
                    )
                    .with_code("ORNI-E151"));
                };
                let TokenKind::Identifier(namespace_name) = &mut namespace.kind else {
                    unreachable!("variable expressions contain identifiers")
                };
                let member_name = match member.kind {
                    TokenKind::Identifier(member_name) => member_name,
                    TokenKind::Patch => "patch".to_owned(),
                    TokenKind::Delete => "delete".to_owned(),
                    _ => unreachable!("the member token was validated"),
                };
                namespace_name.push('.');
                namespace_name.push_str(&member_name);
                namespace.end_line = member.end_line;
                namespace.end_column = member.end_column;
                expression = Expression::Variable(namespace);
                continue;
            }
            if !self.matches(|kind| matches!(kind, TokenKind::LeftParen)) {
                break;
            }
            let named_protect_arguments = matches!(
                &expression,
                Expression::Variable(token)
                    if matches!(&token.kind, TokenKind::Identifier(name) if name == "app.protect")
            ) && matches!(
                self.peek().kind,
                TokenKind::Identifier(_) | TokenKind::Route
            ) && self
                .tokens
                .get(self.current + 1)
                .is_some_and(|token| matches!(token.kind, TokenKind::Colon));
            let arguments = if named_protect_arguments {
                self.protect_arguments()?
            } else {
                let mut arguments = Vec::new();
                if !matches!(self.peek().kind, TokenKind::RightParen) {
                    loop {
                        arguments.push(self.expression()?);
                        if !self.matches(|kind| matches!(kind, TokenKind::Comma)) {
                            break;
                        }
                    }
                }
                arguments
            };
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

    /// Parses readable named arguments for the built-in failed-attempt guard.
    fn protect_arguments(&mut self) -> Result<Vec<Expression>, OrniError> {
        let mut route = None;
        let mut attempts = None;
        let mut seconds = None;
        loop {
            let label = self.consume(
                |kind| matches!(kind, TokenKind::Identifier(_) | TokenKind::Route),
                "expected a named protection argument",
            )?;
            let name = match &label.kind {
                TokenKind::Identifier(name) => name.as_str(),
                TokenKind::Route => "route",
                _ => unreachable!(),
            };
            self.consume(
                |kind| matches!(kind, TokenKind::Colon),
                "expected ':' after the protection argument name",
            )?;
            let value = self.expression()?;
            let target = match name {
                "route" => &mut route,
                "attempts" => &mut attempts,
                "seconds" => &mut seconds,
                _ => return Err(syntax_error(&label, "expected route, attempts, or seconds")),
            };
            if target.replace(value).is_some() {
                return Err(
                    syntax_error(&label, "protection argument is already declared")
                        .with_code("ORNI-E116"),
                );
            }
            if !self.matches(|kind| matches!(kind, TokenKind::Comma)) {
                break;
            }
        }
        let missing = route.is_none() || attempts.is_none() || seconds.is_none();
        if missing {
            return Err(
                self.error_at_current("named app.protect requires route, attempts, and seconds")
            );
        }
        Ok(vec![
            route.expect("checked above"),
            attempts.expect("checked above"),
            seconds.expect("checked above"),
        ])
    }

    /// Parses literals, variable names, and parenthesized expressions.
    fn primary(&mut self) -> Result<Expression, OrniError> {
        let token = self.advance().clone();
        match token.kind.clone() {
            TokenKind::Int(value) => {
                if !IntegerType::UInt64.contains(value) {
                    return Err(syntax_error(&token, "integer literal exceeds uint64 range"));
                }
                Ok(Expression::Literal(Value::Integer {
                    value,
                    integer_type: if value > i64::MAX as i128 {
                        IntegerType::UInt64
                    } else {
                        IntegerType::smallest_signed(value)
                    },
                }))
            }
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
            _ => Err(syntax_error(&token, "expected an expression")),
        }
    }
}
