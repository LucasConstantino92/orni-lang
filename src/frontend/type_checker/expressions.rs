use crate::{
    diagnostics::OrniError,
    syntax::{Expression, Token, TokenKind, TypeName},
};

use super::{TypeChecker, identifier_name, is_numeric, numeric_result, semantic_error};

impl TypeChecker {
    /// Infers and validates the type produced by an expression.
    pub(super) fn expression_type(
        &mut self,
        expression: &Expression,
    ) -> Result<TypeName, OrniError> {
        self.operations += 1;
        if self.operations > 100_000 {
            return Err(OrniError::semantic(
                1,
                1,
                "semantic analysis exceeds the limit of 100000 operations",
            )
            .with_code("ORNI-E140"));
        }
        match expression {
            Expression::Literal(value) => Ok(value.type_name()),
            Expression::Grouping(expression) => self.expression_type(expression),
            Expression::Variable(token) => self.lookup_variable(token),
            Expression::Unary { operator, right } => {
                let actual = self.expression_type(right)?;
                match operator.kind {
                    TokenKind::Minus => {
                        self.require_numeric(operator, actual.clone(), "unary operand")?;
                        if matches!(
                            actual,
                            TypeName::Integer(
                                crate::syntax::IntegerType::UInt8
                                    | crate::syntax::IntegerType::UInt16
                                    | crate::syntax::IntegerType::UInt32
                                    | crate::syntax::IntegerType::UInt64
                            )
                        ) {
                            return Err(semantic_error(
                                operator,
                                "cannot negate an unsigned integer; convert it to a signed type first",
                            ).with_code("ORNI-E105"));
                        }
                        Ok(actual)
                    }
                    TokenKind::Bang => {
                        self.require_type(operator, TypeName::Bool, actual, "unary operand")?;
                        Ok(TypeName::Bool)
                    }
                    _ => unreachable!("unary operators are validated by the parser"),
                }
            }
            Expression::Binary {
                left,
                operator,
                right,
            } => self.binary_type(left, operator, right),
            Expression::Call {
                callee,
                closing_parenthesis,
                arguments,
            } => self.call_type(callee, closing_parenthesis, arguments),
        }
    }

    /// Validates a binary operation and returns its resulting type.
    fn binary_type(
        &mut self,
        left: &Expression,
        operator: &Token,
        right: &Expression,
    ) -> Result<TypeName, OrniError> {
        let left_type = self.expression_type(left)?;
        let right_type = self.expression_type(right)?;
        match operator.kind {
            TokenKind::Plus
                if matches!(left_type, TypeName::String(_))
                    || matches!(right_type, TypeName::String(_)) =>
            {
                Ok(string_concatenation_type(left_type, right_type))
            }
            TokenKind::Plus | TokenKind::Minus | TokenKind::Star | TokenKind::Slash => {
                self.require_numeric(operator, left_type.clone(), "left operand")?;
                self.require_numeric(operator, right_type.clone(), "right operand")?;
                Ok(numeric_result(left_type, right_type))
            }
            TokenKind::Greater
            | TokenKind::GreaterEqual
            | TokenKind::Less
            | TokenKind::LessEqual => {
                self.require_numeric(operator, left_type, "left operand")?;
                self.require_numeric(operator, right_type, "right operand")?;
                Ok(TypeName::Bool)
            }
            TokenKind::EqualEqual | TokenKind::BangEqual => {
                let both_numeric = is_numeric(&left_type) && is_numeric(&right_type);
                let both_strings = matches!(left_type, TypeName::String(_))
                    && matches!(right_type, TypeName::String(_));
                if !both_numeric && !both_strings {
                    self.require_type(operator, left_type, right_type, "right operand")?;
                }
                Ok(TypeName::Bool)
            }
            _ => unreachable!("binary operators are validated by the parser"),
        }
    }

