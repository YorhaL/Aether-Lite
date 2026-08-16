use std::sync::Arc;

use crate::ai_serving::planner::candidate_preparation::resolve_candidate_mapped_model;
use crate::ai_serving::planner::candidate_resolution::EligibleLocalExecutionCandidate;
use crate::ai_serving::transport::auth::{
    resolve_local_auth_type_for_transport_format, resolve_local_gemini_auth,
    resolve_local_openai_bearer_auth, resolve_local_standard_auth,
};
use crate::ai_serving::GatewayProviderTransportSnapshot;
use crate::AppState;

use super::super::{LocalSameFormatProviderDecisionInput, LocalSameFormatProviderSpec};

pub(super) struct PreparedSameFormatProviderCandidate {
    pub(super) transport: Arc<GatewayProviderTransportSnapshot>,
    pub(super) auth_header: Option<String>,
    pub(super) auth_value: Option<String>,
    pub(super) provider_api_format: String,
    pub(super) mapped_model: String,
    pub(super) report_kind: &'static str,
    pub(super) upstream_is_stream: bool,
}

pub(super) async fn prepare_local_same_format_provider_candidate(
    state: &AppState,
    trace_id: &str,
    input: &LocalSameFormatProviderDecisionInput,
    eligible: &EligibleLocalExecutionCandidate,
    candidate_index: u32,
    candidate_id: &str,
    spec: LocalSameFormatProviderSpec,
) -> Option<PreparedSameFormatProviderCandidate> {
    let candidate = &eligible.candidate;
    let transport = Arc::clone(&eligible.transport);
    let provider_api_format = eligible.provider_api_format.as_str();
    let auth_type = resolve_local_auth_type_for_transport_format(&transport);
    let auth = match auth_type.as_str() {
        _ if provider_api_format.starts_with("openai:") => {
            resolve_local_openai_bearer_auth(&transport)
        }
        _ if provider_api_format.starts_with("gemini:") => resolve_local_gemini_auth(&transport),
        _ => resolve_local_standard_auth(&transport),
    };
    let (auth_header, auth_value) = match auth {
        Some((name, value)) => (Some(name), Some(value)),
        None => {
            super::super::payload::mark_skipped_local_same_format_provider_candidate(
                state,
                input,
                trace_id,
                candidate,
                candidate_index,
                candidate_id,
                "transport_auth_unavailable",
            )
            .await;
            return None;
        }
    };

    let mapped_model = match resolve_candidate_mapped_model(candidate) {
        Ok(mapped_model) => mapped_model,
        Err(skip_reason) => {
            super::super::payload::mark_skipped_local_same_format_provider_candidate(
                state,
                input,
                trace_id,
                candidate,
                candidate_index,
                candidate_id,
                skip_reason,
            )
            .await;
            return None;
        }
    };

    Some(PreparedSameFormatProviderCandidate {
        transport,
        auth_header,
        auth_value,
        provider_api_format: provider_api_format.to_string(),
        mapped_model,
        report_kind: spec.report_kind,
        upstream_is_stream: spec.require_streaming,
    })
}
