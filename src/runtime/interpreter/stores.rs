//! In-memory CRUD operations used by the native HTTP runtime.

use std::cmp::Ordering;

use crate::{
    diagnostics::OrniError,
    runtime::MemoryStore,
    syntax::{IntegerType, ModelFieldRule, Token, TypeName, Value},
};

use super::{Interpreter, identifier_name, runtime_error};

impl Interpreter {
    /// Performs one CRUD operation against an HTTP-owned in-memory store.
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn store_call(
        &mut self,
        name: &str,
        _token: &Token,
        values: &[Value],
    ) -> Result<Option<Value>, OrniError> {
        let Some((store_name, operation)) = name.rsplit_once('.') else {
            return Ok(None);
        };
        let Some(stores) = &self.stores else {
            return Ok(None);
        };
        let mut stores = stores.lock().map_err(|_| {
            runtime_error(_token, "in-memory store lock was poisoned").with_code("ORNI-R001")
        })?;
        let Some(store) = stores.get_mut(store_name) else {
            return Ok(None);
        };
        let response = match operation {
            "list" if values.is_empty() => Value::Response {
                status: 200,
                body: Box::new(Value::List(store.values.values().cloned().collect())),
                headers: Vec::new(),
                json_envelope: false,
            },
            "list" if matches!(values, [Value::Integer { .. }, Value::String(_)]) => {
                let limit = match values.first() {
                    Some(Value::Integer { value, .. }) if (1..=100).contains(value) => *value,
                    _ => return Ok(Some(client_error("limit must be between 1 and 100"))),
                };
                let cursor = match values.get(1) {
                    Some(Value::String(cursor)) if cursor.is_empty() => None,
                    Some(Value::String(cursor)) => match uuid::Uuid::parse_str(cursor) {
                        Ok(cursor) => Some(cursor),
                        Err(_) => {
                            return Ok(Some(client_error(
                                "cursor must be a value returned by this endpoint",
                            )));
                        }
                    },
                    _ => unreachable!("type checking validates cursor arguments"),
                };
                let mut rows = store
                    .values
                    .iter()
                    .filter(|(id, _)| cursor.is_none_or(|cursor| **id > cursor))
                    .take(limit as usize + 1)
                    .map(|(id, model)| (*id, model.clone()))
                    .collect::<Vec<_>>();
                let has_more = rows.len() > limit as usize;
                rows.truncate(limit as usize);
                let mut fields = std::collections::BTreeMap::from([
                    (
                        "data".to_owned(),
                        Value::List(rows.iter().map(|(_, model)| model.clone()).collect()),
                    ),
                    ("limit".to_owned(), pagination_integer(limit)),
                    ("has_more".to_owned(), Value::Bool(has_more)),
                ]);
                if has_more && let Some((id, _)) = rows.last() {
                    fields.insert(
                        "next_cursor".to_owned(),
                        Value::String(id.to_string().into()),
                    );
                }
                Value::Response {
                    status: 200,
                    body: Box::new(Value::Model {
                        name: "CursorPage".to_owned(),
                        fields,
                    }),
                    headers: Vec::new(),
                    json_envelope: false,
                }
            }
            "list" => {
                let page = match values.first() {
                    Some(Value::Integer { value, .. }) if *value > 0 => *value,
                    _ => return Ok(Some(client_error("page must be at least 1"))),
                };
                let limit = match values.get(1) {
                    Some(Value::Integer { value, .. }) if (1..=100).contains(value) => *value,
                    _ => return Ok(Some(client_error("limit must be between 1 and 100"))),
                };
                let total = store.values.len() as i128;
                let offset = (page - 1)
                    .checked_mul(limit)
                    .and_then(|value| usize::try_from(value).ok())
                    .unwrap_or(usize::MAX);
                let mut rows = store.values.values().cloned().collect::<Vec<_>>();
                if values.len() == 4 {
                    let (field, direction) = match (&values[2], &values[3]) {
                        (Value::String(field), Value::String(direction)) => {
                            (field.as_ref(), direction.as_ref())
                        }
                        _ => unreachable!("type checking validates list sorting arguments"),
                    };
                    if !store
                        .fields
                        .iter()
                        .any(|candidate| identifier_name(&candidate.name) == field)
                    {
                        return Ok(Some(client_error(&format!("unknown sort field '{field}'"))));
                    }
                    if !matches!(direction, "asc" | "desc") {
                        return Ok(Some(client_error("sort direction must be 'asc' or 'desc'")));
                    }
                    rows.sort_by(|left, right| {
                        let ordering = model_field_order(left, right, field);
                        if direction == "desc" {
                            ordering.reverse()
                        } else {
                            ordering
                        }
                    });
                }
                let data = rows.into_iter().skip(offset).take(limit as usize).collect();
                let total_pages = if total == 0 {
                    0
                } else {
                    (total + limit - 1) / limit
                };
                Value::Response {
                    status: 200,
                    body: Box::new(Value::Model {
                        name: "Page".to_owned(),
                        fields: std::collections::BTreeMap::from([
                            ("data".to_owned(), Value::List(data)),
                            ("page".to_owned(), pagination_integer(page)),
                            ("limit".to_owned(), pagination_integer(limit)),
                            ("total".to_owned(), pagination_integer(total)),
                            ("total_pages".to_owned(), pagination_integer(total_pages)),
                        ]),
                    }),
                    headers: Vec::new(),
                    json_envelope: false,
                }
            }
            "search" => {
                let (field, query, limit, cursor, sort, direction) = match values {
                    [
                        Value::String(field),
                        Value::String(query),
                        Value::Integer { value: limit, .. },
                        Value::String(cursor),
                    ] => (
                        field.to_string(),
                        query.to_string(),
                        *limit,
                        cursor.to_string(),
                        field.to_string(),
                        "asc".to_owned(),
                    ),
                    [
                        Value::String(field),
                        Value::String(query),
                        Value::Integer { value: limit, .. },
                        Value::String(cursor),
                        Value::String(sort),
                        Value::String(direction),
                    ] => (
                        field.to_string(),
                        query.to_string(),
                        *limit,
                        cursor.to_string(),
                        sort.to_string(),
                        direction.to_string(),
                    ),
                    _ => unreachable!("type checking validates search arguments"),
                };
                if !(1..=100).contains(&limit) {
                    return Ok(Some(client_error("limit must be between 1 and 100")));
                }
                let searchable = store.fields.iter().any(|candidate| {
                    identifier_name(&candidate.name) == field
                        && matches!(candidate.type_name, TypeName::String(_))
                });
                if !searchable {
                    return Ok(Some(client_error(&format!(
                        "search field '{field}' must be a declared string field"
                    ))));
                }
                if !store
                    .fields
                    .iter()
                    .any(|candidate| identifier_name(&candidate.name) == sort)
                {
                    return Ok(Some(client_error(&format!("unknown sort field '{sort}'"))));
                }
                if !matches!(direction.as_str(), "asc" | "desc") {
                    return Ok(Some(client_error("sort direction must be 'asc' or 'desc'")));
                }
                let query = query.trim().to_lowercase();
                let cursor = if cursor.is_empty() {
                    None
                } else {
                    let cursor = match decode_search_cursor(&cursor) {
                        Ok(cursor) => cursor,
                        Err(()) => {
                            return Ok(Some(client_error(
                                "cursor must be a value returned by this search",
                            )));
                        }
                    };
                    if cursor.field != field
                        || cursor.query != query
                        || cursor.sort != sort
                        || cursor.direction != direction
                    {
                        return Ok(Some(client_error(
                            "cursor does not belong to this search or ordering",
                        )));
                    }
                    Some(cursor)
                };
                let mut rows = store
                    .values
                    .iter()
                    .filter(|(_, model)| model_matches_search(model, &field, &query))
                    .map(|(id, model)| (*id, model.clone()))
                    .collect::<Vec<_>>();
                rows.sort_by(|(left_id, left), (right_id, right)| {
                    let ordering = model_field_order(left, right, &sort);
                    if direction == "desc" {
                        ordering.reverse()
                    } else {
                        ordering
                    }
                    .then_with(|| left_id.cmp(right_id))
                });
                if let Some(cursor) = cursor {
                    let Some(position) = rows.iter().position(|(id, _)| *id == cursor.id) else {
                        return Ok(Some(client_error(
                            "cursor is no longer available; restart the search",
                        )));
                    };
                    rows = rows.split_off(position + 1);
                }
                rows.truncate(limit as usize + 1);
                let has_more = rows.len() > limit as usize;
                rows.truncate(limit as usize);
                let mut fields = std::collections::BTreeMap::from([
                    (
                        "data".to_owned(),
                        Value::List(rows.iter().map(|(_, model)| model.clone()).collect()),
                    ),
                    ("limit".to_owned(), pagination_integer(limit)),
                    ("has_more".to_owned(), Value::Bool(has_more)),
                ]);
                if has_more && let Some((id, _)) = rows.last() {
                    fields.insert(
                        "next_cursor".to_owned(),
                        Value::String(
                            encode_search_cursor(&field, &query, &sort, &direction, *id).into(),
                        ),
                    );
                }
                Value::Response {
                    status: 200,
                    body: Box::new(Value::Model {
                        name: "CursorPage".to_owned(),
                        fields,
                    }),
                    headers: Vec::new(),
                    json_envelope: false,
                }
            }
            "create" => {
                let input = values
                    .first()
                    .cloned()
                    .expect("type checking validates store arity");
                let Value::Model { fields, .. } = input else {
                    unreachable!("type checking validates create models")
                };
                let id = uuid::Uuid::new_v4();
                let mut fields = fields;
                fields.insert("id".to_owned(), Value::Uuid(id));
                let model = Value::Model {
                    name: store.model.clone(),
                    fields,
                };
                if let Some(field) = duplicate_unique_field(store, &model, None) {
                    return Ok(Some(conflict_error(&format!(
                        "a resource with this '{field}' already exists"
                    ))));
                }
                store.values.insert(id, model.clone());
                Value::Response {
                    status: 201,
                    body: Box::new(model),
                    headers: Vec::new(),
                    json_envelope: false,
                }
            }
            "find" => match values
                .first()
                .cloned()
                .expect("type checking validates store arity")
            {
                Value::Uuid(id) => store
                    .values
                    .get(&id)
                    .cloned()
                    .map(|model| Value::Response {
                        status: 200,
                        body: Box::new(model),
                        headers: Vec::new(),
                        json_envelope: false,
                    })
                    .unwrap_or_else(|| Value::Response {
                        status: 404,
                        body: Box::new(Value::String("Not Found".into())),
                        headers: Vec::new(),
                        json_envelope: false,
                    }),
                _ => unreachable!("type checking validates store IDs"),
            },
            "replace" => {
                let id = match values.first() {
                    Some(Value::Uuid(id)) => *id,
                    _ => unreachable!("type checking validates store IDs"),
                };
                let model = values
                    .get(1)
                    .cloned()
                    .expect("type checking validates store arity");
                let Value::Model { fields, .. } = model else {
                    unreachable!("type checking validates replace models")
                };
                let mut fields = fields;
                fields.insert("id".to_owned(), Value::Uuid(id));
                let model = Value::Model {
                    name: store.model.clone(),
                    fields,
                };
                if let Some(field) = duplicate_unique_field(store, &model, Some(id)) {
                    return Ok(Some(conflict_error(&format!(
                        "a resource with this '{field}' already exists"
                    ))));
                }
                if let std::collections::btree_map::Entry::Occupied(mut entry) =
                    store.values.entry(id)
                {
                    entry.insert(model.clone());
                    Value::Response {
                        status: 200,
                        body: Box::new(model),
                        headers: Vec::new(),
                        json_envelope: false,
                    }
                } else {
                    Value::Response {
                        status: 404,
                        body: Box::new(Value::String("Not Found".into())),
                        headers: Vec::new(),
                        json_envelope: false,
                    }
                }
            }
            "patch" => {
                let id = match values.first() {
                    Some(Value::Uuid(id)) => *id,
                    _ => unreachable!("type checking validates store IDs"),
                };
                let Value::Model { fields, .. } = values
                    .get(1)
                    .cloned()
                    .expect("type checking validates store arity")
                else {
                    unreachable!("type checking validates patch models")
                };
                let Some(Value::Model {
                    fields: current, ..
                }) = store.values.get(&id)
                else {
                    return Ok(Some(Value::Response {
                        status: 404,
                        body: Box::new(Value::String("Not Found".into())),
                        headers: Vec::new(),
                        json_envelope: false,
                    }));
                };
                let mut updated_fields = current.clone();
                for (name, value) in fields {
                    updated_fields.insert(name, value);
                }
                let updated = Value::Model {
                    name: store.model.clone(),
                    fields: updated_fields,
                };
                if let Some(field) = duplicate_unique_field(store, &updated, Some(id)) {
                    return Ok(Some(conflict_error(&format!(
                        "a resource with this '{field}' already exists"
                    ))));
                }
                store.values.insert(id, updated);
                Value::Response {
                    status: 200,
                    body: Box::new(
                        store
                            .values
                            .get(&id)
                            .cloned()
                            .expect("the stored value exists"),
                    ),
                    headers: Vec::new(),
                    json_envelope: false,
                }
            }
            "delete" => match values.first() {
                Some(Value::Uuid(id)) => {
                    if store.values.remove(id).is_some() {
                        Value::Response {
                            status: 204,
                            body: Box::new(Value::Unit),
                            headers: Vec::new(),
                            json_envelope: false,
                        }
                    } else {
                        Value::Response {
                            status: 404,
                            body: Box::new(Value::String("Not Found".into())),
                            headers: Vec::new(),
                            json_envelope: false,
                        }
                    }
                }
                _ => unreachable!("type checking validates store IDs"),
            },
            _ => return Ok(None),
        };
        Ok(Some(response))
    }
}

