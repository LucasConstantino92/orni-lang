use std::collections::{HashMap, HashSet};

use crate::{
    diagnostics::{OrniDiagnostic, OrniError},
    syntax::{ModelFieldRule, Parameter, Statement, Token, TokenKind, TypeName},
};

use super::{FunctionSignature, TypeChecker, identifier_name, semantic_error, statements_return};

impl TypeChecker {
    /// Registers contracts, providers, bindings, and provider methods.
    pub(super) fn declare_framework(&mut self, statements: &[Statement]) -> Vec<OrniError> {
        let mut errors = Vec::new();
        for statement in statements {
            let Statement::Model { name, fields } = statement else {
                continue;
            };
            let model_name = identifier_name(name);
            if self
                .models
                .insert(model_name.to_owned(), fields.clone())
                .is_some()
            {
                errors.push(
                    semantic_error(name, &format!("model '{model_name}' is already declared"))
                        .with_code("ORNI-E116"),
                );
                continue;
            }
            for field in fields {
                if !matches!(
                    field.type_name,
                    TypeName::String(_)
                        | TypeName::Integer(_)
                        | TypeName::Double
                        | TypeName::Bool
                        | TypeName::Char
                        | TypeName::Uuid
                ) {
                    errors.push(
                        semantic_error(
                            &field.name,
                            "model fields must use a built-in value type in this preview",
                        )
                        .with_code("ORNI-E107"),
                    );
                }
                if field.rules.contains(&ModelFieldRule::Email)
                    && !matches!(field.type_name, TypeName::String(_))
                {
                    errors.push(
                        semantic_error(&field.name, "the 'email' rule requires a string field")
                            .with_code("ORNI-E107"),
                    );
                }
                if let Some(default) = &field.default_value {
                    if !matches!(default, crate::syntax::Expression::Literal(_)) {
                        errors.push(
                            semantic_error(
                                &field.name,
                                "model field defaults must use literal values",
                            )
                            .with_code("ORNI-E107"),
                        );
                    } else if let Err(error) = self
                        .require_literal_range(&field.name, &field.type_name, default)
                        .and_then(|_| {
                            self.expression_type(default).and_then(|actual| {
                                self.require_type(
                                    &field.name,
                                    field.type_name.clone(),
                                    actual,
                                    "model field default",
                                )
                            })
                        })
                    {
                        errors.push(error);
                    }
                }
                let allowed_values = field.rules.iter().find_map(|rule| match rule {
                    ModelFieldRule::OneOf(values) => Some(values),
                    _ => None,
                });
                if let Some(values) = allowed_values {
                    if !matches!(field.type_name, TypeName::String(_)) {
                        errors.push(
                            semantic_error(
                                &field.name,
                                "the 'one_of' rule requires a string field",
                            )
                            .with_code("ORNI-E107"),
                        );
                    }
                    if let TypeName::String(Some(capacity)) = field.type_name
                        && values.iter().any(|value| value.len() > capacity)
                    {
                        errors.push(
                            semantic_error(
                                &field.name,
                                "'one_of' values cannot exceed the declared string capacity",
                            )
                            .with_code("ORNI-E106"),
                        );
                    }
                }
                let minimum = field.rules.iter().find_map(|rule| match rule {
                    ModelFieldRule::Min(value) => Some(*value),
                    _ => None,
                });
                let maximum = field.rules.iter().find_map(|rule| match rule {
                    ModelFieldRule::Max(value) => Some(*value),
                    _ => None,
                });
                if (minimum.is_some() || maximum.is_some())
                    && !matches!(
                        field.type_name,
                        TypeName::String(_) | TypeName::Integer(_) | TypeName::Double
                    )
                {
                    errors.push(
                        semantic_error(
                            &field.name,
                            "the 'min' and 'max' rules require a string or numeric field",
                        )
                        .with_code("ORNI-E107"),
                    );
                }
                if matches!(field.type_name, TypeName::String(_))
                    && (minimum.is_some_and(|value| value < 0)
                        || maximum.is_some_and(|value| value < 0))
                {
                    errors.push(
                        semantic_error(
                            &field.name,
                            "string 'min' and 'max' rules must use non-negative byte lengths",
                        )
                        .with_code("ORNI-E107"),
                    );
                }
                if let (Some(minimum), Some(maximum)) = (minimum, maximum)
                    && minimum > maximum
                {
                    errors.push(
                        semantic_error(
                            &field.name,
                            "the 'min' rule cannot be greater than the 'max' rule",
                        )
                        .with_code("ORNI-E107"),
                    );
                }
                if let TypeName::String(Some(capacity)) = field.type_name
                    && (minimum.is_some_and(|value| value >= 0 && value as u128 > capacity as u128)
                        || maximum
                            .is_some_and(|value| value >= 0 && value as u128 > capacity as u128))
                {
                    errors.push(
                        semantic_error(
                            &field.name,
                            "string rule boundaries cannot exceed the declared string capacity",
                        )
                        .with_code("ORNI-E107"),
                    );
                }
                if let TypeName::Integer(integer_type) = field.type_name {
                    for boundary in [minimum, maximum].into_iter().flatten() {
                        if !integer_type.contains(boundary) {
                            errors.push(
                                semantic_error(
                                    &field.name,
                                    "integer rule boundaries must fit the declared integer type",
                                )
                                .with_code("ORNI-E104"),
                            );
                        }
                    }
                }
            }
        }
        for statement in statements {
            let Statement::Store {
                keyword,
                name,
                model,
            } = statement
            else {
                continue;
            };
            let store_name = identifier_name(name);
            let model_name = identifier_name(model);
            let Some(fields) = self.models.get(model_name) else {
                errors.push(
                    semantic_error(
                        model,
                        &format!("store '{store_name}' references unknown model '{model_name}'"),
                    )
                    .with_code("ORNI-E103"),
                );
                continue;
            };
            if !fields.iter().any(|field| {
                identifier_name(&field.name) == "id" && matches!(field.type_name, TypeName::Uuid)
            }) {
                errors.push(
                    semantic_error(keyword, "store models must declare an 'id: uuid' field")
                        .with_code("ORNI-E107"),
                );
                continue;
            }
            if self
                .stores
                .insert(store_name.to_owned(), model_name.to_owned())
                .is_some()
            {
                errors.push(
                    semantic_error(name, &format!("store '{store_name}' is already declared"))
                        .with_code("ORNI-E116"),
                );
            }
        }
        for statement in statements {
            let Statement::Contract { name, methods } = statement else {
                continue;
            };
            let contract_name = identifier_name(name);
            if self.contracts.contains_key(contract_name) {
                errors.push(
                    semantic_error(
                        name,
                        &format!("contract '{contract_name}' is already declared"),
                    )
                    .with_code("ORNI-E116"),
                );
                continue;
            }
            let mut signatures = HashMap::new();
            for method in methods {
                let method_name = identifier_name(&method.name);
                if method
                    .parameters
                    .iter()
                    .any(|parameter| parameter.default_value.is_some())
                {
                    errors.push(semantic_error(
                        &method.name,
                        "contract methods cannot declare default values; pass arguments explicitly",
                    ).with_code("ORNI-E111"));
                }
                if signatures.contains_key(method_name) {
                    errors.push(
                        semantic_error(
                            &method.name,
                            &format!("contract method '{method_name}' is already declared"),
                        )
                        .with_code("ORNI-E116"),
                    );
                    continue;
                }
                signatures.insert(
                    method_name.to_owned(),
                    FunctionSignature {
                        parameters: method.parameters.clone(),
                        return_type: method.return_type.clone(),
                        body: Vec::new().into(),
                    },
                );
            }
            self.contracts.insert(contract_name.to_owned(), signatures);
        }

        for statement in statements {
            let Statement::Provider {
                name,
                dependencies: _,
                contract,
                methods,
            } = statement
            else {
                continue;
            };
            let provider_name = identifier_name(name);
            let contract_name = identifier_name(contract);
            let Some(required_methods) = self.contracts.get(contract_name).cloned() else {
                errors.push(semantic_error(
                    contract,
                    &format!(
                        "provider '{provider_name}' references unknown contract '{contract_name}'"
                    ),
                ).with_code("ORNI-E113"));
                continue;
            };
            if self
                .providers
                .insert(provider_name.to_owned(), contract_name.to_owned())
                .is_some()
            {
                errors.push(
                    semantic_error(
                        name,
                        &format!("provider '{provider_name}' is already declared"),
                    )
                    .with_code("ORNI-E116"),
                );
                continue;
            }
            let mut implemented = HashMap::new();
            for method in methods {
                let Statement::Function {
                    name: method_name,
                    parameters,
                    return_type,
                    body,
                } = method
                else {
                    unreachable!("providers contain only parser-validated functions")
                };
                let method_name_text = identifier_name(method_name);
                if implemented
                    .insert(method_name_text.to_owned(), ())
                    .is_some()
                {
                    errors.push(
                        semantic_error(
                            method_name,
                            &format!("provider method '{method_name_text}' is already declared"),
                        )
                        .with_code("ORNI-E116"),
                    );
                    continue;
                }
                if let Some(required) = required_methods.get(method_name_text)
                    && (required.parameters.len() != parameters.len()
                        || required
                            .parameters
                            .iter()
                            .zip(parameters)
                            .any(|(left, right)| left.type_name != right.type_name)
                        || required.return_type != *return_type)
                {
                    errors.push(semantic_error(
                        method_name,
                        &format!(
                            "provider method '{method_name_text}' does not match contract '{contract_name}'"
                        ),
                    ).with_code("ORNI-E110"));
                }
                self.functions.insert(
                    format!("{provider_name}.{method_name_text}"),
                    FunctionSignature {
                        parameters: parameters.clone(),
                        return_type: return_type.clone(),
                        body: body.clone().into(),
                    },
                );
            }
            for required_name in required_methods.keys() {
                if !implemented.contains_key(required_name) {
                    errors.push(semantic_error(
                        name,
                        &format!(
                            "provider '{provider_name}' is missing contract method '{required_name}'"
                        ),
                    ).with_code("ORNI-E109"));
                }
            }
        }

        for statement in statements {
            let Statement::Bind {
                contract, provider, ..
            } = statement
            else {
                continue;
            };
            let contract_name = identifier_name(contract);
            let provider_name = identifier_name(provider);
            if !self.contracts.contains_key(contract_name) {
                errors.push(
                    semantic_error(
                        contract,
                        &format!("cannot bind unknown contract '{contract_name}'"),
                    )
                    .with_code("ORNI-E113"),
                );
                continue;
            }
            match self.providers.get(provider_name) {
                Some(implemented_contract) if implemented_contract == contract_name => {}
                Some(implemented_contract) => {
                    errors.push(semantic_error(
                        provider,
                        &format!(
                            "provider '{provider_name}' implements '{implemented_contract}', not '{contract_name}'"
                        ),
                    ).with_code("ORNI-E114"));
                    continue;
                }
                None => {
                    errors.push(
                        semantic_error(
                            provider,
                            &format!("cannot bind unknown provider '{provider_name}'"),
                        )
                        .with_code("ORNI-E113"),
                    );
                    continue;
                }
            }
            if self
                .bindings
                .insert(contract_name.to_owned(), provider_name.to_owned())
                .is_some()
            {
                errors.push(
                    semantic_error(
                        contract,
                        &format!("contract '{contract_name}' is already bound"),
                    )
                    .with_code("ORNI-E115"),
                );
            }
        }

        for statement in statements {
            let Statement::Provider {
                name: _,
                dependencies,
                ..
            } = statement
            else {
                continue;
            };
            let mut dependency_names = HashSet::new();
            for dependency in dependencies {
                let dependency_name = identifier_name(&dependency.name);
                if !dependency_names.insert(dependency_name) {
                    errors.push(
                        semantic_error(
                            &dependency.name,
                            &format!("provider dependency '{dependency_name}' is already declared"),
                        )
                        .with_code("ORNI-E116"),
                    );
                }
                if dependency.default_value.is_some() {
                    errors.push(
                        semantic_error(
                            &dependency.name,
                            "provider dependencies cannot declare default values",
                        )
                        .with_code("ORNI-E111"),
                    );
                }
                let TypeName::Contract(contract_name) = &dependency.type_name else {
                    errors.push(
                        semantic_error(
                            &dependency.name,
                            &format!(
                                "provider dependency '{dependency_name}' must use a contract type"
                            ),
                        )
                        .with_code("ORNI-E112"),
                    );
                    continue;
                };
                if !self.contracts.contains_key(contract_name) {
                    errors.push(
                        semantic_error(
                            &dependency.name,
                            &format!("unknown contract type '{contract_name}'"),
                        )
                        .with_code("ORNI-E113"),
                    );
                }
            }
        }
        if !self.allow_unbound {
            errors.extend(self.validate_dependency_graph(statements));
        }
        errors
    }

