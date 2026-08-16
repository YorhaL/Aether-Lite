use aether_ai_serving::{
    append_ai_ranking_metadata_to_object, build_ai_candidate_metadata_from_candidate,
};
use aether_scheduler_core::{SchedulerMinimalCandidateSelectionCandidate, SchedulerRankingOutcome};
use serde_json::{Map, Value};

use crate::ai_serving::planner::candidate_resolution::EligibleLocalExecutionCandidate;
use crate::ai_serving::transport::append_transport_diagnostics_to_value;
use crate::ai_serving::GatewayProviderTransportSnapshot;

pub(crate) struct LocalExecutionCandidateMetadataParts<'a> {
    pub(crate) eligible: &'a EligibleLocalExecutionCandidate,
    pub(crate) provider_api_format: &'a str,
    pub(crate) client_api_format: &'a str,
    pub(crate) extra_fields: Map<String, Value>,
}

pub(crate) fn append_ranking_metadata_to_object(
    object: &mut Map<String, Value>,
    ranking: &SchedulerRankingOutcome,
) {
    append_ai_ranking_metadata_to_object(object, ranking);
}

pub(crate) fn build_local_execution_candidate_metadata(
    parts: LocalExecutionCandidateMetadataParts<'_>,
) -> Value {
    build_local_execution_candidate_metadata_for_candidate(
        &parts.eligible.candidate,
        Some(parts.eligible.transport.as_ref()),
        parts.provider_api_format,
        parts.client_api_format,
        parts.extra_fields,
    )
}

pub(crate) fn build_local_execution_candidate_metadata_for_candidate(
    candidate: &SchedulerMinimalCandidateSelectionCandidate,
    transport: Option<&GatewayProviderTransportSnapshot>,
    provider_api_format: &str,
    client_api_format: &str,
    extra_fields: Map<String, Value>,
) -> Value {
    append_transport_diagnostics_to_value(
        build_ai_candidate_metadata_from_candidate(
            candidate,
            provider_api_format,
            client_api_format,
            extra_fields,
        ),
        transport,
        client_api_format,
        provider_api_format,
    )
}
