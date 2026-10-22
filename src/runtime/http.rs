use std::{collections::BTreeMap, rc::Rc};

use crate::syntax::{
    Expression, ModelField, ModelFieldRule, Statement, Token, TokenKind, TypeName, Value,
};

use super::{AuthConfig, ExecutionLimits, Interpreter, MemoryStore, MemoryStores};

pub(crate) mod auth;
mod request;
mod response;
mod server;

use response::{HttpResponse, response_value};
pub(crate) use server::serve;
pub(super) const MAX_REQUEST_LINE_BYTES: usize = 8_192;
pub(super) const MAX_REQUEST_HEADER_BYTES: usize = 32_768;
pub(super) const MAX_REQUEST_HEADERS: usize = 64;

/// One validated route and the parameter names it receives from the URL.
pub(super) struct Route {
    method: &'static str,
    path: String,
    handler: String,
    parameters: Vec<String>,
    path_parameter_types: Vec<TypeName>,
    query_parameters: Vec<QueryParameter>,
    body_model: Option<ModelSchema>,
}

/// One typed GET query parameter declared by a route handler.
struct QueryParameter {
    name: String,
    type_name: TypeName,
    has_default: bool,
}

/// In-memory application state used by Orni API test files.
pub(crate) struct ApiTestContext {
    program: Rc<[Statement]>,
    stores: MemoryStores,
    limits: ExecutionLimits,
}

/// Prepares one application for route assertions without opening a TCP listener.
pub(crate) fn api_test_context(program: &[Statement], limits: ExecutionLimits) -> ApiTestContext {
    ApiTestContext {
        program: program.to_vec().into(),
        stores: memory_stores(program),
        limits,
    }
}

/// One in-memory HTTP response exposed to Orni API test helpers.
pub(crate) struct ApiTestResponse {
    pub(crate) status: u16,
    pub(crate) body: String,
    headers: Vec<(String, String)>,
}

impl ApiTestResponse {
    /// Reads one response header without making header-name casing observable.
    pub(crate) fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(candidate, _)| candidate.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }
}

/// Executes one in-memory request for Orni API test helpers.
pub(crate) fn api_test_request(
    context: &ApiTestContext,
    method: &str,
    path: &str,
    body: &str,
    request_header: Option<(&str, &str)>,
) -> ApiTestResponse {
    let routes = route_table(&context.program);
    let mut response = route_response_with_stores(
        method,
        path,
        body,
        &context.program,
        &routes,
        context.stores.clone(),
        context.limits,
    );
    if let Some((name, value)) = request_header
        && name.eq_ignore_ascii_case("if-none-match")
    {
        response = response.not_modified_if_matches(Some(value));
    }
    ApiTestResponse {
        status: response.status,
        body: response.body,
        headers: response.headers,
    }
}

/// Schema kept by the HTTP boundary to validate a JSON request body.
#[derive(Clone)]
struct ModelSchema {
    name: String,
    fields: Vec<ModelField>,
}

/// A client body validation failure, optionally tied to one JSON field.
struct BodyValidationError {
    message: String,
    field: Option<String>,
}

impl BodyValidationError {
    fn general(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            field: None,
        }
    }

    fn field(name: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            field: Some(name.into()),
        }
    }
}

/// Creates the collections retained for the lifetime of one HTTP server.
fn memory_stores(program: &[Statement]) -> MemoryStores {
    std::sync::Arc::new(std::sync::Mutex::new(
        program
            .iter()
            .filter_map(|statement| match statement {
                Statement::Store { name, model, .. } => program.iter().find_map(|candidate| {
                    let Statement::Model {
                        name: model_name,
                        fields,
                    } = candidate
                    else {
                        return None;
                    };
                    (handler_name(model_name) == handler_name(model)).then(|| {
                        (
                            handler_name(name).to_owned(),
                            MemoryStore {
                                model: handler_name(model).to_owned(),
                                fields: fields.clone(),
                                values: Default::default(),
                                versions: Default::default(),
                            },
                        )
                    })
                }),
                _ => None,
            })
            .collect(),
    ))
}

