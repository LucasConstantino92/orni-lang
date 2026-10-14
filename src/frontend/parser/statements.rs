use crate::{
    diagnostics::OrniError,
    syntax::{IntegerType, Parameter, Statement, TokenKind, TypeName},
};

use super::{Parser, syntax_error};

impl Parser {
    /// Parses the next statement by inspecting its leading token.
    pub(super) fn statement(&mut self) -> Result<Statement, OrniError> {
        if self.matches(|kind| matches!(kind, TokenKind::Import)) {
            return self.import_statement();
        }
        if self.matches(|kind| matches!(kind, TokenKind::Pub)) {
            let token = self.previous();
            return Err(syntax_error(
                token,
                "'pub' is unnecessary; module declarations are public unless their name starts with '_'",
            ));
        }
        if self.matches(|kind| matches!(kind, TokenKind::Func)) {
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
            "expected a function name after 'func'",
        )?;
        self.consume(
            |kind| matches!(kind, TokenKind::LeftParen),
            "expected '(' after the function name",
        )?;

        let mut parameters = Vec::new();
        let mut found_default = false;
        if !matches!(self.peek().kind, TokenKind::RightParen) {
            loop {
                let parameter_name = self.consume(
                    |kind| matches!(kind, TokenKind::Identifier(_)),
                    "expected a parameter name",
                )?;
                self.consume(
                    |kind| matches!(kind, TokenKind::Colon),
                    "expected ':' after the parameter name",
                )?;
                let type_name = self.type_name()?;
                let default_value = if self.matches(|kind| matches!(kind, TokenKind::Equal)) {
                    found_default = true;
                    Some(self.expression()?)
                } else {
                    if found_default {
                        return Err(syntax_error(
                            &parameter_name,
                            "required parameters cannot follow optional parameters",
                        ));
                    }
                    None
                };
                parameters.push(Parameter {
                    name: parameter_name,
                    type_name,
                    default_value,
                });
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
            |kind| matches!(kind, TokenKind::Arrow),
            "expected '->' after the parameters",
        )?;
        let return_type = self.type_name()?;
        self.consume(
            |kind| matches!(kind, TokenKind::LeftBrace),
            "expected '{' before the function body",
        )?;
        let body = self.block()?;

        Ok(Statement::Function {
            name,
            parameters,
            return_type,
            body,
        })
    }

    /// Parses a dotted module path such as `services.greeting`.
    fn import_statement(&mut self) -> Result<Statement, OrniError> {
        let keyword = self.previous().clone();
        let mut path = vec![self.consume(
            |kind| matches!(kind, TokenKind::Identifier(_)),
            "expected a module path after 'import'",
        )?];
        let mut members = None;
        while self.matches(|kind| matches!(kind, TokenKind::Dot)) {
            if self.matches(|kind| matches!(kind, TokenKind::LeftBrace)) {
                let mut selected = Vec::new();
                loop {
                    selected.push(self.consume(
                        |kind| matches!(kind, TokenKind::Identifier(_)),
                        "expected a public member in the selective import",
                    )?);
                    if !self.matches(|kind| matches!(kind, TokenKind::Comma)) {
                        break;
                    }
                }
                self.consume(
                    |kind| matches!(kind, TokenKind::RightBrace),
                    "expected '}' after the selected members",
                )?;
                members = Some(selected);
                break;
            }
            path.push(self.consume(
                |kind| matches!(kind, TokenKind::Identifier(_)),
                "expected a module name or '{' after '.'",
            )?);
        }
        let alias = if self.matches(|kind| matches!(kind, TokenKind::As)) {
            Some(self.consume(
                |kind| matches!(kind, TokenKind::Identifier(_)),
                "expected a namespace after 'as'",
            )?)
        } else {
            None
        };
        if members.is_some()
            && let Some(alias) = &alias
        {
            return Err(syntax_error(
                alias,
                "selective imports cannot also declare a namespace alias",
            ));
        }
        self.consume(
            |kind| matches!(kind, TokenKind::Semicolon),
            "expected ';' after the import",
        )?;
        Ok(Statement::Import {
            keyword,
            path,
            alias,
            members,
        })
    }

    /// Parses one of Orni's built-in type names.
    fn type_name(&mut self) -> Result<TypeName, OrniError> {
        let token = self.consume(
            |kind| matches!(kind, TokenKind::Identifier(_)),
            "expected a type name",
        )?;
        let TokenKind::Identifier(name) = &token.kind else {
            unreachable!("the token was validated as an identifier")
        };
        match name.as_str() {
            "int" | "int64" => Ok(TypeName::Integer(IntegerType::Int64)),
            "int8" => Ok(TypeName::Integer(IntegerType::Int8)),
            "int16" => Ok(TypeName::Integer(IntegerType::Int16)),
            "int32" => Ok(TypeName::Integer(IntegerType::Int32)),
            "uint8" => Ok(TypeName::Integer(IntegerType::UInt8)),
            "uint16" => Ok(TypeName::Integer(IntegerType::UInt16)),
            "uint32" => Ok(TypeName::Integer(IntegerType::UInt32)),
            "uint64" => Ok(TypeName::Integer(IntegerType::UInt64)),
            "double" => Ok(TypeName::Double),
            "string" => {
                let max_bytes = if self.matches(|kind| matches!(kind, TokenKind::Less)) {
                    let size_token = self.consume(
                        |kind| matches!(kind, TokenKind::Int(_)),
                        "expected a byte limit after 'string<'",
                    )?;
                    let TokenKind::Int(size) = &size_token.kind else {
                        unreachable!("the string size was validated as an integer")
                    };
                    if *size <= 0 || *size > usize::MAX as i128 {
                        return Err(syntax_error(
                            &size_token,
                            "string capacity must be a positive platform-sized integer",
                        ));
                    }
                    self.consume(
                        |kind| matches!(kind, TokenKind::Greater),
                        "expected '>' after the string byte limit",
                    )?;
                    Some(*size as usize)
                } else {
                    None
                };
                Ok(TypeName::String(max_bytes))
            }
            "bool" => Ok(TypeName::Bool),
            "char" => Ok(TypeName::Char),
            "unit" => Ok(TypeName::Unit),
            _ => Err(syntax_error(&token, format!("unknown type '{name}'"))),
        }
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
        let declared_type = if self.matches(|kind| matches!(kind, TokenKind::Colon)) {
            Some(self.type_name()?)
        } else {
            None
        };
        self.consume(
            |kind| matches!(kind, TokenKind::Equal),
            "expected '=' after the name",
        )?;
        let value = self.expression()?;
        self.consume(
            |kind| matches!(kind, TokenKind::Semicolon),
            "expected ';' after the declaration",
        )?;
        Ok(Statement::Let {
            name,
            declared_type,
            value,
        })
    }

    /// Parses a print statement terminated by a semicolon.
    fn print_statement(&mut self) -> Result<Statement, OrniError> {
        let keyword = self.previous().clone();
        let expression = self.expression()?;
        self.consume(
            |kind| matches!(kind, TokenKind::Semicolon),
            "expected ';' after the value",
        )?;
        Ok(Statement::Print {
            keyword,
            expression,
        })
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
}
