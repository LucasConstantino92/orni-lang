//! Namespace transformation for resolved modules.
//!
//! Imports have already been resolved by the loader when this layer rewrites
//! local declarations and references into their final module identities.

use std::collections::{HashMap, HashSet};

use crate::{
    diagnostics::OrniError,
    syntax::{Expression, Statement, Token, TokenKind, TypeName},
};

/// Immutable module information shared while names are rewritten.
pub(super) struct TransformContext<'a> {
    pub(super) namespace: Option<&'a str>,
    pub(super) aliases: &'a HashMap<String, String>,
    pub(super) selected_imports: &'a HashMap<String, String>,
    pub(super) module_functions: &'a HashSet<String>,
    pub(super) module_variables: &'a HashSet<String>,
    pub(super) module_contracts: &'a HashSet<String>,
    pub(super) module_models: &'a HashSet<String>,
    pub(super) module_stores: &'a HashSet<String>,
    pub(super) module_providers: &'a HashSet<String>,
}

pub(super) fn transform_statement(
    statement: &mut Statement,
    context: &TransformContext<'_>,
    locals: &mut HashSet<String>,
    module_level: bool,
) -> Result<(), OrniError> {
    match statement {
        Statement::Import { .. } => unreachable!("imports are removed before transformation"),
        Statement::Model { name, .. } => {
            if module_level && let Some(namespace) = context.namespace {
                qualify_token(name, namespace);
            }
        }
        Statement::Store { name, model, .. } => {
            transform_reference_token(model, context, context.module_models);
            if module_level && let Some(namespace) = context.namespace {
                qualify_token(name, namespace);
            }
        }
        Statement::Contract { name, methods } => {
            for method in methods {
                for parameter in &mut method.parameters {
                    transform_type_name(&mut parameter.type_name, context);
                    if let Some(default) = &mut parameter.default_value {
                        transform_expression(default, context, locals)?;
                    }
                }
                transform_type_name(&mut method.return_type, context);
            }
            if module_level && let Some(namespace) = context.namespace {
                qualify_token(name, namespace);
            }
        }
        Statement::Provider {
            name,
            dependencies,
            contract,
            methods,
        } => {
            transform_reference_token(contract, context, context.module_contracts);
            let mut provider_locals = locals.clone();
            for dependency in dependencies {
                transform_type_name(&mut dependency.type_name, context);
                if let Some(default) = &mut dependency.default_value {
                    transform_expression(default, context, &provider_locals)?;
                }
                provider_locals.insert(identifier_name(&dependency.name).to_owned());
            }
            for method in methods {
                transform_statement(method, context, &mut provider_locals, false)?;
            }
            if module_level && let Some(namespace) = context.namespace {
                qualify_token(name, namespace);
            }
        }
        Statement::Bind {
            contract, provider, ..
        } => {
            transform_reference_token(contract, context, context.module_contracts);
            transform_reference_token(provider, context, context.module_providers);
        }
        Statement::Route { handler, .. } => {
            transform_reference_token(handler, context, context.module_functions);
        }
        Statement::Let { name, value, .. } => {
            transform_expression(value, context, locals)?;
            if module_level && let Some(namespace) = context.namespace {
                qualify_token(name, namespace);
            } else {
                locals.insert(identifier_name(name).to_owned());
            }
        }
        Statement::Function {
            name,
            parameters,
            return_type,
            body,
        } => {
            for parameter in &mut *parameters {
                transform_type_name(&mut parameter.type_name, context);
            }
            let mut function_locals = parameters
                .iter()
                .map(|parameter| identifier_name(&parameter.name).to_owned())
                .collect::<HashSet<_>>();
            for parameter in parameters {
                if let Some(default) = &mut parameter.default_value {
                    transform_expression(default, context, &function_locals)?;
                }
            }
            transform_type_name(return_type, context);
            for statement in body {
                transform_statement(statement, context, &mut function_locals, false)?;
            }
            if module_level && let Some(namespace) = context.namespace {
                qualify_token(name, namespace);
            }
        }
        Statement::Print { expression, .. }
        | Statement::Assert { expression, .. }
        | Statement::Expression(expression) => {
            transform_expression(expression, context, locals)?;
        }
        Statement::Return { value, .. } => {
            if let Some(value) = value {
                transform_expression(value, context, locals)?;
            }
        }
        Statement::If {
            condition,
            then_branch,
            else_branch,
            ..
        } => {
            transform_expression(condition, context, locals)?;
            let mut then_locals = locals.clone();
            transform_statement(then_branch, context, &mut then_locals, false)?;
            if let Some(else_branch) = else_branch {
                let mut else_locals = locals.clone();
                transform_statement(else_branch, context, &mut else_locals, false)?;
            }
        }
        Statement::Block(statements) => {
            let mut block_locals = locals.clone();
            for statement in statements {
                transform_statement(statement, context, &mut block_locals, false)?;
            }
        }
        Statement::Test { body, .. } => {
            let mut test_locals = locals.clone();
            for statement in body {
                transform_statement(statement, context, &mut test_locals, false)?;
            }
        }
    }
    Ok(())
}