    /// Validates the target, arity, and argument types of a function call.
    fn call_type(
        &mut self,
        callee: &Expression,
        closing_parenthesis: &Token,
        arguments: &[Expression],
    ) -> Result<TypeName, OrniError> {
        let Expression::Variable(name_token) = callee else {
            return Err(
                semantic_error(closing_parenthesis, "only named functions can be called")
                    .with_code("ORNI-E150"),
            );
        };
        let name = identifier_name(name_token);
        if name == "Size" {
            if arguments.len() != 2 {
                return Err(semantic_error(
                    closing_parenthesis,
                    "Size expects exactly one named unit argument",
                )
                .with_code("ORNI-E108"));
            }
            if arguments
                .iter()
                .any(|argument| !matches!(self.expression_type(argument), Ok(TypeName::Integer(_))))
            {
                return Err(semantic_error(
                    name_token,
                    "Size requires a non-negative integer amount",
                )
                .with_code("ORNI-E107"));
            }
            return Ok(TypeName::Size);
        }
        if name == "Duration" {
            if arguments.len() != 2 {
                return Err(semantic_error(
                    closing_parenthesis,
                    "Duration expects exactly one named unit argument",
                )
                .with_code("ORNI-E108"));
            }
            if arguments
                .iter()
                .any(|argument| !matches!(self.expression_type(argument), Ok(TypeName::Integer(_))))
            {
                return Err(semantic_error(
                    name_token,
                    "Duration requires a non-negative integer amount",
                )
                .with_code("ORNI-E107"));
            }
            return Ok(TypeName::Duration);
        }
        if name == "auth.cookie" {
            if arguments.len() != 2 {
                return Err(semantic_error(
                    closing_parenthesis,
                    "function 'auth.cookie' expects 2 argument(s)",
                )
                .with_code("ORNI-E108"));
            }
            if !matches!(self.expression_type(&arguments[0])?, TypeName::Response)
                || !matches!(self.expression_type(&arguments[1])?, TypeName::String(_))
            {
                return Err(semantic_error(
                    name_token,
                    "auth.cookie requires a response and refresh token string",
                )
                .with_code("ORNI-E107"));
            }
            return Ok(TypeName::Response);
        }
        if matches!(
            name,
            "api.status" | "api.body" | "api.contains" | "api.header" | "api.status_with"
        ) {
            let valid_arity = match name {
                "api.contains" | "api.header" => (3..=4).contains(&arguments.len()),
                "api.status_with" => (4..=5).contains(&arguments.len()),
                _ => (2..=3).contains(&arguments.len()),
            };
            if !valid_arity {
                let expected = match name {
                    "api.contains" | "api.header" => "3 or 4",
                    "api.status_with" => "4 or 5",
                    _ => "2 or 3",
                };
                return Err(semantic_error(
                    closing_parenthesis,
                    &format!(
                        "function '{name}' expects {expected} argument(s), but received {}",
                        arguments.len()
                    ),
                )
                .with_code("ORNI-E108"));
            }
            for argument in arguments {
                if !matches!(self.expression_type(argument)?, TypeName::String(_)) {
                    return Err(semantic_error(
                        name_token,
                        "api request method, path, and body must be strings",
                    )
                    .with_code("ORNI-E107"));
                }
            }
            return Ok(match name {
                "api.status" | "api.status_with" => {
                    TypeName::Integer(crate::syntax::IntegerType::Int64)
                }
                "api.body" | "api.header" => TypeName::String(None),
                _ => TypeName::Bool,
            });
        }
        if matches!(
            name,
            "app.name"
                | "app.port"
                | "app.protect"
                | "app.cors"
                | "app.auth"
                | "app.auth_env"
                | "app.require_auth"
                | "app.require_refresh"
                | "app.logout"
                | "app.body_limit"
                | "app.cache_limit"
        ) {
            let valid_arity = if name == "app.protect" {
                matches!(arguments.len(), 1 | 3)
            } else if matches!(name, "app.auth" | "app.auth_env") {
                matches!(arguments.len(), 2..=4)
            } else {
                arguments.len() == 1
            };
            if !valid_arity {
                let expected = match name {
                    "app.protect" => "1 or 3",
                    "app.auth" | "app.auth_env" => "2, 3, or 4",
                    _ => "1",
                };
                return Err(semantic_error(
                    closing_parenthesis,
                    &format!(
                        "function '{name}' expects {expected} argument(s), but received {}",
                        arguments.len()
                    ),
                )
                .with_code("ORNI-E108"));
            }
            let actual = self.expression_type(&arguments[0])?;
            if matches!(
                name,
                "app.name" | "app.cors" | "app.require_auth" | "app.require_refresh" | "app.logout"
            ) {
                if !matches!(actual, TypeName::String(_)) {
                    return Err(semantic_error(
                        name_token,
                        match name {
                            "app.cors" => "app.cors requires an HTTP or HTTPS origin string",
                            "app.require_auth" => "app.require_auth requires a route path string",
                            "app.require_refresh" => {
                                "app.require_refresh requires a route path string"
                            }
                            "app.logout" => "app.logout requires a route path string",
                            _ => "app.name requires a string",
                        },
                    )
                    .with_code("ORNI-E107"));
                }
            } else if name == "app.port" && !matches!(actual, TypeName::Integer(_)) {
                return Err(semantic_error(name_token, "app.port requires an integer")
                    .with_code("ORNI-E107"));
            }
            if name == "app.protect" {
                if !matches!(actual, TypeName::String(_)) {
                    return Err(semantic_error(
                        name_token,
                        "app.protect requires a route path string",
                    )
                    .with_code("ORNI-E107"));
                }
                if arguments.len() == 3
                    && (!matches!(self.expression_type(&arguments[1])?, TypeName::Integer(_))
                        || !matches!(self.expression_type(&arguments[2])?, TypeName::Duration))
                {
                    return Err(semantic_error(
                        name_token,
                        "app.protect requires integer attempts and a Duration lock_time",
                    )
                    .with_code("ORNI-E107"));
                }
            }
            if matches!(name, "app.auth" | "app.auth_env")
                && (!matches!(actual, TypeName::String(_))
                    || !matches!(self.expression_type(&arguments[1]), Ok(TypeName::Duration))
                    || arguments.get(2).is_some_and(|argument| {
                        !matches!(self.expression_type(argument), Ok(TypeName::Duration))
                    })
                    || arguments.get(3).is_some_and(|argument| {
                        !matches!(self.expression_type(argument), Ok(TypeName::Integer(_)))
                    }))
            {
                return Err(semantic_error(
                    name_token,
                    "authentication configuration requires a string secret or environment name, Duration values, and an optional integer session limit",
                )
                .with_code("ORNI-E107"));
            }
            if matches!(name, "app.body_limit" | "app.cache_limit")
                && !matches!(actual, TypeName::Size)
            {
                return Err(
                    semantic_error(name_token, "resource limits require a Size value")
                        .with_code("ORNI-E107"),
                );
            }
            return Ok(TypeName::Unit);
        }
        if matches!(
            name,
            "auth.token" | "auth.refresh_token" | "auth.user" | "auth.refresh"
        ) {
            let valid_arity = if matches!(name, "auth.token" | "auth.refresh_token") {
                arguments.len() == 1
            } else {
                arguments.is_empty()
            };
            if !valid_arity {
                return Err(semantic_error(
                    closing_parenthesis,
                    &format!(
                        "function '{name}' expects {} argument(s), but received {}",
                        if matches!(name, "auth.token" | "auth.refresh_token") {
                            1
                        } else {
                            0
                        },
                        arguments.len()
                    ),
                )
                .with_code("ORNI-E108"));
            }
            if matches!(name, "auth.token" | "auth.refresh_token")
                && !matches!(self.expression_type(&arguments[0])?, TypeName::String(_))
            {
                return Err(
                    semantic_error(name_token, "token helpers require a string subject")
                        .with_code("ORNI-E107"),
                );
            }
            return Ok(TypeName::String(None));
        }
        if matches!(
            name,
            "response.ok"
                | "response.created"
                | "response.not_found"
                | "response.bad_request"
                | "response.unauthorized"
                | "response.forbidden"
                | "response.conflict"
                | "response.internal_error"
                | "response.redirect"
        ) {
            if arguments.len() != 1 {
                return Err(semantic_error(
                    closing_parenthesis,
                    &format!(
                        "function '{name}' expects 1 argument(s), but received {}",
                        arguments.len()
                    ),
                )
                .with_code("ORNI-E108"));
            }
            let body_type = self.expression_type(&arguments[0])?;
            if !matches!(body_type, TypeName::Model(_) | TypeName::String(_)) {
                return Err(semantic_error(
                    closing_parenthesis,
                    "response functions accept a model or string body",
                )
                .with_code("ORNI-E107"));
            }
            return Ok(TypeName::Response);
        }
        if matches!(name, "response.json" | "response.created_json") {
            if arguments.len() != 1 {
                return Err(semantic_error(
                    closing_parenthesis,
                    &format!(
                        "function '{name}' expects 1 argument(s), but received {}",
                        arguments.len()
                    ),
                )
                .with_code("ORNI-E108"));
            }
            let value_type = self.expression_type(&arguments[0])?;
            if !matches!(value_type, TypeName::Model(_) | TypeName::Response) {
                return Err(semantic_error(
                    closing_parenthesis,
                    "response.json requires a model or response containing JSON data",
                )
                .with_code("ORNI-E107"));
            }
            return Ok(TypeName::Response);
        }
        if name == "response.collection" {
            if arguments.len() != 1 {
                return Err(semantic_error(
                    closing_parenthesis,
                    &format!(
                        "function 'response.collection' expects 1 argument(s), but received {}",
                        arguments.len()
                    ),
                )
                .with_code("ORNI-E108"));
            }
            let value_type = self.expression_type(&arguments[0])?;
            self.require_type(name_token, TypeName::Response, value_type, "first argument")?;
            return Ok(TypeName::Response);
        }
        if name == "response.no_content" {
            if !arguments.is_empty() {
                return Err(semantic_error(
                    closing_parenthesis,
                    "function 'response.no_content' expects 0 argument(s)",
                )
                .with_code("ORNI-E108"));
            }
            return Ok(TypeName::Response);
        }
        if name == "response.header" {
            if arguments.len() != 3 {
                return Err(semantic_error(
                    closing_parenthesis,
                    "function 'response.header' expects 3 argument(s)",
                )
                .with_code("ORNI-E108"));
            }
            let response_type = self.expression_type(&arguments[0])?;
            self.require_type(
                name_token,
                TypeName::Response,
                response_type,
                "first argument",
            )?;
            for argument in &arguments[1..] {
                if !matches!(self.expression_type(argument)?, TypeName::String(_)) {
                    return Err(semantic_error(
                        name_token,
                        "response.header requires string header names and values",
                    )
                    .with_code("ORNI-E107"));
                }
            }
            return Ok(TypeName::Response);
        }
        if let Some((store, operation)) = name.rsplit_once('.')
            && let Some(model) = self.stores.get(store).cloned()
        {
            let expected = match operation {
                "create" => vec![TypeName::Model(model.clone())],
                "find" | "delete" => vec![TypeName::Uuid],
                "list" => {
                    if arguments.is_empty() {
                        return Ok(TypeName::Response);
                    }
                    match arguments.len() {
                        2 => {
                            let first = self.expression_type(&arguments[0])?;
                            let second = self.expression_type(&arguments[1])?;
                            if matches!(
                                first,
                                TypeName::Integer(crate::syntax::IntegerType::UInt16)
                            ) && matches!(second, TypeName::String(_))
                            {
                                vec![
                                    TypeName::Integer(crate::syntax::IntegerType::UInt16),
                                    TypeName::String(None),
                                ]
                            } else {
                                vec![
                                    TypeName::Integer(crate::syntax::IntegerType::UInt32),
                                    TypeName::Integer(crate::syntax::IntegerType::UInt16),
                                ]
                            }
                        }
                        4 => vec![
                            TypeName::Integer(crate::syntax::IntegerType::UInt32),
                            TypeName::Integer(crate::syntax::IntegerType::UInt16),
                            TypeName::String(None),
                            TypeName::String(None),
                        ],
                        _ => {
                            return Err(semantic_error(
                                closing_parenthesis,
                                &format!(
                                    "store operation '{name}' expects 0, 2, or 4 argument(s), but received {}",
                                    arguments.len()
                                ),
                            )
                            .with_code("ORNI-E108"));
                        }
                    }
                }
                "search" => match arguments.len() {
                    4 => vec![
                        TypeName::String(None),
                        TypeName::String(None),
                        TypeName::Integer(crate::syntax::IntegerType::UInt16),
                        TypeName::String(None),
                    ],
                    6 => vec![
                        TypeName::String(None),
                        TypeName::String(None),
                        TypeName::Integer(crate::syntax::IntegerType::UInt16),
                        TypeName::String(None),
                        TypeName::String(None),
                        TypeName::String(None),
                    ],
                    _ => {
                        return Err(semantic_error(
                            closing_parenthesis,
                            &format!(
                                "store operation '{name}' expects 4 or 6 argument(s), but received {}",
                                arguments.len()
                            ),
                        )
                        .with_code("ORNI-E108"));
                    }
                },
                "replace" | "patch" => vec![TypeName::Uuid, TypeName::Model(model.clone())],
                _ => {
                    return Err(semantic_error(
                        name_token,
                        "stores support create, list, search, find, replace, patch, and delete",
                    )
                    .with_code("ORNI-E119"));
                }
            };
            if arguments.len() != expected.len() {
                return Err(semantic_error(
                    closing_parenthesis,
                    &format!(
                        "store operation '{name}' expects {} argument(s), but received {}",
                        expected.len(),
                        arguments.len()
                    ),
                )
                .with_code("ORNI-E108"));
            }
            for (index, (argument, expected)) in arguments.iter().zip(expected).enumerate() {
                let actual = self.expression_type(argument)?;
                if operation == "create" || (matches!(operation, "replace" | "patch") && index == 1)
                {
                    let TypeName::Model(input_model) = actual else {
                        unreachable!("model declarations are checked before store calls")
                    };
                    let stored_fields = self
                        .models
                        .get(&model)
                        .expect("stores reference known models");
                    let input_fields = self
                        .models
                        .get(&input_model)
                        .expect("model parameters reference known models");
                    let compatible = stored_fields
                        .iter()
                        .filter(|field| identifier_name(&field.name) != "id")
                        .all(|field| {
                            input_fields.iter().any(|input| {
                                identifier_name(&input.name) == identifier_name(&field.name)
                                    && input.type_name == field.type_name
                            })
                        })
                        && input_fields.len() + 1 == stored_fields.len();
                    if !compatible {
                        return Err(semantic_error(
                            closing_parenthesis,
                            &format!("store '{store}' create input must match model '{model}' except for id: uuid"),
                        ).with_code("ORNI-E107"));
                    }
                    continue;
                }
                self.require_type(closing_parenthesis, expected, actual, "store argument")?;
            }
            return Ok(TypeName::Response);
        }
        let signature = if let Some(signature) = self.functions.get(name).cloned() {
            signature
        } else if let Some((receiver, method)) = name.split_once('.') {
            let receiver_type = self
                .scopes
                .iter()
                .rev()
                .find_map(|scope| scope.get(receiver).cloned());
            let Some(TypeName::Contract(contract_name)) = receiver_type else {
                return Err(
                    semantic_error(name_token, &format!("undefined function '{name}'"))
                        .with_code("ORNI-E103"),
                );
            };
            self.contracts
                .get(&contract_name)
                .and_then(|methods| methods.get(method))
                .cloned()
                .ok_or_else(|| {
                    semantic_error(
                        name_token,
                        &format!("contract '{contract_name}' has no method '{method}'"),
                    )
                    .with_code("ORNI-E119")
                })?
        } else {
            return Err(
                semantic_error(name_token, &format!("undefined function '{name}'"))
                    .with_code("ORNI-E103"),
            );
        };
        for parameter in &signature.parameters {
            if let TypeName::Contract(contract_name) = &parameter.type_name
                && !self.allow_unbound
                && !self.bindings.contains_key(contract_name)
            {
                return Err(semantic_error(
                    &parameter.name,
                    &format!("contract '{contract_name}' has no provider binding"),
                )
                .with_code("ORNI-E102"));
            }
        }
        let supplied_parameters = signature
            .parameters
            .iter()
            .filter(|parameter| !matches!(parameter.type_name, TypeName::Contract(_)))
            .collect::<Vec<_>>();
        let required = supplied_parameters
            .iter()
            .filter(|parameter| parameter.default_value.is_none())
            .count();
        if arguments.len() < required || arguments.len() > supplied_parameters.len() {
            let expected = if required == supplied_parameters.len() {
                format!("{} argument(s)", supplied_parameters.len())
            } else {
                format!(
                    "between {required} and {} argument(s)",
                    supplied_parameters.len()
                )
            };
            return Err(semantic_error(
                closing_parenthesis,
                &format!(
                    "function '{name}' expects {expected}, but received {}",
                    arguments.len()
                ),
            )
            .with_code("ORNI-E108"));
        }
        let mut argument_types = Vec::with_capacity(arguments.len());
        for (argument, parameter) in arguments.iter().zip(&supplied_parameters) {
            self.require_literal_range(closing_parenthesis, &parameter.type_name, argument)?;
            let actual = self.expression_type(argument)?;
            self.require_type(
                closing_parenthesis,
                parameter.type_name.clone(),
                actual.clone(),
                "function argument",
            )?;
            argument_types.push(actual);
        }
        if signature.return_type == TypeName::String(None) {
            self.infer_bounded_string_return(name, &signature, &argument_types)
        } else {
            Ok(signature.return_type)
        }
    }

