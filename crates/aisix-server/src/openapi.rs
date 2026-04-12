use std::sync::OnceLock;

use aisix_config::etcd_model::{
    ApiKeyConfig, CacheMode, ModelConfig, PolicyConfig, ProviderConfig, ProviderKind,
};
use serde_json::{json, Map, Value};

use crate::admin::AdminWriteResult;

static ADMIN_OPENAPI_JSON: OnceLock<Value> = OnceLock::new();

pub fn admin_openapi() -> &'static Value {
    ADMIN_OPENAPI_JSON.get_or_init(build_admin_openapi)
}

pub fn admin_put_schema(collection: &str) -> Option<&'static Value> {
    admin_openapi()
        .get("x-aisix-admin-put-schemas")?
        .get(collection)
}

fn build_admin_openapi() -> Value {
    let mut put_schemas = Map::new();
    put_schemas.insert("providers".to_string(), provider_schema());
    put_schemas.insert("models".to_string(), model_schema());
    put_schemas.insert("apikeys".to_string(), apikey_schema());
    put_schemas.insert("policies".to_string(), policy_schema());

    json!({
        "openapi": "3.1.0",
        "info": {
            "title": "AISIX Admin API",
            "version": env!("CARGO_PKG_VERSION")
        },
        "paths": {
            "/admin/providers": list_path("ProviderConfig"),
            "/admin/providers/{id}": item_path("ProviderConfig"),
            "/admin/models": list_path("ModelConfig"),
            "/admin/models/{id}": item_path("ModelConfig"),
            "/admin/apikeys": list_path("ApiKeyConfig"),
            "/admin/apikeys/{id}": item_path("ApiKeyConfig"),
            "/admin/policies": list_path("PolicyConfig"),
            "/admin/policies/{id}": item_path("PolicyConfig")
        },
        "components": {
            "schemas": {
                "ProviderConfig": provider_schema(),
                "ModelConfig": model_schema(),
                "ApiKeyConfig": apikey_schema(),
                "PolicyConfig": policy_schema(),
                "AdminWriteResult": admin_write_result_schema()
            }
        },
        "x-aisix-admin-put-schemas": Value::Object(put_schemas)
    })
}

fn list_path(component: &str) -> Value {
    json!({
        "get": {
            "parameters": [admin_key_parameter()],
            "responses": {
                "200": {
                    "description": "OK",
                    "content": {
                        "application/json": {
                            "schema": {
                                "type": "array",
                                "items": { "$ref": format!("#/components/schemas/{component}") }
                            }
                        }
                    }
                }
            }
        }
    })
}

fn item_path(component: &str) -> Value {
    json!({
        "get": {
            "parameters": [admin_key_parameter(), id_parameter()],
            "responses": {
                "200": {
                    "description": "OK",
                    "content": {
                        "application/json": {
                            "schema": { "$ref": format!("#/components/schemas/{component}") }
                        }
                    }
                }
            }
        },
        "put": {
            "parameters": [admin_key_parameter(), id_parameter()],
            "requestBody": {
                "required": true,
                "content": {
                    "application/json": {
                        "schema": { "$ref": format!("#/components/schemas/{component}") }
                    }
                }
            },
            "responses": {
                "200": {
                    "description": "OK",
                    "content": {
                        "application/json": {
                            "schema": { "$ref": "#/components/schemas/AdminWriteResult" }
                        }
                    }
                }
            }
        },
        "delete": {
            "parameters": [admin_key_parameter(), id_parameter()],
            "responses": {
                "200": {
                    "description": "OK",
                    "content": {
                        "application/json": {
                            "schema": { "$ref": "#/components/schemas/AdminWriteResult" }
                        }
                    }
                }
            }
        }
    })
}

fn admin_key_parameter() -> Value {
    json!({
        "name": "x-admin-key",
        "in": "header",
        "required": true,
        "schema": { "type": "string" }
    })
}

fn id_parameter() -> Value {
    json!({
        "name": "id",
        "in": "path",
        "required": true,
        "schema": { "type": "string" }
    })
}

fn admin_write_result_schema() -> Value {
    let _ = std::any::type_name::<AdminWriteResult>();
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["id", "path", "revision"],
        "properties": {
            "id": string_schema(),
            "path": string_schema(),
            "revision": { "type": "integer" }
        }
    })
}

fn provider_schema() -> Value {
    let _ = std::any::type_name::<ProviderConfig>();
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["id", "kind", "base_url", "auth"],
        "properties": {
            "id": string_schema(),
            "kind": enum_schema(provider_kind_values()),
            "base_url": string_schema(),
            "auth": {
                "type": "object",
                "additionalProperties": false,
                "required": ["secret_ref"],
                "properties": {
                    "secret_ref": string_schema()
                }
            },
            "policy_id": nullable_string_schema(),
            "rate_limit": nullable_rate_limit_schema(),
            "cache": nullable_cache_schema()
        }
    })
}

fn model_schema() -> Value {
    let _ = std::any::type_name::<ModelConfig>();
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["id", "provider_id", "upstream_model"],
        "properties": {
            "id": string_schema(),
            "provider_id": string_schema(),
            "upstream_model": string_schema(),
            "policy_id": nullable_string_schema(),
            "rate_limit": nullable_rate_limit_schema(),
            "cache": nullable_cache_schema()
        }
    })
}

fn apikey_schema() -> Value {
    let _ = std::any::type_name::<ApiKeyConfig>();
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["id", "key", "allowed_models"],
        "properties": {
            "id": string_schema(),
            "key": string_schema(),
            "allowed_models": {
                "type": "array",
                "items": string_schema()
            },
            "policy_id": nullable_string_schema(),
            "rate_limit": nullable_rate_limit_schema()
        }
    })
}

fn policy_schema() -> Value {
    let _ = std::any::type_name::<PolicyConfig>();
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["id", "rate_limit"],
        "properties": {
            "id": string_schema(),
            "rate_limit": rate_limit_schema()
        }
    })
}

fn rate_limit_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "properties": {
            "rpm": nullable_schema(json!({ "type": "integer" })),
            "tpm": nullable_schema(json!({ "type": "integer" })),
            "concurrency": nullable_schema(json!({ "type": "integer" }))
        }
    })
}

fn nullable_rate_limit_schema() -> Value {
    nullable_schema(rate_limit_schema())
}

fn nullable_cache_schema() -> Value {
    nullable_schema(json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["mode"],
        "properties": {
            "mode": enum_schema(cache_mode_values())
        }
    }))
}

fn string_schema() -> Value {
    json!({ "type": "string" })
}

fn nullable_string_schema() -> Value {
    nullable_schema(string_schema())
}

fn nullable_schema(schema: Value) -> Value {
    json!({
        "anyOf": [
            schema,
            { "type": "null" }
        ]
    })
}

fn enum_schema(values: Vec<&'static str>) -> Value {
    json!({
        "type": "string",
        "enum": values
    })
}

fn provider_kind_values() -> Vec<&'static str> {
    let _ = [
        ProviderKind::OpenAi,
        ProviderKind::AzureOpenAi,
        ProviderKind::Anthropic,
    ];
    vec!["openai", "azure_openai", "anthropic"]
}

fn cache_mode_values() -> Vec<&'static str> {
    let _ = [CacheMode::Inherit, CacheMode::Enabled, CacheMode::Disabled];
    vec!["inherit", "enabled", "disabled"]
}