fn route_table(program: &[Statement]) -> Vec<Route> {
    program
        .iter()
        .filter_map(|statement| match statement {
            Statement::Route {
                method,
                path:
                    Token {
                        kind: TokenKind::String(path),
                        ..
                    },
                handler,
                ..
            } => {
                let route_handler = handler_name(handler);
                let handler_parameters = program
                    .iter()
                    .find_map(|statement| match statement {
                        Statement::Function {
                            name, parameters, ..
                        } if handler_name(name) == route_handler => Some(parameters.clone()),
                        _ => None,
                    })
                    .expect("the type checker validates every route handler");
                let parameter_types: Vec<_> = handler_parameters
                    .iter()
                    .map(|parameter| parameter.type_name.clone())
                    .collect();
                let body_model = parameter_types
                    .last()
                    .and_then(|type_name| match type_name {
                        TypeName::Model(model_name) => {
                            program.iter().find_map(|statement| match statement {
                                Statement::Model { name, fields }
                                    if handler_name(name) == model_name =>
                                {
                                    Some(ModelSchema {
                                        name: model_name.clone(),
                                        fields: fields.clone(),
                                    })
                                }
                                _ => None,
                            })
                        }
                        _ => None,
                    });
                let parameters: Vec<String> = route_parameters(path)
                    .into_iter()
                    .map(ToOwned::to_owned)
                    .collect();
                let path_parameter_types = parameter_types[..parameters.len()].to_vec();
                let query_parameter_end = if body_model.is_some() {
                    handler_parameters.len() - 1
                } else {
                    handler_parameters.len()
                };
                let query_parameters = handler_parameters[parameters.len()..query_parameter_end]
                    .iter()
                    .map(|parameter| QueryParameter {
                        name: handler_name(&parameter.name).to_owned(),
                        type_name: parameter.type_name.clone(),
                        has_default: parameter.default_value.is_some(),
                    })
                    .collect();
                Some(Route {
                    method: match method.kind {
                        TokenKind::Get => "GET",
                        TokenKind::Post => "POST",
                        TokenKind::Put => "PUT",
                        TokenKind::Patch => "PATCH",
                        TokenKind::Delete => "DELETE",
                        _ => unreachable!("the parser validates route methods"),
                    },
                    path: path.clone(),
                    handler: route_handler.to_owned(),
                    parameters,
                    path_parameter_types,
                    query_parameters,
                    body_model,
                })
            }
            _ => None,
        })
        .collect()
}

pub(super) fn route_response_with_stores(
    method: &str,
    path: &str,
    body: &str,
    program: &[Statement],
    routes: &[Route],
    stores: MemoryStores,
    limits: ExecutionLimits,
) -> HttpResponse {
    route_response_with_stores_and_expected_version(
        method, path, body, program, routes, stores, limits, None,
    )
}

/// Evaluates one route while applying an optional HTTP entity-tag precondition.
#[allow(clippy::too_many_arguments)]
pub(super) fn route_response_with_stores_and_expected_version(
    method: &str,
    path: &str,
    body: &str,
    program: &[Statement],
    routes: &[Route],
    stores: MemoryStores,
    limits: ExecutionLimits,
    expected_store_version: Option<u64>,
) -> HttpResponse {
    route_response_with_http_context(
        method,
        path,
        body,
        program,
        routes,
        stores,
        limits,
        expected_store_version,
        None,
        None,
        false,
    )
}

