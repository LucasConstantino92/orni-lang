//! Static analysis helpers for module declarations and import usage.

use std::{collections::HashSet, path::Path};

use crate::{
    diagnostics::OrniError,
    syntax::{Expression, Statement, TypeName},
};

use super::transform::identifier_name;

pub(super) type ModuleDeclarations = (
    HashSet<String>,
    HashSet<String>,
    HashSet<String>,
    HashSet<String>,
    HashSet<String>,
    HashSet<String>,
);

pub(super) fn collect_module_declarations(
    statements: &[Statement],
    source_path: &Path,
) -> Result<ModuleDeclarations, OrniError> {
    let mut functions = HashSet::new();
    let mut variables = HashSet::new();
    let mut contracts = HashSet::new();
    let mut models = HashSet::new();
    let mut stores = HashSet::new();
    let mut providers = HashSet::new();
    let mut declarations = HashSet::new();

    for statement in statements {
        let (name, token, destination) = match statement {
            Statement::Function { name, .. } => (identifier_name(name), name, &mut functions),
            Statement::Let { name, .. } => (identifier_name(name), name, &mut variables),
            Statement::Contract { name, .. } => (identifier_name(name), name, &mut contracts),
            Statement::Model { name, .. } => (identifier_name(name), name, &mut models),
            Statement::Store { name, .. } => (identifier_name(name), name, &mut stores),
            Statement::Provider { name, .. } => (identifier_name(name), name, &mut providers),
            _ => continue,
        };
        if !declarations.insert(name.to_owned()) {
            return Err(OrniError::module(
                token.line,
                token.column,
                format!("module member '{name}' is already declared"),
            )
            .with_code("ORNI-E116")
            .with_token(token)
            .with_source(source_path));
        }
        destination.insert(name.to_owned());
    }

    Ok((functions, variables, contracts, models, stores, providers))
}

pub(super) fn namespace_is_used(statements: &[Statement], namespace: &str) -> bool {
    statements.iter().any(|statement| match statement {
        Statement::Import { .. } => false,
        Statement::Model { .. } => false,
        Statement::Store { .. } => false,
        Statement::Route { handler, .. } => identifier_name(handler)
            .strip_prefix(namespace)
            .is_some_and(|suffix| suffix.starts_with('.')),
        Statement::Contract { methods, .. } => methods.iter().any(|method| {
            method
                .parameters
                .iter()
                .any(|parameter| type_uses_namespace(&parameter.type_name, namespace))
                || type_uses_namespace(&method.return_type, namespace)
        }),
        Statement::Provider {
            dependencies,
            contract,
            methods,
            ..
        } => {
            identifier_name(contract)
                .strip_prefix(namespace)
                .is_some_and(|suffix| suffix.starts_with('.'))
                || dependencies.iter().any(|dependency| {
                    type_uses_namespace(&dependency.type_name, namespace)
                        || dependency
                            .default_value
                            .as_ref()
                            .is_some_and(|value| expression_uses_namespace(value, namespace))
                })
                || namespace_is_used(methods, namespace)
        }
        Statement::Bind {
            contract, provider, ..
        } => [contract, provider].iter().any(|token| {
            identifier_name(token)
                .strip_prefix(namespace)
                .is_some_and(|suffix| suffix.starts_with('.'))
        }),
        Statement::Let { value, .. } => expression_uses_namespace(value, namespace),
        Statement::Function {
            parameters, body, ..
        } => {
            parameters.iter().any(|parameter| {
                type_uses_namespace(&parameter.type_name, namespace)
                    || parameter
                        .default_value
                        .as_ref()
                        .is_some_and(|value| expression_uses_namespace(value, namespace))
            }) || namespace_is_used(body, namespace)
        }
        Statement::Print { expression, .. }
        | Statement::Assert { expression, .. }
        | Statement::Expression(expression) => expression_uses_namespace(expression, namespace),
        Statement::Return { value, .. } => value
            .as_ref()
            .is_some_and(|value| expression_uses_namespace(value, namespace)),
        Statement::If {
            condition,
            then_branch,
            else_branch,
            ..
        } => {
            expression_uses_namespace(condition, namespace)
                || namespace_is_used(std::slice::from_ref(then_branch.as_ref()), namespace)
                || else_branch.as_ref().is_some_and(|branch| {
                    namespace_is_used(std::slice::from_ref(branch.as_ref()), namespace)
                })
        }
        Statement::Block(statements) => namespace_is_used(statements, namespace),
        Statement::Test { body, .. } => namespace_is_used(body, namespace),
    })
}

