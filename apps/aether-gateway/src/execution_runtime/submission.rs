use axum::body::Body;
use axum::http::Response;
use base64::Engine as _;

use crate::ai_serving::api::core_error_background_report_kind;
use crate::api::response::build_client_response_from_parts;
use crate::control::GatewayControlDecision;
use crate::usage::{spawn_sync_report, GatewaySyncReportRequest};
use crate::{AppState, GatewayError};

pub(crate) fn resolve_core_error_background_report_kind(report_kind: &str) -> Option<String> {
    core_error_background_report_kind(report_kind).map(ToOwned::to_owned)
}

pub(crate) fn resolve_local_sync_error_status_code(
    status_code: u16,
    body_json: &serde_json::Value,
) -> u16 {
    if (400..600).contains(&status_code) {
        return status_code;
    }

    let object = body_json.as_object();
    let error = object
        .and_then(|value| value.get("error"))
        .and_then(serde_json::Value::as_object);
    for key in ["status", "code"] {
        for source in [error, object].into_iter().flatten() {
            let Some(value) = source.get(key) else {
                continue;
            };
            let parsed = value
                .as_u64()
                .and_then(|value| u16::try_from(value).ok())
                .or_else(|| value.as_str().and_then(|value| value.parse::<u16>().ok()));
            if parsed.is_some_and(|value| (400..600).contains(&value)) {
                return parsed.unwrap_or(400);
            }
        }
    }
    400
}

pub(crate) fn strip_utf8_bom_and_ws(mut body: &[u8]) -> &[u8] {
    loop {
        body = body
            .iter()
            .position(|byte| !byte.is_ascii_whitespace())
            .map_or(&[], |index| &body[index..]);
        if body.starts_with(&[0xEF, 0xBB, 0xBF]) {
            body = &body[3..];
        } else {
            return body;
        }
    }
}

pub(crate) fn has_nested_error(value: &serde_json::Value) -> bool {
    let Some(object) = value.as_object() else {
        return false;
    };
    object.get("error").is_some_and(|error| !error.is_null())
        || object
            .get("type")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|value| value == "error")
}

pub(crate) async fn submit_local_core_error_or_sync_finalize(
    state: &AppState,
    trace_id: &str,
    decision: &GatewayControlDecision,
    payload: GatewaySyncReportRequest,
) -> Result<Response<Body>, GatewayError> {
    let body_bytes = if let Some(body_base64) = payload.body_base64.as_deref() {
        base64::engine::general_purpose::STANDARD
            .decode(body_base64)
            .map_err(|error| GatewayError::Internal(error.to_string()))?
    } else if let Some(body_json) = payload.body_json.as_ref() {
        serde_json::to_vec(body_json).map_err(|error| GatewayError::Internal(error.to_string()))?
    } else {
        Vec::new()
    };

    let mut headers = payload.headers.clone();
    headers.remove("content-length");
    headers.insert("content-length".to_string(), body_bytes.len().to_string());
    let response = build_client_response_from_parts(
        payload.status_code,
        &headers,
        Body::from(body_bytes),
        trace_id,
        Some(decision),
    )?;

    if !payload.report_kind.trim().is_empty() {
        spawn_sync_report(state.clone(), payload);
    }
    Ok(response)
}