/// Evaluates one route with optional authenticated-request context.
#[allow(clippy::too_many_arguments)]
pub(super) fn route_response_with_http_context(
    method: &str,
    path: &str,
    body: &str,
    program: &[Statement],
    routes: &[Route],
    stores: MemoryStores,
    limits: ExecutionLimits,
    expected_store_version: Option<u64>,
    auth: Option<AuthConfig>,
    authenticated_subject: Option<String>,
    authenticated_with_refresh_token: bool,
) -> HttpResponse {
    if let Some((route, arguments)) = find_route(routes, path, Some(method)) {
        let arguments = match route_arguments(route, arguments) {
            Ok(arguments) => arguments,
            Err(message) => {
                return HttpResponse::error(400, "ORNI_HTTP_INVALID_PATH_PARAMETER", message);
            }
        };
        let query_arguments = match query_arguments(route, path) {
            Ok(arguments) => arguments,
            Err(message) => {
                return HttpResponse::error(400, "ORNI_HTTP_INVALID_QUERY_PARAMETER", message);
            }
        };
        let mut arguments: Vec<Option<Value>> = arguments.into_iter().map(Some).collect();
        arguments.extend(query_arguments);
        if let Some(model) = &route.body_model {
            match parse_model_body(body, model, route.method == "PATCH") {
                Ok(value) => arguments.push(Some(value)),
                Err(error) => return HttpResponse::invalid_body(error),
            }
        }
        let interpreter = Interpreter::with_http_context(
            limits,
            stores,
            expected_store_version,
            auth,
            authenticated_subject,
            authenticated_with_refresh_token,
        );
        let result = if arguments.iter().all(Option::is_some) {
            interpreter.execute_handler(
                program,
                &route.handler,
                arguments.into_iter().flatten().collect(),
            )
        } else {
            interpreter.execute_handler_with_optional_arguments(program, &route.handler, arguments)
        };
        match result {
            Ok(value) => {
                let mut response = response_value(value);
                if route.method == "POST"
                    && route.body_model.is_some()
                    && response.status == 201
                    && let Some(id) = response_location_id(&response.body)
                {
                    response = response.with_header(
                        "Location",
                        format!("{}/{}", route.path.trim_end_matches('/'), id),
                    );
                }
                response
            }
            Err(error)
                if error
                    .diagnostics()
                    .first()
                    .is_some_and(|diagnostic| diagnostic.code() == "ORNI_HTTP_UNAUTHORIZED") =>
            {
                HttpResponse::error(401, "ORNI_HTTP_UNAUTHORIZED", "Authentication is required")
            }
            Err(error) => {
                eprintln!("Orni route handler failed: {error}");
                HttpResponse::error(500, "ORNI_HTTP_INTERNAL_ERROR", "Internal Server Error")
            }
        }
    } else {
        if find_route(routes, path, None).is_some() {
            let allowed = routes
                .iter()
                .filter(|route| find_route(std::slice::from_ref(route), path, None).is_some())
                .map(|route| route.method)
                .collect::<Vec<_>>()
                .join(", ");
            HttpResponse::error(405, "ORNI_HTTP_METHOD_NOT_ALLOWED", "Method Not Allowed")
                .with_header("Allow", allowed)
        } else {
            HttpResponse::error(404, "ORNI_HTTP_ROUTE_NOT_FOUND", "Route not found")
        }
    }
}

/// Checks whether the matched route is explicitly protected by `app.require_auth`.
pub(super) fn route_requires_auth(path: &str, routes: &[Route], protected: &[String]) -> bool {
    find_route(routes, path, None)
        .is_some_and(|(route, _)| protected.iter().any(|configured| configured == &route.path))
}

/// Extracts a generated resource ID from either raw or enveloped JSON output.
fn response_location_id(body: &str) -> Option<String> {
    let json = serde_json::from_str::<serde_json::Value>(body).ok()?;
    json.get("id")
        .or_else(|| json.get("data").and_then(|data| data.get("id")))
        .and_then(serde_json::Value::as_str)
        .map(ToOwned::to_owned)
}

/// Reports whether a matching route receives a typed JSON model body.
///
/// The native socket layer uses this before dispatching so that a malformed
/// media type never reaches a handler. In-memory API tests already supply the
/// body directly and intentionally do not need to construct HTTP headers.
pub(super) fn route_requires_json_body(method: &str, path: &str, routes: &[Route]) -> bool {
    find_route(routes, path, Some(method)).is_some_and(|(route, _)| route.body_model.is_some())
}

/// Lists the route methods a browser may request for one concrete path.
pub(super) fn allowed_methods_for_path(path: &str, routes: &[Route]) -> Option<String> {
    const METHODS: [&str; 5] = ["GET", "POST", "PUT", "PATCH", "DELETE"];
    let methods = METHODS
        .into_iter()
        .filter(|method| find_route(routes, path, Some(method)).is_some())
        .collect::<Vec<_>>();
    (!methods.is_empty()).then(|| format!("{}, OPTIONS", methods.join(", ")))
}

/// Evaluates one request with an isolated in-memory store for unit tests.
#[cfg(test)]
fn route_response_with_body(
    method: &str,
    path: &str,
    body: &str,
    program: &[Statement],
    routes: &[Route],
    limits: ExecutionLimits,
) -> HttpResponse {
    route_response_with_stores(
        method,
        path,
        body,
        program,
        routes,
        memory_stores(program),
        limits,
    )
}

