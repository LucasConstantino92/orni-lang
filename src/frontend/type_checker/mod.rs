mod expressions;
mod statements;

use std::collections::HashMap;

use crate::{
    diagnostics::OrniError,
    syntax::{IntegerType, Statement, Token, TokenKind, TypeName},
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
        if expected == actual
            || (expected == TypeName::Double && matches!(actual, TypeName::Integer(_)))
            || matches!(
                (expected, actual),
                (TypeName::Integer(_), TypeName::Integer(_))
            )
            || string_type_accepts(expected, actual)
        {
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
    matches!(type_name, TypeName::Integer(_) | TypeName::Double)
}

/// Reports whether an actual string capacity satisfies a declared capacity.
fn string_type_accepts(expected: TypeName, actual: TypeName) -> bool {
    match (expected, actual) {
        (TypeName::String(None), TypeName::String(_)) => true,
        (TypeName::String(Some(maximum)), TypeName::String(Some(actual))) => actual <= maximum,
        _ => false,
    }
}

/// Chooses `double` when either numeric operand requires floating point.
fn numeric_result(left: TypeName, right: TypeName) -> TypeName {
    match (left, right) {
        (TypeName::Double, _) | (_, TypeName::Double) => TypeName::Double,
        (TypeName::Integer(left), TypeName::Integer(right)) if left == right => {
            TypeName::Integer(left)
        }
        (TypeName::Integer(_), TypeName::Integer(_)) => TypeName::Integer(IntegerType::Int64),
        _ => unreachable!("numeric operands were validated before promotion"),
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