fn pagination_integer(value: i128) -> Value {
    Value::Integer {
        value,
        integer_type: IntegerType::smallest_signed(value),
    }
}

fn client_error(message: &str) -> Value {
    Value::Response {
        status: 400,
        body: Box::new(Value::String(message.into())),
        headers: Vec::new(),
        json_envelope: false,
    }
}

/// Produces a conflict response when a model would violate one unique field.
fn conflict_error(message: &str) -> Value {
    Value::Response {
        status: 409,
        body: Box::new(Value::String(message.into())),
        headers: Vec::new(),
        json_envelope: false,
    }
}

/// Finds a configured unique field that another record already owns.
fn duplicate_unique_field(
    store: &MemoryStore,
    candidate: &Value,
    excluded_id: Option<uuid::Uuid>,
) -> Option<String> {
    let Value::Model { fields, .. } = candidate else {
        return None;
    };
    store.fields.iter().find_map(|field| {
        if !field.rules.contains(&ModelFieldRule::Unique) {
            return None;
        }
        let name = identifier_name(&field.name);
        let value = fields.get(name)?;
        store.values.iter().find_map(|(id, model)| {
            if Some(*id) == excluded_id {
                return None;
            }
            let Value::Model { fields, .. } = model else {
                return None;
            };
            (fields.get(name) == Some(value)).then(|| name.to_owned())
        })
    })
}

