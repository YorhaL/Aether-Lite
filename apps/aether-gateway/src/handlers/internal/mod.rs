use axum::{
    body::Body,
    http,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;

pub(crate) fn build_internal_control_error_response(
    status: http::StatusCode,
    message: impl Into<String>,
) -> Response<Body> {
    (status, Json(json!({ "detail": message.into() }))).into_response()
}

mod gateway_helpers;
use self::gateway_helpers::*;
pub(crate) use self::gateway_helpers::{
    build_management_token_payload, resolve_local_proxy_execution_path,
};
mod gateway;
pub(crate) use self::gateway::maybe_build_local_internal_proxy_response_impl;