    /// Validates the complete provider graph after every declaration is known.
    fn validate_dependency_graph(&self, statements: &[Statement]) -> Vec<OrniError> {
        let mut provider_dependencies = HashMap::<String, Vec<(String, Token)>>::new();
        for statement in statements {
            let Statement::Provider {
                name, dependencies, ..
            } = statement
            else {
                continue;
            };
            let entries = dependencies
                .iter()
                .filter_map(|dependency| {
                    let TypeName::Contract(contract) = &dependency.type_name else {
                        return None;
                    };
                    self.contracts
                        .contains_key(contract)
                        .then(|| (contract.clone(), dependency.name.clone()))
                })
                .collect();
            provider_dependencies.insert(identifier_name(name).to_owned(), entries);
        }

        let mut incoming = HashSet::new();
        for provider in self.bindings.values() {
            if let Some(dependencies) = provider_dependencies.get(provider) {
                incoming.extend(
                    dependencies
                        .iter()
                        .filter(|(contract, _)| self.bindings.contains_key(contract))
                        .map(|(contract, _)| contract.clone()),
                );
            }
        }

        let mut roots = self
            .bindings
            .keys()
            .filter(|contract| !incoming.contains(*contract))
            .cloned()
            .collect::<Vec<_>>();
        roots.sort();
        let mut remaining = self.bindings.keys().cloned().collect::<Vec<_>>();
        remaining.sort();
        roots.extend(remaining);

        let mut errors = Vec::new();
        let mut visited = HashSet::new();
        let mut visiting = HashMap::new();
        let mut path = Vec::new();
        let mut reported = HashSet::new();
        for contract in roots {
            validate_contract_dependencies(
                &contract,
                &self.bindings,
                &provider_dependencies,
                &mut visited,
                &mut visiting,
                &mut path,
                &mut reported,
                &mut errors,
            );
        }

        errors
    }