/// One opaque cursor bound to its normalized search and ordering context.
struct SearchCursor {
    field: String,
    query: String,
    sort: String,
    direction: String,
    id: uuid::Uuid,
}

/// Encodes search state without exposing separators from user-provided text.
fn encode_search_cursor(
    field: &str,
    query: &str,
    sort: &str,
    direction: &str,
    id: uuid::Uuid,
) -> String {
    format!(
        "orni1.{}.{}.{}.{}.{}",
        hex_component(field),
        hex_component(query),
        hex_component(sort),
        hex_component(direction),
        id,
    )
}

/// Decodes and validates the opaque cursor format before applying it.
fn decode_search_cursor(value: &str) -> Result<SearchCursor, ()> {
    let parts = value.split('.').collect::<Vec<_>>();
    let ["orni1", field, query, sort, direction, id] = parts.as_slice() else {
        return Err(());
    };
    Ok(SearchCursor {
        field: decode_hex_component(field)?,
        query: decode_hex_component(query)?,
        sort: decode_hex_component(sort)?,
        direction: decode_hex_component(direction)?,
        id: uuid::Uuid::parse_str(id).map_err(|_| ())?,
    })
}

/// Hex-encodes UTF-8 cursor components using only URL-safe characters.
fn hex_component(value: &str) -> String {
    value.bytes().map(|byte| format!("{byte:02x}")).collect()
}