fn transform_expression(
    expression: &mut Expression,
    context: &TransformContext<'_>,
    locals: &HashSet<String>,
) -> Result<(), OrniError> {
    match expression {
        Expression::Literal(_) => {}
        Expression::Grouping(expression) => transform_expression(expression, context, locals)?,
        Expression::Variable(token) => {
            let name = identifier_name(token).to_owned();
            if let Some((prefix, member)) = name.split_once('.') {
                if let Some(target_namespace) = context.aliases.get(prefix) {
                    if member.split('.').any(|part| part.starts_with('_')) {
                        return Err(OrniError::module(
                            token.line,
                            token.column,
                            format!(
                                "private member '{member}' cannot be accessed outside its module"
                            ),
                        )
                        .with_code("ORNI-E124")
                        .with_token(token));
                    }
                    set_identifier(token, format!("{target_namespace}.{member}"));
                } else if let Some(selected) = context.selected_imports.get(prefix) {
                    set_identifier(token, format!("{selected}.{member}"));
                } else if context.module_stores.contains(prefix)
                    && let Some(namespace) = context.namespace
                {
                    set_identifier(token, format!("{namespace}.{name}"));
                }
            } else if !locals.contains(&name)
                && (context.module_functions.contains(&name)
                    || context.module_variables.contains(&name))
                && let Some(namespace) = context.namespace
            {
                set_identifier(token, format!("{namespace}.{name}"));
            } else if !locals.contains(&name)
                && let Some(imported_name) = context.selected_imports.get(&name)
            {
                set_identifier(token, imported_name.clone());
            }
        }
        Expression::Unary { right, .. } => transform_expression(right, context, locals)?,
        Expression::Binary { left, right, .. } => {
            transform_expression(left, context, locals)?;
            transform_expression(right, context, locals)?;
        }
        Expression::Call {
            callee, arguments, ..
        } => {
            transform_expression(callee, context, locals)?;
            for argument in arguments {
                transform_expression(argument, context, locals)?;
            }
        }
    }
    Ok(())
}

fn transform_type_name(type_name: &mut TypeName, context: &TransformContext<'_>) {
    match type_name {
        TypeName::Contract(name) => {
            *name = resolved_reference(name, context, context.module_contracts);
        }
        TypeName::Model(name) => {
            *name = resolved_reference(name, context, context.module_models);
        }
        _ => {}
    }
}

fn transform_reference_token(
    token: &mut Token,
    context: &TransformContext<'_>,
    local_declarations: &HashSet<String>,
) {
    let resolved = resolved_reference(identifier_name(token), context, local_declarations);
    set_identifier(token, resolved);
}

fn resolved_reference(
    name: &str,
    context: &TransformContext<'_>,
    local_declarations: &HashSet<String>,
) -> String {
    if let Some(selected) = context.selected_imports.get(name) {
        return selected.clone();
    }
    if local_declarations.contains(name)
        && let Some(namespace) = context.namespace
    {
        return format!("{namespace}.{name}");
    }
    if let Some((prefix, member)) = name.split_once('.')
        && let Some(namespace) = context.aliases.get(prefix)
    {
        return format!("{namespace}.{member}");
    }
    name.to_owned()
}

fn qualify_token(token: &mut Token, namespace: &str) {
    let name = identifier_name(token).to_owned();
    set_identifier(token, format!("{namespace}.{name}"));
}

fn set_identifier(token: &mut Token, name: String) {
    token.kind = TokenKind::Identifier(name);
}

pub(super) fn identifier_name(token: &Token) -> &str {
    let TokenKind::Identifier(name) = &token.kind else {
        unreachable!("module names contain only parser-validated identifiers")
    };
    name
}
