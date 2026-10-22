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
            let named_auth_arguments = matches!(
                &expression,
                Expression::Variable(token)
                    if matches!(&token.kind, TokenKind::Identifier(name) if name == "app.auth" || name == "app.auth_env")
            ) && matches!(self.peek().kind, TokenKind::Identifier(_))
                && self
                    .tokens
                    .get(self.current + 1)
                    .is_some_and(|token| matches!(token.kind, TokenKind::Colon));
            let named_size_argument = matches!(
                &expression,
                Expression::Variable(token)
                    if matches!(&token.kind, TokenKind::Identifier(name) if name == "Size")
            ) && matches!(self.peek().kind, TokenKind::Identifier(_))
                && self
                    .tokens
                    .get(self.current + 1)
                    .is_some_and(|token| matches!(token.kind, TokenKind::Colon));
            let named_duration_argument = matches!(
                &expression,
                Expression::Variable(token)
                    if matches!(&token.kind, TokenKind::Identifier(name) if name == "Duration")
            ) && matches!(self.peek().kind, TokenKind::Identifier(_))
                && self
                    .tokens
                    .get(self.current + 1)
                    .is_some_and(|token| matches!(token.kind, TokenKind::Colon));
            let arguments = if named_protect_arguments {
                self.protect_arguments()?
            } else if named_auth_arguments {
                let environment = matches!(&expression, Expression::Variable(token) if matches!(&token.kind, TokenKind::Identifier(name) if name == "app.auth_env"));
                self.auth_arguments(environment)?
            } else if named_size_argument {
                self.size_argument()?
            } else if named_duration_argument {
                self.duration_argument()?
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

    /// Parses one explicit storage unit and preserves its byte multiplier for `Size`.
    fn size_argument(&mut self) -> Result<Vec<Expression>, OrniError> {
        let unit = self.consume(
            |kind| matches!(kind, TokenKind::Identifier(_)),
            "expected a Size unit such as bytes or megabytes",
        )?;
        let TokenKind::Identifier(name) = &unit.kind else {
            unreachable!("the size unit was validated as an identifier")
        };
        let multiplier = match name.as_str() {
            "bytes" => 1,
            "kilobytes" => 1024,
            "megabytes" => 1024 * 1024,
            "gigabytes" => 1024 * 1024 * 1024,
            _ => {
                return Err(syntax_error(
                    &unit,
                    "Size expects bytes, kilobytes, megabytes, or gigabytes",
                )
                .with_code("ORNI-E107"));
            }
        };
        self.consume(
            |kind| matches!(kind, TokenKind::Colon),
            "expected ':' after the Size unit",
        )?;
        let value = self.expression()?;
        let multiplier = Expression::Literal(Value::Integer {
            value: multiplier,
            integer_type: IntegerType::smallest_signed(multiplier),
        });
        Ok(vec![multiplier, value])
    }

    /// Parses one explicit time unit and preserves its millisecond multiplier for `Duration`.
    fn duration_argument(&mut self) -> Result<Vec<Expression>, OrniError> {
        let unit = self.consume(
            |kind| matches!(kind, TokenKind::Identifier(_)),
            "expected a Duration unit such as seconds or minutes",
        )?;
        let TokenKind::Identifier(name) = &unit.kind else {
            unreachable!("the duration unit was validated as an identifier")
        };
        let multiplier = match name.as_str() {
            "milliseconds" => 1,
            "seconds" => 1_000,
            "minutes" => 60_000,
            "hours" => 3_600_000,
            "days" => 86_400_000,
            _ => {
                return Err(syntax_error(
                    &unit,
                    "Duration expects milliseconds, seconds, minutes, hours, or days",
                )
                .with_code("ORNI-E107"));
            }
        };
        self.consume(
            |kind| matches!(kind, TokenKind::Colon),
            "expected ':' after the Duration unit",
        )?;
        let value = self.expression()?;
        let multiplier = Expression::Literal(Value::Integer {
            value: multiplier,
            integer_type: IntegerType::smallest_signed(multiplier),
        });
        Ok(vec![multiplier, value])
    }

    /// Parses named authentication settings in a stable declaration order.
    fn auth_arguments(&mut self, environment: bool) -> Result<Vec<Expression>, OrniError> {
        let mut secret = None;
        let mut access = None;
        let mut refresh = None;
        let mut sessions = None;
        loop {
            let label = self.consume(
                |kind| matches!(kind, TokenKind::Identifier(_)),
                "expected a named authentication argument",
            )?;
            let TokenKind::Identifier(name) = &label.kind else {
                unreachable!()
            };
            self.consume(
                |kind| matches!(kind, TokenKind::Colon),
                "expected ':' after the authentication argument name",
            )?;
            let value = self.expression()?;
            let target = match name.as_str() {
                "secret" if !environment => &mut secret,
                "variable" if environment => &mut secret,
                "access_time" => &mut access,
                "refresh_time" => &mut refresh,
                "max_sessions" => &mut sessions,
                _ => {
                    return Err(syntax_error(
                        &label,
                        "expected secret or variable, access_time, refresh_time, or max_sessions",
                    ));
                }
            };
            if target.replace(value).is_some() {
                return Err(
                    syntax_error(&label, "authentication argument is already declared")
                        .with_code("ORNI-E116"),
                );
            }
            if !self.matches(|kind| matches!(kind, TokenKind::Comma)) {
                break;
            }
        }
        let Some(secret) = secret else {
            return Err(self.error_at_current("authentication requires secret or variable"));
        };
        let Some(access) = access else {
            return Err(self.error_at_current("authentication requires access_time"));
        };
        if sessions.is_some() && refresh.is_none() {
            return Err(self.error_at_current("max_sessions requires refresh_time"));
        }
        let mut arguments = vec![secret, access];
        if let Some(refresh) = refresh {
            arguments.push(refresh);
        }
        if let Some(sessions) = sessions {
            arguments.push(sessions);
        }
        Ok(arguments)
    }

    /// Parses readable named arguments for the built-in failed-attempt guard.
    fn protect_arguments(&mut self) -> Result<Vec<Expression>, OrniError> {
        let mut route = None;
        let mut attempts = None;
        let mut lock_time = None;
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
                "lock_time" => &mut lock_time,
                _ => {
                    return Err(syntax_error(
                        &label,
                        "expected route, attempts, or lock_time",
                    ));
                }
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
        let missing = route.is_none() || attempts.is_none() || lock_time.is_none();
        if missing {
            return Err(
                self.error_at_current("named app.protect requires route, attempts, and lock_time")
            );
        }
        Ok(vec![
            route.expect("checked above"),
            attempts.expect("checked above"),
            lock_time.expect("checked above"),
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
