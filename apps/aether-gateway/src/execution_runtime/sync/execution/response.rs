use std::collections::BTreeMap;

use aether_contracts::ExecutionPlan;
use axum::body::Body;
use axum::http::header::HeaderValue;
use axum::http::Response;
use serde_json::json;

use crate::api::response::{
    build_client_response_from_parts, build_client_response_from_parts_with_mutator,
};
use crate::async_task::VideoTaskService;
use crate::control::GatewayControlDecision;
use crate::video_tasks::{build_local_sync_finalize_read_response, LocalVideoTaskSnapshot};
pub(crate) use crate::video_tasks::{
    resolve_local_sync_error_background_report_kind,
    resolve_local_sync_success_background_report_kind,
};
use crate::{usage::GatewaySyncReportRequest, GatewayError};

pub(crate) enum LocalVideoSyncSuccessBuild {
    Handled(LocalVideoSyncSuccessOutcome),
    NotHandled(GatewaySyncReportRequest),
}

pub(crate) struct LocalVideoSyncSuccessOutcome {
    pub(crate) response: Response<Body>,
    pub(crate) report_payload: GatewaySyncReportRequest,
    pub(crate) original_report_context: Option<serde_json::Value>,
    pub(crate) local_task_snapshot: LocalVideoTaskSnapshot,
}

fn cloned_report_context_object(
    payload: &GatewaySyncReportRequest,
) -> serde_json::Map<String, serde_json::Value> {
    payload
        .report_context
        .as_ref()
        .and_then(serde_json::Value::as_object)
        .cloned()
        .unwrap_or_default()
}

fn build_local_video_success_response(
    trace_id: &str,
    decision: &GatewayControlDecision,
    body_json: &serde_json::Value,
) -> Result<Response<Body>, GatewayError> {
    let body_bytes =
        serde_json::to_vec(body_json).map_err(|err| GatewayError::Internal(err.to_string()))?;
    let mut headers = BTreeMap::new();
    headers.insert("content-type".to_string(), "application/json".to_string());
    headers.insert("content-length".to_string(), body_bytes.len().to_string());
    build_client_response_from_parts(
        http::StatusCode::OK.as_u16(),
        &headers,
        Body::from(body_bytes),
        trace_id,
        Some(decision),
    )
}

pub(crate) fn maybe_build_local_video_success_outcome(
    trace_id: &str,
    decision: &GatewayControlDecision,
    mut payload: GatewaySyncReportRequest,
    video_tasks: &VideoTaskService,
    plan: &ExecutionPlan,
) -> Result<LocalVideoSyncSuccessBuild, GatewayError> {
    if payload.status_code >= 400 {
        return Ok(LocalVideoSyncSuccessBuild::NotHandled(payload));
    }

    let mut report_context = cloned_report_context_object(&payload);
    let prepared_plan = {
        let provider_body = match payload
            .body_json
            .as_ref()
            .and_then(serde_json::Value::as_object)
        {
            Some(value) => value,
            None => return Ok(LocalVideoSyncSuccessBuild::NotHandled(payload)),
        };
        video_tasks.prepare_sync_success(
            payload.report_kind.as_str(),
            provider_body,
            &report_context,
            plan,
        )
    };
    let Some(plan) = prepared_plan else {
        return Ok(LocalVideoSyncSuccessBuild::NotHandled(payload));
    };
    plan.apply_to_report_context(&mut report_context);
    let client_body_json = plan.client_body_json();

    let response = build_local_video_success_response(trace_id, decision, &client_body_json)?;
    let original_report_context = payload.report_context.take();
    payload.report_kind = plan.success_report_kind().to_string();
    payload.report_context = Some(serde_json::Value::Object(report_context));
    payload.client_body_json = Some(client_body_json);

    Ok(LocalVideoSyncSuccessBuild::Handled(
        LocalVideoSyncSuccessOutcome {
            response,
            report_payload: payload,
            original_report_context,
            local_task_snapshot: plan.to_snapshot(),
        },
    ))
}

