use std::collections::HashMap;

use crate::{
    diagnostics::OrniError,
    syntax::{Parameter, Statement, Token, TypeName},
};

use super::{FunctionSignature, TypeChecker, identifier_name, semantic_error, statements_return};

impl TypeChecker {
    /// Makes top-level functions visible to each other before bodies are checked.
    pub(super) fn declare_functions(&mut self, statements: &[Statement]) -> Result<(), OrniError> {
        for statement in statements {
            if let Statement::Function {
                name,
                parameters,
                return_type,
                ..
            } = statement
            {
                let function_name = identifier_name(name);
                if self.functions.contains_key(function_name) {
                    return Err(semantic_error(
                        name,
                        &format!("function '{function_name}' is already declared"),
                    ));
                }
                self.functions.insert(
                    function_name.to_owned(),
                    FunctionSignature {
                        parameters: parameters
                            .iter()
                            .map(|parameter| {
                                (parameter.type_name, parameter.default_value.is_some())
                            })
                            .collect(),
                        return_type: *return_type,
                    },
                );
            }
        }
        Ok(())
    }

    /// Checks one statement and any nested expressions or statements.
    pub(super) fn check_statement(&mut self, statement: &Statement) -> Result<(), OrniError> {
        match statement {
            Statement::Let {
                name,
                declared_type,
                value,
            } => {
                let value_type = self.expression_type(value)?;
                if let Some(declared_type) = declared_type {
                    self.require_type(name, *declared_type, value_type, "variable value")?;
                }
                self.define(identifier_name(name), declared_type.unwrap_or(value_type));
            }
            Statement::Function {
                name,
                parameters,
                return_type,
                body,
            } => self.check_function(name, parameters, *return_type, body)?,
            Statement::Print(expression) | Statement::Expression(expression) => {
                self.expression_type(expression)?;
            }
            Statement::Return { keyword, value } => {
                let Some(expected) = self.current_return_type else {
                    return Err(semantic_error(
                        keyword,
                        "return can only be used inside a function",
                    ));
                };
                let actual = value
                    .as_ref()
                    .map(|expression| self.expression_type(expression))
                    .transpose()?
                    .unwrap_or(TypeName::Unit);
                self.require_type(keyword, expected, actual, "return value")?;
            }
            Statement::If {
                keyword,
                condition,
                then_branch,
                else_branch,
            } => {
                let condition_type = self.expression_type(condition)?;
                self.require_type(keyword, TypeName::Bool, condition_type, "if condition")?;
                self.check_statement(then_branch)?;
                if let Some(else_branch) = else_branch {
                    self.check_statement(else_branch)?;
                }
            }
            Statement::Block(statements) => {
                self.scopes.push(HashMap::new());
                let result = self.check_statements(statements);
                self.scopes.pop();
                result?;
            }
        }
        Ok(())
    }

    /// Checks a sequence of statements in its current scope.
    fn check_statements(&mut self, statements: &[Statement]) -> Result<(), OrniError> {
        for statement in statements {
            self.check_statement(statement)?;
        }
        Ok(())
    }

    /// Checks a function's defaults, body, return values, and return coverage.
    fn check_function(
        &mut self,
        name: &Token,
        parameters: &[Parameter],
        return_type: TypeName,
        body: &[Statement],
    ) -> Result<(), OrniError> {
        let previous_return_type = self.current_return_type.replace(return_type);
        self.scopes.push(HashMap::new());

        let result = (|| {
            for parameter in parameters {
                if let Some(default) = &parameter.default_value {
                    let default_type = self.expression_type(default)?;
                    self.require_type(
                        &parameter.name,
                        parameter.type_name,
                        default_type,
                        "default value",
                    )?;
                }
                self.define(identifier_name(&parameter.name), parameter.type_name);
            }
            self.check_statements(body)?;
            if return_type != TypeName::Unit && !statements_return(body) {
                return Err(semantic_error(
                    name,
                    &format!(
                        "function '{}' must return a {return_type} value on every path",
                        identifier_name(name)
                    ),
                ));
            }
            Ok(())
        })();

        self.scopes.pop();
        self.current_return_type = previous_return_type;
        result
    }
}
