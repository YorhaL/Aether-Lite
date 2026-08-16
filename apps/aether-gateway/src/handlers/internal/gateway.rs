use super::{
    attach_execution_path_header, build_internal_control_error_response,
    build_internal_gateway_fallback_plan_payload, build_internal_gateway_header_map,
    build_internal_gateway_passthrough_payload, build_internal_gateway_proxy_public_response,
    build_internal_gateway_request_parts, build_internal_gateway_resolve_payload,
    build_internal_gateway_uri, build_management_token_payload,
};
use crate::ai_serving::api;
use crate::constants::{
    CONTROL_EXECUTED_HEADER, EXECUTION_PATH_EXECUTION_RUNTIME_STREAM,
    EXECUTION_PATH_EXECUTION_RUNTIME_SYNC,
};
use crate::control::GatewayControlDecision;
use crate::control::GatewayPublicRequestContext;
use crate::execution_runtime::{execute_execution_runtime_stream, execute_execution_runtime_sync};
use crate::handlers::shared::{
    InternalGatewayAuthContextRequest, InternalGatewayExecuteRequest, InternalGatewayResolveRequest,
};
use crate::{AppState, GatewayError};
use axum::body::{Body, Bytes};
use axum::http::{self, HeaderName, HeaderValue, Response};
use axum::response::IntoResponse;
use axum::Json;
use serde_json::json;

async fn apply_supplied_auth_context(
    state: &AppState,
    decision: &mut GatewayControlDecision,
    auth_context: Option<crate::control::GatewayControlAuthContext>,
) -> Result<bool, GatewayError> {
    let Some(auth_context) = auth_context else {
        return Ok(false);
    };
    let refreshed = crate::control::refresh_execution_runtime_auth_context(
        state,
        auth_context,
        decision.auth_endpoint_signature.as_deref(),
    )
    .await?;
    decision.local_auth_rejection = refreshed.local_rejection.clone();
    decision.auth_context = Some(refreshed);
    Ok(true)
}

