use std::collections::BTreeMap;
use std::sync::Arc;

use aether_contracts::ResolvedTransportProfile;
use serde_json::Value;

use crate::ai_serving::planner::redaction::{
    request_identity_response_encoding_when_redacted, resolve_provider_chat_pii_redaction,
};
use crate::ai_serving::transport::{
    apply_local_body_rules_with_request_headers, apply_local_header_rules_with_request_headers,
    build_passthrough_headers, build_transport_request_url_for_request_body,
    ensure_upstream_auth_header, resolve_transport_profile, TransportRequestUrlParams,
};
use crate::ai_serving::{CandidateFailureDiagnostic, GatewayProviderTransportSnapshot};
use crate::{AppState, GatewayError};

mod prepare;

use self::prepare::prepare_local_same_format_provider_candidate;
use super::payload::{
    mark_skipped_local_same_format_provider_candidate,
    mark_skipped_local_same_format_provider_candidate_with_failure_diagnostic,
};
use super::{
    LocalSameFormatProviderCandidateAttempt, LocalSameFormatProviderDecisionInput,
    LocalSameFormatProviderFamily, LocalSameFormatProviderSpec,
};

pub(crate) struct LocalSameFormatProviderCandidatePayloadParts {
    pub(super) transport: Arc<GatewayProviderTransportSnapshot>,
    pub(super) auth_header: Option<String>,
    pub(super) auth_value: Option<String>,
    pub(super) provider_api_format: String,
    pub(super) mapped_model: String,
    pub(super) report_kind: &'static str,
    pub(super) upstream_is_stream: bool,
    pub(super) upstream_url: String,
    pub(super) provider_request_headers: BTreeMap<String, String>,
    pub(super) provider_request_body: Value,
    pub(super) transport_profile: Option<ResolvedTransportProfile>,
    pub(super) request_redacted: bool,
}

pub(crate) async fn resolve_local_same_format_provider_candidate_payload_parts(
    state: &AppState,
    parts: &http::request::Parts,
    trace_id: &str,
    body_json: &Value,
    input: &LocalSameFormatProviderDecisionInput,
    attempt: &LocalSameFormatProviderCandidateAttempt,
    spec: LocalSameFormatProviderSpec,
) -> Result<Option<LocalSameFormatProviderCandidatePayloadParts>, GatewayError> {
    let candidate = &attempt.eligible.candidate;
    let Some(prepared) = prepare_local_same_format_provider_candidate(
        state,
        trace_id,
        input,
        &attempt.eligible,
        attempt.candidate_index,
        &attempt.candidate_id,
        spec,
    )
    .await
    else {
        return Ok(None);
    };

    let redaction = resolve_provider_chat_pii_redaction(
        state,
        parts,
        body_json,
        &input.auth_context,
        spec.api_format,
        &attempt.candidate_id,
    )
    .await?;
    let original_body = redaction.body_json.as_ref();
    let Some(mut provider_request_body) = original_body.as_object().cloned().map(Value::Object)
    else {
        mark_skipped_local_same_format_provider_candidate(
            state,
            input,
            trace_id,
            candidate,
            attempt.candidate_index,
            &attempt.candidate_id,
            "provider_request_body_missing",
        )
        .await;
        return Ok(None);
    };

    apply_mapped_model(
        &mut provider_request_body,
        spec.family,
        prepared.provider_api_format.as_str(),
        prepared.mapped_model.as_str(),
    );
    let effective_headers = input.effective_headers(&parts.headers);
    if !apply_local_body_rules_with_request_headers(
        &mut provider_request_body,
        prepared.transport.endpoint.body_rules.as_ref(),
        Some(original_body),
        Some(effective_headers),
    ) {
        mark_skipped_local_same_format_provider_candidate(
            state,
            input,
            trace_id,
            candidate,
            attempt.candidate_index,
            &attempt.candidate_id,
            "transport_body_rules_apply_failed",
        )
        .await;
        return Ok(None);
    }

    let upstream_url = build_transport_request_url_for_request_body(
        &prepared.transport,
        TransportRequestUrlParams {
            provider_api_format: prepared.provider_api_format.as_str(),
            mapped_model: Some(prepared.mapped_model.as_str()),
            upstream_is_stream: prepared.upstream_is_stream,
            request_query: parts.uri.query(),
            api_operation: spec.operation,
        },
        Some(&provider_request_body),
    );
    let Some(upstream_url) = upstream_url else {
        mark_skipped_local_same_format_provider_candidate_with_failure_diagnostic(
            state,
            input,
            trace_id,
            candidate,
            attempt.candidate_index,
            &attempt.candidate_id,
            "upstream_url_missing",
            CandidateFailureDiagnostic::upstream_url_missing(
                prepared.provider_api_format.as_str(),
                spec.api_format,
                "same_format_provider_url",
            ),
        )
        .await;
        return Ok(None);
    };

    let content_type = effective_headers
        .get(http::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok());
    let mut provider_request_headers =
        build_passthrough_headers(effective_headers, &BTreeMap::new(), content_type);
    if let (Some(name), Some(value)) = (
        prepared.auth_header.as_deref(),
        prepared.auth_value.as_deref(),
    ) {
        ensure_upstream_auth_header(&mut provider_request_headers, name, value);
    }
    if !apply_local_header_rules_with_request_headers(
        &mut provider_request_headers,
        prepared.transport.endpoint.header_rules.as_ref(),
        &[],
        &provider_request_body,
        Some(original_body),
        Some(effective_headers),
    ) {
        mark_skipped_local_same_format_provider_candidate_with_failure_diagnostic(
            state,
            input,
            trace_id,
            candidate,
            attempt.candidate_index,
            &attempt.candidate_id,
            "transport_header_rules_apply_failed",
            CandidateFailureDiagnostic::header_rules_apply_failed(
                prepared.provider_api_format.as_str(),
                spec.api_format,
                "same_format_provider_headers",
            ),
        )
        .await;
        return Ok(None);
    }
    request_identity_response_encoding_when_redacted(
        &mut provider_request_headers,
        redaction.redacted,
    );
    let transport_profile = resolve_transport_profile(&prepared.transport);

    Ok(Some(LocalSameFormatProviderCandidatePayloadParts {
        transport: prepared.transport,
        auth_header: prepared.auth_header,
        auth_value: prepared.auth_value,
        provider_api_format: prepared.provider_api_format,
        mapped_model: prepared.mapped_model,
        report_kind: prepared.report_kind,
        upstream_is_stream: prepared.upstream_is_stream,
        upstream_url,
        provider_request_headers,
        provider_request_body,
        transport_profile,
        request_redacted: redaction.redacted,
    }))
}

fn apply_mapped_model(
    body: &mut Value,
    family: LocalSameFormatProviderFamily,
    api_format: &str,
    mapped_model: &str,
) {
    let Some(object) = body.as_object_mut() else {
        return;
    };
    match family {
        LocalSameFormatProviderFamily::Standard => {
            object.insert("model".to_string(), Value::String(mapped_model.to_string()));
        }
        LocalSameFormatProviderFamily::Gemini
            if crate::ai_serving::api_format_alias_matches(api_format, "gemini:interactions") =>
        {
            let field = if object.contains_key("agent") {
                "agent"
            } else {
                "model"
            };
            object.insert(field.to_string(), Value::String(mapped_model.to_string()));
        }
        LocalSameFormatProviderFamily::Gemini => {}
    }
}