/// Restores one validated UTF-8 component from an opaque cursor.
fn decode_hex_component(value: &str) -> Result<String, ()> {
    if !value.len().is_multiple_of(2) {
        return Err(());
    }
    let bytes = (0..value.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&value[index..index + 2], 16).map_err(|_| ()))
        .collect::<Result<Vec<_>, _>>()?;
    String::from_utf8(bytes).map_err(|_| ())
}

/// Matches a normalized case-insensitive text query against one declared field.
fn model_matches_search(model: &Value, field: &str, query: &str) -> bool {
    query.is_empty()
        || matches!(
            model,
            Value::Model { fields, .. }
                if fields
                    .get(field)
                    .is_some_and(|value| matches!(value, Value::String(value) if value.to_lowercase().contains(query)))
        )
}

fn model_field_order(left: &Value, right: &Value, field: &str) -> Ordering {
    let (Value::Model { fields: left, .. }, Value::Model { fields: right, .. }) = (left, right)
    else {
        return Ordering::Equal;
    };
    let ordering = match (left.get(field), right.get(field)) {
        (Some(Value::Integer { value: left, .. }), Some(Value::Integer { value: right, .. })) => {
            left.cmp(right)
        }
        (Some(Value::Double(left)), Some(Value::Double(right))) => {
            left.partial_cmp(right).unwrap_or(Ordering::Equal)
        }
        (Some(Value::String(left)), Some(Value::String(right))) => left.cmp(right),
        (Some(Value::Bool(left)), Some(Value::Bool(right))) => left.cmp(right),
        (Some(Value::Char(left)), Some(Value::Char(right))) => left.cmp(right),
        (Some(Value::Uuid(left)), Some(Value::Uuid(right))) => left.cmp(right),
        _ => Ordering::Equal,
    };
    ordering.then_with(|| match (left.get("id"), right.get("id")) {
        (Some(Value::Uuid(left)), Some(Value::Uuid(right))) => left.cmp(right),
        _ => Ordering::Equal,
    })
}
