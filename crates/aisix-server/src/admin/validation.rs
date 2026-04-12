use aisix_types::error::{ErrorKind, GatewayError};
use serde_json::Value;

use crate::openapi;

pub fn validate_admin_put_request(collection: &str, payload: &Value) -> Result<(), GatewayError> {
    let schema = openapi::admin_put_schema(collection).ok_or_else(|| GatewayError {
        kind: ErrorKind::Internal,
        message: format!("missing admin schema for collection '{collection}'"),
    })?;

    validate_value(schema, payload, "$")
}

fn validate_value(schema: &Value, value: &Value, path: &str) -> Result<(), GatewayError> {
    if let Some(any_of) = schema.get("anyOf").and_then(Value::as_array) {
        for candidate in any_of {
            if validate_value(candidate, value, path).is_ok() {
                return Ok(());
            }
        }
        return Err(invalid_request(format!(
            "{path} does not match any allowed schema"
        )));
    }

    match schema.get("type").and_then(Value::as_str) {
        Some("object") => validate_object(schema, value, path),
        Some("array") => validate_array(schema, value, path),
        Some("string") => validate_string(schema, value, path),
        Some("integer") => validate_integer(value, path),
        Some("null") => {
            if value.is_null() {
                Ok(())
            } else {
                Err(invalid_request(format!("{path} must be null")))
            }
        }
        Some(other) => Err(invalid_request(format!(
            "unsupported schema type '{other}' at {path}"
        ))),
        None => Ok(()),
    }
}

fn validate_object(schema: &Value, value: &Value, path: &str) -> Result<(), GatewayError> {
    let object = value
        .as_object()
        .ok_or_else(|| invalid_request(format!("{path} must be an object")))?;
    let properties = schema
        .get("properties")
        .and_then(Value::as_object)
        .ok_or_else(|| invalid_request(format!("{path} schema is missing properties")))?;

    if schema.get("additionalProperties").and_then(Value::as_bool) == Some(false) {
        for key in object.keys() {
            if !properties.contains_key(key) {
                return Err(invalid_request(format!("{path}.{key} is not allowed")));
            }
        }
    }

    for required in schema
        .get("required")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let key = required
            .as_str()
            .ok_or_else(|| invalid_request(format!("{path} schema has non-string required key")))?;
        if !object.contains_key(key) {
            return Err(invalid_request(format!("{path}.{key} is required")));
        }
    }

    for (key, child_value) in object {
        if let Some(child_schema) = properties.get(key) {
            validate_value(child_schema, child_value, &format!("{path}.{key}"))?;
        }
    }

    Ok(())
}

fn validate_array(schema: &Value, value: &Value, path: &str) -> Result<(), GatewayError> {
    let array = value
        .as_array()
        .ok_or_else(|| invalid_request(format!("{path} must be an array")))?;
    let item_schema = schema
        .get("items")
        .ok_or_else(|| invalid_request(format!("{path} schema is missing items")))?;

    for (index, item) in array.iter().enumerate() {
        validate_value(item_schema, item, &format!("{path}[{index}]"))?;
    }

    Ok(())
}

fn validate_string(schema: &Value, value: &Value, path: &str) -> Result<(), GatewayError> {
    let string = value
        .as_str()
        .ok_or_else(|| invalid_request(format!("{path} must be a string")))?;

    if let Some(enum_values) = schema.get("enum").and_then(Value::as_array) {
        let valid = enum_values
            .iter()
            .filter_map(Value::as_str)
            .any(|item| item == string);
        if !valid {
            return Err(invalid_request(format!(
                "{path} must be one of the allowed enum values"
            )));
        }
    }

    Ok(())
}

fn validate_integer(value: &Value, path: &str) -> Result<(), GatewayError> {
    if value.as_i64().is_some() || value.as_u64().is_some() {
        return Ok(());
    }

    Err(invalid_request(format!("{path} must be an integer")))
}

fn invalid_request(message: String) -> GatewayError {
    GatewayError {
        kind: ErrorKind::InvalidRequest,
        message,
    }
}
