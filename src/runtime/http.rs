use std::{
    collections::BTreeMap,
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    time::Duration,
};

use crate::{
    diagnostics::OrniError,
    syntax::{ModelField, Statement, Token, TokenKind, TypeName, Value},
};

use super::{ExecutionLimits, Interpreter, MemoryStore, MemoryStores};

const HOST: &str = "127.0.0.1";
const MAX_REQUEST_BYTES: usize = 1_048_576;

/// One validated route and the parameter names it receives from the URL.
struct Route<'a> {
    method: &'static str,
    path: &'a str,
    handler: &'a str,
    parameters: Vec<&'a str>,
    path_parameter_types: Vec<TypeName>,
    body_model: Option<ModelSchema>,
}

/// Schema kept by the HTTP boundary to validate a JSON request body.
#[derive(Clone)]
struct ModelSchema {
    name: String,
    fields: Vec<ModelField>,
}

/// A fully formed HTTP response, including its representation type.
#[derive(Debug, PartialEq)]
struct HttpResponse {
    status: u16,
    content_type: &'static str,
    body: String,
}

impl HttpResponse {
    fn text(status: u16, body: impl Into<String>) -> Self {
        Self {
            status,
            content_type: "text/plain; charset=utf-8",
            body: body.into(),
        }
    }
}

/// Serves the validated GET routes on the local development address.
pub(crate) fn serve(program: &[Statement], limits: ExecutionLimits) -> Result<(), OrniError> {
    let routes = route_table(program);
    let stores = memory_stores(program);
    if routes.is_empty() {
        return Err(
            OrniError::runtime(1, 1, "application has no HTTP routes").with_code("ORNI-E152")
        );
    }
    let (startup_output, application) = Interpreter::new(limits).execute_main(program)?;
    print!("{startup_output}");
    let address = format!("{HOST}:{}", application.port);

    let listener = TcpListener::bind(&address).map_err(|error| {
        OrniError::runtime(
            1,
            1,
            format!("could not start HTTP server on {address}: {error}"),
        )
        .with_code("ORNI-E152")
    })?;
    let label = application.name.as_deref().unwrap_or("Orni");
    println!("{label} HTTP server listening on http://{address}");

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => handle_request(stream, program, &routes, stores.clone(), limits),
            Err(error) => eprintln!("Orni HTTP connection failed: {error}"),
        }
    }
    Ok(())
}

/// Creates the collections retained for the lifetime of one HTTP server.
fn memory_stores(program: &[Statement]) -> MemoryStores {
    std::rc::Rc::new(std::cell::RefCell::new(
        program
            .iter()
            .filter_map(|statement| match statement {
                Statement::Store { name, model, .. } => Some((
                    handler_name(name).to_owned(),
                    MemoryStore {
                        model: handler_name(model).to_owned(),
                        values: Default::default(),
                    },
                )),
                _ => None,
            })
            .collect(),
    ))
}

fn route_table(program: &[Statement]) -> Vec<Route<'_>> {
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
                let parameter_types: Vec<_> = program
                    .iter()
                    .find_map(|statement| match statement {
                        Statement::Function {
                            name, parameters, ..
                        } if handler_name(name) == route_handler => Some(
                            parameters
                                .iter()
                                .map(|parameter| parameter.type_name.clone())
                                .collect(),
                        ),
                        _ => None,
                    })
                    .expect("the type checker validates every route handler");
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
                let path_parameter_types = if body_model.is_some() {
                    parameter_types[..parameter_types.len() - 1].to_vec()
                } else {
                    parameter_types
                };
                Some(Route {
                    method: match method.kind {
                        TokenKind::Get => "GET",
                        TokenKind::Post => "POST",
                        TokenKind::Put => "PUT",
                        TokenKind::Patch => "PATCH",
                        TokenKind::Delete => "DELETE",
                        _ => unreachable!("the parser validates route methods"),
                    },
                    path: path.as_str(),
                    handler: route_handler,
                    parameters: route_parameters(path),
                    path_parameter_types,
                    body_model,
                })
            }
            _ => None,
        })
        .collect()
}

