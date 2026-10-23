//! Function invocation and framework built-ins.

use crate::{
    diagnostics::OrniError,
    syntax::{Expression, IntegerType, Parameter, Token, TypeName, Value},
};

use super::super::core::duration::{Duration as NativeDuration, DurationError};
#[cfg(not(target_arch = "wasm32"))]
use super::super::http::auth;
#[cfg(not(target_arch = "wasm32"))]
use super::api_test_request;
use super::{
    Interpreter, coerce_value, expression_token, identifier_name, runtime_error,
    validate_response_header,
};

impl Interpreter {
    /// Invokes a named function with evaluated argument values.
    pub(super) fn call(
        &mut self,
        callee: &Expression,
        closing_parenthesis: &Token,
        arguments: &[Expression],
    ) -> Result<Value, OrniError> {
        let Expression::Variable(name_token) = callee else {
            return Err(
                runtime_error(closing_parenthesis, "only named functions can be called")
                    .with_code("ORNI-E150"),
            );
        };
        let name = identifier_name(name_token);
        if name == "Size" {
            let [multiplier, amount] = arguments else {
                return Err(runtime_error(
                    closing_parenthesis,
                    "Size expects exactly one named unit argument",
                )
                .with_code("ORNI-E108"));
            };
            let Value::Integer {
                value: multiplier, ..
            } = self.evaluate(multiplier)?
            else {
                unreachable!("the parser stores the Size unit as an integer multiplier")
            };
            let Value::Integer { value: amount, .. } = self.evaluate(amount)? else {
                return Err(runtime_error(
                    closing_parenthesis,
                    "Size requires a non-negative integer amount",
                )
                .with_code("ORNI-E107"));
            };
            let amount = u64::try_from(amount).map_err(|_| {
                runtime_error(
                    closing_parenthesis,
                    "Size must fit in an unsigned 64-bit byte count",
                )
                .with_code("ORNI-E104")
            })?;
            let multiplier = u64::try_from(multiplier).expect("Size multipliers are positive");
            let bytes = amount.checked_mul(multiplier).ok_or_else(|| {
                runtime_error(
                    closing_parenthesis,
                    "Size exceeds the maximum unsigned 64-bit byte count",
                )
                .with_code("ORNI-E104")
            })?;
            return Ok(Value::Size(bytes));
        }
        if name == "Duration" {
            let [multiplier, amount] = arguments else {
                return Err(runtime_error(
                    closing_parenthesis,
                    "Duration expects exactly one named unit argument",
                )
                .with_code("ORNI-E108"));
            };
            let Value::Integer {
                value: multiplier, ..
            } = self.evaluate(multiplier)?
            else {
                unreachable!("the parser stores the Duration unit as an integer multiplier")
            };
            let Value::Integer { value: amount, .. } = self.evaluate(amount)? else {
                return Err(runtime_error(
                    closing_parenthesis,
                    "Duration requires a non-negative integer amount",
                )
                .with_code("ORNI-E107"));
            };
            let milliseconds = NativeDuration::from_unit(amount, multiplier).map_err(|error| {
                let message = match error {
                    DurationError::Negative => "Duration requires a non-negative integer amount",
                    DurationError::OutOfRange => {
                        "Duration must fit in an unsigned 64-bit millisecond count"
                    }
                    DurationError::Overflow => {
                        "Duration exceeds the maximum unsigned 64-bit millisecond count"
                    }
                };
                runtime_error(closing_parenthesis, message).with_code("ORNI-E104")
            })?;
            return Ok(Value::Duration(milliseconds));
        }
        if matches!(
            name,
            "api.status" | "api.body" | "api.contains" | "api.header" | "api.status_with"
        ) {
            let valid_arity = match name {
                "api.contains" | "api.header" => (3..=4).contains(&arguments.len()),
                "api.status_with" => (4..=5).contains(&arguments.len()),
                _ => (2..=3).contains(&arguments.len()),
            };
            if !valid_arity {
                let expected = match name {
                    "api.contains" | "api.header" => "3 or 4",
                    "api.status_with" => "4 or 5",
                    _ => "2 or 3",
                };
                return Err(runtime_error(
                    closing_parenthesis,
                    &format!(
                        "function '{name}' expects {expected} argument(s), but received {}",
                        arguments.len()
                    ),
                )
                .with_code("ORNI-E108"));
            }
            let values = arguments
                .iter()
                .map(|argument| self.evaluate(argument))
                .collect::<Result<Vec<_>, _>>()?;
            let [method, path, remaining @ ..] = values.as_slice() else {
                unreachable!()
            };
            let (Value::String(method), Value::String(path)) = (method, path) else {
                return Err(runtime_error(
                    closing_parenthesis,
                    "api request method and path must be strings",
                )
                .with_code("ORNI-E107"));
            };
            let (fragment, response_header, request_header, body) = if name == "api.contains" {
                let [Value::String(fragment), body @ ..] = remaining else {
                    return Err(runtime_error(
                        closing_parenthesis,
                        "api.contains requires a string fragment",
                    )
                    .with_code("ORNI-E107"));
                };
                let body = match body {
                    [] => "",
                    [Value::String(body)] => body,
                    _ => {
                        return Err(runtime_error(
                            closing_parenthesis,
                            "api request body must be a string",
                        )
                        .with_code("ORNI-E107"));
                    }
                };
                (Some(fragment), None, None, body)
            } else if name == "api.header" {
                let [Value::String(response_header), body @ ..] = remaining else {
                    return Err(runtime_error(
                        closing_parenthesis,
                        "api.header requires a string header name",
                    )
                    .with_code("ORNI-E107"));
                };
                let body = match body {
                    [] => "",
                    [Value::String(body)] => body,
                    _ => {
                        return Err(runtime_error(
                            closing_parenthesis,
                            "api request body must be a string",
                        )
                        .with_code("ORNI-E107"));
                    }
                };
                (None, Some(response_header), None, body)
            } else if name == "api.status_with" {
                let [
                    Value::String(header_name),
                    Value::String(header_value),
                    body @ ..,
                ] = remaining
                else {
                    return Err(runtime_error(
                        closing_parenthesis,
                        "api.status_with requires string header name and value",
                    )
                    .with_code("ORNI-E107"));
                };
                let body = match body {
                    [] => "",
                    [Value::String(body)] => body,
                    _ => {
                        return Err(runtime_error(
                            closing_parenthesis,
                            "api request body must be a string",
                        )
                        .with_code("ORNI-E107"));
                    }
                };
                (None, None, Some((header_name, header_value)), body)
            } else {
                let body = match remaining {
                    [] => "",
                    [Value::String(body)] => body,
                    _ => {
                        return Err(runtime_error(
                            closing_parenthesis,
                            "api request body must be a string",
                        )
                        .with_code("ORNI-E107"));
                    }
                };
                (None, None, None, body)
            };
            #[cfg(not(target_arch = "wasm32"))]
            if let Some(context) = &self.api_test {
                let response = api_test_request(
                    context,
                    method,
                    path,
                    body,
                    request_header.map(|(name, value)| (name.as_ref(), value.as_ref())),
                );
                return Ok(if name == "api.status" {
                    Value::Integer {
                        value: response.status.into(),
                        integer_type: IntegerType::smallest_signed(response.status.into()),
                    }
                } else if name == "api.body" {
                    Value::String(response.body.into())
                } else if name == "api.header" {
                    Value::String(
                        response
                            .header(response_header.expect("header has a name").as_ref())
                            .unwrap_or_default()
                            .into(),
                    )
                } else if name == "api.status_with" {
                    Value::Integer {
                        value: response.status.into(),
                        integer_type: IntegerType::smallest_signed(response.status.into()),
                    }
                } else {
                    Value::Bool(
                        response
                            .body
                            .contains(fragment.expect("contains has a fragment").as_ref()),
                    )
                });
            }
            #[cfg(target_arch = "wasm32")]
            let _ = (
                method,
                path,
                fragment,
                response_header,
                request_header,
                body,
            );
            return Err(runtime_error(
                closing_parenthesis,
                "api helpers are available only through 'orni test' in a project",
            )
            .with_code("ORNI-E150"));
        }
        if matches!(
            name,
            "response.ok"
                | "response.created"
                | "response.not_found"
                | "response.bad_request"
                | "response.unauthorized"
                | "response.forbidden"
                | "response.conflict"
                | "response.internal_error"
                | "response.redirect"
        ) {
            if arguments.len() != 1 {
                return Err(runtime_error(
                    closing_parenthesis,
                    &format!(
                        "function '{name}' expects 1 argument(s), but received {}",
                        arguments.len()
                    ),
                )
                .with_code("ORNI-E108"));
            }
            let body = self.evaluate(&arguments[0])?;
            return Ok(Value::Response {
                status: match name {
                    "response.created" => 201,
                    "response.not_found" => 404,
                    "response.bad_request" => 400,
                    "response.unauthorized" => 401,
                    "response.forbidden" => 403,
                    "response.conflict" => 409,
                    "response.internal_error" => 500,
                    "response.redirect" => 302,
                    _ => 200,
                },
                body: Box::new(body),
                headers: Vec::new(),
                json_envelope: false,
            });
        }
        if matches!(name, "response.json" | "response.created_json") {
            if arguments.len() != 1 {
                return Err(runtime_error(
                    closing_parenthesis,
                    &format!(
                        "function '{name}' expects 1 argument(s), but received {}",
                        arguments.len()
                    ),
                )
                .with_code("ORNI-E108"));
            }
            let value = self.evaluate(&arguments[0])?;
            let (status, body, headers, envelope) = match value {
                Value::Response {
                    status,
                    body,
                    headers,
                    ..
                } => (status, body, headers, status < 400),
                Value::Model { .. } => (200, Box::new(value), Vec::new(), true),
                _ => {
                    return Err(runtime_error(
                        expression_token(&arguments[0]).unwrap_or(closing_parenthesis),
                        "response.json requires a model or response containing JSON data",
                    )
                    .with_code("ORNI-E107"));
                }
            };
            return Ok(Value::Response {
                status: if name == "response.created_json" && status < 400 {
                    201
                } else {
                    status
                },
                body,
                headers,
                json_envelope: envelope,
            });
        }
        if name == "response.collection" {
            let [argument] = arguments else {
                return Err(runtime_error(
                    closing_parenthesis,
                    &format!(
                        "function 'response.collection' expects 1 argument(s), but received {}",
                        arguments.len()
                    ),
                )
                .with_code("ORNI-E108"));
            };
            let Value::Response {
                status,
                body,
                headers,
                json_envelope,
            } = self.evaluate(argument)?
            else {
                return Err(runtime_error(
                    expression_token(argument).unwrap_or(closing_parenthesis),
                    "response.collection requires a store list response",
                )
                .with_code("ORNI-E107"));
            };
            if status >= 400 {
                return Ok(Value::Response {
                    status,
                    body,
                    headers,
                    json_envelope,
                });
            }
            let collection = match *body {
                Value::List(values) => {
                    let total = values.len() as i128;
                    Value::Collection {
                        data: Box::new(Value::List(values)),
                        meta: std::collections::BTreeMap::from([(
                            "total".to_owned(),
                            Value::Integer {
                                value: total,
                                integer_type: IntegerType::smallest_signed(total),
                            },
                        )]),
                    }
                }
                Value::Model { name, mut fields }
                    if matches!(name.as_str(), "Page" | "CursorPage") =>
                {
                    let Some(data) = fields.remove("data") else {
                        unreachable!("store pages always contain data")
                    };
                    Value::Collection {
                        data: Box::new(data),
                        meta: fields,
                    }
                }
                _ => {
                    return Err(runtime_error(
                        expression_token(argument).unwrap_or(closing_parenthesis),
                        "response.collection requires users.list() or users.list(page, limit)",
                    )
                    .with_code("ORNI-E107"));
                }
            };
            return Ok(Value::Response {
                status,
                body: Box::new(collection),
                headers,
                json_envelope: false,
            });
        }
        if name == "response.no_content" {
            if !arguments.is_empty() {
                return Err(runtime_error(
                    closing_parenthesis,
                    "function 'response.no_content' expects 0 argument(s)",
                )
                .with_code("ORNI-E108"));
            }
            return Ok(Value::Response {
                status: 204,
                body: Box::new(Value::Unit),
                headers: Vec::new(),
                json_envelope: false,
            });
        }
        if name == "response.header" {
            let [response, header_name, header_value] = arguments else {
                return Err(runtime_error(
                    closing_parenthesis,
                    "function 'response.header' expects 3 argument(s)",
                )
                .with_code("ORNI-E108"));
            };
            let Value::Response {
                status,
                body,
                mut headers,
                json_envelope,
            } = self.evaluate(response)?
            else {
                return Err(runtime_error(
                    expression_token(response).unwrap_or(closing_parenthesis),
                    "response.header requires a response as its first argument",
                )
                .with_code("ORNI-E107"));
            };
            let Value::String(header_name) = self.evaluate(header_name)? else {
                return Err(runtime_error(
                    expression_token(header_name).unwrap_or(closing_parenthesis),
                    "response.header requires a string header name",
                )
                .with_code("ORNI-E107"));
            };
            let Value::String(header_value) = self.evaluate(header_value)? else {
                return Err(runtime_error(
                    expression_token(header_value).unwrap_or(closing_parenthesis),
                    "response.header requires a string header value",
                )
                .with_code("ORNI-E107"));
            };
            validate_response_header(&header_name, &header_value, closing_parenthesis)?;
            headers.retain(|(name, _)| !name.eq_ignore_ascii_case(&header_name));
            headers.push((header_name.to_string(), header_value.to_string()));
            return Ok(Value::Response {
                status,
                body,
                headers,
                json_envelope,
            });
        }
        if name == "auth.cookie" {
            let [response, token] = arguments else {
                return Err(runtime_error(
                    closing_parenthesis,
                    "function 'auth.cookie' expects 2 arguments",
                )
                .with_code("ORNI-E108"));
            };
            let Value::Response {
                status,
                body,
                mut headers,
                json_envelope,
            } = self.evaluate(response)?
            else {
                return Err(runtime_error(
                    expression_token(response).unwrap_or(closing_parenthesis),
                    "auth.cookie requires a response as its first argument",
                )
                .with_code("ORNI-E107"));
            };
            let Value::String(token) = self.evaluate(token)? else {
                return Err(runtime_error(
                    expression_token(token).unwrap_or(closing_parenthesis),
                    "auth.cookie requires a refresh token string",
                )
                .with_code("ORNI-E107"));
            };
            #[cfg(not(target_arch = "wasm32"))]
            let lifetime = self
                .application
                .auth
                .as_ref()
                .map(|configuration| configuration.refresh_duration_milliseconds / 1_000)
                .unwrap_or(604_800);
            #[cfg(target_arch = "wasm32")]
            let lifetime = 604_800;
            let cookie = format!(
                "orni_refresh={token}; Max-Age={lifetime}; HttpOnly; Secure; SameSite=Strict; Path=/auth"
            );
            headers.retain(|(name, _)| !name.eq_ignore_ascii_case("set-cookie"));
            headers.push(("Set-Cookie".to_owned(), cookie));
            return Ok(Value::Response {
                status,
                body,
                headers,
                json_envelope,
            });
        }
        if name == "app.name" {
            let [value] = arguments else {
                return Err(runtime_error(
                    closing_parenthesis,
                    "function 'app.name' expects 1 argument",
                )
                .with_code("ORNI-E108"));
            };
            let Value::String(name) = self.evaluate(value)? else {
                return Err(runtime_error(
                    expression_token(value).unwrap_or(closing_parenthesis),
                    "app.name requires a string",
                )
                .with_code("ORNI-E107"));
            };
            #[cfg(not(target_arch = "wasm32"))]
            {
                self.application.name = Some(name.to_string());
            }
            #[cfg(target_arch = "wasm32")]
            let _ = name;
            return Ok(Value::Unit);
        }
        if name == "app.port" {
            let [value] = arguments else {
                return Err(runtime_error(
                    closing_parenthesis,
                    "function 'app.port' expects 1 argument",
                )
                .with_code("ORNI-E108"));
            };
            let Value::Integer { value: port, .. } = self.evaluate(value)? else {
                return Err(runtime_error(
                    expression_token(value).unwrap_or(closing_parenthesis),
                    "app.port requires an integer",
                )
                .with_code("ORNI-E107"));
            };
            let port = u16::try_from(port)
                .ok()
                .filter(|port| *port > 0)
                .ok_or_else(|| {
                    runtime_error(
                        expression_token(value).unwrap_or(closing_parenthesis),
                        "app.port must be between 1 and 65535",
                    )
                    .with_code("ORNI-E107")
                })?;
            #[cfg(not(target_arch = "wasm32"))]
            {
                self.application.port = port;
            }
            #[cfg(target_arch = "wasm32")]
            let _ = port;
            return Ok(Value::Unit);
        }
        if name == "app.timeout" {
            let [value] = arguments else {
                return Err(runtime_error(
                    closing_parenthesis,
                    "function 'app.timeout' expects 1 argument",
                )
                .with_code("ORNI-E108"));
            };
            let Value::Duration(milliseconds) = self.evaluate(value)? else {
                return Err(runtime_error(
                    expression_token(value).unwrap_or(closing_parenthesis),
                    "app.timeout requires a Duration value",
                )
                .with_code("ORNI-E107"));
            };
            if milliseconds == 0 {
                return Err(runtime_error(
                    expression_token(value).unwrap_or(closing_parenthesis),
                    "app.timeout requires a positive Duration",
                )
                .with_code("ORNI-E107"));
            }
            #[cfg(not(target_arch = "wasm32"))]
            {
                self.application.request_timeout_milliseconds = Some(milliseconds);
            }
            #[cfg(target_arch = "wasm32")]
            let _ = milliseconds;
            return Ok(Value::Unit);
        }
        if name == "app.log_requests" {
            let [value] = arguments else {
                return Err(runtime_error(
                    closing_parenthesis,
                    "function 'app.log_requests' expects 1 argument",
                )
                .with_code("ORNI-E108"));
            };
            let Value::Bool(enabled) = self.evaluate(value)? else {
                return Err(runtime_error(
                    expression_token(value).unwrap_or(closing_parenthesis),
                    "app.log_requests requires a bool value",
                )
                .with_code("ORNI-E107"));
            };
            #[cfg(not(target_arch = "wasm32"))]
            {
                self.application.request_logging = enabled;
            }
            #[cfg(target_arch = "wasm32")]
            let _ = enabled;
            return Ok(Value::Unit);
        }
        if matches!(name, "app.body_limit" | "app.cache_limit") {
            let [value] = arguments else {
                return Err(runtime_error(
                    closing_parenthesis,
                    &format!("function '{name}' expects 1 argument"),
                )
                .with_code("ORNI-E108"));
            };
            let Value::Size(bytes) = self.evaluate(value)? else {
                return Err(runtime_error(
                    expression_token(value).unwrap_or(closing_parenthesis),
                    &format!("{name} requires a Size value"),
                )
                .with_code("ORNI-E107"));
            };
            let bytes = usize::try_from(bytes)
                .ok()
                .filter(|bytes| *bytes > 0)
                .ok_or_else(|| {
                    runtime_error(
                        expression_token(value).unwrap_or(closing_parenthesis),
                        &format!("{name} requires a positive platform-sized byte count"),
                    )
                    .with_code("ORNI-E107")
                })?;
            #[cfg(not(target_arch = "wasm32"))]
            if name == "app.body_limit" {
                self.application.body_limit_bytes = Some(bytes);
            } else {
                self.application.cache_limit_bytes = Some(bytes);
            }
            #[cfg(target_arch = "wasm32")]
            let _ = bytes;
            return Ok(Value::Unit);
        }
        if name == "app.protect" {
            if !matches!(arguments.len(), 1 | 3) {
                return Err(runtime_error(
                    closing_parenthesis,
                    "function 'app.protect' expects 1 or 3 arguments",
                )
                .with_code("ORNI-E108"));
            }
            let Value::String(path) = self.evaluate(&arguments[0])? else {
                return Err(runtime_error(
                    expression_token(&arguments[0]).unwrap_or(closing_parenthesis),
                    "app.protect requires a route path string",
                )
                .with_code("ORNI-E107"));
            };
            let (maximum_attempts, lockout_milliseconds) = if arguments.len() == 3 {
                let Value::Integer {
                    value: attempts, ..
                } = self.evaluate(&arguments[1])?
                else {
                    return Err(runtime_error(
                        closing_parenthesis,
                        "app.protect limits must be integers",
                    )
                    .with_code("ORNI-E107"));
                };
                let Value::Duration(lockout_milliseconds) = self.evaluate(&arguments[2])? else {
                    return Err(runtime_error(
                        closing_parenthesis,
                        "app.protect lock_time must be a Duration",
                    )
                    .with_code("ORNI-E107"));
                };
                (usize::try_from(attempts).ok(), Some(lockout_milliseconds))
            } else {
                (Some(5), Some(900_000))
            };
            let (Some(maximum_attempts), Some(lockout_milliseconds)) =
                (maximum_attempts, lockout_milliseconds)
            else {
                return Err(runtime_error(
                    closing_parenthesis,
                    "app.protect limits must be positive values",
                )
                .with_code("ORNI-E107"));
            };
            if !path.starts_with('/') || maximum_attempts == 0 || lockout_milliseconds == 0 {
                return Err(runtime_error(
                    closing_parenthesis,
                    "app.protect requires an absolute path and positive limits",
                )
                .with_code("ORNI-E107"));
            }
            #[cfg(not(target_arch = "wasm32"))]
            self.application
                .protected_routes
                .push(super::RouteProtection {
                    path: path.to_string(),
                    maximum_attempts,
                    lockout_milliseconds,
                });
            #[cfg(target_arch = "wasm32")]
            let _ = (path, maximum_attempts, lockout_milliseconds);
            return Ok(Value::Unit);
        }
        if name == "app.cors" {
            let [origin] = arguments else {
                return Err(runtime_error(
                    closing_parenthesis,
                    "function 'app.cors' expects 1 argument",
                )
                .with_code("ORNI-E108"));
            };
            let origin_token = expression_token(origin).unwrap_or(closing_parenthesis);
            let Value::String(origin) = self.evaluate(origin)? else {
                return Err(runtime_error(
                    origin_token,
                    "app.cors requires an HTTP or HTTPS origin string",
                )
                .with_code("ORNI-E107"));
            };
            if !is_valid_cors_origin(&origin) {
                return Err(runtime_error(
                    origin_token,
                    "app.cors requires an HTTP or HTTPS origin without a path",
                )
                .with_code("ORNI-E107"));
            }
            #[cfg(not(target_arch = "wasm32"))]
            if !self
                .application
                .cors_origins
                .iter()
                .any(|existing| existing == &*origin)
            {
                self.application.cors_origins.push(origin.to_string());
            }
            #[cfg(target_arch = "wasm32")]
            let _ = origin;
            return Ok(Value::Unit);
        }
        if name == "app.auth_env" {
            if !matches!(arguments.len(), 2..=4) {
                return Err(runtime_error(
                    closing_parenthesis,
                    "function 'app.auth_env' expects 2, 3, or 4 arguments",
                )
                .with_code("ORNI-E108"));
            }
            let Value::String(variable) = self.evaluate(&arguments[0])? else {
                return Err(runtime_error(
                    expression_token(&arguments[0]).unwrap_or(closing_parenthesis),
                    "app.auth_env requires an environment variable name",
                )
                .with_code("ORNI-E107"));
            };
            let access = positive_duration(
                self.evaluate(&arguments[1])?,
                closing_parenthesis,
                "app.auth_env access_time",
            )?;
            let refresh = arguments
                .get(2)
                .map(|argument| self.evaluate(argument))
                .transpose()?
                .map(|value| {
                    positive_duration(value, closing_parenthesis, "app.auth_env refresh_time")
                })
                .transpose()?
                .unwrap_or(604_800_000);
            let sessions = arguments
                .get(3)
                .map(|argument| self.evaluate(argument))
                .transpose()?
                .and_then(|value| match value {
                    Value::Integer { value, .. } => {
                        usize::try_from(value).ok().filter(|value| *value > 0)
                    }
                    _ => None,
                })
                .unwrap_or(2);
            #[cfg(not(target_arch = "wasm32"))]
            {
                let secret = std::env::var(&*variable).map_err(|_| {
                    runtime_error(
                        closing_parenthesis,
                        &format!("environment variable '{variable}' is not set"),
                    )
                    .with_code("ORNI-E107")
                })?;
                if secret.len() < 16 {
                    return Err(runtime_error(
                        closing_parenthesis,
                        "app.auth_env secret must contain at least 16 UTF-8 bytes",
                    )
                    .with_code("ORNI-E107"));
                }
                self.application.auth = Some(super::AuthConfig {
                    secret,
                    access_duration_milliseconds: access,
                    refresh_duration_milliseconds: refresh,
                    maximum_sessions_per_subject: sessions,
                });
            }
            #[cfg(target_arch = "wasm32")]
            let _ = (variable, access, refresh, sessions);
            return Ok(Value::Unit);
        }
        if name == "app.auth" {
            if !matches!(arguments.len(), 2..=4) {
                return Err(runtime_error(
                    closing_parenthesis,
                    "function 'app.auth' expects 2, 3, or 4 arguments",
                )
                .with_code("ORNI-E108"));
            }
            let secret = &arguments[0];
            let access_time = &arguments[1];
            let secret_token = expression_token(secret).unwrap_or(closing_parenthesis);
            let Value::String(secret) = self.evaluate(secret)? else {
                return Err(
                    runtime_error(secret_token, "app.auth requires a string secret")
                        .with_code("ORNI-E107"),
                );
            };
            let access_duration_milliseconds = positive_duration(
                self.evaluate(access_time)?,
                expression_token(access_time).unwrap_or(closing_parenthesis),
                "app.auth access_time",
            )?;
            let refresh_duration_milliseconds = if let Some(refresh) = arguments.get(2) {
                positive_duration(
                    self.evaluate(refresh)?,
                    expression_token(refresh).unwrap_or(closing_parenthesis),
                    "app.auth refresh_time",
                )?
            } else {
                604_800_000
            };
            let maximum_sessions_per_subject = if let Some(limit) = arguments.get(3) {
                let Value::Integer { value, .. } = self.evaluate(limit)? else {
                    return Err(runtime_error(
                        expression_token(limit).unwrap_or(closing_parenthesis),
                        "app.auth session limit must be a positive integer",
                    )
                    .with_code("ORNI-E107"));
                };
                usize::try_from(value)
                    .ok()
                    .filter(|value| *value > 0)
                    .ok_or_else(|| {
                        runtime_error(
                            closing_parenthesis,
                            "app.auth session limit must be a positive integer",
                        )
                        .with_code("ORNI-E107")
                    })?
            } else {
                2
            };
            if secret.len() < 16 {
                return Err(runtime_error(
                    secret_token,
                    "app.auth secret must contain at least 16 UTF-8 bytes",
                )
                .with_code("ORNI-E107"));
            }
            #[cfg(not(target_arch = "wasm32"))]
            {
                self.application.auth = Some(super::AuthConfig {
                    secret: secret.to_string(),
                    access_duration_milliseconds,
                    refresh_duration_milliseconds,
                    maximum_sessions_per_subject,
                });
            }
            #[cfg(target_arch = "wasm32")]
            let _ = (
                secret,
                access_duration_milliseconds,
                refresh_duration_milliseconds,
                maximum_sessions_per_subject,
            );
            return Ok(Value::Unit);
        }
        if name == "app.require_auth" {
            let [path] = arguments else {
                return Err(runtime_error(
                    closing_parenthesis,
                    "function 'app.require_auth' expects 1 argument",
                )
                .with_code("ORNI-E108"));
            };
            let Value::String(path) = self.evaluate(path)? else {
                return Err(runtime_error(
                    expression_token(path).unwrap_or(closing_parenthesis),
                    "app.require_auth requires an absolute route path string",
                )
                .with_code("ORNI-E107"));
            };
            if !path.starts_with('/') {
                return Err(runtime_error(
                    closing_parenthesis,
                    "app.require_auth requires an absolute route path string",
                )
                .with_code("ORNI-E107"));
            }
            #[cfg(not(target_arch = "wasm32"))]
            if !self
                .application
                .authenticated_routes
                .iter()
                .any(|route| route == &*path)
            {
                self.application.authenticated_routes.push(path.to_string());
            }
            #[cfg(target_arch = "wasm32")]
            let _ = path;
            return Ok(Value::Unit);
        }
        if name == "app.require_refresh" {
            let [path] = arguments else {
                return Err(runtime_error(
                    closing_parenthesis,
                    "function 'app.require_refresh' expects 1 argument",
                )
                .with_code("ORNI-E108"));
            };
            let Value::String(path) = self.evaluate(path)? else {
                return Err(runtime_error(
                    expression_token(path).unwrap_or(closing_parenthesis),
                    "app.require_refresh requires an absolute route path string",
                )
                .with_code("ORNI-E107"));
            };
            if !path.starts_with('/') {
                return Err(runtime_error(
                    closing_parenthesis,
                    "app.require_refresh requires an absolute route path string",
                )
                .with_code("ORNI-E107"));
            }
            #[cfg(not(target_arch = "wasm32"))]
            if !self
                .application
                .refresh_routes
                .iter()
                .any(|route| route == &*path)
            {
                self.application.refresh_routes.push(path.to_string());
            }
            #[cfg(target_arch = "wasm32")]
            let _ = path;
            return Ok(Value::Unit);
        }
        if name == "app.logout" {
            let [path] = arguments else {
                return Err(runtime_error(
                    closing_parenthesis,
                    "function 'app.logout' expects 1 argument",
                )
                .with_code("ORNI-E108"));
            };
            let Value::String(path) = self.evaluate(path)? else {
                return Err(runtime_error(
                    expression_token(path).unwrap_or(closing_parenthesis),
                    "app.logout requires an absolute route path string",
                )
                .with_code("ORNI-E107"));
            };
            if !path.starts_with('/') {
                return Err(runtime_error(
                    closing_parenthesis,
                    "app.logout requires an absolute route path string",
                )
                .with_code("ORNI-E107"));
            }
            #[cfg(not(target_arch = "wasm32"))]
            {
                if !self
                    .application
                    .logout_routes
                    .iter()
                    .any(|route| route == &*path)
                {
                    self.application.logout_routes.push(path.to_string());
                }
                if !self
                    .application
                    .refresh_routes
                    .iter()
                    .any(|route| route == &*path)
                {
                    self.application.refresh_routes.push(path.to_string());
                }
            }
            #[cfg(target_arch = "wasm32")]
            let _ = path;
            return Ok(Value::Unit);
        }
        if name == "auth.token" {
            let [subject] = arguments else {
                return Err(runtime_error(
                    closing_parenthesis,
                    "function 'auth.token' expects 1 argument",
                )
                .with_code("ORNI-E108"));
            };
            let Value::String(subject) = self.evaluate(subject)? else {
                return Err(runtime_error(
                    expression_token(subject).unwrap_or(closing_parenthesis),
                    "auth.token requires a string subject",
                )
                .with_code("ORNI-E107"));
            };
            #[cfg(not(target_arch = "wasm32"))]
            {
                let Some(configuration) = self.application.auth.as_ref() else {
                    return Err(runtime_error(
                        closing_parenthesis,
                        "configure app.auth before issuing tokens",
                    )
                    .with_code("ORNI-E107"));
                };
                return Ok(Value::String(auth::issue(configuration, &subject).into()));
            }
            #[cfg(target_arch = "wasm32")]
            {
                let _ = subject;
                return Err(runtime_error(
                    closing_parenthesis,
                    "auth is available on the native HTTP server only",
                )
                .with_code("ORNI-E107"));
            }
        }
        if name == "auth.user" {
            if !arguments.is_empty() {
                return Err(runtime_error(
                    closing_parenthesis,
                    "function 'auth.user' expects 0 arguments",
                )
                .with_code("ORNI-E108"));
            }
            #[cfg(not(target_arch = "wasm32"))]
            if let Some(subject) = &self.authenticated_subject {
                return Ok(Value::String(subject.clone().into()));
            }
            return Err(runtime_error(
                closing_parenthesis,
                "a valid Bearer token is required for auth.user",
            )
            .with_code("ORNI_HTTP_UNAUTHORIZED"));
        }
        if name == "auth.refresh_token" {
            let [subject] = arguments else {
                return Err(runtime_error(
                    closing_parenthesis,
                    "function 'auth.refresh_token' expects 1 argument",
                )
                .with_code("ORNI-E108"));
            };
            let Value::String(subject) = self.evaluate(subject)? else {
                return Err(runtime_error(
                    expression_token(subject).unwrap_or(closing_parenthesis),
                    "auth.refresh_token requires a string subject",
                )
                .with_code("ORNI-E107"));
            };
            #[cfg(not(target_arch = "wasm32"))]
            {
                let Some(configuration) = self.application.auth.as_ref() else {
                    return Err(runtime_error(
                        closing_parenthesis,
                        "configure app.auth before issuing tokens",
                    )
                    .with_code("ORNI-E107"));
                };
                return Ok(Value::String(
                    auth::issue_refresh(configuration, &subject).into(),
                ));
            }
            #[cfg(target_arch = "wasm32")]
            let _ = subject;
        }
        if name == "auth.refresh" {
            if !arguments.is_empty() {
                return Err(runtime_error(
                    closing_parenthesis,
                    "function 'auth.refresh' expects 0 arguments",
                )
                .with_code("ORNI-E108"));
            }
            #[cfg(not(target_arch = "wasm32"))]
            if self.authenticated_with_refresh_token
                && let (Some(configuration), Some(subject)) = (
                    self.application.auth.as_ref(),
                    self.authenticated_subject.as_ref(),
                )
            {
                return Ok(Value::String(auth::issue(configuration, subject).into()));
            }
            return Err(runtime_error(
                closing_parenthesis,
                "a valid refresh token is required for auth.refresh",
            )
            .with_code("ORNI_HTTP_UNAUTHORIZED"));
        }
        let values = arguments
            .iter()
            .map(|argument| self.evaluate(argument))
            .collect::<Result<Vec<_>, _>>()?;
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(response) = self.store_call(name, closing_parenthesis, &values)? {
            return Ok(response);
        }
        self.call_values(name_token, closing_parenthesis, values)
    }

