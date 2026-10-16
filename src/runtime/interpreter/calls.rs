//! Function invocation and framework built-ins.

use crate::{
    diagnostics::OrniError,
    syntax::{Expression, Parameter, Token, TypeName, Value},
};

#[cfg(not(target_arch = "wasm32"))]
use super::api_test_request;
use super::{
    Interpreter, coerce_value, expression_token, identifier_name, runtime_error,
    validate_response_header,
};
#[cfg(not(target_arch = "wasm32"))]
use crate::syntax::IntegerType;

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
        if matches!(name, "api.status" | "api.body" | "api.contains") {
            let valid_arity = if name == "api.contains" {
                (3..=4).contains(&arguments.len())
            } else {
                (2..=3).contains(&arguments.len())
            };
            if !valid_arity {
                let expected = if name == "api.contains" {
                    "3 or 4"
                } else {
                    "2 or 3"
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
            let (fragment, body) = if name == "api.contains" {
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
                (Some(fragment), body)
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
                (None, body)
            };
            #[cfg(not(target_arch = "wasm32"))]
            if let Some(context) = &self.api_test {
                let (status, response_body) = api_test_request(context, method, path, body);
                return Ok(if name == "api.status" {
                    Value::Integer {
                        value: status.into(),
                        integer_type: IntegerType::smallest_signed(status.into()),
                    }
                } else if name == "api.body" {
                    Value::String(response_body.into())
                } else {
                    Value::Bool(
                        response_body.contains(fragment.expect("contains has a fragment").as_ref()),
                    )
                });
            }
            #[cfg(target_arch = "wasm32")]
            let _ = (method, path, fragment, body);
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