fn handle_request(
    mut stream: TcpStream,
    program: &[Statement],
    routes: &[Route<'_>],
    stores: MemoryStores,
    limits: ExecutionLimits,
) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let response = match read_request(&stream) {
        Ok(Some((method, path, body))) => {
            route_response_with_stores(&method, &path, &body, program, routes, stores, limits)
        }
        Ok(None) => return,
        Err(message) => HttpResponse::text(400, message),
    };
    let reason = match response.status {
        200 => "OK",
        201 => "Created",
        204 => "No Content",
        400 => "Bad Request",
        404 => "Not Found",
        405 => "Method Not Allowed",
        409 => "Conflict",
        _ => "Internal Server Error",
    };
    let wire_response = format!(
        "HTTP/1.1 {} {reason}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        response.status,
        response.content_type,
        response.body.len(),
        response.body,
    );
    if let Err(error) = stream.write_all(wire_response.as_bytes()) {
        eprintln!("Orni HTTP response write failed: {error}");
    }
}

/// Reads a bounded HTTP/1.1 request, including a JSON body when present.
fn read_request(stream: &TcpStream) -> Result<Option<(String, String, String)>, String> {
    let mut reader = BufReader::new(stream);
    let mut request_line = String::new();
    let bytes_read = reader
        .read_line(&mut request_line)
        .map_err(|error| format!("could not read request: {error}"))?;
    if bytes_read == 0 {
        return Ok(None);
    }
    let mut content_length = 0_usize;
    loop {
        let mut header = String::new();
        reader
            .read_line(&mut header)
            .map_err(|error| format!("could not read request header: {error}"))?;
        if header == "\r\n" || header == "\n" {
            break;
        }
        let Some((name, value)) = header.split_once(':') else {
            return Err("malformed HTTP request header".to_owned());
        };
        if name.eq_ignore_ascii_case("content-length") {
            content_length = value
                .trim()
                .parse()
                .map_err(|_| "invalid Content-Length header".to_owned())?;
        }
    }
    if content_length > MAX_REQUEST_BYTES {
        return Err(format!(
            "request body exceeds the {MAX_REQUEST_BYTES}-byte limit"
        ));
    }
    let mut body = vec![0; content_length];
    reader
        .read_exact(&mut body)
        .map_err(|_| "request body ended before Content-Length bytes were received".to_owned())?;
    let body =
        String::from_utf8(body).map_err(|_| "request body must be valid UTF-8".to_owned())?;
    let mut parts = request_line.split_ascii_whitespace();
    let method = parts
        .next()
        .ok_or("malformed HTTP request line")?
        .to_owned();
    let path = parts
        .next()
        .ok_or("malformed HTTP request line")?
        .to_owned();
    Ok(Some((method, path, body)))
}