    /// Invokes a named function with values supplied by the embedding runtime.
    pub(super) fn call_values(
        &mut self,
        name_token: &Token,
        closing_parenthesis: &Token,
        values: Vec<Value>,
    ) -> Result<Value, OrniError> {
        self.call_optional_values(
            name_token,
            closing_parenthesis,
            values.into_iter().map(Some).collect(),
        )
    }

    /// Invokes a function while allowing the embedding runtime to omit named
    /// optional parameters in their declared positions.
    pub(super) fn call_optional_values(
        &mut self,
        name_token: &Token,
        closing_parenthesis: &Token,
        values: Vec<Option<Value>>,
    ) -> Result<Value, OrniError> {
        let name = identifier_name(name_token);
        if self.call_depth >= self.limits.max_call_depth {
            return Err(runtime_error(
                name_token,
                &format!(
                    "function call depth exceeds the configured limit of {}",
                    self.limits.max_call_depth
                ),
            )
            .with_code("ORNI-E134"));
        }
        let resolved_name = if self.functions.contains_key(name) {
            name.to_owned()
        } else if let Some((receiver, method)) = name.split_once('.') {
            match self.environment.get(receiver) {
                Some(Value::Provider { provider, .. }) => format!("{provider}.{method}"),
                _ => name.to_owned(),
            }
        } else {
            name.to_owned()
        };
        let function = self.functions.get(&resolved_name).cloned().ok_or_else(|| {
            runtime_error(name_token, &format!("undefined function '{name}'"))
                .with_code("ORNI-E103")
        })?;

        let required_arguments = function
            .parameters
            .iter()
            .filter(|parameter| !matches!(&parameter.type_name, TypeName::Contract(_)))
            .filter(|parameter| parameter.default_value.is_none())
            .count();
        let supplied_parameter_count = function
            .parameters
            .iter()
            .filter(|parameter| !matches!(&parameter.type_name, TypeName::Contract(_)))
            .count();
        if values.len() < required_arguments || values.len() > supplied_parameter_count {
            let expected = if required_arguments == supplied_parameter_count {
                format!("{supplied_parameter_count} argument(s)")
            } else {
                format!(
                    "between {required_arguments} and {} argument(s)",
                    supplied_parameter_count
                )
            };
            return Err(runtime_error(
                closing_parenthesis,
                &format!(
                    "function '{name}' expects {expected}, but received {}",
                    values.len(),
                ),
            )
            .with_code("ORNI-E108"));
        }

        self.call_depth += 1;
        self.environment.enter_function_scope();
        let result = self
            .bind_parameters(&function.parameters, values)
            .and_then(|()| self.execute_function_body(&function.body))
            .and_then(|value| coerce_value(value, Some(function.return_type), closing_parenthesis));
        self.environment.exit_function_scope();
        self.call_depth -= 1;
        result
    }

