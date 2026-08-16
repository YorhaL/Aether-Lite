use crate::ai_serving::{is_json_request, GatewayControlDecision};

pub(crate) use crate::ai_serving::{
    build_local_same_format_stream_attempt_source, build_local_same_format_stream_plan_and_reports,
    build_local_same_format_sync_attempt_source, build_local_same_format_sync_plan_and_reports,
    maybe_build_stream_decision_payload, maybe_build_stream_plan_payload,
    maybe_build_sync_decision_payload, maybe_build_sync_plan_payload,
};
pub(crate) use crate::ai_serving::{
    AiExecutionDecision, AiExecutionPlanPayload, AiStreamAttempt, AiSyncAttempt,
};
pub(crate) use aether_ai_formats::api::*;

pub(crate) fn parse_direct_request_body(
    parts: &http::request::Parts,
    body_bytes: &axum::body::Bytes,
) -> Option<(serde_json::Value, Option<String>)> {
    let is_json_request = is_json_request(&parts.headers);
    let body_bytes = if is_json_request {
        crate::ai_serving::decoded_request_body_bytes(&parts.headers, body_bytes.as_ref()).ok()?
    } else {
        std::borrow::Cow::Borrowed(body_bytes.as_ref())
    };
    aether_ai_formats::api::parse_direct_request_body(is_json_request, body_bytes.as_ref())
}

pub(crate) fn resolve_execution_runtime_stream_plan_kind(
    parts: &http::request::Parts,
    decision: &GatewayControlDecision,
) -> Option<&'static str> {
    let plan_kind =
        aether_ai_formats::api::resolve_execution_runtime_stream_plan_kind_with_client_surface(
            decision.route_class.as_deref(),
            decision.route_family.as_deref(),
            decision.route_kind.as_deref(),
            decision.client_surface,
            decision.request_auth_channel.as_deref(),
            &parts.method,
            parts.uri.path(),
        )?;
    crate::ai_serving::plan_kind_matches_api_operation(plan_kind, true, decision.api_operation)
        .then_some(plan_kind)
}

pub(crate) fn resolve_execution_runtime_sync_plan_kind(
    parts: &http::request::Parts,
    decision: &GatewayControlDecision,
) -> Option<&'static str> {
    let plan_kind =
        aether_ai_formats::api::resolve_execution_runtime_sync_plan_kind_with_client_surface(
            decision.route_class.as_deref(),
            decision.route_family.as_deref(),
            decision.route_kind.as_deref(),
            decision.client_surface,
            decision.request_auth_channel.as_deref(),
            &parts.method,
            parts.uri.path(),
        )?;
    crate::ai_serving::plan_kind_matches_api_operation(plan_kind, false, decision.api_operation)
        .then_some(plan_kind)
}

pub(crate) fn is_matching_stream_request(
    plan_kind: &str,
    parts: &http::request::Parts,
    body_json: &serde_json::Value,
    body_base64: Option<&str>,
) -> bool {
    crate::ai_serving::planner_is_matching_stream_request(plan_kind, parts, body_json, body_base64)
}

pub(crate) fn supports_sync_execution_decision_kind(plan_kind: &str) -> bool {
    aether_ai_formats::api::supports_sync_execution_decision_kind(plan_kind)
}

pub(crate) fn supports_stream_execution_decision_kind(plan_kind: &str) -> bool {
    aether_ai_formats::api::supports_stream_execution_decision_kind(plan_kind)
}

#[cfg(test)]
mod tests {
    use super::parse_direct_request_body;
    use axum::body::Bytes;
    use axum::http::{header, Request};

    #[test]
    fn parse_direct_request_body_reads_zstd_encoded_json_body() {
        let (parts, _) = Request::builder()
            .method("POST")
            .uri("/v1/responses")
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::CONTENT_ENCODING, "zstd")
            .body(())
            .expect("request should build")
            .into_parts();
        let encoded =
            zstd::stream::encode_all(br#"{"model":"gpt-5.4","stream":true}"#.as_slice(), 0)
                .expect("zstd body should encode");

        let (body_json, body_base64) =
            parse_direct_request_body(&parts, &Bytes::from(encoded)).expect("body should parse");

        assert_eq!(body_json["model"].as_str(), Some("gpt-5.4"));
        assert_eq!(body_json["stream"].as_bool(), Some(true));
        assert!(body_base64.is_none());
    }
}
