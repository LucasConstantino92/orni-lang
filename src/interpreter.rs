use std::collections::HashMap;

use crate::{
    ast::{Expression, Statement, Value},
    environment::Environment,
    error::OrniError,
    token::{Token, TokenKind},
};

#[derive(Clone)]
/// A function declaration stored for later invocation.
struct Function {
    parameters: Vec<Token>,
    body: Vec<Statement>,
}

/// Evaluates an Orni abstract syntax tree and captures printed output.
pub(crate) struct Interpreter {
    environment: Environment,
    functions: HashMap<String, Function>,
    output: String,
}

impl Interpreter {
    /// Creates an interpreter with empty global state and output.
    pub fn new() -> Self {
        Self {
            environment: Environment::new(),
            functions: HashMap::new(),
            output: String::new(),
        }
    }

    /// Executes a complete program and returns its captured output.
    pub fn execute(mut self, program: &[Statement]) -> Result<String, OrniError> {
        for statement in program {
            self.execute_statement(statement)?;
        }
        Ok(self.output)
    }

    /// Executes one statement and updates interpreter state when necessary.
    fn execute_statement(&mut self, statement: &Statement) -> Result<(), OrniError> {
        match statement {
            Statement::Let { name, value } => {
                let identifier = identifier_name(name);
                let value = self.evaluate(value)?;
                self.environment.define(identifier.to_owned(), value);
            }
            Statement::Function {
                name,
                parameters,
                body,
            } => {
                self.functions.insert(
                    identifier_name(name).to_owned(),
                    Function {
                        parameters: parameters.clone(),
                        body: body.clone(),
                    },
                );
            }
            Statement::Print(expression) => {
                let value = self.evaluate(expression)?;
                self.output.push_str(&value.to_string());
                self.output.push('\n');
            }
            Statement::Expression(expression) => {
                self.evaluate(expression)?;
            }
        }
        Ok(())
    }

    /// Evaluates one expression into a runtime value.
    fn evaluate(&mut self, expression: &Expression) -> Result<Value, OrniError> {
        match expression {
            Expression::Literal(value) => Ok(value.clone()),
            Expression::Grouping(expression) => self.evaluate(expression),
            Expression::Variable(token) => {
                let name = identifier_name(token);
                self.environment.get(name).cloned().ok_or_else(|| {
                    OrniError::runtime(
                        token.line,
                        token.column,
                        format!("undefined variable '{name}'"),
                    )
                })
            }
            Expression::Unary { operator, right } => {
                let right = self.evaluate(right)?;
                match (&operator.kind, right) {
                    (TokenKind::Minus, Value::Number(number)) => Ok(Value::Number(-number)),
                    (TokenKind::Minus, _) => {
                        Err(runtime_error(operator, "'-' only accepts numbers"))
                    }
                    _ => unreachable!("unary operator validated by the parser"),
                }
            }
            Expression::Binary {
                left,
                operator,
                right,
            } => {
                let left = self.evaluate(left)?;
                let right = self.evaluate(right)?;
                self.binary(left, operator, right)
            }
            Expression::Call {
                callee,
                closing_parenthesis,
                arguments,
            } => self.call(callee, closing_parenthesis, arguments),
        }
    }

    /// Invokes a named function with evaluated argument values.
    fn call(
        &mut self,
        callee: &Expression,
        closing_parenthesis: &Token,
        arguments: &[Expression],
    ) -> Result<Value, OrniError> {
        let Expression::Variable(name_token) = callee else {
            return Err(runtime_error(
                closing_parenthesis,
                "only named functions can be called",
            ));
        };
        let name = identifier_name(name_token);
        let function = self.functions.get(name).cloned().ok_or_else(|| {
            OrniError::runtime(
                name_token.line,
                name_token.column,
                format!("undefined function '{name}'"),
            )
        })?;

        if arguments.len() != function.parameters.len() {
            return Err(runtime_error(
                closing_parenthesis,
                &format!(
                    "function '{name}' expects {} argument(s), but received {}",
                    function.parameters.len(),
                    arguments.len()
                ),
            ));
        }

        let values = arguments
            .iter()
            .map(|argument| self.evaluate(argument))
            .collect::<Result<Vec<_>, _>>()?;

        self.environment.enter_function_scope();
        for (parameter, value) in function.parameters.iter().zip(values) {
            self.environment
                .define(identifier_name(parameter).to_owned(), value);
        }
        let result = function
            .body
            .iter()
            .try_for_each(|statement| self.execute_statement(statement));
        self.environment.exit_function_scope();
        result?;

        Ok(Value::Unit)
    }

    /// Applies a binary operator to two already evaluated operands.
    fn binary(&self, left: Value, operator: &Token, right: Value) -> Result<Value, OrniError> {
        match (&operator.kind, left, right) {
            (TokenKind::Plus, Value::Number(left), Value::Number(right)) => {
                Ok(Value::Number(left + right))
            }
            (TokenKind::Minus, Value::Number(left), Value::Number(right)) => {
                Ok(Value::Number(left - right))
            }
            (TokenKind::Star, Value::Number(left), Value::Number(right)) => {
                Ok(Value::Number(left * right))
            }
            (TokenKind::Slash, Value::Number(_), Value::Number(0.0)) => {
                Err(runtime_error(operator, "division by zero"))
            }
            (TokenKind::Slash, Value::Number(left), Value::Number(right)) => {
                Ok(Value::Number(left / right))
            }
            (TokenKind::Plus, Value::String(left), right) => {
                Ok(Value::String(left + &right.to_string()))
            }
            (TokenKind::Plus, left, Value::String(right)) => {
                Ok(Value::String(left.to_string() + &right))
            }
            (TokenKind::Plus | TokenKind::Minus | TokenKind::Star | TokenKind::Slash, _, _) => Err(
                runtime_error(operator, "incompatible types for this operation"),
            ),
            _ => unreachable!("binary operator validated by the parser"),
        }
    }
}

/// Extracts an identifier name from a parser-validated token.
fn identifier_name(token: &Token) -> &str {
    let TokenKind::Identifier(name) = &token.kind else {
        unreachable!("the parser only produces valid names")
    };
    name
}

/// Creates a runtime error positioned at the provided token.
fn runtime_error(token: &Token, message: &str) -> OrniError {
    OrniError::runtime(token.line, token.column, message)
}
