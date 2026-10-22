//! HTTP response construction and serialization.
//!
//! Route execution yields Orni values. This boundary converts those values into
//! safe, deterministic HTTP representations.

use crate::syntax::Value;

use super::BodyValidationError;

/// A fully formed HTTP response, including its representation type.
#[derive(Debug, Clone, PartialEq)]
pub(in crate::runtime) struct HttpResponse {
    pub(super) status: u16,
    pub(super) content_type: String,
    pub(super) headers: Vec<(String, String)>,
    pub(super) body: String,
}

impl HttpResponse {
    pub(super) fn text(status: u16, body: impl Into<String>) -> Self {
        Self {
            status,
            content_type: "text/plain; charset=utf-8".to_owned(),
            headers: Vec::new(),
            body: body.into(),
        }
    }

    /// Creates the stable JSON representation used by framework and application errors.
    pub(super) fn error(status: u16, code: &'static str, message: impl Into<String>) -> Self {
        let message = message.into();
        Self {
            status,
            content_type: "application/json; charset=utf-8".to_owned(),
            headers: Vec::new(),
            body: serde_json::json!({
                "error": {
                    "status": status,
                    "code": code,
                    "message": message,
                }
            })
            .to_string(),
        }
    }

    /// Creates a JSON validation response with optional field-specific details.
    pub(super) fn invalid_body(error: BodyValidationError) -> Self {
        let mut body = serde_json::json!({
            "error": {
                "status": 400,
                "code": "ORNI_HTTP_INVALID_BODY",
                "message": error.message,
            }
        });
        if let Some(field) = error.field {
            body["error"]["details"] = serde_json::json!({ field: body["error"]["message"] });
        }
        Self {
            status: 400,
            content_type: "application/json; charset=utf-8".to_owned(),
            headers: Vec::new(),
            body: body.to_string(),
        }
    }

    /// Adds one response header while preserving the response body.
    pub(super) fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        let name = name.into();
        let value = value.into();
        if name.eq_ignore_ascii_case("content-type") {
            self.content_type = value;
            return self;
        }
        self.headers
            .retain(|(existing, _)| !existing.eq_ignore_ascii_case(&name));
        self.headers.push((name, value));
        self
    }

    /// Converts a successful tagged representation into a conditional GET response.
    pub(super) fn not_modified_if_matches(mut self, if_none_match: Option<&str>) -> Self {
        let Some(if_none_match) = if_none_match else {
            return self;
        };
        if self.status != 200 {
            return self;
        }
        let Some((_, entity_tag)) = self
            .headers
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case("etag"))
        else {
            return self;
        };
        let matches = if_none_match
            .split(',')
            .map(str::trim)
            .any(|candidate| candidate == "*" || candidate == entity_tag);
        if matches {
            self.status = 304;
            self.body.clear();
        }
        self
    }
}

/// Selects a status and serializes the route value at the HTTP boundary.
pub(super) fn response_value(value: Value) -> HttpResponse {
    let (status, body, headers, json_envelope) = match value {
        Value::Response {
            status,
            body,
            headers,
            json_envelope,
        } => (status, *body, headers, json_envelope),
        body => (200, body, Vec::new(), false),
    };
    let response = if json_envelope {
        match serde_json::to_string(&serde_json::json!({ "data": model_json(&body) })) {
            Ok(body) => HttpResponse {
                status,
                content_type: "application/json; charset=utf-8".to_owned(),
                headers: Vec::new(),
                body,
            },
            Err(_) => HttpResponse::error(500, "ORNI_HTTP_INTERNAL_ERROR", "Internal Server Error"),
        }
    } else {
        match body {
            Value::String(location) if status == 302 => {
                HttpResponse::text(302, "Redirecting to documentation.")
                    .with_header("Location", location.to_string())
            }
            Value::String(body) if status >= 400 => {
                HttpResponse::error(status, response_error_code(status), body.to_string())
            }
            Value::String(body) => HttpResponse::text(status, body.to_string()),
            Value::Unit if status == 204 => HttpResponse::text(status, ""),
            Value::Collection { data, meta } => {
                let meta = meta
                    .iter()
                    .map(|(name, value)| (name.clone(), model_json(value)))
                    .collect::<serde_json::Map<_, _>>();
                match serde_json::to_string(&serde_json::json!({
                    "data": model_json(&data),
                    "meta": meta,
                })) {
                    Ok(body) => HttpResponse {
                        status,
                        content_type: "application/json; charset=utf-8".to_owned(),
                        headers: Vec::new(),
                        body,
                    },
                    Err(_) => HttpResponse::error(
                        500,
                        "ORNI_HTTP_INTERNAL_ERROR",
                        "Internal Server Error",
                    ),
                }
            }
            Value::Model { .. } | Value::List(_) => {
                match serde_json::to_string(&model_json(&body)) {
                    Ok(body) => HttpResponse {
                        status,
                        content_type: "application/json; charset=utf-8".to_owned(),
                        headers: Vec::new(),
                        body,
                    },
                    Err(_) => HttpResponse::error(
                        500,
                        "ORNI_HTTP_INTERNAL_ERROR",
                        "Internal Server Error",
                    ),
                }
            }
            _ => HttpResponse::error(500, "ORNI_HTTP_INTERNAL_ERROR", "Internal Server Error"),
        }
    };
    headers
        .into_iter()
        .fold(response, |response, (name, value)| {
            response.with_header(name, value)
        })
}

/// Maps public HTTP status codes to stable response error identities.
fn response_error_code(status: u16) -> &'static str {
    match status {
        400 => "ORNI_HTTP_BAD_REQUEST",
        401 => "ORNI_HTTP_UNAUTHORIZED",
        403 => "ORNI_HTTP_FORBIDDEN",
        404 => "ORNI_HTTP_RESOURCE_NOT_FOUND",
        409 => "ORNI_HTTP_CONFLICT",
        412 => "ORNI_HTTP_PRECONDITION_FAILED",
        _ => "ORNI_HTTP_INTERNAL_ERROR",
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
        Value::Size(bytes) => serde_json::Value::Number((*bytes).into()),
        Value::Duration(milliseconds) => serde_json::Value::Number((*milliseconds).into()),
        Value::Model { fields, .. } => serde_json::Value::Object(
            fields
                .iter()
                .map(|(name, value)| (name.clone(), model_json(value)))
                .collect(),
        ),
        Value::List(values) => serde_json::Value::Array(values.iter().map(model_json).collect()),
        Value::Collection { data, meta } => serde_json::json!({
            "data": model_json(data),
            "meta": meta
                .iter()
                .map(|(name, value)| (name.clone(), model_json(value)))
                .collect::<serde_json::Map<_, _>>(),
        }),
        Value::Unit | Value::Response { .. } | Value::Provider { .. } => serde_json::Value::Null,
    }
}
