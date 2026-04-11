use std::{
    sync::{Mutex, MutexGuard, OnceLock},
    time::{Duration, Instant},
};

use aisix_config::etcd_model::{
    ApiKeyConfig, ModelConfig, PolicyConfig, ProviderAuth, ProviderConfig, ProviderKind,
    RateLimitConfig,
};
use aisix_providers::ProviderRegistry;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use hyper::{
    body::{Bytes, Incoming},
    service::service_fn,
};
use hyper_util::rt::TokioIo;
use serde_json::{json, Value};
use tower::ServiceExt;

mod support {
    pub mod etcd {
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../aisix-config/tests/support/etcd.rs"
        ));
    }
}

#[tokio::test]
async fn admin_can_create_provider_model_and_apikey_then_gateway_uses_reloaded_snapshot() {
    let upstream = spawn_openai_mock().await;

    with_env_var("OPENAI_API_KEY", Some("test-openai-key"), || async {
        let fixture = LiveEtcdTestApp::start().await;
        let app = fixture.router();

        let provider_response = app
            .clone()
            .oneshot(admin_put_request(
                "/admin/providers/openai",
                json!(ProviderConfig {
                    id: "openai".to_string(),
                    kind: ProviderKind::OpenAi,
                    base_url: upstream.base_url.clone(),
                    auth: ProviderAuth {
                        secret_ref: "env:OPENAI_API_KEY".to_string(),
                    },
                    policy_id: None,
                    rate_limit: None,
                    cache: None,
                }),
            ))
            .await
            .unwrap();
        assert_eq!(provider_response.status(), StatusCode::OK);

        let model_response = app
            .clone()
            .oneshot(admin_put_request(
                "/admin/models/gpt-4o-mini",
                json!(ModelConfig {
                    id: "gpt-4o-mini".to_string(),
                    provider_id: "openai".to_string(),
                    upstream_model: "gpt-4o-mini-2024-07-18".to_string(),
                    policy_id: None,
                    rate_limit: None,
                    cache: None,
                }),
            ))
            .await
            .unwrap();
        assert_eq!(model_response.status(), StatusCode::OK);

        let key_response = app
            .clone()
            .oneshot(admin_put_request(
                "/admin/apikeys/vk_admin",
                json!(ApiKeyConfig {
                    id: "vk_admin".to_string(),
                    key: "live-token".to_string(),
                    allowed_models: vec!["gpt-4o-mini".to_string()],
                    policy_id: None,
                    rate_limit: None,
                }),
            ))
            .await
            .unwrap();
        assert_eq!(key_response.status(), StatusCode::OK);

        let response = poll_until_response(|| {
            let app = app.clone();
            async move {
                app.oneshot(chat_request("live-token", "gpt-4o-mini"))
                    .await
                    .unwrap()
            }
        })
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get("x-aisix-provider")
                .and_then(|value| value.to_str().ok()),
            Some("openai")
        );
    })
    .await;
}