pub(crate) fn maybe_build_local_sync_finalize_response(
    trace_id: &str,
    decision: &GatewayControlDecision,
    payload: &GatewaySyncReportRequest,
) -> Result<Option<Response<Body>>, GatewayError> {
    let Some(read_response) = build_local_sync_finalize_read_response(
        payload.report_kind.as_str(),
        payload.status_code,
        payload.report_context.as_ref(),
    ) else {
        return Ok(None);
    };

    let body_bytes = serde_json::to_vec(&read_response.body_json)
        .map_err(|err| GatewayError::Internal(err.to_string()))?;
    let mut headers = BTreeMap::new();
    headers.insert("content-type".to_string(), "application/json".to_string());
    headers.insert("content-length".to_string(), body_bytes.len().to_string());

    Ok(Some(build_client_response_from_parts(
        read_response.status_code,
        &headers,
        Body::from(body_bytes),
        trace_id,
        Some(decision),
    )?))
}

pub(crate) fn maybe_build_local_video_error_response(
    trace_id: &str,
    decision: &GatewayControlDecision,
    payload: &GatewaySyncReportRequest,
) -> Result<Option<Response<Body>>, GatewayError> {
    if resolve_local_sync_error_background_report_kind(payload.report_kind.as_str()).is_none() {
        return Ok(None);
    }

    if payload.status_code < 400 {
        return Ok(None);
    }

    let empty_body = json!({});
    let response_body = payload.body_json.as_ref().unwrap_or(&empty_body);
    let body_bytes =
        serde_json::to_vec(response_body).map_err(|err| GatewayError::Internal(err.to_string()))?;
    let body_len = body_bytes.len().to_string();

    Ok(Some(build_client_response_from_parts_with_mutator(
        payload.status_code,
        &payload.headers,
        Body::from(body_bytes),
        trace_id,
        Some(decision),
        |headers| {
            headers.remove(http::header::CONTENT_ENCODING);
            headers.remove(http::header::CONTENT_LENGTH);
            headers.insert(
                http::header::CONTENT_TYPE,
                HeaderValue::from_static("application/json"),
            );
            headers.insert(
                http::header::CONTENT_LENGTH,
                HeaderValue::from_str(body_len.as_str())
                    .map_err(|err| GatewayError::Internal(err.to_string()))?,
            );
            Ok(())
        },
    )?))
}

#[cfg(test)]
mod tests {
    use super::*;

    use axum::body::to_bytes;
    use serde_json::json;

    #[tokio::test]
    async fn local_video_error_response_rewrites_headers_without_mutating_payload() {
        let decision = GatewayControlDecision::synthetic(
            "/v1/videos",
            Some("ai_public".to_string()),
            Some("openai".to_string()),
            Some("video".to_string()),
            Some("openai:video".to_string()),
        )
        .with_execution_runtime_candidate(true);
        let payload = GatewaySyncReportRequest {
            trace_id: "trace-payload".to_string(),
            report_kind: "openai_video_create_sync_finalize".to_string(),
            report_context: Some(json!({
                "request_id": "req_123",
            })),
            status_code: http::StatusCode::BAD_GATEWAY.as_u16(),
            headers: BTreeMap::from([
                ("content-encoding".to_string(), "gzip".to_string()),
                ("content-length".to_string(), "999".to_string()),
                ("x-upstream-id".to_string(), "video-123".to_string()),
            ]),
            body_json: Some(json!({
                "error": {
                    "type": "video_backend_error",
                    "message": "backend failed",
                }
            })),
            client_body_json: None,
            body_base64: None,
            telemetry: None,
        };

        let response =
            maybe_build_local_video_error_response("trace-response", &decision, &payload)
                .expect("video error response should build")
                .expect("video error response should match local video error kinds");

        assert_eq!(response.status(), http::StatusCode::BAD_GATEWAY);
        assert_eq!(
            response
                .headers()
                .get(http::header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok()),
            Some("application/json")
        );
        assert_eq!(response.headers().get(http::header::CONTENT_ENCODING), None);
        assert_eq!(
            response
                .headers()
                .get("x-upstream-id")
                .and_then(|value| value.to_str().ok()),
            Some("video-123")
        );
        assert_eq!(
            payload.headers.get("content-encoding").map(String::as_str),
            Some("gzip")
        );
        assert_eq!(
            payload.headers.get("content-length").map(String::as_str),
            Some("999")
        );

        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body should read");
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&body).expect("response body should parse"),
            payload
                .body_json
                .clone()
                .expect("payload body should exist")
        );
    }
}