/// Evaluates a body-less request in unit tests and simple embedding scenarios.
#[cfg(test)]
fn route_response(
    method: &str,
    path: &str,
    program: &[Statement],
    routes: &[Route],
    limits: ExecutionLimits,
) -> (u16, String) {
    let response = route_response_with_body(method, path, "", program, routes, limits);
    (response.status, response.body)
}

/// Converts URL segments to the route handler's declared parameter types.
fn route_arguments(route: &Route, values: Vec<&str>) -> Result<Vec<Value>, String> {
    values
        .into_iter()
        .zip(&route.path_parameter_types)
        .zip(&route.parameters)
        .map(|((value, type_name), parameter)| parse_route_value(parameter, value, type_name))
        .collect()
}

/// Converts a request query string into declared GET handler arguments.
fn query_arguments(route: &Route, path: &str) -> Result<Vec<Option<Value>>, String> {
    let values = parse_query_string(path)?;
    for name in values.keys() {
        if !route
            .query_parameters
            .iter()
            .any(|parameter| &parameter.name == name)
        {
            return Err(format!("Unexpected query parameter '{name}'"));
        }
    }
    route
        .query_parameters
        .iter()
        .map(|parameter| match values.get(&parameter.name) {
            Some(value) => {
                parse_query_value(&parameter.name, value, &parameter.type_name).map(Some)
            }
            None if parameter.has_default => Ok(None),
            None => Err(format!(
                "Missing required query parameter '{}'",
                parameter.name
            )),
        })
        .collect()
}

/// Parses a URL query string with safe percent-decoding and duplicate detection.
fn parse_query_string(path: &str) -> Result<BTreeMap<String, String>, String> {
    let Some((_, query)) = path.split_once('?') else {
        return Ok(BTreeMap::new());
    };
    if query.is_empty() {
        return Ok(BTreeMap::new());
    }
    let mut values = BTreeMap::new();
    for pair in query.split('&') {
        if pair.is_empty() {
            continue;
        }
        let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
        let name = decode_query_component(name)?;
        let value = decode_query_component(value)?;
        if values.insert(name.clone(), value).is_some() {
            return Err(format!(
                "Query parameter '{name}' was provided more than once"
            ));
        }
    }
    Ok(values)
}

