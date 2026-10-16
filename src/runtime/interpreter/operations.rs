//! Runtime operations and resource-boundary enforcement.

use crate::{
    diagnostics::OrniError,
    syntax::{IntegerType, Token, TokenKind, Value},
};

use super::{Interpreter, checked_integer, integer_to_double, runtime_error};

impl Interpreter {
    /// Applies a binary operator to two already evaluated operands.
    pub(super) fn binary(
        &self,
        left: Value,
        operator: &Token,
        right: Value,
    ) -> Result<Value, OrniError> {
        match (&operator.kind, left, right) {
            (TokenKind::Plus, Value::String(left), Value::String(right)) => {
                self.concatenate(&left, &right, operator)
            }
            (TokenKind::Plus, Value::String(left), right) => {
                self.concatenate(&left, &right.to_string(), operator)
            }
            (TokenKind::Plus, left, Value::String(right)) => {
                self.concatenate(&left.to_string(), &right, operator)
            }
            (
                _,
                Value::Integer {
                    value: left,
                    integer_type: left_type,
                },
                Value::Integer {
                    value: right,
                    integer_type: right_type,
                },
            ) => self.integer_binary(left, left_type, operator, right, right_type),
            (_, Value::Double(left), Value::Double(right)) => {
                self.double_binary(left, operator, right)
            }
            (_, Value::Integer { value: left, .. }, Value::Double(right)) => {
                self.double_binary(integer_to_double(left, operator)?, operator, right)
            }
            (_, Value::Double(left), Value::Integer { value: right, .. }) => {
                self.double_binary(left, operator, integer_to_double(right, operator)?)
            }
            (TokenKind::EqualEqual, left, right) => Ok(Value::Bool(left == right)),
            (TokenKind::BangEqual, left, right) => Ok(Value::Bool(left != right)),
            (
                TokenKind::Plus
                | TokenKind::Minus
                | TokenKind::Star
                | TokenKind::Slash
                | TokenKind::Greater
                | TokenKind::GreaterEqual
                | TokenKind::Less
                | TokenKind::LessEqual,
                _,
                _,
            ) => Err(runtime_error(
                operator,
                "incompatible types for this operation",
            )),
            _ => unreachable!("binary operator validated by the parser"),
        }
    }

    /// Allocates a concatenated string once and enforces its configured limit.
    fn concatenate(&self, left: &str, right: &str, token: &Token) -> Result<Value, OrniError> {
        let length = left
            .len()
            .checked_add(right.len())
            .ok_or_else(|| runtime_error(token, "string size overflow").with_code("ORNI-E137"))?;
        if length > self.limits.max_string_bytes {
            return Err(runtime_error(
                token,
                &format!(
                    "string requires {length} UTF-8 byte(s), but the runtime limit is {}",
                    self.limits.max_string_bytes
                ),
            )
            .with_code("ORNI-E137"));
        }

        let mut combined = String::with_capacity(length);
        combined.push_str(left);
        combined.push_str(right);
        Ok(Value::String(combined.into()))
    }

    /// Retains a variable only when the active environment has capacity.
    pub(super) fn define_variable(
        &mut self,
        name: &str,
        value: Value,
        token: &Token,
    ) -> Result<(), OrniError> {
        let replaces_existing = self.environment.current_value(name).is_some();
        if !replaces_existing && self.environment.binding_count() >= self.limits.max_variables {
            return Err(runtime_error(
                token,
                &format!(
                    "variable count exceeds the configured limit of {}",
                    self.limits.max_variables
                ),
            )
            .with_code("ORNI-E138"));
        }

        let value_bytes = match &value {
            Value::String(value) => value.len(),
            _ => 0,
        };
        if value_bytes > self.limits.max_string_bytes {
            return Err(runtime_error(
                token,
                &format!(
                    "string requires {value_bytes} UTF-8 byte(s), but the runtime limit is {}",
                    self.limits.max_string_bytes
                ),
            )
            .with_code("ORNI-E137"));
        }
        let replaced_bytes =
            self.environment
                .current_value(name)
                .map_or(0, |existing| match existing {
                    Value::String(value) => value.len(),
                    _ => 0,
                });
        let retained_bytes = self
            .environment
            .retained_string_bytes()
            .saturating_sub(replaced_bytes)
            .checked_add(value_bytes)
            .ok_or_else(|| {
                runtime_error(token, "retained string size overflow").with_code("ORNI-E135")
            })?;
        if retained_bytes > self.limits.max_retained_string_bytes {
            return Err(runtime_error(
                token,
                &format!(
                    "retained strings exceed the configured limit of {} UTF-8 byte(s)",
                    self.limits.max_retained_string_bytes
                ),
            )
            .with_code("ORNI-E135"));
        }
        self.environment.define(name.to_owned(), value);
        Ok(())
    }

