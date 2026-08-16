use super::build_internal_control_error_response;
use crate::constants::{
    CONTROL_ACTION_HEADER, CONTROL_ACTION_PROXY_PUBLIC, CONTROL_EXECUTED_HEADER,
    CONTROL_EXECUTION_RUNTIME_CANDIDATE_KEY, EXECUTION_PATH_CONTROL_EXECUTE_STREAM,
    EXECUTION_PATH_CONTROL_EXECUTE_SYNC, EXECUTION_PATH_DISTRIBUTED_OVERLOADED,
    EXECUTION_PATH_EXECUTION_RUNTIME_STREAM, EXECUTION_PATH_EXECUTION_RUNTIME_SYNC,
    EXECUTION_PATH_HEADER, EXECUTION_PATH_LOCAL_AUTH_DENIED, EXECUTION_PATH_LOCAL_OVERLOADED,
    EXECUTION_PATH_LOCAL_RATE_LIMITED, EXECUTION_PATH_PUBLIC_PROXY_PASSTHROUGH,
};
use crate::control::{management_token_permission_mode_and_summary, GatewayControlDecision};
use crate::handlers::shared::unix_secs_to_rfc3339;
use crate::{AppState, GatewayError};
use aether_data::repository::management_tokens::{
    StoredManagementToken, StoredManagementTokenUserSummary,
};
use axum::body::Body;
use axum::http::{self, header::HeaderName, header::HeaderValue, Response};
use axum::response::IntoResponse;
use axum::Json;
use serde_json::{json, Value};
use std::collections::BTreeMap;

fn insert_execution_runtime_candidate_fields(
    payload: &mut serde_json::Map<String, serde_json::Value>,
    value: bool,
) {
    payload.insert(
        CONTROL_EXECUTION_RUNTIME_CANDIDATE_KEY.to_string(),
        json!(value),
    );
}

pub(crate) fn build_internal_gateway_passthrough_payload(uri: &http::Uri) -> serde_json::Value {
    let mut payload = serde_json::Map::new();
    payload.insert("action".to_string(), json!("proxy_public"));
    payload.insert("route_class".to_string(), json!("passthrough"));
    payload.insert("public_path".to_string(), json!(uri.path()));
    insert_execution_runtime_candidate_fields(&mut payload, false);
    if let Some(query) = uri.query().filter(|value| !value.is_empty()) {
        payload.insert("public_query_string".to_string(), json!(query));
    }
    serde_json::Value::Object(payload)
}

pub(crate) fn build_internal_gateway_resolve_payload(
    decision: GatewayControlDecision,
) -> serde_json::Value {
    let mut payload = serde_json::Map::new();
    payload.insert("action".to_string(), json!("proxy_public"));
    payload.insert("route_class".to_string(), json!(decision.route_class));
    payload.insert("public_path".to_string(), json!(decision.public_path));
    insert_execution_runtime_candidate_fields(
        &mut payload,
        decision.is_execution_runtime_candidate(),
    );
    if let Some(query) = decision.public_query_string {
        payload.insert("public_query_string".to_string(), json!(query));
    }
    if let Some(route_family) = decision.route_family {
        payload.insert("route_family".to_string(), json!(route_family));
    }
    if let Some(route_kind) = decision.route_kind {
        payload.insert("route_kind".to_string(), json!(route_kind));
    }
    if let Some(request_auth_channel) = decision.request_auth_channel {
        payload.insert(
            "request_auth_channel".to_string(),
            json!(request_auth_channel),
        );
    }
    if let Some(signature) = decision.auth_endpoint_signature {
        payload.insert("auth_endpoint_signature".to_string(), json!(signature));
    }
    if let Some(auth_context) = decision.auth_context {
        payload.insert(
            "auth_context".to_string(),
            serde_json::to_value(auth_context).unwrap_or(serde_json::Value::Null),
        );
    }
    serde_json::Value::Object(payload)
}

pub(crate) fn build_internal_gateway_fallback_plan_payload(
    auth_context: Option<&crate::control::GatewayControlAuthContext>,
) -> serde_json::Value {
    let mut payload = serde_json::Map::new();
    payload.insert("action".to_string(), json!("fallback_plan"));
    if let Some(auth_context) = auth_context {
        payload.insert(
            "auth_context".to_string(),
            serde_json::to_value(auth_context).unwrap_or(serde_json::Value::Null),
        );
    }
    serde_json::Value::Object(payload)
}

pub(crate) fn build_internal_gateway_proxy_public_response() -> Response<Body> {
    (
        http::StatusCode::CONFLICT,
        [(CONTROL_ACTION_HEADER, CONTROL_ACTION_PROXY_PUBLIC)],
        Json(json!({ "action": CONTROL_ACTION_PROXY_PUBLIC })),
    )
        .into_response()
}

pub(crate) fn attach_execution_path_header(
    mut response: Response<Body>,
    execution_path: &'static str,
) -> Response<Body> {
    response.headers_mut().insert(
        HeaderName::from_static(EXECUTION_PATH_HEADER),
        HeaderValue::from_static(execution_path),
    );
    response
}