fn decode_query_component(value: &str) -> Result<String, String> {
    let mut decoded = Vec::with_capacity(value.len());
    let bytes = value.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'+' => {
                decoded.push(b' ');
                index += 1;
            }
            b'%' if index + 2 < bytes.len() => {
                let hex = |byte: u8| match byte {
                    b'0'..=b'9' => Some(byte - b'0'),
                    b'a'..=b'f' => Some(byte - b'a' + 10),
                    b'A'..=b'F' => Some(byte - b'A' + 10),
                    _ => None,
                };
                let Some(high) = hex(bytes[index + 1]) else {
                    return Err("Query string contains invalid percent encoding".to_owned());
                };
                let Some(low) = hex(bytes[index + 2]) else {
                    return Err("Query string contains invalid percent encoding".to_owned());
                };
                decoded.push(high * 16 + low);
                index += 3;
            }
            b'%' => return Err("Query string contains invalid percent encoding".to_owned()),
            byte => {
                decoded.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8(decoded).map_err(|_| "Query string must be valid UTF-8".to_owned())
}

/// Validates an object-shaped JSON request against one declared Orni model.
fn parse_model_body(
    body: &str,
    model: &ModelSchema,
    allow_missing_fields: bool,
) -> Result<Value, BodyValidationError> {
    if body.trim().is_empty() {
        return Err(BodyValidationError::general(
            "request body is required for this route",
        ));
    }
    let json: serde_json::Value = serde_json::from_str(body).map_err(|error| {
        BodyValidationError::general(format!("request body is not valid JSON: {error}"))
    })?;
    let object = json.as_object().ok_or_else(|| {
        BodyValidationError::general(format!(
            "request body must be a JSON object for model '{}'",
            model.name
        ))
    })?;
    for name in object.keys() {
        if !model
            .fields
            .iter()
            .any(|field| handler_name(&field.name) == name)
        {
            return Err(BodyValidationError::field(
                name,
                format!("unexpected JSON field '{name}' for model '{}'", model.name),
            ));
        }
    }
    let mut fields = BTreeMap::new();
    for field in &model.fields {
        let name = handler_name(&field.name);
        let value = if let Some(json_value) = object.get(name) {
            parse_json_value(name, json_value, &field.type_name)
                .map_err(|message| BodyValidationError::field(name, message))?
        } else if !allow_missing_fields && field.default_value.is_some() {
            model_default_value(field)
                .map_err(|message| BodyValidationError::field(name, message))?
        } else if allow_missing_fields || field.rules.contains(&ModelFieldRule::Optional) {
            continue;
        } else {
            return Err(BodyValidationError::field(
                name,
                format!("missing required JSON field '{name}'"),
            ));
        };
        if field.rules.contains(&ModelFieldRule::Email)
            && !matches!(&value, Value::String(value) if is_email(value))
        {
            return Err(BodyValidationError::field(
                name,
                format!("Invalid JSON field '{name}': expected a valid email address"),
            ));
        }
        validate_field_boundaries(name, &value, &field.rules)
            .map_err(|message| BodyValidationError::field(name, message))?;
        fields.insert(name.to_owned(), value);
    }
    if allow_missing_fields && fields.is_empty() {
        return Err(BodyValidationError::general(
            "PATCH request body must include at least one field",
        ));
    }
    Ok(Value::Model {
        name: model.name.clone(),
        fields,
    })
}

/// Converts a parser-validated literal model default to its declared runtime type.
fn model_default_value(field: &ModelField) -> Result<Value, String> {
    let Some(Expression::Literal(value)) = &field.default_value else {
        return Err("model field default must be a literal value".to_owned());
    };
    match (&field.type_name, value) {
        (TypeName::String(_), Value::String(value)) => Ok(Value::String(value.clone())),
        (TypeName::Integer(integer_type), Value::Integer { value, .. }) => Ok(Value::Integer {
            value: *value,
            integer_type: *integer_type,
        }),
        (TypeName::Double, Value::Integer { value, .. }) => Ok(Value::Double(*value as f64)),
        (TypeName::Double, Value::Double(value)) => Ok(Value::Double(*value)),
        (TypeName::Bool, Value::Bool(value)) => Ok(Value::Bool(*value)),
        (TypeName::Char, Value::Char(value)) => Ok(Value::Char(*value)),
        (TypeName::Uuid, Value::String(value)) => uuid::Uuid::parse_str(value)
            .map(Value::Uuid)
            .map_err(|_| "model field default is not a valid UUID".to_owned()),
        _ => Err("model field default does not match its declared type".to_owned()),
    }
}

/// Checks declarative lower and upper boundaries after JSON has a typed value.
fn validate_field_boundaries(
    field: &str,
    value: &Value,
    rules: &[ModelFieldRule],
) -> Result<(), String> {
    let minimum = rules.iter().find_map(|rule| match rule {
        ModelFieldRule::Min(value) => Some(*value),
        _ => None,
    });
    let maximum = rules.iter().find_map(|rule| match rule {
        ModelFieldRule::Max(value) => Some(*value),
        _ => None,
    });
    let allowed_values = rules.iter().find_map(|rule| match rule {
        ModelFieldRule::OneOf(values) => Some(values),
        _ => None,
    });
    if minimum.is_none() && maximum.is_none() && allowed_values.is_none() {
        return Ok(());
    }
    match value {
        Value::String(value) => {
            if allowed_values.is_some_and(|values| {
                !values
                    .iter()
                    .any(|allowed| allowed.as_str() == value.as_ref())
            }) {
                return Err(format!(
                    "Invalid JSON field '{field}': expected one of the declared values"
                ));
            }
            let length = value.len() as i128;
            if minimum.is_some_and(|minimum| length < minimum) {
                return Err(format!(
                    "Invalid JSON field '{field}': expected at least {} UTF-8 byte(s)",
                    minimum.expect("checked above")
                ));
            }
            if maximum.is_some_and(|maximum| length > maximum) {
                return Err(format!(
                    "Invalid JSON field '{field}': expected at most {} UTF-8 byte(s)",
                    maximum.expect("checked above")
                ));
            }
        }
        Value::Integer { value, .. } => {
            if minimum.is_some_and(|minimum| *value < minimum) {
                return Err(format!(
                    "Invalid JSON field '{field}': expected a value of at least {}",
                    minimum.expect("checked above")
                ));
            }
            if maximum.is_some_and(|maximum| *value > maximum) {
                return Err(format!(
                    "Invalid JSON field '{field}': expected a value of at most {}",
                    maximum.expect("checked above")
                ));
            }
        }
        Value::Double(value) => {
            if minimum.is_some_and(|minimum| *value < minimum as f64) {
                return Err(format!(
                    "Invalid JSON field '{field}': expected a value of at least {}",
                    minimum.expect("checked above")
                ));
            }
            if maximum.is_some_and(|maximum| *value > maximum as f64) {
                return Err(format!(
                    "Invalid JSON field '{field}': expected a value of at most {}",
                    maximum.expect("checked above")
                ));
            }
        }
        _ => unreachable!("the type checker validates model boundary rules"),
    }
    Ok(())
}

/// Performs a deliberately small, dependency-free e-mail shape validation.
fn is_email(value: &str) -> bool {
    let Some((local, domain)) = value.rsplit_once('@') else {
        return false;
    };
    !local.is_empty()
        && !domain.is_empty()
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && !value.chars().any(char::is_whitespace)
}

/// Converts one JSON value without widening its declared model field type.
fn parse_json_value(
    field: &str,
    value: &serde_json::Value,
    type_name: &TypeName,
) -> Result<Value, String> {
    let invalid = || format!("Invalid JSON field '{field}': expected {type_name}");
    match type_name {
        TypeName::String(maximum) => {
            let value = value.as_str().ok_or_else(invalid)?;
            if maximum.is_some_and(|maximum| value.len() > maximum) {
                return Err(invalid());
            }
            Ok(Value::String(value.into()))
        }
        TypeName::Integer(integer_type) => value
            .as_i64()
            .map(i128::from)
            .or_else(|| value.as_u64().map(i128::from))
            .filter(|value| integer_type.contains(*value))
            .map(|value| Value::Integer {
                value,
                integer_type: *integer_type,
            })
            .ok_or_else(invalid),
        TypeName::Double => value
            .as_f64()
            .filter(|value| value.is_finite())
            .map(Value::Double)
            .ok_or_else(invalid),
        TypeName::Bool => value.as_bool().map(Value::Bool).ok_or_else(invalid),
        TypeName::Char => {
            let value = value.as_str().ok_or_else(invalid)?;
            let mut characters = value.chars();
            match (characters.next(), characters.next()) {
                (Some(character), None) => Ok(Value::Char(character)),
                _ => Err(invalid()),
            }
        }
        TypeName::Uuid => value
            .as_str()
            .and_then(|value| uuid::Uuid::parse_str(value).ok())
            .map(Value::Uuid)
            .ok_or_else(invalid),
        TypeName::Size => value.as_u64().map(Value::Size).ok_or_else(invalid),
        TypeName::Duration => value.as_u64().map(Value::Duration).ok_or_else(invalid),
        TypeName::Unit | TypeName::Contract(_) | TypeName::Model(_) | TypeName::Response => {
            Err(invalid())
        }
    }
}

/// Parses one URL segment without widening its declared Orni type.
fn parse_route_value(parameter: &str, value: &str, type_name: &TypeName) -> Result<Value, String> {
    let invalid = || format!("Invalid route parameter '{parameter}': expected {type_name}");
    match type_name {
        TypeName::String(_) => Ok(Value::String(value.into())),
        TypeName::Integer(integer_type) => value
            .parse::<i128>()
            .ok()
            .filter(|value| integer_type.contains(*value))
            .map(|value| Value::Integer {
                value,
                integer_type: *integer_type,
            })
            .ok_or_else(invalid),
        TypeName::Double => value
            .parse::<f64>()
            .ok()
            .filter(|value| value.is_finite())
            .map(Value::Double)
            .ok_or_else(invalid),
        TypeName::Bool => match value {
            "true" => Ok(Value::Bool(true)),
            "false" => Ok(Value::Bool(false)),
            _ => Err(invalid()),
        },
        TypeName::Char => {
            let mut characters = value.chars();
            match (characters.next(), characters.next()) {
                (Some(character), None) => Ok(Value::Char(character)),
                _ => Err(invalid()),
            }
        }
        TypeName::Uuid => uuid::Uuid::parse_str(value)
            .map(Value::Uuid)
            .map_err(|_| invalid()),
        TypeName::Size => value.parse::<u64>().map(Value::Size).map_err(|_| invalid()),
        TypeName::Duration => value
            .parse::<u64>()
            .map(Value::Duration)
            .map_err(|_| invalid()),
        TypeName::Unit | TypeName::Contract(_) | TypeName::Model(_) | TypeName::Response => {
            Err(invalid())
        }
    }
}

/// Parses one decoded query value without widening its declared Orni type.
fn parse_query_value(parameter: &str, value: &str, type_name: &TypeName) -> Result<Value, String> {
    parse_typed_url_value(parameter, value, type_name, "query")
}

fn parse_typed_url_value(
    parameter: &str,
    value: &str,
    type_name: &TypeName,
    source: &str,
) -> Result<Value, String> {
    let invalid = || format!("Invalid {source} parameter '{parameter}': expected {type_name}");
    match type_name {
        TypeName::String(maximum) if maximum.is_none_or(|maximum| value.len() <= maximum) => {
            Ok(Value::String(value.into()))
        }
        TypeName::String(_) => Err(invalid()),
        TypeName::Integer(integer_type) => value
            .parse::<i128>()
            .ok()
            .filter(|value| integer_type.contains(*value))
            .map(|value| Value::Integer {
                value,
                integer_type: *integer_type,
            })
            .ok_or_else(invalid),
        TypeName::Double => value
            .parse::<f64>()
            .ok()
            .filter(|value| value.is_finite())
            .map(Value::Double)
            .ok_or_else(invalid),
        TypeName::Bool => match value {
            "true" => Ok(Value::Bool(true)),
            "false" => Ok(Value::Bool(false)),
            _ => Err(invalid()),
        },
        TypeName::Char => {
            let mut characters = value.chars();
            match (characters.next(), characters.next()) {
                (Some(character), None) => Ok(Value::Char(character)),
                _ => Err(invalid()),
            }
        }
        TypeName::Uuid => uuid::Uuid::parse_str(value)
            .map(Value::Uuid)
            .map_err(|_| invalid()),
        TypeName::Size => value.parse::<u64>().map(Value::Size).map_err(|_| invalid()),
        TypeName::Duration => value
            .parse::<u64>()
            .map(Value::Duration)
            .map_err(|_| invalid()),
        TypeName::Unit | TypeName::Contract(_) | TypeName::Model(_) | TypeName::Response => {
            Err(invalid())
        }
    }
}

/// Finds an exact route first, then a parameterized route with matching segments.
fn find_route<'routes, 'request>(
    routes: &'routes [Route],
    request_path: &'request str,
    method: Option<&str>,
) -> Option<(&'routes Route, Vec<&'request str>)> {
    let request_path = request_path.split('?').next().unwrap_or(request_path);
    if let Some(route) = routes.iter().find(|route| {
        method.is_none_or(|method| route.method == method)
            && route.parameters.is_empty()
            && route.path == request_path
    }) {
        return Some((route, Vec::new()));
    }
    routes
        .iter()
        .filter_map(|route| {
            if method.is_some_and(|method| route.method != method) {
                return None;
            }
            if route.parameters.is_empty() {
                return None;
            }
            let route_segments = route.path.split('/').skip(1).collect::<Vec<_>>();
            let request_segments = request_path.split('/').skip(1).collect::<Vec<_>>();
            if route_segments.len() != request_segments.len() {
                return None;
            }
            let mut values = Vec::with_capacity(route.parameters.len());
            for (route_segment, request_segment) in route_segments.iter().zip(request_segments) {
                if route_segment.starts_with(':') {
                    if request_segment.is_empty() {
                        return None;
                    }
                    values.push(request_segment);
                } else if route_segment != &request_segment {
                    return None;
                }
            }
            Some((route, values))
        })
        .next()
}

/// Reads parameter names from an already validated route path.
fn route_parameters(path: &str) -> Vec<&str> {
    path.split('/')
        .skip(1)
        .filter_map(|segment| segment.strip_prefix(':'))
        .collect()
}

fn handler_name(token: &Token) -> &str {
    let TokenKind::Identifier(name) = &token.kind else {
        unreachable!("the parser only produces named route handlers")
    };
    name
}

#[cfg(test)]
mod tests;