    /// Specializes an automatically sized string return for one call site.
    fn infer_bounded_string_return(
        &mut self,
        function_name: &str,
        signature: &super::FunctionSignature,
        argument_types: &[TypeName],
    ) -> Result<TypeName, OrniError> {
        if self.inference_stack.len() >= 64 {
            return Err(OrniError::semantic(
                1,
                1,
                "string inference exceeds the limit of 64 calls",
            )
            .with_code("ORNI-E140"));
        }
        if self
            .inference_stack
            .iter()
            .any(|active| active == function_name)
        {
            return Ok(signature.return_type.clone());
        }

        let previous_return_type = self
            .current_return_type
            .replace(signature.return_type.clone());
        let previous_inferred_return_type = self.inferred_return_type.take();
        self.inference_stack.push(function_name.to_owned());
        self.scopes.push(std::collections::HashMap::new());

        let result = (|| {
            let mut supplied_types = argument_types.iter();
            for parameter in &signature.parameters {
                let parameter_type = if matches!(parameter.type_name, TypeName::Contract(_)) {
                    parameter.type_name.clone()
                } else if let Some(argument_type) = supplied_types.next() {
                    argument_type.clone()
                } else {
                    self.expression_type(
                        parameter
                            .default_value
                            .as_ref()
                            .expect("the arity check guarantees a default value"),
                    )?
                };
                self.define(identifier_name(&parameter.name), parameter_type);
            }
            self.check_statements(&signature.body)?;
            Ok(self
                .inferred_return_type
                .clone()
                .unwrap_or_else(|| signature.return_type.clone()))
        })();

        self.scopes.pop();
        self.inference_stack.pop();
        self.current_return_type = previous_return_type;
        self.inferred_return_type = previous_inferred_return_type;
        result
    }
}

/// Infers the maximum UTF-8 byte length produced by string concatenation.
fn string_concatenation_type(left: TypeName, right: TypeName) -> TypeName {
    match (left, right) {
        (TypeName::String(Some(left)), TypeName::String(Some(right))) => {
            TypeName::String(left.checked_add(right))
        }
        _ => TypeName::String(None),
    }
}
