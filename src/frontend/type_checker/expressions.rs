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