#[tokio::test]
async fn admin_put_seeds_from_live_snapshot_and_advances_revision() {
    let upstream = spawn_openai_mock().await;

    with_env_var("OPENAI_API_KEY", Some("test-openai-key"), || async {
        let fixture = LiveEtcdTestApp::start_seeded(&upstream.base_url, None).await;
        let app = fixture.router();

        let response = app
            .clone()
            .oneshot(admin_put_request(
                "/admin/apikeys/vk_new",
                json!(ApiKeyConfig {
                    id: "vk_new".to_string(),
                    key: "new-token".to_string(),
                    allowed_models: vec!["gpt-4o-mini".to_string()],
                    policy_id: None,
                    rate_limit: None,
                }),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let json: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(
            json["revision"].as_i64().unwrap(),
            fixture.seeded_revision() + 1
        );

        let old_key = app
            .clone()
            .oneshot(chat_request("live-token", "gpt-4o-mini"))
            .await
            .unwrap();
        assert_eq!(old_key.status(), StatusCode::OK);

        let new_key = poll_until_response(|| {
            let app = app.clone();
            async move {
                app.oneshot(chat_request("new-token", "gpt-4o-mini"))
                    .await
                    .unwrap()
            }
        })
        .await;
        assert_eq!(new_key.status(), StatusCode::OK);
    })
    .await;
}

#[tokio::test]
async fn unrelated_admin_put_preserves_seeded_apikey_limit_config() {
    let upstream = spawn_openai_mock().await;

    with_env_var("OPENAI_API_KEY", Some("test-openai-key"), || async {
        let fixture = LiveEtcdTestApp::start_seeded_with_limited_key(&upstream.base_url).await;
        let app = fixture.router();

        let first = app
            .clone()
            .oneshot(chat_request("limited-token", "gpt-4o-mini"))
            .await
            .unwrap();
        assert_eq!(first.status(), StatusCode::OK);

        let put = app
            .clone()
            .oneshot(admin_put_request(
                "/admin/providers/openai",
                json!(ProviderConfig {
                    id: "openai".to_string(),
                    kind: ProviderKind::OpenAi,
                    base_url: upstream.base_url.clone(),
                    auth: ProviderAuth {
                        secret_ref: "env:OPENAI_API_KEY".to_string(),
                    },
                    policy_id: None,
                    rate_limit: None,
                    cache: None,
                }),
            ))
            .await
            .unwrap();
        assert_eq!(put.status(), StatusCode::OK);

        let second = poll_until_status(
            || {
                let app = app.clone();
                async move {
                    app.oneshot(chat_request("limited-token", "gpt-4o-mini"))
                        .await
                        .unwrap()
                }
            },
            StatusCode::TOO_MANY_REQUESTS,
        )
        .await;
        assert_eq!(second.status(), StatusCode::TOO_MANY_REQUESTS);

        let body = second.into_body().collect().await.unwrap().to_bytes();
        let json: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"]["message"], "request rate limit exceeded");
    })
    .await;
}

#[tokio::test]
async fn admin_requires_valid_x_admin_key() {
    let fixture = LiveEtcdTestApp::start().await;
    let app = fixture.router();

    let missing = app
        .clone()
        .oneshot(admin_request_with_key(
            "PUT",
            "/admin/providers/openai",
            Some(json!({
                "id": "openai",
                "kind": "openai",
                "base_url": "http://127.0.0.1:1",
                "auth": {"secret_ref": "env:OPENAI_API_KEY"},
                "policy_id": null,
                "rate_limit": null
            })),
            None,
        ))
        .await
        .unwrap();
    assert_eq!(missing.status(), StatusCode::UNAUTHORIZED);

    let invalid = app
        .clone()
        .oneshot(admin_request_with_key(
            "PUT",
            "/admin/providers/openai",
            Some(json!({
                "id": "openai",
                "kind": "openai",
                "base_url": "http://127.0.0.1:1",
                "auth": {"secret_ref": "env:OPENAI_API_KEY"},
                "policy_id": null,
                "rate_limit": null
            })),
            Some("wrong-key"),
        ))
        .await
        .unwrap();
    assert_eq!(invalid.status(), StatusCode::UNAUTHORIZED);

    let missing_get = app
        .clone()
        .oneshot(admin_request_with_key(
            "GET",
            "/admin/providers/openai",
            None,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(missing_get.status(), StatusCode::UNAUTHORIZED);

    let invalid_get = app
        .clone()
        .oneshot(admin_request_with_key(
            "GET",
            "/admin/providers/openai",
            None,
            Some("wrong-key"),
        ))
        .await
        .unwrap();
    assert_eq!(invalid_get.status(), StatusCode::UNAUTHORIZED);

    let missing_delete = app
        .clone()
        .oneshot(admin_request_with_key(
            "DELETE",
            "/admin/providers/openai",
            None,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(missing_delete.status(), StatusCode::UNAUTHORIZED);

    let invalid_delete = app
        .oneshot(admin_request_with_key(
            "DELETE",
            "/admin/providers/openai",
            None,
            Some("wrong-key"),
        ))
        .await
        .unwrap();
    assert_eq!(invalid_delete.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn admin_get_missing_resources_return_not_found() {
    let fixture = LiveEtcdTestApp::start().await;
    let app = fixture.router();

    for path in [
        "/admin/providers/missing-provider",
        "/admin/models/missing-model",
        "/admin/apikeys/missing-key",
        "/admin/policies/missing-policy",
    ] {
        let response = app
            .clone()
            .oneshot(admin_request("GET", path, None))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "path: {path}");
    }
}

#[tokio::test]
async fn admin_delete_missing_resources_return_not_found() {
    let fixture = LiveEtcdTestApp::start().await;
    let app = fixture.router();

    for path in [
        "/admin/providers/missing-provider",
        "/admin/models/missing-model",
        "/admin/apikeys/missing-key",
        "/admin/policies/missing-policy",
    ] {
        let response = app
            .clone()
            .oneshot(admin_request("DELETE", path, None))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "path: {path}");
    }
}

#[tokio::test]
async fn admin_ui_entrypoint_serves_html_shell() {
    let fixture = LiveEtcdTestApp::start().await;
    let app = fixture.router();

    let response = app.oneshot(Request::builder().uri("/ui").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok()),
        Some("text/html; charset=utf-8")
    );

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let html = String::from_utf8(body.to_vec()).unwrap();
    assert!(html.contains("AISIX Control Plane"));
    assert!(html.contains("/ui/app.js"));
}

#[tokio::test]
async fn admin_ui_script_serves_browser_app() {
    let fixture = LiveEtcdTestApp::start().await;
    let app = fixture.router();

    let response = app
        .oneshot(Request::builder().uri("/ui/app.js").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok()),
        Some("application/javascript; charset=utf-8")
    );

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let script = String::from_utf8(body.to_vec()).unwrap();
    assert!(script.contains("AISIX Control Plane"));
    assert!(script.contains("/admin/providers"));
}

#[tokio::test]
async fn admin_namespace_does_not_expose_ui_entrypoint() {
    let fixture = LiveEtcdTestApp::start().await;
    let app = fixture.router();

    let response = app
        .oneshot(Request::builder().uri("/admin/ui").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn admin_rejects_path_and_body_id_mismatch() {
    let fixture = LiveEtcdTestApp::start().await;
    let app = fixture.router();

    let response = app
        .oneshot(admin_put_request(
            "/admin/providers/openai",
            json!(ProviderConfig {
                id: "different-provider".to_string(),
                kind: ProviderKind::OpenAi,
                base_url: "http://127.0.0.1:1".to_string(),
                auth: ProviderAuth {
                    secret_ref: "env:OPENAI_API_KEY".to_string(),
                },
                policy_id: None,
                rate_limit: None,
                cache: None,
            }),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        json["error"]["message"],
        "path id 'openai' does not match body id 'different-provider'"
    );
}

#[tokio::test]
async fn admin_rejects_ids_with_path_separators() {
    let fixture = LiveEtcdTestApp::start().await;
    let app = fixture.router();

    let response = app
        .oneshot(admin_put_request(
            "/admin/providers/a%2Fb",
            json!(ProviderConfig {
                id: "a/b".to_string(),
                kind: ProviderKind::OpenAi,
                base_url: "https://api.openai.com".to_string(),
                auth: ProviderAuth {
                    secret_ref: "env:OPENAI_API_KEY".to_string(),
                },
                policy_id: None,
                rate_limit: None,
                cache: None,
            }),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        json["error"]["message"],
        "admin resource id 'a/b' must not contain '/'"
    );
}

#[tokio::test]
async fn admin_put_persists_provider_in_etcd_and_returns_write_revision() {
    let fixture = LiveEtcdTestApp::start().await;
    let app = fixture.router();

    let response = app
        .oneshot(admin_put_request(
            "/admin/providers/openai",
            json!(ProviderConfig {
                id: "openai".to_string(),
                kind: ProviderKind::OpenAi,
                base_url: "https://api.openai.com".to_string(),
                auth: ProviderAuth {
                    secret_ref: "env:OPENAI_API_KEY".to_string(),
                },
                policy_id: None,
                rate_limit: None,
                cache: None,
            }),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["id"], "openai");
    assert_eq!(json["path"], "/aisix/providers/openai");
    assert!(json["revision"].as_i64().unwrap() > 0);

    let stored = fixture
        .harness()
        .get_json("/aisix/providers/openai")
        .await
        .expect("etcd read should succeed")
        .expect("provider should be persisted");
    assert_eq!(stored["id"], "openai");
    assert_eq!(stored["base_url"], "https://api.openai.com");
}

#[tokio::test]
async fn admin_provider_get_list_and_delete_routes_use_live_etcd() {
    let fixture = LiveEtcdTestApp::start().await;
    fixture
        .harness()
        .put_json(
            "/aisix/providers/z-provider",
            &json!(ProviderConfig {
                id: "z-provider".to_string(),
                kind: ProviderKind::OpenAi,
                base_url: "https://z.example.com".to_string(),
                auth: ProviderAuth {
                    secret_ref: "env:OPENAI_API_KEY".to_string(),
                },
                policy_id: None,
                rate_limit: None,
                cache: None,
            }),
        )
        .await
        .expect("provider fixture should be written");
    fixture
        .harness()
        .put_json(
            "/aisix/providers/a-provider",
            &json!(ProviderConfig {
                id: "a-provider".to_string(),
                kind: ProviderKind::OpenAi,
                base_url: "https://a.example.com".to_string(),
                auth: ProviderAuth {
                    secret_ref: "env:OPENAI_API_KEY".to_string(),
                },
                policy_id: None,
                rate_limit: None,
                cache: None,
            }),
        )
        .await
        .expect("provider fixture should be written");
    let app = fixture.router();

    let list = app
        .clone()
        .oneshot(admin_get_request("/admin/providers"))
        .await
        .unwrap();
    assert_eq!(list.status(), StatusCode::OK);

    let list_body = list.into_body().collect().await.unwrap().to_bytes();
    let list_json: Value = serde_json::from_slice(&list_body).unwrap();
    assert_eq!(list_json.as_array().map(Vec::len), Some(2));
    assert_eq!(ids(&list_json), vec!["a-provider", "z-provider"]);

    let get = app
        .clone()
        .oneshot(admin_get_request("/admin/providers/a-provider"))
        .await
        .unwrap();
    assert_eq!(get.status(), StatusCode::OK);

    let get_body = get.into_body().collect().await.unwrap().to_bytes();
    let get_json: Value = serde_json::from_slice(&get_body).unwrap();
    assert_eq!(get_json["id"], "a-provider");
    assert_eq!(get_json["base_url"], "https://a.example.com");

    let missing_before_delete = app
        .clone()
        .oneshot(admin_get_request("/admin/providers/missing-provider"))
        .await
        .unwrap();
    assert_eq!(missing_before_delete.status(), StatusCode::NOT_FOUND);

    let delete = app
        .clone()
        .oneshot(admin_delete_request("/admin/providers/a-provider"))
        .await
        .unwrap();
    assert_eq!(delete.status(), StatusCode::OK);

    let delete_body = delete.into_body().collect().await.unwrap().to_bytes();
    let delete_json: Value = serde_json::from_slice(&delete_body).unwrap();
    assert_eq!(delete_json["id"], "a-provider");
    assert_eq!(delete_json["path"], "/aisix/providers/a-provider");
    assert!(delete_json["revision"].as_i64().unwrap() > 0);

    let deleted = fixture
        .harness()
        .get_json("/aisix/providers/a-provider")
        .await
        .expect("etcd read should succeed");
    assert!(deleted.is_none());

    let missing_after_delete = app
        .clone()
        .oneshot(admin_get_request("/admin/providers/a-provider"))
        .await
        .unwrap();
    assert_eq!(missing_after_delete.status(), StatusCode::NOT_FOUND);

    let missing_delete = app
        .oneshot(admin_delete_request("/admin/providers/a-provider"))
        .await
        .unwrap();
    assert_eq!(missing_delete.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn admin_model_get_list_and_delete_routes_use_live_etcd() {
    let fixture = LiveEtcdTestApp::start().await;
    fixture
        .harness()
        .put_json(
            "/aisix/models/z-model",
            &json!(ModelConfig {
                id: "z-model".to_string(),
                provider_id: "openai".to_string(),
                upstream_model: "z-upstream".to_string(),
                policy_id: None,
                rate_limit: None,
                cache: None,
            }),
        )
        .await
        .expect("model fixture should be written");
    fixture
        .harness()
        .put_json(
            "/aisix/models/a-model",
            &json!(ModelConfig {
                id: "a-model".to_string(),
                provider_id: "openai".to_string(),
                upstream_model: "a-upstream".to_string(),
                policy_id: None,
                rate_limit: None,
                cache: None,
            }),
        )
        .await
        .expect("model fixture should be written");
    let app = fixture.router();

    let list = app
        .clone()
        .oneshot(admin_get_request("/admin/models"))
        .await
        .unwrap();
    assert_eq!(list.status(), StatusCode::OK);

    let list_body = list.into_body().collect().await.unwrap().to_bytes();
    let list_json: Value = serde_json::from_slice(&list_body).unwrap();
    assert_eq!(list_json.as_array().map(Vec::len), Some(2));
    assert_eq!(ids(&list_json), vec!["a-model", "z-model"]);

    let get = app
        .clone()
        .oneshot(admin_get_request("/admin/models/a-model"))
        .await
        .unwrap();
    assert_eq!(get.status(), StatusCode::OK);

    let get_body = get.into_body().collect().await.unwrap().to_bytes();
    let get_json: Value = serde_json::from_slice(&get_body).unwrap();
    assert_eq!(get_json["id"], "a-model");
    assert_eq!(get_json["upstream_model"], "a-upstream");

    let missing_before_delete = app
        .clone()
        .oneshot(admin_get_request("/admin/models/missing-model"))
        .await
        .unwrap();
    assert_eq!(missing_before_delete.status(), StatusCode::NOT_FOUND);

    let delete = app
        .clone()
        .oneshot(admin_delete_request("/admin/models/a-model"))
        .await
        .unwrap();
    assert_eq!(delete.status(), StatusCode::OK);

    let delete_body = delete.into_body().collect().await.unwrap().to_bytes();
    let delete_json: Value = serde_json::from_slice(&delete_body).unwrap();
    assert_eq!(delete_json["id"], "a-model");
    assert_eq!(delete_json["path"], "/aisix/models/a-model");
    assert!(delete_json["revision"].as_i64().unwrap() > 0);

    let deleted = fixture
        .harness()
        .get_json("/aisix/models/a-model")
        .await
        .expect("etcd read should succeed");
    assert!(deleted.is_none());

    let missing_after_delete = app
        .clone()
        .oneshot(admin_get_request("/admin/models/a-model"))
        .await
        .unwrap();
    assert_eq!(missing_after_delete.status(), StatusCode::NOT_FOUND);

    let missing_delete = app
        .oneshot(admin_delete_request("/admin/models/a-model"))
        .await
        .unwrap();
    assert_eq!(missing_delete.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn admin_apikey_get_list_and_delete_routes_use_live_etcd() {
    let fixture = LiveEtcdTestApp::start().await;
    fixture
        .harness()
        .put_json(
            "/aisix/apikeys/z-key",
            &json!(ApiKeyConfig {
                id: "z-key".to_string(),
                key: "z-secret".to_string(),
                allowed_models: vec!["gpt-4o-mini".to_string()],
                policy_id: None,
                rate_limit: None,
            }),
        )
        .await
        .expect("apikey fixture should be written");
    fixture
        .harness()
        .put_json(
            "/aisix/apikeys/a-key",
            &json!(ApiKeyConfig {
                id: "a-key".to_string(),
                key: "a-secret".to_string(),
                allowed_models: vec!["gpt-4o-mini".to_string()],
                policy_id: None,
                rate_limit: Some(RateLimitConfig {
                    rpm: Some(10),
                    tpm: None,
                    concurrency: None,
                }),
            }),
        )
        .await
        .expect("apikey fixture should be written");
    let app = fixture.router();

    let list = app
        .clone()
        .oneshot(admin_get_request("/admin/apikeys"))
        .await
        .unwrap();
    assert_eq!(list.status(), StatusCode::OK);

    let list_body = list.into_body().collect().await.unwrap().to_bytes();
    let list_json: Value = serde_json::from_slice(&list_body).unwrap();
    assert_eq!(list_json.as_array().map(Vec::len), Some(2));
    assert_eq!(ids(&list_json), vec!["a-key", "z-key"]);
    assert_eq!(list_json[0]["key"], "a-secret");
    assert_eq!(list_json[1]["key"], "z-secret");

    let get = app
        .clone()
        .oneshot(admin_get_request("/admin/apikeys/a-key"))
        .await
        .unwrap();
    assert_eq!(get.status(), StatusCode::OK);

    let get_body = get.into_body().collect().await.unwrap().to_bytes();
    let get_json: Value = serde_json::from_slice(&get_body).unwrap();
    assert_eq!(get_json["id"], "a-key");
    assert_eq!(get_json["key"], "a-secret");

    let missing_before_delete = app
        .clone()
        .oneshot(admin_get_request("/admin/apikeys/missing-key"))
        .await
        .unwrap();
    assert_eq!(missing_before_delete.status(), StatusCode::NOT_FOUND);

    let delete = app
        .clone()
        .oneshot(admin_delete_request("/admin/apikeys/a-key"))
        .await
        .unwrap();
    assert_eq!(delete.status(), StatusCode::OK);

    let delete_body = delete.into_body().collect().await.unwrap().to_bytes();
    let delete_json: Value = serde_json::from_slice(&delete_body).unwrap();
    assert_eq!(delete_json["id"], "a-key");
    assert_eq!(delete_json["path"], "/aisix/apikeys/a-key");
    assert!(delete_json["revision"].as_i64().unwrap() > 0);

    let deleted = fixture
        .harness()
        .get_json("/aisix/apikeys/a-key")
        .await
        .expect("etcd read should succeed");
    assert!(deleted.is_none());

    let missing_after_delete = app
        .clone()
        .oneshot(admin_get_request("/admin/apikeys/a-key"))
        .await
        .unwrap();
    assert_eq!(missing_after_delete.status(), StatusCode::NOT_FOUND);

    let missing_delete = app
        .oneshot(admin_delete_request("/admin/apikeys/a-key"))
        .await
        .unwrap();
    assert_eq!(missing_delete.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn admin_policy_get_list_and_delete_routes_use_live_etcd() {
    let fixture = LiveEtcdTestApp::start().await;
    fixture
        .harness()
        .put_json(
            "/aisix/policies/z-policy",
            &json!(PolicyConfig {
                id: "z-policy".to_string(),
                rate_limit: RateLimitConfig {
                    rpm: Some(200),
                    tpm: None,
                    concurrency: None,
                },
            }),
        )
        .await
        .expect("policy fixture should be written");
    fixture
        .harness()
        .put_json(
            "/aisix/policies/a-policy",
            &json!(PolicyConfig {
                id: "a-policy".to_string(),
                rate_limit: RateLimitConfig {
                    rpm: Some(100),
                    tpm: Some(1000),
                    concurrency: Some(2),
                },
            }),
        )
        .await
        .expect("policy fixture should be written");
    let app = fixture.router();

    let list = app
        .clone()
        .oneshot(admin_get_request("/admin/policies"))
        .await
        .unwrap();
    assert_eq!(list.status(), StatusCode::OK);

    let list_body = list.into_body().collect().await.unwrap().to_bytes();
    let list_json: Value = serde_json::from_slice(&list_body).unwrap();
    assert_eq!(list_json.as_array().map(Vec::len), Some(2));
    assert_eq!(ids(&list_json), vec!["a-policy", "z-policy"]);

    let get = app
        .clone()
        .oneshot(admin_get_request("/admin/policies/a-policy"))
        .await
        .unwrap();
    assert_eq!(get.status(), StatusCode::OK);

    let get_body = get.into_body().collect().await.unwrap().to_bytes();
    let get_json: Value = serde_json::from_slice(&get_body).unwrap();
    assert_eq!(get_json["id"], "a-policy");
    assert_eq!(get_json["rate_limit"]["rpm"], 100);
    assert_eq!(get_json["rate_limit"]["tpm"], 1000);
    assert_eq!(get_json["rate_limit"]["concurrency"], 2);

    let missing_before_delete = app
        .clone()
        .oneshot(admin_get_request("/admin/policies/missing-policy"))
        .await
        .unwrap();
    assert_eq!(missing_before_delete.status(), StatusCode::NOT_FOUND);

    let delete = app
        .clone()
        .oneshot(admin_delete_request("/admin/policies/a-policy"))
        .await
        .unwrap();
    assert_eq!(delete.status(), StatusCode::OK);

    let delete_body = delete.into_body().collect().await.unwrap().to_bytes();
    let delete_json: Value = serde_json::from_slice(&delete_body).unwrap();
    assert_eq!(delete_json["id"], "a-policy");
    assert_eq!(delete_json["path"], "/aisix/policies/a-policy");
    assert!(delete_json["revision"].as_i64().unwrap() > 0);

    let deleted = fixture
        .harness()
        .get_json("/aisix/policies/a-policy")
        .await
        .expect("etcd read should succeed");
    assert!(deleted.is_none());

    let missing_after_delete = app
        .clone()
        .oneshot(admin_get_request("/admin/policies/a-policy"))
        .await
        .unwrap();
    assert_eq!(missing_after_delete.status(), StatusCode::NOT_FOUND);

    let missing_delete = app
        .oneshot(admin_delete_request("/admin/policies/a-policy"))
        .await
        .unwrap();
    assert_eq!(missing_delete.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn admin_update_reloads_key_rate_limit_without_poisoning_current_snapshot() {
    let upstream = spawn_openai_mock().await;

    with_env_var("OPENAI_API_KEY", Some("test-openai-key"), || async {
        let fixture = LiveEtcdTestApp::start_seeded(&upstream.base_url, Some(1)).await;
        let app = fixture.router();

        let first = app
            .clone()
            .oneshot(chat_request("live-token", "gpt-4o-mini"))
            .await
            .unwrap();
        assert_eq!(first.status(), StatusCode::OK);

        let update = app
            .clone()
            .oneshot(admin_put_request(
                "/admin/apikeys/vk_admin",
                json!(ApiKeyConfig {
                    id: "vk_admin".to_string(),
                    key: "live-token".to_string(),
                    allowed_models: vec!["gpt-4o-mini".to_string()],
                    policy_id: None,
                    rate_limit: Some(RateLimitConfig {
                        rpm: Some(2),
                        tpm: None,
                        concurrency: None,
                    }),
                }),
            ))
            .await
            .unwrap();
        assert_eq!(update.status(), StatusCode::OK);

        let second = poll_until_response(|| {
            let app = app.clone();
            async move {
                app.oneshot(chat_request("live-token", "gpt-4o-mini"))
                    .await
                    .unwrap()
            }
        })
        .await;
        assert_eq!(second.status(), StatusCode::OK);

        let invalid_update = app
            .clone()
            .oneshot(admin_put_request(
                "/admin/models/gpt-4o-mini",
                json!(ModelConfig {
                    id: "gpt-4o-mini".to_string(),
                    provider_id: "missing-provider".to_string(),
                    upstream_model: "gpt-4o-mini-2024-07-18".to_string(),
                    policy_id: None,
                    rate_limit: None,
                    cache: None,
                }),
            ))
            .await
            .unwrap();
        assert_eq!(invalid_update.status(), StatusCode::OK);

        let third = poll_until_status(
            || {
                let app = app.clone();
                async move {
                    app.oneshot(chat_request("live-token", "gpt-4o-mini"))
                        .await
                        .unwrap()
                }
            },
            StatusCode::TOO_MANY_REQUESTS,
        )
        .await;
        assert_eq!(third.status(), StatusCode::TOO_MANY_REQUESTS);

        let body = third.into_body().collect().await.unwrap().to_bytes();
        let json: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"]["message"], "request rate limit exceeded");
    })
    .await;
}

struct LiveEtcdTestApp {
    router: axum::Router,
    _state: aisix_server::app::ServerState,
    harness: support::etcd::EtcdHarness,
    seeded_revision: i64,
}

impl LiveEtcdTestApp {
    async fn start() -> Self {
        let harness = support::etcd::EtcdHarness::start()
            .await
            .expect("test etcd should start");
        Self::start_with_harness(harness, 0).await
    }

    async fn start_seeded(base_url: &str, rpm: Option<u64>) -> Self {
        Self::start_with_seed(Some((base_url, rpm))).await
    }

    async fn start_seeded_with_limited_key(base_url: &str) -> Self {
        let harness = support::etcd::EtcdHarness::start()
            .await
            .expect("test etcd should start");
        let _seeded_revision = seed_valid_runtime_config_in_etcd(&harness, base_url, None).await;
        let seeded_revision = seed_limited_runtime_key_in_etcd(&harness).await;

        Self::start_with_harness(harness, seeded_revision).await
    }

    async fn start_with_seed(seed: Option<(&str, Option<u64>)>) -> Self {
        let harness = support::etcd::EtcdHarness::start()
            .await
            .expect("test etcd should start");
        let mut seeded_revision = 0;
        if let Some((base_url, rpm)) = seed {
            seeded_revision = seed_valid_runtime_config_in_etcd(&harness, base_url, rpm).await;
        }
        Self::start_with_harness(harness, seeded_revision).await
    }

    async fn start_with_harness(harness: support::etcd::EtcdHarness, seeded_revision: i64) -> Self {
        let startup = test_startup_config(harness.config());
        let app = aisix_runtime::bootstrap::bootstrap(&startup)
            .await
            .expect("runtime bootstrap should succeed");
        let admin = aisix_server::admin::AdminState::from_startup_config(&startup)
            .await
            .expect("admin state should initialize")
            .expect("admin state should be enabled");
        let state = aisix_server::app::ServerState {
            app,
            providers: ProviderRegistry::default(),
            admin: Some(admin),
        };

        Self {
            router: aisix_server::app::build_router(state.clone()),
            _state: state,
            harness,
            seeded_revision,
        }
    }

    fn router(&self) -> axum::Router {
        self.router.clone()
    }

    fn harness(&self) -> &support::etcd::EtcdHarness {
        &self.harness
    }

    fn seeded_revision(&self) -> i64 {
        self.seeded_revision
    }
}

async fn seed_valid_runtime_config_in_etcd(
    harness: &support::etcd::EtcdHarness,
    base_url: &str,
    rpm: Option<u64>,
) -> i64 {
    harness
        .put_json(
            "/aisix/providers/openai",
            &json!(ProviderConfig {
                id: "openai".to_string(),
                kind: ProviderKind::OpenAi,
                base_url: base_url.to_string(),
                auth: ProviderAuth {
                    secret_ref: "env:OPENAI_API_KEY".to_string(),
                },
                policy_id: None,
                rate_limit: None,
                cache: None,
            }),
        )
        .await
        .expect("provider fixture should be written");
    harness
        .put_json(
            "/aisix/models/gpt-4o-mini",
            &json!(ModelConfig {
                id: "gpt-4o-mini".to_string(),
                provider_id: "openai".to_string(),
                upstream_model: "gpt-4o-mini-2024-07-18".to_string(),
                policy_id: None,
                rate_limit: None,
                cache: None,
            }),
        )
        .await
        .expect("model fixture should be written");
    harness
        .put_json(
            "/aisix/apikeys/vk_admin",
            &json!(ApiKeyConfig {
                id: "vk_admin".to_string(),
                key: "live-token".to_string(),
                allowed_models: vec!["gpt-4o-mini".to_string()],
                policy_id: None,
                rate_limit: rpm.map(|rpm| RateLimitConfig {
                    rpm: Some(rpm),
                    tpm: None,
                    concurrency: None,
                }),
            }),
        )
        .await
        .expect("apikey fixture should be written")
}

async fn seed_limited_runtime_key_in_etcd(harness: &support::etcd::EtcdHarness) -> i64 {
    harness
        .put_json(
            "/aisix/apikeys/vk_limited",
            &json!(ApiKeyConfig {
                id: "vk_limited".to_string(),
                key: "limited-token".to_string(),
                allowed_models: vec!["gpt-4o-mini".to_string()],
                policy_id: None,
                rate_limit: Some(RateLimitConfig {
                    rpm: Some(1),
                    tpm: None,
                    concurrency: None,
                }),
            }),
        )
        .await
        .expect("limited apikey fixture should be written")
}

fn test_startup_config(
    etcd: aisix_config::startup::EtcdConfig,
) -> aisix_config::startup::StartupConfig {
    aisix_config::startup::StartupConfig {
        server: aisix_config::startup::ServerConfig {
            listen: "127.0.0.1:0".to_string(),
            metrics_listen: "127.0.0.1:0".to_string(),
            request_body_limit_mb: 1,
        },
        etcd,
        redis: aisix_config::startup::RedisConfig {
            url: "redis://127.0.0.1:1".to_string(),
        },
        log: aisix_config::startup::LogConfig {
            level: "info".to_string(),
        },
        runtime: aisix_config::startup::RuntimeConfig { worker_threads: 1 },
        cache: aisix_config::startup::CacheConfig {
            default: aisix_config::startup::CacheDefaultMode::Disabled,
        },
        deployment: aisix_config::startup::DeploymentConfig {
            admin: aisix_config::startup::AdminConfig {
                enabled: true,
                admin_keys: vec![aisix_config::startup::AdminKey {
                    key: "test-admin-key".to_string(),
                }],
            },
        },
    }
}

fn admin_put_request(path: &str, body: Value) -> Request<Body> {
    admin_request_with_key("PUT", path, Some(body), Some("test-admin-key"))
}

fn admin_get_request(path: &str) -> Request<Body> {
    admin_request_with_key("GET", path, None, Some("test-admin-key"))
}

fn admin_delete_request(path: &str) -> Request<Body> {
    admin_request_with_key("DELETE", path, None, Some("test-admin-key"))
}

fn admin_request(method: &str, path: &str, body: Option<Value>) -> Request<Body> {
    admin_request_with_key(method, path, body, Some("test-admin-key"))
}

fn admin_request_with_key(
    method: &str,
    path: &str,
    body: Option<Value>,
    admin_key: Option<&str>,
) -> Request<Body> {
    let mut builder = Request::builder().method(method).uri(path);
    if let Some(admin_key) = admin_key {
        builder = builder.header("x-admin-key", admin_key);
    }
    if body.is_some() {
        builder = builder.header("content-type", "application/json");
    }

    builder
        .body(match body {
            Some(body) => Body::from(serde_json::to_vec(&body).unwrap()),
            None => Body::empty(),
        })
        .unwrap()
}

fn chat_request(token: &str, model: &str) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri("/v1/chat/completions")
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {token}"))
        .body(Body::from(
            serde_json::to_vec(&json!({
                "model": model,
                "messages": [{"role": "user", "content": "hello"}],
                "stream": false
            }))
            .unwrap(),
        ))
        .unwrap()
}

fn ids(list_json: &Value) -> Vec<&str> {
    list_json
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["id"].as_str().unwrap())
        .collect()
}

async fn with_env_var<F, Fut, T>(name: &str, value: Option<&str>, f: F) -> T
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = T>,
{
    let _restore = EnvVarGuard::set(name, value);

    f().await
}

async fn poll_until_response<F, Fut>(request: F) -> axum::response::Response
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = axum::response::Response>,
{
    poll_until_status(request, StatusCode::OK).await
}

async fn poll_until_status<F, Fut>(mut request: F, expected: StatusCode) -> axum::response::Response
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = axum::response::Response>,
{
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let response = request().await;
        if response.status() == expected {
            return response;
        }

        let status = response.status();
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let body = String::from_utf8_lossy(&body).into_owned();

        if Instant::now() >= deadline {
            panic!(
                "response did not reach expected status {expected} before timeout; last status: {status}; last body: {body}"
            );
        }

        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

struct EnvVarGuard {
    _lock: MutexGuard<'static, ()>,
    name: String,
    previous: Option<String>,
}

impl EnvVarGuard {
    fn set(name: &str, value: Option<&str>) -> Self {
        let lock = env_var_lock().lock().unwrap();
        let previous = std::env::var(name).ok();
        match value {
            Some(value) => std::env::set_var(name, value),
            None => std::env::remove_var(name),
        }

        Self {
            _lock: lock,
            name: name.to_string(),
            previous,
        }
    }
}

fn env_var_lock() -> &'static Mutex<()> {
    static ENV_VAR_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    ENV_VAR_LOCK.get_or_init(|| Mutex::new(()))
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        match self.previous.take() {
            Some(previous) => std::env::set_var(&self.name, previous),
            None => std::env::remove_var(&self.name),
        }
    }
}

struct MockUpstream {
    base_url: String,
}

async fn spawn_openai_mock() -> MockUpstream {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();

    tokio::spawn(async move {
        loop {
            let (stream, _) = listener.accept().await.unwrap();
            let io = TokioIo::new(stream);

            tokio::spawn(async move {
                let service = service_fn(move |request: hyper::Request<Incoming>| async move {
                    let _body = request.into_body().collect().await.unwrap().to_bytes();

                    Ok::<_, std::convert::Infallible>(
                        hyper::Response::builder()
                            .status(StatusCode::OK)
                            .header("content-type", "application/json")
                            .body(http_body_util::Full::new(Bytes::from(
                                serde_json::to_vec(&json!({
                                    "id": "chatcmpl-admin-test",
                                    "object": "chat.completion",
                                    "created": 1,
                                    "model": "gpt-4o-mini-2024-07-18",
                                    "choices": [{
                                        "index": 0,
                                        "message": {"role": "assistant", "content": "hi"},
                                        "finish_reason": "stop"
                                    }],
                                    "usage": {
                                        "prompt_tokens": 1,
                                        "completion_tokens": 1,
                                        "total_tokens": 2
                                    }
                                }))
                                .unwrap(),
                            )))
                            .unwrap(),
                    )
                });

                hyper::server::conn::http1::Builder::new()
                    .serve_connection(io, service)
                    .await
                    .unwrap();
            });
        }
    });

    MockUpstream {
        base_url: format!("http://{}", address),
    }
}