    /// Reports bindings that cannot participate in dependency injection.
    pub(super) fn framework_warnings(&self, statements: &[Statement]) -> Vec<OrniDiagnostic> {
        statements
            .iter()
            .filter_map(|statement| {
                let Statement::Bind { contract, .. } = statement else {
                    return None;
                };
                let contract_name = identifier_name(contract);
                (!statements_request_contract(statements, contract_name)).then(|| {
                    OrniDiagnostic::semantic_warning(
                        contract,
                        "ORNI-W004",
                        format!("binding for contract '{contract_name}' is never injected"),
                    )
                })
            })
            .collect()
    }

    /// Makes top-level functions visible to each other before bodies are checked.
    pub(super) fn declare_functions(&mut self, statements: &[Statement]) -> Vec<OrniError> {
        let mut errors = Vec::new();
        for statement in statements {
            if let Statement::Function {
                name,
                parameters,
                return_type,
                body,
                ..
            } = statement
            {
                let function_name = identifier_name(name);
                if self.functions.contains_key(function_name) {
                    errors.push(
                        semantic_error(
                            name,
                            &format!("function '{function_name}' is already declared"),
                        )
                        .with_code("ORNI-E116"),
                    );
                    continue;
                }
                if function_name == "main" && !parameters.is_empty() {
                    errors.push(
                        semantic_error(
                            name,
                            "application entry function 'main' cannot declare parameters",
                        )
                        .with_code("ORNI-E108"),
                    );
                    continue;
                }
                if function_name == "main" && *return_type != TypeName::Unit {
                    errors.push(
                        semantic_error(name, "application entry function 'main' must return unit")
                            .with_code("ORNI-E117"),
                    );
                    continue;
                }
                self.functions.insert(
                    function_name.to_owned(),
                    FunctionSignature {
                        parameters: parameters.clone(),
                        return_type: return_type.clone(),
                        body: body.clone().into(),
                    },
                );
            }
        }
        errors
    }