pub(crate) fn resolve_local_proxy_execution_path(
    response: &Response<Body>,
    default_execution_path: &'static str,
) -> &'static str {
    match response
        .headers()
        .get(EXECUTION_PATH_HEADER)
        .and_then(|value| value.to_str().ok())
    {
        Some(EXECUTION_PATH_EXECUTION_RUNTIME_SYNC) => EXECUTION_PATH_EXECUTION_RUNTIME_SYNC,
        Some(EXECUTION_PATH_EXECUTION_RUNTIME_STREAM) => EXECUTION_PATH_EXECUTION_RUNTIME_STREAM,
        Some(EXECUTION_PATH_CONTROL_EXECUTE_SYNC) => EXECUTION_PATH_CONTROL_EXECUTE_SYNC,
        Some(EXECUTION_PATH_CONTROL_EXECUTE_STREAM) => EXECUTION_PATH_CONTROL_EXECUTE_STREAM,
        Some(EXECUTION_PATH_LOCAL_AUTH_DENIED) => EXECUTION_PATH_LOCAL_AUTH_DENIED,
        Some(EXECUTION_PATH_LOCAL_RATE_LIMITED) => EXECUTION_PATH_LOCAL_RATE_LIMITED,
        Some(EXECUTION_PATH_LOCAL_OVERLOADED) => EXECUTION_PATH_LOCAL_OVERLOADED,
        Some(EXECUTION_PATH_DISTRIBUTED_OVERLOADED) => EXECUTION_PATH_DISTRIBUTED_OVERLOADED,
        Some(EXECUTION_PATH_PUBLIC_PROXY_PASSTHROUGH) => EXECUTION_PATH_PUBLIC_PROXY_PASSTHROUGH,
        _ => default_execution_path,
    }
}

pub(crate) fn build_internal_gateway_header_map(
    headers: &BTreeMap<String, String>,
) -> Result<http::HeaderMap, Response<Body>> {
    let mut mapped = http::HeaderMap::new();
    for (name, value) in headers {
        let header_name = match HeaderName::from_bytes(name.as_bytes()) {
            Ok(name) => name,
            Err(_) => {
                return Err(build_internal_control_error_response(
                    http::StatusCode::BAD_REQUEST,
                    "invalid internal gateway header",
                ));
            }
        };
        let header_value = match HeaderValue::from_str(value) {
            Ok(value) => value,
            Err(_) => {
                return Err(build_internal_control_error_response(
                    http::StatusCode::BAD_REQUEST,
                    "invalid internal gateway header",
                ));
            }
        };
        mapped.append(header_name, header_value);
    }
    Ok(mapped)
}

pub(crate) fn build_internal_gateway_request_parts(
    method: &str,
    path: &str,
    query_string: Option<&str>,
    headers: &BTreeMap<String, String>,
) -> Result<http::request::Parts, Response<Body>> {
    let mapped_headers = build_internal_gateway_header_map(headers)?;
    let method = match http::Method::from_bytes(method.as_bytes()) {
        Ok(method) => method,
        Err(_) => {
            return Err(build_internal_control_error_response(
                http::StatusCode::BAD_REQUEST,
                "invalid internal gateway method",
            ));
        }
    };
    let uri = build_internal_gateway_uri(path, query_string)?;
    let request = match http::Request::builder().method(method).uri(uri).body(()) {
        Ok(request) => request,
        Err(_) => {
            return Err(build_internal_control_error_response(
                http::StatusCode::BAD_REQUEST,
                "invalid internal gateway request",
            ));
        }
    };
    let (mut parts, _) = request.into_parts();
    parts.headers = mapped_headers;
    Ok(parts)
}

pub(crate) fn build_internal_gateway_uri(
    path: &str,
    query_string: Option<&str>,
) -> Result<http::Uri, Response<Body>> {
    let normalized_path = if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{path}")
    };
    let uri_text = if let Some(query) = query_string.filter(|value| !value.is_empty()) {
        format!("{normalized_path}?{query}")
    } else {
        normalized_path
    };
    uri_text.parse::<http::Uri>().map_err(|_| {
        build_internal_control_error_response(
            http::StatusCode::BAD_REQUEST,
            "invalid internal gateway uri",
        )
    })
}

fn build_management_token_user_payload(
    user: &StoredManagementTokenUserSummary,
) -> serde_json::Value {
    json!({
        "id": user.id,
        "email": user.email,
        "username": user.username,
        "role": user.role,
    })
}

pub(crate) fn build_management_token_payload(
    token: &StoredManagementToken,
    user: Option<&StoredManagementTokenUserSummary>,
) -> serde_json::Value {
    let (permission_mode, permission_summary) =
        management_token_permission_mode_and_summary(token.permissions.as_ref());
    let mut payload = json!({
        "id": token.id,
        "user_id": token.user_id,
        "name": token.name,
        "description": token.description,
        "token_display": token.token_display(),
        "allowed_ips": token.allowed_ips,
        "permissions": token.permissions,
        "permission_mode": permission_mode,
        "permission_summary": permission_summary,
        "expires_at": token.expires_at_unix_secs.and_then(unix_secs_to_rfc3339),
        "last_used_at": token.last_used_at_unix_secs.and_then(unix_secs_to_rfc3339),
        "last_used_ip": token.last_used_ip,
        "usage_count": token.usage_count,
        "is_active": token.is_active,
        "created_at": token.created_at_unix_ms.and_then(unix_secs_to_rfc3339),
        "updated_at": token.updated_at_unix_secs.and_then(unix_secs_to_rfc3339),
    });
    if let Some(user) = user {
        payload["user"] = build_management_token_user_payload(user);
    }
    payload
}
