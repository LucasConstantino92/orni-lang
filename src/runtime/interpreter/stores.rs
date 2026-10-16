//! In-memory CRUD operations used by the native HTTP runtime.

use std::cmp::Ordering;

use crate::{
    diagnostics::OrniError,
    syntax::{IntegerType, Token, Value},
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
            },
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
                store.values.insert(id, model.clone());
                Value::Response {
                    status: 201,
                    body: Box::new(model),
                    headers: Vec::new(),
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
                    })
                    .unwrap_or_else(|| Value::Response {
                        status: 404,
                        body: Box::new(Value::String("Not Found".into())),
                        headers: Vec::new(),
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
                if let std::collections::btree_map::Entry::Occupied(mut entry) =
                    store.values.entry(id)
                {
                    entry.insert(model.clone());
                    Value::Response {
                        status: 200,
                        body: Box::new(model),
                        headers: Vec::new(),
                    }
                } else {
                    Value::Response {
                        status: 404,
                        body: Box::new(Value::String("Not Found".into())),
                        headers: Vec::new(),
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
                }) = store.values.get_mut(&id)
                else {
                    return Ok(Some(Value::Response {
                        status: 404,
                        body: Box::new(Value::String("Not Found".into())),
                        headers: Vec::new(),
                    }));
                };
                for (name, value) in fields {
                    current.insert(name, value);
                }
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
                }
            }
            "delete" => match values.first() {
                Some(Value::Uuid(id)) => {
                    if store.values.remove(id).is_some() {
                        Value::Response {
                            status: 204,
                            body: Box::new(Value::Unit),
                            headers: Vec::new(),
                        }
                    } else {
                        Value::Response {
                            status: 404,
                            body: Box::new(Value::String("Not Found".into())),
                            headers: Vec::new(),
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
    }
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