    /// Checks one statement and any nested expressions or statements.
    pub(super) fn check_statement(&mut self, statement: &Statement) -> Result<(), OrniError> {
        match statement {
            Statement::Import { keyword, .. } => {
                return Err(
                    semantic_error(keyword, "imports require a file-based check or run")
                        .with_code("ORNI-E127"),
                );
            }
            Statement::Contract { .. }
            | Statement::Model { .. }
            | Statement::Store { .. }
            | Statement::Bind { .. } => {}
            Statement::Route {
                keyword,
                method,
                path,
                handler,
            } => {
                let TokenKind::String(path_value) = &path.kind else {
                    unreachable!("the parser only produces quoted route paths")
                };
                if !path_value.starts_with('/') {
                    return Err(semantic_error(path, "route paths must start with '/'")
                        .with_code("ORNI-E152"));
                }
                let parameters = route_parameters(path_value)
                    .map_err(|message| semantic_error(path, &message).with_code("ORNI-E152"))?;
                let route_parameter_count = parameters.len();
                let pattern = format!("{:?}:{}", method.kind, route_pattern(path_value));
                if !self.routes.insert(pattern) {
                    return Err(semantic_error(
                        path,
                        &format!("route '{path_value}' conflicts with an existing route pattern"),
                    )
                    .with_code("ORNI-E152"));
                }
                let Some(function) = self.functions.get(identifier_name(handler)) else {
                    return Err(semantic_error(
                        handler,
                        &format!("undefined route handler '{}'", identifier_name(handler)),
                    )
                    .with_code("ORNI-E103"));
                };
                let has_body = matches!(
                    method.kind,
                    TokenKind::Post | TokenKind::Put | TokenKind::Patch
                );
                if has_body
                    && function
                        .parameters
                        .last()
                        .is_none_or(|parameter| !matches!(parameter.type_name, TypeName::Model(_)))
                {
                    return Err(semantic_error(
                        keyword,
                        "POST and PUT route handlers must accept one model body as their final parameter",
                    )
                    .with_code("ORNI-E152"));
                }
                if has_body && !matches!(function.return_type, TypeName::Response) {
                    return Err(semantic_error(
                        keyword,
                        "POST and PUT route handlers must return response",
                    )
                    .with_code("ORNI-E152"));
                }
                if !has_body
                    && !matches!(
                        function.return_type,
                        TypeName::String(_) | TypeName::Response
                    )
                {
                    return Err(semantic_error(
                        keyword,
                        "GET route handlers must return string or response",
                    )
                    .with_code("ORNI-E152"));
                }
                let handler_path_parameters = if has_body {
                    function.parameters.len().saturating_sub(1)
                } else {
                    function.parameters.len()
                };
                if (has_body && handler_path_parameters != route_parameter_count)
                    || (!has_body && handler_path_parameters < route_parameter_count)
                {
                    return Err(semantic_error(
                        keyword,
                        &format!(
                            "route declares {} path parameter(s), but handler '{}' accepts {}",
                            route_parameter_count,
                            identifier_name(handler),
                            handler_path_parameters,
                        ),
                    )
                    .with_code("ORNI-E152"));
                }
                for (parameter, path_parameter) in function
                    .parameters
                    .iter()
                    .take(route_parameter_count)
                    .zip(parameters)
                {
                    if !matches!(
                        parameter.type_name,
                        TypeName::String(_)
                            | TypeName::Integer(_)
                            | TypeName::Double
                            | TypeName::Bool
                            | TypeName::Char
                            | TypeName::Uuid
                    ) {
                        return Err(semantic_error(
                            &parameter.name,
                            "route path parameters must use string, integer, double, bool, char, or uuid types",
                        )
                        .with_code("ORNI-E152"));
                    }
                    if identifier_name(&parameter.name) != path_parameter {
                        return Err(semantic_error(
                            &parameter.name,
                            &format!(
                                "handler parameter '{}' must match the route parameter ':{path_parameter}'",
                                identifier_name(&parameter.name),
                            ),
                        )
                        .with_code("ORNI-E152"));
                    }
                }
                if !has_body {
                    for parameter in function.parameters.iter().skip(route_parameter_count) {
                        if !matches!(
                            parameter.type_name,
                            TypeName::String(_)
                                | TypeName::Integer(_)
                                | TypeName::Double
                                | TypeName::Bool
                                | TypeName::Char
                                | TypeName::Uuid
                        ) {
                            return Err(semantic_error(
                                &parameter.name,
                                "GET query parameters must use string, integer, double, bool, char, or uuid types",
                            )
                            .with_code("ORNI-E152"));
                        }
                    }
                }
            }
            Statement::Provider {
                dependencies,
                methods,
                ..
            } => {
                self.scopes.push(HashMap::new());
                for dependency in dependencies {
                    self.define(
                        identifier_name(&dependency.name),
                        dependency.type_name.clone(),
                    );
                }
                let mut result = Ok(());
                for method in methods {
                    if let Err(error) = self.check_statement(method) {
                        result = Err(error);
                        break;
                    }
                }
                self.scopes.pop();
                result?;
            }
            Statement::Let {
                name,
                declared_type,
                value,
            } => {
                if self
                    .scopes
                    .last()
                    .is_some_and(|scope| scope.contains_key(identifier_name(name)))
                {
                    return Err(semantic_error(
                        name,
                        &format!(
                            "variable '{}' is already declared in this scope",
                            identifier_name(name)
                        ),
                    )
                    .with_code("ORNI-E116"));
                }
                let value_type = self.expression_type(value)?;
                if let Some(declared_type) = declared_type {
                    self.require_literal_range(name, declared_type, value)?;
                    self.require_type(
                        name,
                        declared_type.clone(),
                        value_type.clone(),
                        "variable value",
                    )?;
                }
                let variable_type = match (declared_type.clone(), value_type.clone()) {
                    (Some(TypeName::String(None)), TypeName::String(Some(_))) => value_type,
                    (Some(declared_type), _) => declared_type,
                    (None, _) => value_type,
                };
                self.define(identifier_name(name), variable_type);
            }
            Statement::Function {
                name,
                parameters,
                return_type,
                body,
                ..
            } => self.check_function(name, parameters, return_type.clone(), body)?,
            Statement::Print { expression, .. } | Statement::Expression(expression) => {
                self.expression_type(expression)?;
            }
            Statement::Assert {
                keyword,
                expression,
            } => {
                let actual = self.expression_type(expression)?;
                self.require_type(keyword, TypeName::Bool, actual, "assertion")?;
            }
            Statement::Return { keyword, value } => {
                let Some(expected) = self.current_return_type.clone() else {
                    return Err(semantic_error(
                        keyword,
                        "return can only be used inside a function",
                    )
                    .with_code("ORNI-E118"));
                };
                let actual = value
                    .as_ref()
                    .map(|expression| self.expression_type(expression))
                    .transpose()?
                    .unwrap_or(TypeName::Unit);
                if let Some(value) = value {
                    self.require_literal_range(keyword, &expected, value)?;
                }
                self.require_type(keyword, expected.clone(), actual.clone(), "return value")?;
                if expected == TypeName::String(None) {
                    self.record_inferred_string_return(actual);
                }
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
            Statement::Test { body, .. } => {
                self.scopes.push(HashMap::new());
                let result = self.check_statements(body);
                self.scopes.pop();
                result?;
            }
        }
        Ok(())
    }

    /// Checks a sequence of statements in its current scope.
    pub(super) fn check_statements(&mut self, statements: &[Statement]) -> Result<(), OrniError> {
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
        let previous_return_type = self.current_return_type.replace(return_type.clone());
        let previous_inferred_return_type = self.inferred_return_type.take();
        self.scopes.push(HashMap::new());

        let result = (|| {
            for parameter in parameters {
                if let TypeName::Contract(contract_name) = &parameter.type_name
                    && !self.contracts.contains_key(contract_name)
                {
                    return Err(semantic_error(
                        &parameter.name,
                        &format!("unknown contract type '{contract_name}'"),
                    )
                    .with_code("ORNI-E113"));
                }
                if let Some(default) = &parameter.default_value {
                    self.require_literal_range(&parameter.name, &parameter.type_name, default)?;
                    let default_type = self.expression_type(default)?;
                    self.require_type(
                        &parameter.name,
                        parameter.type_name.clone(),
                        default_type,
                        "default value",
                    )?;
                }
                self.define(
                    identifier_name(&parameter.name),
                    parameter.type_name.clone(),
                );
            }
            self.check_statements(body)?;
            if return_type != TypeName::Unit && !statements_return(body) {
                return Err(semantic_error(
                    name,
                    &format!(
                        "function '{}' must return a {return_type} value on every path",
                        identifier_name(name)
                    ),
                )
                .with_code("ORNI-E117"));
            }
            Ok(())
        })();

        self.scopes.pop();
        self.current_return_type = previous_return_type;
        self.inferred_return_type = previous_inferred_return_type;
        result
    }

    /// Remembers the largest bounded string returned by the current function.
    pub(super) fn record_inferred_string_return(&mut self, actual: TypeName) {
        let TypeName::String(Some(actual_capacity)) = actual else {
            return;
        };
        let capacity = match self.inferred_return_type {
            Some(TypeName::String(Some(previous_capacity))) => {
                previous_capacity.max(actual_capacity)
            }
            _ => actual_capacity,
        };
        self.inferred_return_type = Some(TypeName::String(Some(capacity)));
    }
}

/// Returns validated parameter names in a route path, in their URL order.
fn route_parameters(path: &str) -> Result<Vec<&str>, String> {
    let mut parameters = Vec::new();
    for segment in path.split('/').skip(1) {
        let Some(name) = segment.strip_prefix(':') else {
            continue;
        };
        if name.is_empty() || !is_identifier(name) {
            return Err(format!(
                "route parameter ':{name}' must use a valid identifier name"
            ));
        }
        if parameters.contains(&name) {
            return Err(format!(
                "route parameter ':{name}' is declared more than once"
            ));
        }
        parameters.push(name);
    }
    Ok(parameters)
}

/// Replaces path parameter names so structurally identical routes conflict.
fn route_pattern(path: &str) -> String {
    path.split('/')
        .map(|segment| {
            if segment.starts_with(':') {
                ":"
            } else {
                segment
            }
        })
        .collect::<Vec<_>>()
        .join("/")
}

/// Limits route parameter names to names that Orni functions can declare.
fn is_identifier(name: &str) -> bool {
    let mut characters = name.chars();
    matches!(characters.next(), Some(character) if character.is_ascii_alphabetic() || character == '_')
        && characters.all(|character| character.is_ascii_alphanumeric() || character == '_')
}

fn statements_request_contract(statements: &[Statement], contract: &str) -> bool {
    statements.iter().any(|statement| match statement {
        Statement::Provider {
            dependencies,
            methods,
            ..
        } => {
            parameters_request_contract(dependencies, contract)
                || statements_request_contract(methods, contract)
        }
        Statement::Contract { methods, .. } => methods
            .iter()
            .any(|method| parameters_request_contract(&method.parameters, contract)),
        Statement::Function {
            parameters, body, ..
        } => {
            parameters_request_contract(parameters, contract)
                || statements_request_contract(body, contract)
        }
        Statement::If {
            then_branch,
            else_branch,
            ..
        } => {
            statements_request_contract(std::slice::from_ref(then_branch.as_ref()), contract)
                || else_branch.as_ref().is_some_and(|branch| {
                    statements_request_contract(std::slice::from_ref(branch.as_ref()), contract)
                })
        }
        Statement::Block(statements) => statements_request_contract(statements, contract),
        Statement::Test { body, .. } => statements_request_contract(body, contract),
        Statement::Import { .. }
        | Statement::Model { .. }
        | Statement::Store { .. }
        | Statement::Bind { .. }
        | Statement::Route { .. }
        | Statement::Let { .. }
        | Statement::Print { .. }
        | Statement::Assert { .. }
        | Statement::Return { .. }
        | Statement::Expression(_) => false,
    })
}

fn parameters_request_contract(parameters: &[Parameter], contract: &str) -> bool {
    parameters.iter().any(
        |parameter| matches!(&parameter.type_name, TypeName::Contract(name) if name == contract),
    )
}

#[allow(clippy::too_many_arguments)]
fn validate_contract_dependencies(
    contract: &str,
    bindings: &HashMap<String, String>,
    provider_dependencies: &HashMap<String, Vec<(String, Token)>>,
    visited: &mut HashSet<String>,
    visiting: &mut HashMap<String, usize>,
    path: &mut Vec<String>,
    reported: &mut HashSet<String>,
    errors: &mut Vec<OrniError>,
) {
    if visited.contains(contract) {
        return;
    }
    visiting.insert(contract.to_owned(), path.len());
    path.push(contract.to_owned());

    if let Some(provider) = bindings.get(contract)
        && let Some(dependencies) = provider_dependencies.get(provider)
    {
        let mut dependencies = dependencies.iter().collect::<Vec<_>>();
        dependencies.sort_by(|left, right| left.0.cmp(&right.0));
        for (dependency, token) in dependencies {
            if let Some(cycle_start) = visiting.get(dependency).copied() {
                let mut cycle = path[cycle_start..].to_vec();
                cycle.push(dependency.clone());
                let chain = cycle.join(" -> ");
                if reported.insert(format!("cycle:{chain}")) {
                    errors.push(
                        semantic_error(
                            token,
                            &format!("circular provider dependency detected: {chain}"),
                        )
                        .with_code("ORNI-E101"),
                    );
                }
                continue;
            }
            if !bindings.contains_key(dependency) {
                let mut incomplete = path.clone();
                incomplete.push(dependency.clone());
                let chain = incomplete.join(" -> ");
                if reported.insert(format!("missing:{chain}")) {
                    errors.push(semantic_error(
                        token,
                        &format!(
                            "provider dependency chain is incomplete: {chain}; contract '{dependency}' has no provider binding"
                        ),
                    ).with_code("ORNI-E102"));
                }
                continue;
            }
            validate_contract_dependencies(
                dependency,
                bindings,
                provider_dependencies,
                visited,
                visiting,
                path,
                reported,
                errors,
            );
        }
    }

    path.pop();
    visiting.remove(contract);
    visited.insert(contract.to_owned());
}