fn expression_uses_namespace(expression: &Expression, namespace: &str) -> bool {
    match expression {
        Expression::Literal(_) => false,
        Expression::Variable(token) => identifier_name(token)
            .strip_prefix(namespace)
            .is_some_and(|suffix| suffix.starts_with('.')),
        Expression::Unary { right, .. } | Expression::Grouping(right) => {
            expression_uses_namespace(right, namespace)
        }
        Expression::Binary { left, right, .. } => {
            expression_uses_namespace(left, namespace)
                || expression_uses_namespace(right, namespace)
        }
        Expression::Call {
            callee, arguments, ..
        } => {
            expression_uses_namespace(callee, namespace)
                || arguments
                    .iter()
                    .any(|argument| expression_uses_namespace(argument, namespace))
        }
    }
}

pub(super) fn name_is_used(statements: &[Statement], name: &str) -> bool {
    statements.iter().any(|statement| match statement {
        Statement::Import { .. } => false,
        Statement::Model { .. } => false,
        Statement::Store { .. } => false,
        Statement::Route { handler, .. } => identifier_name(handler) == name,
        Statement::Contract { methods, .. } => methods.iter().any(|method| {
            method
                .parameters
                .iter()
                .any(|parameter| type_uses_name(&parameter.type_name, name))
                || type_uses_name(&method.return_type, name)
        }),
        Statement::Provider {
            dependencies,
            contract,
            methods,
            ..
        } => {
            identifier_name(contract) == name
                || dependencies.iter().any(|dependency| {
                    type_uses_name(&dependency.type_name, name)
                        || dependency
                            .default_value
                            .as_ref()
                            .is_some_and(|value| expression_uses_name(value, name))
                })
                || name_is_used(methods, name)
        }
        Statement::Bind {
            contract, provider, ..
        } => identifier_name(contract) == name || identifier_name(provider) == name,
        Statement::Let { value, .. } => expression_uses_name(value, name),
        Statement::Function {
            parameters, body, ..
        } => {
            parameters.iter().any(|parameter| {
                type_uses_name(&parameter.type_name, name)
                    || parameter
                        .default_value
                        .as_ref()
                        .is_some_and(|value| expression_uses_name(value, name))
            }) || name_is_used(body, name)
        }
        Statement::Print { expression, .. }
        | Statement::Assert { expression, .. }
        | Statement::Expression(expression) => expression_uses_name(expression, name),
        Statement::Return { value, .. } => value
            .as_ref()
            .is_some_and(|value| expression_uses_name(value, name)),
        Statement::If {
            condition,
            then_branch,
            else_branch,
            ..
        } => {
            expression_uses_name(condition, name)
                || name_is_used(std::slice::from_ref(then_branch.as_ref()), name)
                || else_branch
                    .as_ref()
                    .is_some_and(|branch| name_is_used(std::slice::from_ref(branch.as_ref()), name))
        }
        Statement::Block(statements) => name_is_used(statements, name),
        Statement::Test { body, .. } => name_is_used(body, name),
    })
}

fn expression_uses_name(expression: &Expression, name: &str) -> bool {
    match expression {
        Expression::Literal(_) => false,
        Expression::Variable(token) => identifier_name(token) == name,
        Expression::Unary { right, .. } | Expression::Grouping(right) => {
            expression_uses_name(right, name)
        }
        Expression::Binary { left, right, .. } => {
            expression_uses_name(left, name) || expression_uses_name(right, name)
        }
        Expression::Call {
            callee, arguments, ..
        } => {
            expression_uses_name(callee, name)
                || arguments
                    .iter()
                    .any(|argument| expression_uses_name(argument, name))
        }
    }
}

fn type_uses_name(type_name: &TypeName, name: &str) -> bool {
    matches!(type_name, TypeName::Contract(contract) if contract == name)
}

fn type_uses_namespace(type_name: &TypeName, namespace: &str) -> bool {
    matches!(type_name, TypeName::Contract(contract) if contract
        .strip_prefix(namespace)
        .is_some_and(|suffix| suffix.starts_with('.')))
}