    /// Binds supplied arguments and evaluates defaults for omitted arguments.
    fn bind_parameters(
        &mut self,
        parameters: &[Parameter],
        values: Vec<Option<Value>>,
    ) -> Result<(), OrniError> {
        let mut values = values.into_iter();
        for parameter in parameters {
            let value = match &parameter.type_name {
                TypeName::Contract(contract) => {
                    let provider = self.bindings.get(contract).cloned().ok_or_else(|| {
                        runtime_error(
                            &parameter.name,
                            &format!("contract '{contract}' has no provider binding"),
                        )
                        .with_code("ORNI-E102")
                    })?;
                    Value::Provider {
                        contract: contract.clone(),
                        provider,
                    }
                }
                _ => match values.next().flatten() {
                    Some(value) => value,
                    None => self.evaluate(
                        parameter
                            .default_value
                            .as_ref()
                            .expect("the arity check guarantees a default value"),
                    )?,
                },
            };
            let value = coerce_value(value, Some(parameter.type_name.clone()), &parameter.name)?;
            self.define_variable(identifier_name(&parameter.name), value, &parameter.name)?;
        }
        Ok(())
    }
}

/// Extracts one positive native duration expressed internally in milliseconds.
fn positive_duration(value: Value, token: &Token, setting: &str) -> Result<u64, OrniError> {
    match value {
        Value::Duration(milliseconds) if milliseconds > 0 => Ok(milliseconds),
        _ => Err(
            runtime_error(token, &format!("{setting} must be a positive Duration"))
                .with_code("ORNI-E107"),
        ),
    }
}

/// Rejects wildcard and path-bearing values so browser access stays explicit.
fn is_valid_cors_origin(origin: &str) -> bool {
    let Some((scheme, authority)) = origin.split_once("://") else {
        return false;
    };
    matches!(scheme, "http" | "https")
        && !authority.is_empty()
        && !authority.contains(['/', '?', '#', '@', ' '])
}
