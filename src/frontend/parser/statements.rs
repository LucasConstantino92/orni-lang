use crate::{
    diagnostics::OrniError,
    syntax::{ContractMethod, IntegerType, ModelField, Parameter, Statement, TokenKind, TypeName},
};

use super::{Parser, syntax_error};

impl Parser {
    /// Parses the next statement by inspecting its leading token.
    pub(super) fn statement(&mut self) -> Result<Statement, OrniError> {
        self.with_nesting(|parser| parser.statement_inner())
    }

    fn statement_inner(&mut self) -> Result<Statement, OrniError> {
        if self.matches(|kind| matches!(kind, TokenKind::Import)) {
            return self.import_statement();
        }
        if self.matches(|kind| matches!(kind, TokenKind::Contract)) {
            return self.contract_statement();
        }
        if self.matches(|kind| matches!(kind, TokenKind::Model)) {
            return self.model_statement();
        }
        if self.matches(|kind| matches!(kind, TokenKind::Store)) {
            return self.store_statement();
        }
        if self.matches(|kind| matches!(kind, TokenKind::Provider)) {
            return self.provider_statement();
        }
        if self.matches(|kind| matches!(kind, TokenKind::Bind)) {
            return self.bind_statement();
        }
        if self.matches(|kind| matches!(kind, TokenKind::Route)) {
            return self.route_statement();
        }
        if self.matches(|kind| matches!(kind, TokenKind::Pub)) {
            let token = self.previous();
            return Err(syntax_error(
                token,
                "'pub' is unnecessary; module declarations are public unless their name starts with '_'",
            ).with_code("ORNI-E149"));
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
        if self.matches(|kind| matches!(kind, TokenKind::Assert)) {
            let keyword = self.previous().clone();
            let expression = self.expression()?;
            self.consume(
                |kind| matches!(kind, TokenKind::Semicolon),
                "expected ';' after the assertion",
            )?;
            return Ok(Statement::Assert {
                keyword,
                expression,
            });
        }
        if self.matches(|kind| matches!(kind, TokenKind::Test)) {
            let keyword = self.previous().clone();
            let name = self.consume(
                |kind| matches!(kind, TokenKind::String(_)),
                "expected a quoted test name after 'test'",
            )?;
            self.consume(
                |kind| matches!(kind, TokenKind::LeftBrace),
                "expected '{' before the test body",
            )?;
            return Ok(Statement::Test {
                keyword,
                name,
                body: self.block()?,
            });
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
        let parameters = self.parameters()?;
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

    fn parameters(&mut self) -> Result<Vec<Parameter>, OrniError> {
        self.consume(
            |kind| matches!(kind, TokenKind::LeftParen),
            "expected '(' before the parameters",
        )?;
        let mut parameters = Vec::new();
        let mut found_default = false;
        if !matches!(self.peek().kind, TokenKind::RightParen) {
            loop {
                let parameter_name = self.consume(
                    |kind| matches!(kind, TokenKind::Identifier(_)),
                    "expected a parameter name",
                )?;
                if parameters
                    .iter()
                    .any(|parameter: &Parameter| parameter.name.kind == parameter_name.kind)
                {
                    return Err(syntax_error(
                        &parameter_name,
                        "parameter name is already declared",
                    )
                    .with_code("ORNI-E116"));
                }
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
                        )
                        .with_code("ORNI-E147"));
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
        Ok(parameters)
    }

    /// Parses a contract containing function signatures without bodies.
    fn contract_statement(&mut self) -> Result<Statement, OrniError> {
        let name = self.consume(
            |kind| matches!(kind, TokenKind::Identifier(_)),
            "expected a contract name after 'contract'",
        )?;
        self.consume(
            |kind| matches!(kind, TokenKind::LeftBrace),
            "expected '{' after the contract name",
        )?;
        let mut methods = Vec::new();
        while !matches!(self.peek().kind, TokenKind::RightBrace) && !self.is_at_end() {
            self.consume(
                |kind| matches!(kind, TokenKind::Func),
                "expected a function signature in the contract",
            )?;
            let method_name = self.consume(
                |kind| matches!(kind, TokenKind::Identifier(_)),
                "expected a method name after 'func'",
            )?;
            let parameters = self.parameters()?;
            self.consume(
                |kind| matches!(kind, TokenKind::Arrow),
                "expected '->' after the contract parameters",
            )?;
            let return_type = self.type_name()?;
            self.consume(
                |kind| matches!(kind, TokenKind::Semicolon),
                "expected ';' after the contract method",
            )?;
            methods.push(ContractMethod {
                name: method_name,
                parameters,
                return_type,
            });
        }
        self.consume(
            |kind| matches!(kind, TokenKind::RightBrace),
            "expected '}' after the contract",
        )?;
        Ok(Statement::Contract { name, methods })
    }

    /// Parses a typed data schema with semicolon-terminated fields.
    fn model_statement(&mut self) -> Result<Statement, OrniError> {
        let name = self.consume(
            |kind| matches!(kind, TokenKind::Identifier(_)),
            "expected a model name after 'model'",
        )?;
        self.consume(
            |kind| matches!(kind, TokenKind::LeftBrace),
            "expected '{' after the model name",
        )?;
        let mut fields = Vec::new();
        while !matches!(self.peek().kind, TokenKind::RightBrace) && !self.is_at_end() {
            let field_name = self.consume(
                |kind| matches!(kind, TokenKind::Identifier(_)),
                "expected a field name in the model",
            )?;
            if fields
                .iter()
                .any(|field: &ModelField| field.name.kind == field_name.kind)
            {
                return Err(
                    syntax_error(&field_name, "model field name is already declared")
                        .with_code("ORNI-E116"),
                );
            }
            self.consume(
                |kind| matches!(kind, TokenKind::Colon),
                "expected ':' after the model field name",
            )?;
            let type_name = self.type_name()?;
            let mut rules = Vec::new();
            loop {
                if self.matches(|kind| matches!(kind, TokenKind::Comma))
                    && !matches!(self.peek().kind, TokenKind::Identifier(_))
                {
                    return Err(self.error_at_current("expected a model field rule after ','"));
                }
                let TokenKind::Identifier(rule) = self.peek().kind.clone() else {
                    break;
                };
                let rule_token = self.advance().clone();
                let rule = match rule.as_str() {
                    "email" => crate::syntax::ModelFieldRule::Email,
                    "unique" => crate::syntax::ModelFieldRule::Unique,
                    "optional" => crate::syntax::ModelFieldRule::Optional,
                    "min" | "max" => {
                        self.consume(
                            |kind| matches!(kind, TokenKind::LeftParen),
                            "expected '(' after the model field rule",
                        )?;
                        let negative = self.matches(|kind| matches!(kind, TokenKind::Minus));
                        let value_token = self.consume(
                            |kind| matches!(kind, TokenKind::Int(_)),
                            "expected an integer boundary in the model field rule",
                        )?;
                        let TokenKind::Int(value) = value_token.kind else {
                            unreachable!("the rule boundary was validated as an integer")
                        };
                        self.consume(
                            |kind| matches!(kind, TokenKind::RightParen),
                            "expected ')' after the model field rule boundary",
                        )?;
                        let value = if negative {
                            value.checked_neg().ok_or_else(|| {
                                syntax_error(
                                    &value_token,
                                    "model field rule boundary is outside the supported range",
                                )
                            })?
                        } else {
                            value
                        };
                        if rule == "min" {
                            crate::syntax::ModelFieldRule::Min(value)
                        } else {
                            crate::syntax::ModelFieldRule::Max(value)
                        }
                    }
                    "one_of" => {
                        self.consume(
                            |kind| matches!(kind, TokenKind::LeftParen),
                            "expected '(' after the 'one_of' model field rule",
                        )?;
                        let mut values = Vec::new();
                        loop {
                            let value = self.consume(
                                |kind| matches!(kind, TokenKind::String(_)),
                                "expected a string value in the 'one_of' model field rule",
                            )?;
                            let TokenKind::String(value) = value.kind else {
                                unreachable!("the allowed value was validated as a string")
                            };
                            if values.contains(&value) {
                                return Err(syntax_error(
                                    &rule_token,
                                    "the 'one_of' model field rule cannot repeat a value",
                                )
                                .with_code("ORNI-E116"));
                            }
                            values.push(value);
                            if !self.matches(|kind| matches!(kind, TokenKind::Comma)) {
                                break;
                            }
                        }
                        self.consume(
                            |kind| matches!(kind, TokenKind::RightParen),
                            "expected ')' after the 'one_of' model field rule values",
                        )?;
                        crate::syntax::ModelFieldRule::OneOf(values)
                    }
                    _ => {
                        return Err(syntax_error(
                            &rule_token,
                            "expected a model field rule such as 'email', 'unique', 'optional', 'min', 'max', or 'one_of'",
                        )
                        .with_code("ORNI-S001"));
                    }
                };
                if rules.iter().any(|existing| {
                    matches!(
                        (existing, &rule),
                        (
                            crate::syntax::ModelFieldRule::Email,
                            crate::syntax::ModelFieldRule::Email
                        ) | (
                            crate::syntax::ModelFieldRule::Unique,
                            crate::syntax::ModelFieldRule::Unique
                        ) | (
                            crate::syntax::ModelFieldRule::Optional,
                            crate::syntax::ModelFieldRule::Optional
                        ) | (
                            crate::syntax::ModelFieldRule::Min(_),
                            crate::syntax::ModelFieldRule::Min(_)
                        ) | (
                            crate::syntax::ModelFieldRule::Max(_),
                            crate::syntax::ModelFieldRule::Max(_)
                        ) | (
                            crate::syntax::ModelFieldRule::OneOf(_),
                            crate::syntax::ModelFieldRule::OneOf(_)
                        )
                    )
                }) {
                    return Err(
                        syntax_error(&rule_token, "model field rule is already declared")
                            .with_code("ORNI-E116"),
                    );
                }
                rules.push(rule);
            }
            let default_value = if self.matches(|kind| matches!(kind, TokenKind::Equal)) {
                Some(self.expression()?)
            } else {
                None
            };
            self.consume(
                |kind| matches!(kind, TokenKind::Semicolon),
                "expected ';' after the model field",
            )?;
            fields.push(ModelField {
                name: field_name,
                type_name,
                rules,
                default_value,
            });
        }
        self.consume(
            |kind| matches!(kind, TokenKind::RightBrace),
            "expected '}' after the model fields",
        )?;
        Ok(Statement::Model { name, fields })
    }

    /// Parses a named model collection retained by the native HTTP server.
    fn store_statement(&mut self) -> Result<Statement, OrniError> {
        let keyword = self.previous().clone();
        if self.nesting > 1 {
            return Err(syntax_error(
                &keyword,
                "stores must be declared at the application top level",
            )
            .with_code("ORNI-E152"));
        }
        let name = self.consume(
            |kind| matches!(kind, TokenKind::Identifier(_)),
            "expected a store name after 'store'",
        )?;
        self.consume(
            |kind| matches!(kind, TokenKind::Colon),
            "expected ':' after the store name",
        )?;
        let model = self.consume(
            |kind| matches!(kind, TokenKind::Identifier(_)),
            "expected a model name after ':'",
        )?;
        self.consume(
            |kind| matches!(kind, TokenKind::Semicolon),
            "expected ';' after the store declaration",
        )?;
        Ok(Statement::Store {
            keyword,
            name,
            model,
        })
    }

    /// Parses a concrete provider implementation.
    fn provider_statement(&mut self) -> Result<Statement, OrniError> {
        let name = self.consume(
            |kind| matches!(kind, TokenKind::Identifier(_)),
            "expected a provider name after 'provider'",
        )?;
        let dependencies = if matches!(self.peek().kind, TokenKind::LeftParen) {
            self.parameters()?
        } else {
            Vec::new()
        };
        self.consume(
            |kind| matches!(kind, TokenKind::For),
            "expected 'for' after the provider name",
        )?;
        let contract = self.qualified_name("expected a contract name after 'for'")?;
        self.consume(
            |kind| matches!(kind, TokenKind::LeftBrace),
            "expected '{' before the provider methods",
        )?;
        let mut methods = Vec::new();
        while !matches!(self.peek().kind, TokenKind::RightBrace) && !self.is_at_end() {
            self.consume(
                |kind| matches!(kind, TokenKind::Func),
                "expected a function implementation in the provider",
            )?;
            methods.push(self.function_statement()?);
        }
        self.consume(
            |kind| matches!(kind, TokenKind::RightBrace),
            "expected '}' after the provider",
        )?;
        Ok(Statement::Provider {
            name,
            dependencies,
            contract,
            methods,
        })
    }

    /// Parses a contract-to-provider binding.
    fn bind_statement(&mut self) -> Result<Statement, OrniError> {
        let keyword = self.previous().clone();
        let contract = self.qualified_name("expected a contract name after 'bind'")?;
        self.consume(
            |kind| matches!(kind, TokenKind::To),
            "expected 'to' after the contract name",
        )?;
        let provider = self.qualified_name("expected a provider name after 'to'")?;
        self.consume(
            |kind| matches!(kind, TokenKind::Semicolon),
            "expected ';' after the binding",
        )?;
        Ok(Statement::Bind {
            keyword,
            contract,
            provider,
        })
    }

    fn route_statement(&mut self) -> Result<Statement, OrniError> {
        let keyword = self.previous().clone();
        if self.nesting > 1 {
            return Err(syntax_error(
                &keyword,
                "HTTP routes must be declared at the application top level",
            )
            .with_code("ORNI-E152"));
        }
        let method = self.consume(
            |kind| {
                matches!(
                    kind,
                    TokenKind::Get
                        | TokenKind::Post
                        | TokenKind::Put
                        | TokenKind::Patch
                        | TokenKind::Delete
                )
            },
            "only GET, POST, PUT, and DELETE routes are supported in this preview",
        )?;
        let path = self.consume(
            |kind| matches!(kind, TokenKind::String(_)),
            "expected a quoted path after 'route get'",
        )?;
        self.consume(
            |kind| matches!(kind, TokenKind::FatArrow),
            "expected '=>' before the route handler",
        )?;
        let handler = self.consume(
            |kind| matches!(kind, TokenKind::Identifier(_)),
            "expected a handler function name after '=>'",
        )?;
        self.consume(
            |kind| matches!(kind, TokenKind::Semicolon),
            "expected ';' after the route declaration",
        )?;
        Ok(Statement::Route {
            keyword,
            method,
            path,
            handler,
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
            )
            .with_code("ORNI-E128"));
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
        let token = self.qualified_name("expected a type name")?;
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
                        )
                        .with_code("ORNI-E148"));
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
            "uuid" => Ok(TypeName::Uuid),
            "size" => Ok(TypeName::Size),
            "duration" => Ok(TypeName::Duration),
            "response" => Ok(TypeName::Response),
            "unit" => Ok(TypeName::Unit),
            _ if self.model_names.contains(name) => Ok(TypeName::Model(name.to_owned())),
            _ => Ok(TypeName::Contract(name.to_owned())),
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