pub(crate) async fn maybe_build_local_internal_proxy_response_impl(
    state: &AppState,
    request_context: &GatewayPublicRequestContext,
    remote_addr: &std::net::SocketAddr,
    request_body: Option<&Bytes>,
) -> Result<Option<Response<Body>>, GatewayError> {
    let Some(decision) = request_context.control_decision.as_ref() else {
        return Ok(None);
    };
    if decision.route_class.as_deref() != Some("internal_proxy") {
        return Ok(None);
    }
    if decision.route_family.as_deref() == Some("internal_gateway") {
        if !remote_addr.ip().is_loopback() {
            return Ok(Some(build_internal_control_error_response(
                http::StatusCode::FORBIDDEN,
                "loopback access only",
            )));
        }
        match decision.route_kind.as_deref() {
            Some("resolve") if request_context.request_path == "/api/internal/gateway/resolve" => {
                let Some(request_body) = request_body else {
                    return Ok(Some(build_internal_control_error_response(
                        http::StatusCode::BAD_REQUEST,
                        "invalid internal gateway resolve payload",
                    )));
                };
                let payload =
                    match serde_json::from_slice::<InternalGatewayResolveRequest>(request_body) {
                        Ok(payload) => payload,
                        Err(_) => {
                            return Ok(Some(build_internal_control_error_response(
                                http::StatusCode::BAD_REQUEST,
                                "invalid internal gateway resolve payload",
                            )));
                        }
                    };
                let headers = match build_internal_gateway_header_map(&payload.headers) {
                    Ok(headers) => headers,
                    Err(response) => return Ok(Some(response)),
                };
                let method = match http::Method::from_bytes(payload.method.as_bytes()) {
                    Ok(method) => method,
                    Err(_) => {
                        return Ok(Some(build_internal_control_error_response(
                            http::StatusCode::BAD_REQUEST,
                            "invalid internal gateway method",
                        )));
                    }
                };
                let uri = match build_internal_gateway_uri(
                    &payload.path,
                    payload.query_string.as_deref(),
                ) {
                    Ok(uri) => uri,
                    Err(response) => return Ok(Some(response)),
                };
                let resolved = crate::control::resolve_control_route(
                    state,
                    &method,
                    &uri,
                    &headers,
                    payload
                        .trace_id
                        .as_deref()
                        .unwrap_or(request_context.trace_id.as_str()),
                )
                .await?;
                let response_payload = resolved
                    .map(build_internal_gateway_resolve_payload)
                    .unwrap_or_else(|| build_internal_gateway_passthrough_payload(&uri));
                return Ok(Some(Json(response_payload).into_response()));
            }
            Some("auth_context")
                if request_context.request_path == "/api/internal/gateway/auth-context" =>
            {
                let Some(request_body) = request_body else {
                    return Ok(Some(build_internal_control_error_response(
                        http::StatusCode::BAD_REQUEST,
                        "invalid internal gateway auth-context payload",
                    )));
                };
                let payload =
                    match serde_json::from_slice::<InternalGatewayAuthContextRequest>(request_body)
                    {
                        Ok(payload) => payload,
                        Err(_) => {
                            return Ok(Some(build_internal_control_error_response(
                                http::StatusCode::BAD_REQUEST,
                                "invalid internal gateway auth-context payload",
                            )));
                        }
                    };
                let headers = match build_internal_gateway_header_map(&payload.headers) {
                    Ok(headers) => headers,
                    Err(response) => return Ok(Some(response)),
                };
                let uri = match build_internal_gateway_uri("/", payload.query_string.as_deref()) {
                    Ok(uri) => uri,
                    Err(response) => return Ok(Some(response)),
                };
                let mut synthetic_decision = GatewayControlDecision::synthetic(
                    "/",
                    Some("internal_proxy".to_string()),
                    Some("internal_gateway".to_string()),
                    Some("auth_context".to_string()),
                    Some(payload.auth_endpoint_signature),
                );
                synthetic_decision.public_query_string = uri.query().map(ToOwned::to_owned);
                let auth_context = crate::control::resolve_execution_runtime_auth_context(
                    state,
                    &synthetic_decision,
                    &headers,
                    &uri,
                    payload
                        .trace_id
                        .as_deref()
                        .unwrap_or(request_context.trace_id.as_str()),
                )
                .await?;
                return Ok(Some(
                    Json(json!({ "auth_context": auth_context })).into_response(),
                ));
            }
            Some("decision_sync")
                if request_context.request_path == "/api/internal/gateway/decision-sync" =>
            {
                let Some(request_body) = request_body else {
                    return Ok(Some(build_internal_control_error_response(
                        http::StatusCode::BAD_REQUEST,
                        "invalid internal gateway decision-sync payload",
                    )));
                };
                let payload =
                    match serde_json::from_slice::<InternalGatewayExecuteRequest>(request_body) {
                        Ok(payload) => payload,
                        Err(_) => {
                            return Ok(Some(build_internal_control_error_response(
                                http::StatusCode::BAD_REQUEST,
                                "invalid internal gateway decision-sync payload",
                            )));
                        }
                    };
                let parts = match build_internal_gateway_request_parts(
                    &payload.method,
                    &payload.path,
                    payload.query_string.as_deref(),
                    &payload.headers,
                ) {
                    Ok(parts) => parts,
                    Err(response) => return Ok(Some(response)),
                };
                let trace_id = payload
                    .trace_id
                    .as_deref()
                    .unwrap_or(request_context.trace_id.as_str())
                    .to_string();
                let body_is_empty = payload.body_base64.is_none()
                    && payload
                        .body_json
                        .as_object()
                        .map(|value| value.is_empty())
                        .unwrap_or(false);
                let Some(mut resolved) = crate::control::resolve_control_route(
                    state,
                    &parts.method,
                    &parts.uri,
                    &parts.headers,
                    trace_id.as_str(),
                )
                .await?
                else {
                    return Ok(Some(
                        Json(build_internal_gateway_fallback_plan_payload(None)).into_response(),
                    ));
                };
                let provided_auth_context =
                    apply_supplied_auth_context(state, &mut resolved, payload.auth_context).await?;
                let auth_context = resolved.auth_context.as_ref();
                if auth_context
                    .map(|value| !value.access_allowed)
                    .unwrap_or(true)
                {
                    let fallback_auth_context = if !provided_auth_context {
                        auth_context
                    } else {
                        None
                    };
                    return Ok(Some(
                        Json(build_internal_gateway_fallback_plan_payload(
                            fallback_auth_context,
                        ))
                        .into_response(),
                    ));
                }
                let Some(mut local_payload) = api::maybe_build_sync_decision_payload(
                    state,
                    &parts,
                    trace_id.as_str(),
                    &resolved,
                    &payload.body_json,
                    payload.body_base64.as_deref(),
                    body_is_empty,
                )
                .await?
                else {
                    let fallback_auth_context = if !provided_auth_context {
                        auth_context
                    } else {
                        None
                    };
                    return Ok(Some(
                        Json(build_internal_gateway_fallback_plan_payload(
                            fallback_auth_context,
                        ))
                        .into_response(),
                    ));
                };
                if provided_auth_context {
                    local_payload.auth_context = None;
                }
                return Ok(Some(Json(local_payload).into_response()));
            }
            Some("decision_stream")
                if request_context.request_path == "/api/internal/gateway/decision-stream" =>
            {
                let Some(request_body) = request_body else {
                    return Ok(Some(build_internal_control_error_response(
                        http::StatusCode::BAD_REQUEST,
                        "invalid internal gateway decision-stream payload",
                    )));
                };
                let payload =
                    match serde_json::from_slice::<InternalGatewayExecuteRequest>(request_body) {
                        Ok(payload) => payload,
                        Err(_) => {
                            return Ok(Some(build_internal_control_error_response(
                                http::StatusCode::BAD_REQUEST,
                                "invalid internal gateway decision-stream payload",
                            )));
                        }
                    };
                let parts = match build_internal_gateway_request_parts(
                    &payload.method,
                    &payload.path,
                    payload.query_string.as_deref(),
                    &payload.headers,
                ) {
                    Ok(parts) => parts,
                    Err(response) => return Ok(Some(response)),
                };
                let trace_id = payload
                    .trace_id
                    .as_deref()
                    .unwrap_or(request_context.trace_id.as_str())
                    .to_string();
                let body_is_empty = payload.body_base64.is_none()
                    && payload
                        .body_json
                        .as_object()
                        .map(|value| value.is_empty())
                        .unwrap_or(false);
                let Some(mut resolved) = crate::control::resolve_control_route(
                    state,
                    &parts.method,
                    &parts.uri,
                    &parts.headers,
                    trace_id.as_str(),
                )
                .await?
                else {
                    return Ok(Some(
                        Json(build_internal_gateway_fallback_plan_payload(None)).into_response(),
                    ));
                };
                let provided_auth_context =
                    apply_supplied_auth_context(state, &mut resolved, payload.auth_context).await?;
                let auth_context = resolved.auth_context.as_ref();
                if auth_context
                    .map(|value| !value.access_allowed)
                    .unwrap_or(true)
                {
                    let fallback_auth_context = if !provided_auth_context {
                        auth_context
                    } else {
                        None
                    };
                    return Ok(Some(
                        Json(build_internal_gateway_fallback_plan_payload(
                            fallback_auth_context,
                        ))
                        .into_response(),
                    ));
                }
                let Some(mut local_payload) = api::maybe_build_stream_decision_payload(
                    state,
                    &parts,
                    trace_id.as_str(),
                    &resolved,
                    &payload.body_json,
                    payload.body_base64.as_deref(),
                )
                .await?
                else {
                    let fallback_auth_context = if !provided_auth_context {
                        auth_context
                    } else {
                        None
                    };
                    return Ok(Some(
                        Json(build_internal_gateway_fallback_plan_payload(
                            fallback_auth_context,
                        ))
                        .into_response(),
                    ));
                };
                if provided_auth_context {
                    local_payload.auth_context = None;
                }
                return Ok(Some(Json(local_payload).into_response()));
            }
            Some("plan_sync")
                if request_context.request_path == "/api/internal/gateway/plan-sync" =>
            {
                let Some(request_body) = request_body else {
                    return Ok(Some(build_internal_control_error_response(
                        http::StatusCode::BAD_REQUEST,
                        "invalid internal gateway plan-sync payload",
                    )));
                };
                let payload =
                    match serde_json::from_slice::<InternalGatewayExecuteRequest>(request_body) {
                        Ok(payload) => payload,
                        Err(_) => {
                            return Ok(Some(build_internal_control_error_response(
                                http::StatusCode::BAD_REQUEST,
                                "invalid internal gateway plan-sync payload",
                            )));
                        }
                    };
                let parts = match build_internal_gateway_request_parts(
                    &payload.method,
                    &payload.path,
                    payload.query_string.as_deref(),
                    &payload.headers,
                ) {
                    Ok(parts) => parts,
                    Err(response) => return Ok(Some(response)),
                };
                let trace_id = payload
                    .trace_id
                    .as_deref()
                    .unwrap_or(request_context.trace_id.as_str())
                    .to_string();
                let body_is_empty = payload.body_base64.is_none()
                    && payload
                        .body_json
                        .as_object()
                        .map(|value| value.is_empty())
                        .unwrap_or(false);
                let Some(mut resolved) = crate::control::resolve_control_route(
                    state,
                    &parts.method,
                    &parts.uri,
                    &parts.headers,
                    trace_id.as_str(),
                )
                .await?
                else {
                    return Ok(Some(build_internal_gateway_proxy_public_response()));
                };
                let provided_auth_context =
                    apply_supplied_auth_context(state, &mut resolved, payload.auth_context).await?;
                if let Some(mut planned) = api::maybe_build_sync_plan_payload(
                    state,
                    &parts,
                    trace_id.as_str(),
                    &resolved,
                    &payload.body_json,
                    payload.body_base64.as_deref(),
                    body_is_empty,
                )
                .await?
                {
                    if provided_auth_context {
                        planned.auth_context = None;
                    }
                    return Ok(Some(Json(planned).into_response()));
                }
                return Ok(Some(build_internal_gateway_proxy_public_response()));
            }
            Some("plan_stream")
                if request_context.request_path == "/api/internal/gateway/plan-stream" =>
            {
                let Some(request_body) = request_body else {
                    return Ok(Some(build_internal_control_error_response(
                        http::StatusCode::BAD_REQUEST,
                        "invalid internal gateway plan-stream payload",
                    )));
                };
                let payload =
                    match serde_json::from_slice::<InternalGatewayExecuteRequest>(request_body) {
                        Ok(payload) => payload,
                        Err(_) => {
                            return Ok(Some(build_internal_control_error_response(
                                http::StatusCode::BAD_REQUEST,
                                "invalid internal gateway plan-stream payload",
                            )));
                        }
                    };
                let parts = match build_internal_gateway_request_parts(
                    &payload.method,
                    &payload.path,
                    payload.query_string.as_deref(),
                    &payload.headers,
                ) {
                    Ok(parts) => parts,
                    Err(response) => return Ok(Some(response)),
                };
                let trace_id = payload
                    .trace_id
                    .as_deref()
                    .unwrap_or(request_context.trace_id.as_str())
                    .to_string();
                let Some(mut resolved) = crate::control::resolve_control_route(
                    state,
                    &parts.method,
                    &parts.uri,
                    &parts.headers,
                    trace_id.as_str(),
                )
                .await?
                else {
                    return Ok(Some(build_internal_gateway_proxy_public_response()));
                };
                let provided_auth_context =
                    apply_supplied_auth_context(state, &mut resolved, payload.auth_context).await?;
                if let Some(mut planned) = api::maybe_build_stream_plan_payload(
                    state,
                    &parts,
                    trace_id.as_str(),
                    &resolved,
                    &payload.body_json,
                    payload.body_base64.as_deref(),
                )
                .await?
                {
                    if provided_auth_context {
                        planned.auth_context = None;
                    }
                    return Ok(Some(Json(planned).into_response()));
                }
                return Ok(Some(build_internal_gateway_proxy_public_response()));
            }
            Some("execute_sync")
                if request_context.request_path == "/api/internal/gateway/execute-sync" =>
            {
                let Some(request_body) = request_body else {
                    return Ok(Some(build_internal_control_error_response(
                        http::StatusCode::BAD_REQUEST,
                        "invalid internal gateway execute-sync payload",
                    )));
                };
                let payload =
                    match serde_json::from_slice::<InternalGatewayExecuteRequest>(request_body) {
                        Ok(payload) => payload,
                        Err(_) => {
                            return Ok(Some(build_internal_control_error_response(
                                http::StatusCode::BAD_REQUEST,
                                "invalid internal gateway execute-sync payload",
                            )));
                        }
                    };
                let parts = match build_internal_gateway_request_parts(
                    &payload.method,
                    &payload.path,
                    payload.query_string.as_deref(),
                    &payload.headers,
                ) {
                    Ok(parts) => parts,
                    Err(response) => return Ok(Some(response)),
                };
                let trace_id = payload
                    .trace_id
                    .as_deref()
                    .unwrap_or(request_context.trace_id.as_str())
                    .to_string();
                let body_is_empty = payload.body_base64.is_none()
                    && payload
                        .body_json
                        .as_object()
                        .map(|value| value.is_empty())
                        .unwrap_or(false);
                let Some(mut resolved) = crate::control::resolve_control_route(
                    state,
                    &parts.method,
                    &parts.uri,
                    &parts.headers,
                    trace_id.as_str(),
                )
                .await?
                else {
                    return Ok(None);
                };
                apply_supplied_auth_context(state, &mut resolved, payload.auth_context).await?;
                if let Some(plan_payload) = api::maybe_build_sync_plan_payload(
                    state,
                    &parts,
                    trace_id.as_str(),
                    &resolved,
                    &payload.body_json,
                    payload.body_base64.as_deref(),
                    body_is_empty,
                )
                .await?
                {
                    let plan_kind = plan_payload.plan_kind.unwrap_or_default();
                    if let Some(plan) = plan_payload.plan {
                        if !plan_kind.trim().is_empty() {
                            let executed_response = execute_execution_runtime_sync(
                                state,
                                parts.uri.path(),
                                plan,
                                trace_id.as_str(),
                                &resolved,
                                plan_kind.as_str(),
                                plan_payload.report_kind,
                                plan_payload.report_context,
                            )
                            .await?;
                            if let Some(executed_response) = executed_response {
                                return Ok(Some(attach_execution_path_header(
                                    executed_response,
                                    EXECUTION_PATH_EXECUTION_RUNTIME_SYNC,
                                )));
                            }
                        }
                    }
                }
                return Ok(Some(build_internal_gateway_proxy_public_response()));
            }
            Some("execute_stream")
                if request_context.request_path == "/api/internal/gateway/execute-stream" =>
            {
                let Some(request_body) = request_body else {
                    return Ok(Some(build_internal_control_error_response(
                        http::StatusCode::BAD_REQUEST,
                        "invalid internal gateway execute-stream payload",
                    )));
                };
                let payload =
                    match serde_json::from_slice::<InternalGatewayExecuteRequest>(request_body) {
                        Ok(payload) => payload,
                        Err(_) => {
                            return Ok(Some(build_internal_control_error_response(
                                http::StatusCode::BAD_REQUEST,
                                "invalid internal gateway execute-stream payload",
                            )));
                        }
                    };
                let parts = match build_internal_gateway_request_parts(
                    &payload.method,
                    &payload.path,
                    payload.query_string.as_deref(),
                    &payload.headers,
                ) {
                    Ok(parts) => parts,
                    Err(response) => return Ok(Some(response)),
                };
                let trace_id = payload
                    .trace_id
                    .as_deref()
                    .unwrap_or(request_context.trace_id.as_str())
                    .to_string();
                let Some(mut resolved) = crate::control::resolve_control_route(
                    state,
                    &parts.method,
                    &parts.uri,
                    &parts.headers,
                    trace_id.as_str(),
                )
                .await?
                else {
                    return Ok(None);
                };
                apply_supplied_auth_context(state, &mut resolved, payload.auth_context).await?;
                if let Some(plan_payload) = api::maybe_build_stream_plan_payload(
                    state,
                    &parts,
                    trace_id.as_str(),
                    &resolved,
                    &payload.body_json,
                    payload.body_base64.as_deref(),
                )
                .await?
                {
                    let plan_kind = plan_payload.plan_kind.unwrap_or_default();
                    if let Some(plan) = plan_payload.plan {
                        if !plan_kind.trim().is_empty() {
                            let executed_response = execute_execution_runtime_stream(
                                state,
                                plan,
                                trace_id.as_str(),
                                &resolved,
                                plan_kind.as_str(),
                                plan_payload.report_kind,
                                plan_payload.report_context,
                            )
                            .await?;
                            if let Some(executed_response) = executed_response {
                                return Ok(Some(attach_execution_path_header(
                                    executed_response,
                                    EXECUTION_PATH_EXECUTION_RUNTIME_STREAM,
                                )));
                            }
                        }
                    }
                }
                return Ok(Some(build_internal_gateway_proxy_public_response()));
            }
            Some("report_sync")
                if request_context.request_path == "/api/internal/gateway/report-sync" =>
            {
                let Some(request_body) = request_body else {
                    return Ok(Some(build_internal_control_error_response(
                        http::StatusCode::BAD_REQUEST,
                        "invalid internal gateway report-sync payload",
                    )));
                };
                let payload = match serde_json::from_slice::<crate::usage::GatewaySyncReportRequest>(
                    request_body,
                ) {
                    Ok(payload) => payload,
                    Err(_) => {
                        return Ok(Some(build_internal_control_error_response(
                            http::StatusCode::BAD_REQUEST,
                            "invalid internal gateway report-sync payload",
                        )));
                    }
                };
                crate::usage::submit_sync_report(state, payload).await?;
                return Ok(Some(Json(json!({ "ok": true })).into_response()));
            }
            Some("report_stream")
                if request_context.request_path == "/api/internal/gateway/report-stream" =>
            {
                let Some(request_body) = request_body else {
                    return Ok(Some(build_internal_control_error_response(
                        http::StatusCode::BAD_REQUEST,
                        "invalid internal gateway report-stream payload",
                    )));
                };
                let payload = match serde_json::from_slice::<crate::usage::GatewayStreamReportRequest>(
                    request_body,
                ) {
                    Ok(payload) => payload,
                    Err(_) => {
                        return Ok(Some(build_internal_control_error_response(
                            http::StatusCode::BAD_REQUEST,
                            "invalid internal gateway report-stream payload",
                        )));
                    }
                };
                crate::usage::submit_stream_report(state, payload).await?;
                return Ok(Some(Json(json!({ "ok": true })).into_response()));
            }
            _ => {
                return Ok(Some(build_internal_control_error_response(
                    http::StatusCode::NOT_FOUND,
                    "unsupported internal gateway route",
                )));
            }
        }
    }
    Ok(None)
}
