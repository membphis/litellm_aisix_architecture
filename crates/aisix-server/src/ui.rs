use axum::{
    body::Body,
    http::{header, HeaderValue, Response, StatusCode},
    response::Html,
    Json,
};

use crate::openapi;

const ADMIN_UI_HTML: &str = include_str!("../ui/index.html");
const ADMIN_UI_APP: &str = include_str!("../ui/app.mjs");

pub async fn admin_ui_index() -> Html<&'static str> {
    Html(ADMIN_UI_HTML)
}

pub async fn admin_ui_app_js() -> Response<Body> {
    let mut response = Response::new(Body::from(ADMIN_UI_APP));
    *response.status_mut() = StatusCode::OK;
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/javascript; charset=utf-8"),
    );
    response
}

pub async fn admin_openapi_json() -> Json<serde_json::Value> {
    Json(openapi::admin_openapi().clone())
}

pub async fn admin_openapi_json_response() -> Response<Body> {
    let mut response = Response::new(Body::from(openapi::admin_openapi().to_string()));
    *response.status_mut() = StatusCode::OK;
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json; charset=utf-8"),
    );
    response
}