    /// Consumes one unit of execution fuel before performing an operation.
    pub(super) fn consume_operation(&mut self, token: &Token) -> Result<(), OrniError> {
        if self.operations >= self.limits.max_operations {
            return Err(runtime_error(
                token,
                &format!(
                    "execution exceeds the configured limit of {} operation(s)",
                    self.limits.max_operations
                ),
            )
            .with_code("ORNI-E139"));
        }
        self.operations += 1;
        Ok(())
    }

    /// Applies a checked operation to two integer operands.
    fn integer_binary(
        &self,
        left: i128,
        left_type: IntegerType,
        operator: &Token,
        right: i128,
        right_type: IntegerType,
    ) -> Result<Value, OrniError> {
        let result_type = if left_type == right_type {
            left_type
        } else {
            IntegerType::Int64
        };
        let checked = |value: Option<i128>| {
            value
                .ok_or_else(|| {
                    runtime_error(operator, "integer arithmetic overflow").with_code("ORNI-E132")
                })
                .and_then(|value| checked_integer(value, result_type, operator))
        };
        match operator.kind {
            TokenKind::Plus => checked(left.checked_add(right)),
            TokenKind::Minus => checked(left.checked_sub(right)),
            TokenKind::Star => checked(left.checked_mul(right)),
            TokenKind::Slash if right == 0 => {
                Err(runtime_error(operator, "division by zero").with_code("ORNI-E131"))
            }
            TokenKind::Slash => checked_integer(left / right, result_type, operator),
            TokenKind::Greater => Ok(Value::Bool(left > right)),
            TokenKind::GreaterEqual => Ok(Value::Bool(left >= right)),
            TokenKind::Less => Ok(Value::Bool(left < right)),
            TokenKind::LessEqual => Ok(Value::Bool(left <= right)),
            TokenKind::EqualEqual => Ok(Value::Bool(left == right)),
            TokenKind::BangEqual => Ok(Value::Bool(left != right)),
            _ => unreachable!("operator validated for numeric operands"),
        }
    }

    /// Applies an operation to two double-precision operands.
    fn double_binary(&self, left: f64, operator: &Token, right: f64) -> Result<Value, OrniError> {
        match operator.kind {
            TokenKind::Plus => Ok(Value::Double(left + right)),
            TokenKind::Minus => Ok(Value::Double(left - right)),
            TokenKind::Star => Ok(Value::Double(left * right)),
            TokenKind::Slash if right == 0.0 => {
                Err(runtime_error(operator, "division by zero").with_code("ORNI-E131"))
            }
            TokenKind::Slash => Ok(Value::Double(left / right)),
            TokenKind::Greater => Ok(Value::Bool(left > right)),
            TokenKind::GreaterEqual => Ok(Value::Bool(left >= right)),
            TokenKind::Less => Ok(Value::Bool(left < right)),
            TokenKind::LessEqual => Ok(Value::Bool(left <= right)),
            TokenKind::EqualEqual => Ok(Value::Bool(left == right)),
            TokenKind::BangEqual => Ok(Value::Bool(left != right)),
            _ => unreachable!("operator validated for numeric operands"),
        }
    }
}
