use aether_dispatch_core::{DispatchCandidateRef, DispatchRankFacts, KeyRef, ProviderEndpointRef};

use crate::ai_serving::EligibleLocalExecutionCandidate;

pub(crate) fn dispatch_ref_for_local_candidate(
    eligible: &EligibleLocalExecutionCandidate,
) -> DispatchCandidateRef {
    let rank = DispatchRankFacts {
        provider_priority: eligible.candidate.provider_priority,
        key_priority: Some(eligible.candidate.key_internal_priority),
        ranking_reason: eligible
            .ranking
            .as_ref()
            .and_then(|ranking| ranking.promoted_by.map(str::to_string)),
    };

    DispatchCandidateRef::SingleKey {
        key: key_ref_for_candidate(eligible),
        rank,
    }
}

pub(crate) fn key_ref_for_candidate(eligible: &EligibleLocalExecutionCandidate) -> KeyRef {
    KeyRef {
        provider_id: eligible.candidate.provider_id.clone(),
        endpoint_id: eligible.candidate.endpoint_id.clone(),
        key_id: eligible.candidate.key_id.clone(),
        model_id: eligible.candidate.model_id.clone(),
        selected_provider_model_name: eligible.candidate.selected_provider_model_name.clone(),
        api_format: eligible.candidate.endpoint_api_format.clone(),
    }
}

pub(crate) fn provider_endpoint_ref_for_candidate(
    eligible: &EligibleLocalExecutionCandidate,
) -> ProviderEndpointRef {
    ProviderEndpointRef {
        provider_id: eligible.candidate.provider_id.clone(),
        endpoint_id: eligible.candidate.endpoint_id.clone(),
        model_id: eligible.candidate.model_id.clone(),
        selected_provider_model_name: eligible.candidate.selected_provider_model_name.clone(),
        api_format: eligible.candidate.endpoint_api_format.clone(),
    }
}
