use std::collections::HashMap;

use crate::{
    ast::{Expression, Parameter, Statement, TypeName},
    error::OrniError,
    token::{Token, TokenKind},
};

/// The type information required to validate a function call.
#[derive(Clone)]
struct FunctionSignature {
    parameters: Vec<(TypeName, bool)>,
    return_type: TypeName,
}

/// Validates types and names before the interpreter executes a program.
pub(crate) struct TypeChecker {
    scopes: Vec<HashMap<String, TypeName>>,
    functions: HashMap<String, FunctionSignature>,
    current_return_type: Option<TypeName>,
}

impl TypeChecker {
    /// Creates a checker with an empty global scope.
    pub fn new() -> Self {
        Self {
            scopes: vec![HashMap::new()],
            functions: HashMap::new(),
            current_return_type: None,
        }
    }

    /// Checks a complete program and returns the first semantic error.
    pub fn check(mut self, program: &[Statement]) -> Result<(), OrniError> {
        self.declare_functions(program)?;
        for statement in program {
            self.check_statement(statement)?;
        }
        Ok(())
    }

    /// Makes top-level functions visible to each other before bodies are checked.
    fn declare_functions(&mut self, statements: &[Statement]) -> Result<(), OrniError> {
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
    fn check_statement(&mut self, statement: &Statement) -> Result<(), OrniError> {
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

    /// Infers and validates the type produced by an expression.
    fn expression_type(&mut self, expression: &Expression) -> Result<TypeName, OrniError> {
        match expression {
            Expression::Literal(value) => Ok(value.type_name()),
            Expression::Grouping(expression) => self.expression_type(expression),
            Expression::Variable(token) => self.lookup_variable(token),
            Expression::Unary { operator, right } => {
                let actual = self.expression_type(right)?;
                match operator.kind {
                    TokenKind::Minus => {
                        self.require_numeric(operator, actual, "unary operand")?;
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
            TokenKind::Plus if left_type == TypeName::String || right_type == TypeName::String => {
                Ok(TypeName::String)
            }
            TokenKind::Plus | TokenKind::Minus | TokenKind::Star | TokenKind::Slash => {
                self.require_numeric(operator, left_type, "left operand")?;
                self.require_numeric(operator, right_type, "right operand")?;
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
                if !(is_numeric(left_type) && is_numeric(right_type)) {
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
            return Err(semantic_error(
                closing_parenthesis,
                "only named functions can be called",
            ));
        };
        let name = identifier_name(name_token);
        let signature =
            self.functions.get(name).cloned().ok_or_else(|| {
                semantic_error(name_token, &format!("undefined function '{name}'"))
            })?;
        let required = signature
            .parameters
            .iter()
            .filter(|(_, optional)| !optional)
            .count();
        if arguments.len() < required || arguments.len() > signature.parameters.len() {
            let expected = if required == signature.parameters.len() {
                format!("{} argument(s)", signature.parameters.len())
            } else {
                format!(
                    "between {required} and {} argument(s)",
                    signature.parameters.len()
                )
            };
            return Err(semantic_error(
                closing_parenthesis,
                &format!(
                    "function '{name}' expects {expected}, but received {}",
                    arguments.len()
                ),
            ));
        }
        for (argument, (expected, _)) in arguments.iter().zip(&signature.parameters) {
            let actual = self.expression_type(argument)?;
            self.require_type(closing_parenthesis, *expected, actual, "function argument")?;
        }
        Ok(signature.return_type)
    }

    /// Defines a variable in the innermost active scope.
    fn define(&mut self, name: &str, type_name: TypeName) {
        self.scopes
            .last_mut()
            .expect("a type checker always has a scope")
            .insert(name.to_owned(), type_name);
    }

    /// Finds a variable's type in the active lexical scopes.
    fn lookup_variable(&self, token: &Token) -> Result<TypeName, OrniError> {
        let name = identifier_name(token);
        self.scopes
            .iter()
            .rev()
            .find_map(|scope| scope.get(name).copied())
            .ok_or_else(|| semantic_error(token, &format!("undefined variable '{name}'")))
    }

    /// Reports a useful mismatch when an actual type differs from the expected type.
    fn require_type(
        &self,
        token: &Token,
        expected: TypeName,
        actual: TypeName,
        context: &str,
    ) -> Result<(), OrniError> {
        if expected == actual || (expected == TypeName::Double && actual == TypeName::Int) {
            Ok(())
        } else {
            Err(semantic_error(
                token,
                &format!("{context} expects {expected}, but found {actual}"),
            ))
        }
    }

    /// Reports an error when a value is neither an `int` nor a `double`.
    fn require_numeric(
        &self,
        token: &Token,
        actual: TypeName,
        context: &str,
    ) -> Result<(), OrniError> {
        if is_numeric(actual) {
            Ok(())
        } else {
            Err(semantic_error(
                token,
                &format!("{context} expects int or double, but found {actual}"),
            ))
        }
    }
}

/// Reports whether a type supports numeric operators.
fn is_numeric(type_name: TypeName) -> bool {
    matches!(type_name, TypeName::Int | TypeName::Double)
}

/// Chooses `double` when either numeric operand requires floating point.
fn numeric_result(left: TypeName, right: TypeName) -> TypeName {
    if left == TypeName::Double || right == TypeName::Double {
        TypeName::Double
    } else {
        TypeName::Int
    }
}

/// Returns whether a statement sequence guarantees a return value.
fn statements_return(statements: &[Statement]) -> bool {
    statements.iter().any(statement_returns)
}

/// Returns whether every path through a statement returns from its function.
fn statement_returns(statement: &Statement) -> bool {
    match statement {
        Statement::Return { .. } => true,
        Statement::Block(statements) => statements_return(statements),
        Statement::If {
            then_branch,
            else_branch: Some(else_branch),
            ..
        } => statement_returns(then_branch) && statement_returns(else_branch),
        _ => false,
    }
}

/// Extracts an identifier name from a parser-validated token.
fn identifier_name(token: &Token) -> &str {
    let TokenKind::Identifier(name) = &token.kind else {
        unreachable!("the parser only produces valid names")
    };
    name
}

/// Creates a semantic error positioned at the provided token.
fn semantic_error(token: &Token, message: &str) -> OrniError {
    OrniError::semantic(token.line, token.column, message)
}
