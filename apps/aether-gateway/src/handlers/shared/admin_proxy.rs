use crate::audit::attach_admin_audit_event;
use crate::control::GatewayPublicRequestContext;
use axum::{
    body::Body,
    http,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;

pub(crate) fn build_admin_route_not_found_response(
    request_context: &GatewayPublicRequestContext,
) -> Response<Body> {
    let decision = request_context.control_decision.as_ref();
    (
        http::StatusCode::NOT_FOUND,
        Json(json!({
            "detail": "Route not found",
            "route_family": decision.and_then(|value| value.route_family.as_deref()),
            "route_kind": decision.and_then(|value| value.route_kind.as_deref()),
            "request_path": request_context.request_path,
        })),
    )
        .into_response()
}

pub(crate) fn build_admin_proxy_auth_required_response(
    request_context: &GatewayPublicRequestContext,
) -> Response<Body> {
    let decision = request_context.control_decision.as_ref();
    (
        http::StatusCode::UNAUTHORIZED,
        Json(json!({
            "detail": "admin authentication required",
            "route_family": decision.and_then(|value| value.route_family.as_deref()),
            "route_kind": decision.and_then(|value| value.route_kind.as_deref()),
            "request_path": request_context.request_path,
        })),
    )
        .into_response()
}

pub(crate) fn attach_admin_audit_response(
    mut response: Response<Body>,
    event_name: &'static str,
    action: &'static str,
    target_type: &'static str,
    target_id: &str,
) -> Response<Body> {
    attach_admin_audit_event(&mut response, event_name, action, target_type, target_id);
    response
}