fn route_response_with_stores(
    method: &str,
    path: &str,
    body: &str,
    program: &[Statement],
    routes: &[Route<'_>],
    stores: MemoryStores,
    limits: ExecutionLimits,
) -> HttpResponse {
    if let Some((route, arguments)) = find_route(routes, path, Some(method)) {
        let arguments = match route_arguments(route, arguments) {
            Ok(arguments) => arguments,
            Err(message) => return HttpResponse::text(400, message),
        };
        let arguments = if let Some(model) = &route.body_model {
            match parse_model_body(body, model, route.method == "PATCH") {
                Ok(value) => arguments
                    .into_iter()
                    .chain(std::iter::once(value))
                    .collect(),
                Err(message) => return HttpResponse::text(400, message),
            }
        } else {
            arguments
        };
        match Interpreter::with_stores(limits, stores).execute_handler(
            program,
            route.handler,
            arguments,
        ) {
            Ok(value) => response_value(value),
            Err(error) => {
                eprintln!("Orni route handler failed: {error}");
                HttpResponse::text(500, "Internal Server Error")
            }
        }
    } else {
        if find_route(routes, path, None).is_some() {
            HttpResponse::text(405, "Method Not Allowed")
        } else {
            HttpResponse::text(404, "Not Found")
        }
    }
}

/// Evaluates one request with an isolated in-memory store for unit tests.
#[cfg(test)]
fn route_response_with_body(
    method: &str,
    path: &str,
    body: &str,
    program: &[Statement],
    routes: &[Route<'_>],
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
    routes: &[Route<'_>],
    limits: ExecutionLimits,
) -> (u16, String) {
    let response = route_response_with_body(method, path, "", program, routes, limits);
    (response.status, response.body)
}

/// Converts URL segments to the route handler's declared parameter types.
fn route_arguments(route: &Route<'_>, values: Vec<&str>) -> Result<Vec<Value>, String> {
    values
        .into_iter()
        .zip(&route.path_parameter_types)
        .zip(&route.parameters)
        .map(|((value, type_name), parameter)| parse_route_value(parameter, value, type_name))
        .collect()
}

/// Validates an object-shaped JSON request against one declared Orni model.
fn parse_model_body(
    body: &str,
    model: &ModelSchema,
    allow_missing_fields: bool,
) -> Result<Value, String> {
    let json: serde_json::Value = serde_json::from_str(body)
        .map_err(|error| format!("request body is not valid JSON: {error}"))?;
    let object = json.as_object().ok_or_else(|| {
        format!(
            "request body must be a JSON object for model '{}'",
            model.name
        )
    })?;
    for name in object.keys() {
        if !model
            .fields
            .iter()
            .any(|field| handler_name(&field.name) == name)
        {
            return Err(format!(
                "unexpected JSON field '{name}' for model '{}'",
                model.name
            ));
        }
    }
    let mut fields = BTreeMap::new();
    for field in &model.fields {
        let name = handler_name(&field.name);
        let Some(json_value) = object.get(name) else {
            if allow_missing_fields {
                continue;
            }
            return Err(format!("missing required JSON field '{name}'"));
        };
        let value = parse_json_value(name, json_value, &field.type_name)?;
        fields.insert(name.to_owned(), value);
    }
    if allow_missing_fields && fields.is_empty() {
        return Err("PATCH request body must include at least one field".to_owned());
    }
    Ok(Value::Model {
        name: model.name.clone(),
        fields,
    })
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
        TypeName::Unit | TypeName::Contract(_) | TypeName::Model(_) | TypeName::Response => {
            Err(invalid())
        }
    }
}

/// Selects a status and serializes the route value at the HTTP boundary.
fn response_value(value: Value) -> HttpResponse {
    let (status, body) = match value {
        Value::Response { status, body } => (status, *body),
        body => (200, body),
    };
    match body {
        Value::String(body) => HttpResponse::text(status, body.to_string()),
        Value::Unit if status == 204 => HttpResponse::text(status, ""),
        Value::Model { .. } | Value::List(_) => match serde_json::to_string(&model_json(&body)) {
            Ok(body) => HttpResponse {
                status,
                content_type: "application/json; charset=utf-8",
                body,
            },
            Err(_) => HttpResponse::text(500, "Internal Server Error"),
        },
        _ => HttpResponse::text(500, "Internal Server Error"),
    }
}

/// Serializes values that models may contain while preserving JSON value kinds.
fn model_json(value: &Value) -> serde_json::Value {
    match value {
        Value::Integer { value, .. } if *value >= 0 => {
            serde_json::Value::Number((*value as u64).into())
        }
        Value::Integer { value, .. } => serde_json::Value::Number((*value as i64).into()),
        Value::Double(value) => serde_json::Number::from_f64(*value)
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null),
        Value::String(value) => serde_json::Value::String(value.to_string()),
        Value::Bool(value) => serde_json::Value::Bool(*value),
        Value::Char(value) => serde_json::Value::String(value.to_string()),
        Value::Uuid(value) => serde_json::Value::String(value.to_string()),
        Value::Model { fields, .. } => serde_json::Value::Object(
            fields
                .iter()
                .map(|(name, value)| (name.clone(), model_json(value)))
                .collect(),
        ),
        Value::List(values) => serde_json::Value::Array(values.iter().map(model_json).collect()),
        Value::Unit | Value::Response { .. } | Value::Provider { .. } => serde_json::Value::Null,
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
        TypeName::Unit | TypeName::Contract(_) | TypeName::Model(_) | TypeName::Response => {
            Err(invalid())
        }
    }
}

/// Finds an exact route first, then a parameterized route with matching segments.
fn find_route<'routes, 'request>(
    routes: &'routes [Route<'routes>],
    request_path: &'request str,
    method: Option<&str>,
) -> Option<(&'routes Route<'routes>, Vec<&'request str>)> {
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
mod tests {
    use super::{
        ExecutionLimits, HttpResponse, memory_stores, route_response, route_response_with_body,
        route_response_with_stores, route_table,
    };

    #[test]
    fn routes_get_requests_to_orni_string_handlers() {
        let program = crate::compile(
            r#"
                func hello(name: string) -> string { return "Hello, " + name + "!"; }
                route get "/hello/:name" => hello;
            "#,
        )
        .expect("the example route should compile");
        let routes = route_table(&program);

        assert_eq!(
            route_response(
                "GET",
                "/hello/Orni",
                &program,
                &routes,
                ExecutionLimits::default()
            ),
            (200, "Hello, Orni!".to_owned())
        );
        assert_eq!(
            route_response(
                "GET",
                "/hello/Orni?locale=en",
                &program,
                &routes,
                ExecutionLimits::default()
            ),
            (200, "Hello, Orni!".to_owned())
        );
        assert_eq!(
            route_response(
                "GET",
                "/missing",
                &program,
                &routes,
                ExecutionLimits::default()
            ),
            (404, "Not Found".to_owned())
        );
        assert_eq!(
            route_response(
                "POST",
                "/hello/Orni",
                &program,
                &routes,
                ExecutionLimits::default()
            ),
            (405, "Method Not Allowed".to_owned())
        );
    }

    #[test]
    fn prefers_literal_routes_over_parameterized_routes() {
        let program = crate::compile(
            r#"
                func settings() -> string { return "Settings"; }
                func profile(name: string) -> string { return name; }
                route get "/users/settings" => settings;
                route get "/users/:name" => profile;
            "#,
        )
        .expect("literal and parameterized routes should coexist");
        let routes = route_table(&program);

        assert_eq!(
            route_response(
                "GET",
                "/users/settings",
                &program,
                &routes,
                ExecutionLimits::default()
            ),
            (200, "Settings".to_owned())
        );
    }

    #[test]
    fn converts_path_parameters_to_the_handler_types() {
        let program = crate::compile(
            r#"
                func user(id: uint16) -> string { return "User #" + id; }
                route get "/users/:id" => user;
            "#,
        )
        .expect("typed route parameters should compile");
        let routes = route_table(&program);

        assert_eq!(
            route_response(
                "GET",
                "/users/42",
                &program,
                &routes,
                ExecutionLimits::default()
            ),
            (200, "User #42".to_owned())
        );
        assert_eq!(
            route_response(
                "GET",
                "/users/70000",
                &program,
                &routes,
                ExecutionLimits::default()
            ),
            (
                400,
                "Invalid route parameter 'id': expected uint16".to_owned()
            )
        );
    }

    #[test]
    fn converts_uuid_path_parameters_before_calling_handlers() {
        let program = crate::compile(
            r#"
                func user(id: uuid) -> string { return "User #" + id; }
                route get "/users/:id" => user;
            "#,
        )
        .expect("UUID route parameters should compile");
        let routes = route_table(&program);
        let uuid = "550e8400-e29b-41d4-a716-446655440000";

        assert_eq!(
            route_response(
                "GET",
                "/users/550e8400-e29b-41d4-a716-446655440000",
                &program,
                &routes,
                ExecutionLimits::default()
            ),
            (200, format!("User #{uuid}"))
        );
        assert_eq!(
            route_response(
                "GET",
                "/users/not-a-uuid",
                &program,
                &routes,
                ExecutionLimits::default()
            ),
            (
                400,
                "Invalid route parameter 'id': expected uuid".to_owned()
            )
        );
    }

    #[test]
    fn validates_json_models_and_returns_created_resources() {
        let program = crate::compile(
            r#"
                model CreateUser {
                    id: uuid;
                    name: string<5>;
                    email: string<254>;
                    active: bool;
                }
                func create_user(input: CreateUser) -> response {
                    return response.created(input);
                }
                route post "/users" => create_user;
            "#,
        )
        .expect("a POST route with a model body should compile");
        let routes = route_table(&program);

        let created = route_response_with_body(
            "POST",
            "/users",
            r#"{"id":"550e8400-e29b-41d4-a716-446655440000","name":"Orni","email":"hello@orni.dev","active":true}"#,
            &program,
            &routes,
            ExecutionLimits::default(),
        );
        assert_eq!(created.status, 201);
        assert_eq!(created.content_type, "application/json; charset=utf-8");
        assert_eq!(
            created.body,
            r#"{"active":true,"email":"hello@orni.dev","id":"550e8400-e29b-41d4-a716-446655440000","name":"Orni"}"#
        );

        let missing = route_response_with_body(
            "POST",
            "/users",
            r#"{"id":"550e8400-e29b-41d4-a716-446655440000","name":"Orni","active":true}"#,
            &program,
            &routes,
            ExecutionLimits::default(),
        );
        assert_eq!(missing.status, 400);
        assert!(missing.body.contains("missing required JSON field 'email'"));

        let too_long = route_response_with_body(
            "POST",
            "/users",
            r#"{"id":"550e8400-e29b-41d4-a716-446655440000","name":"Too long","email":"hello@orni.dev","active":true}"#,
            &program,
            &routes,
            ExecutionLimits::default(),
        );
        assert_eq!(too_long.status, 400);
        assert!(too_long.body.contains("expected string<5>"));

        let extra = route_response_with_body(
            "POST",
            "/users",
            r#"{"id":"550e8400-e29b-41d4-a716-446655440000","name":"Orni","email":"hello@orni.dev","active":true,"role":"admin"}"#,
            &program,
            &routes,
            ExecutionLimits::default(),
        );
        assert_eq!(extra.status, 400);
        assert!(extra.body.contains("unexpected JSON field 'role'"));

        let invalid_uuid = route_response_with_body(
            "POST",
            "/users",
            r#"{"id":"not-a-uuid","name":"Orni","email":"hello@orni.dev","active":true}"#,
            &program,
            &routes,
            ExecutionLimits::default(),
        );
        assert_eq!(invalid_uuid.status, 400);
        assert!(invalid_uuid.body.contains("expected uuid"));

        let wrong_type = route_response_with_body(
            "POST",
            "/users",
            r#"{"id":"550e8400-e29b-41d4-a716-446655440000","name":"Orni","email":"hello@orni.dev","active":"true"}"#,
            &program,
            &routes,
            ExecutionLimits::default(),
        );
        assert_eq!(wrong_type.status, 400);
        assert!(wrong_type.body.contains("expected bool"));

        let wrong_method = route_response_with_body(
            "GET",
            "/users",
            "",
            &program,
            &routes,
            ExecutionLimits::default(),
        );
        assert_eq!(wrong_method.status, 405);
    }

    #[test]
    fn returns_an_explicit_ok_response_from_a_get_route() {
        let program = crate::compile(
            r#"
                func health() -> response { return response.ok("ready"); }
                route get "/health" => health;
            "#,
        )
        .expect("an explicit GET response should compile");
        let routes = route_table(&program);

        let response = route_response_with_body(
            "GET",
            "/health",
            "",
            &program,
            &routes,
            ExecutionLimits::default(),
        );
        assert_eq!(response, HttpResponse::text(200, "ready"));
    }

    #[test]
    fn keeps_crud_models_in_memory_for_one_server_lifetime() {
        let program = crate::compile(
            r#"
                model User { id: uuid; name: string<80>; }
                model CreateUser { name: string<80>; }
                model UpdateUser { name: string<80>; }
                model PatchUser { name: string<80>; }
                store users: User;
                func list() -> response { return users.list(); }
                func create(input: CreateUser) -> response { return users.create(input); }
                func find(id: uuid) -> response { return users.find(id); }
                func replace(id: uuid, input: UpdateUser) -> response { return users.replace(id, input); }
                func patch_user(id: uuid, input: PatchUser) -> response { return users.patch(id, input); }
                func remove(id: uuid) -> response { return users.delete(id); }
                route post "/users" => create;
                route get "/users" => list;
                route get "/users/:id" => find;
                route put "/users/:id" => replace;
                route patch "/users/:id" => patch_user;
                route delete "/users/:id" => remove;
            "#,
        )
        .expect("a CRUD application should compile");
        let routes = route_table(&program);
        let stores = memory_stores(&program);
        let created = route_response_with_stores(
            "POST",
            "/users",
            r#"{"name":"Orni"}"#,
            &program,
            &routes,
            stores.clone(),
            ExecutionLimits::default(),
        );
        assert_eq!(created.status, 201);
        let id = serde_json::from_str::<serde_json::Value>(&created.body)
            .expect("created JSON should be valid")
            .get("id")
            .and_then(serde_json::Value::as_str)
            .expect("created JSON should contain a UUID")
            .to_owned();

        let listed = route_response_with_stores(
            "GET",
            "/users",
            "",
            &program,
            &routes,
            stores.clone(),
            ExecutionLimits::default(),
        );
        assert_eq!(listed.status, 200);
        assert!(listed.body.starts_with('['));
        assert!(listed.body.contains("\"name\":\"Orni\""));

        let found = route_response_with_stores(
            "GET",
            &format!("/users/{id}"),
            "",
            &program,
            &routes,
            stores.clone(),
            ExecutionLimits::default(),
        );
        assert_eq!(found.status, 200);
        assert!(found.body.contains("\"name\":\"Orni\""));

        let updated = route_response_with_stores(
            "PUT",
            &format!("/users/{id}"),
            r#"{"name":"Orni Updated"}"#,
            &program,
            &routes,
            stores.clone(),
            ExecutionLimits::default(),
        );
        assert_eq!(updated.status, 200);
        assert!(updated.body.contains("\"name\":\"Orni Updated\""));

        let patched = route_response_with_stores(
            "PATCH",
            &format!("/users/{id}"),
            r#"{"name":"Orni Patched"}"#,
            &program,
            &routes,
            stores.clone(),
            ExecutionLimits::default(),
        );
        assert_eq!(patched.status, 200);
        assert!(patched.body.contains("\"name\":\"Orni Patched\""));

        let deleted = route_response_with_stores(
            "DELETE",
            &format!("/users/{id}"),
            "",
            &program,
            &routes,
            stores.clone(),
            ExecutionLimits::default(),
        );
        assert_eq!(deleted.status, 204);
        assert!(deleted.body.is_empty());

        let missing = route_response_with_stores(
            "GET",
            &format!("/users/{id}"),
            "",
            &program,
            &routes,
            stores,
            ExecutionLimits::default(),
        );
        assert_eq!(missing.status, 404);
    }
}
