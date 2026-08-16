use std::sync::Arc;

use aether_ai_serving::{
    run_ai_candidate_resolution, AiCandidateResolutionPort, AiCandidateResolutionRequest,
};
use aether_routing_core::ResolvedRoutingPolicy;
use async_trait::async_trait;
use std::convert::Infallible;
use std::time::Instant;
use tracing::warn;

use aether_scheduler_core::{
    ClientSessionAffinity, SchedulerMinimalCandidateSelectionCandidate, SchedulerRankingOutcome,
};

use crate::ai_serving::{
    candidate_common_transport_skip_reason, candidate_transport_pair_skip_reason,
    CandidateTransportPolicyFacts, GatewayAuthApiKeySnapshot, GatewayProviderTransportSnapshot,
    PlannerAppState,
};
use crate::orchestration::LocalExecutionCandidateMetadata;
use crate::stage_metrics::observe_gateway_stage_ms;

use super::candidate_ranking::rank_eligible_local_execution_candidates;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct EligibleLocalExecutionCandidate {
    pub(crate) candidate: SchedulerMinimalCandidateSelectionCandidate,
    pub(crate) transport: Arc<GatewayProviderTransportSnapshot>,
    pub(crate) provider_api_format: String,
    pub(crate) orchestration: LocalExecutionCandidateMetadata,
    pub(crate) ranking: Option<SchedulerRankingOutcome>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SkippedLocalExecutionCandidate {
    pub(crate) candidate: SchedulerMinimalCandidateSelectionCandidate,
    pub(crate) skip_reason: &'static str,
    pub(crate) transport: Option<Arc<GatewayProviderTransportSnapshot>>,
    pub(crate) ranking: Option<SchedulerRankingOutcome>,
    pub(crate) extra_data: Option<serde_json::Value>,
}

impl SkippedLocalExecutionCandidate {
    pub(crate) fn transport_ref(&self) -> Option<&GatewayProviderTransportSnapshot> {
        self.transport.as_deref()
    }
}

struct GatewayLocalCandidateResolutionPort<'a> {
    state: PlannerAppState<'a>,
    requested_model: Option<&'a str>,
    auth_snapshot: Option<&'a GatewayAuthApiKeySnapshot>,
    client_session_affinity: Option<&'a ClientSessionAffinity>,
    required_capabilities: Option<&'a serde_json::Value>,
    routing_policy: Option<&'a ResolvedRoutingPolicy>,
}

#[async_trait]
impl AiCandidateResolutionPort for GatewayLocalCandidateResolutionPort<'_> {
    type Candidate = SchedulerMinimalCandidateSelectionCandidate;
    type Transport = Arc<GatewayProviderTransportSnapshot>;
    type Eligible = EligibleLocalExecutionCandidate;
    type Skipped = SkippedLocalExecutionCandidate;
    type Error = Infallible;

    async fn read_candidate_transport(
        &self,
        candidate: &Self::Candidate,
    ) -> Result<Option<Self::Transport>, Self::Error> {
        let started_at = Instant::now();
        let transport = read_candidate_transport_snapshot_arc(self.state, candidate).await;
        let elapsed_ms = started_at.elapsed().as_millis() as u64;
        observe_gateway_stage_ms("candidate_transport_snapshot", elapsed_ms);
        observe_gateway_stage_ms("candidate_resolution_transport_read", elapsed_ms);
        Ok(transport)
    }

    fn build_missing_transport_skipped_candidate(
        &self,
        candidate: Self::Candidate,
    ) -> Self::Skipped {
        SkippedLocalExecutionCandidate {
            candidate,
            skip_reason: "transport_snapshot_missing",
            transport: None,
            ranking: None,
            extra_data: None,
        }
    }

    fn candidate_common_skip_reason(
        &self,
        candidate: &Self::Candidate,
        transport: &Self::Transport,
        requested_model: Option<&str>,
    ) -> Option<&'static str> {
        if let Some(skip_reason) =
            routing_policy_candidate_skip_reason(self.routing_policy, candidate, transport)
        {
            return Some(skip_reason);
        }
        candidate_common_transport_skip_reason(
            transport,
            candidate_transport_policy_facts(candidate),
            requested_model,
        )
    }

    fn candidate_transport_pair_skip_reason(
        &self,
        candidate: &Self::Candidate,
        transport: &Self::Transport,
        normalized_client_api_format: &str,
        requested_model: &str,
    ) -> Option<&'static str> {
        let _ = (candidate, requested_model);
        candidate_transport_pair_skip_reason(transport, normalized_client_api_format)
    }

    fn build_skipped_candidate(
        &self,
        candidate: Self::Candidate,
        transport: Self::Transport,
        skip_reason: &'static str,
    ) -> Self::Skipped {
        SkippedLocalExecutionCandidate {
            candidate,
            skip_reason,
            transport: Some(transport),
            ranking: None,
            extra_data: None,
        }
    }

    fn build_eligible_candidate(
        &self,
        candidate: Self::Candidate,
        transport: Self::Transport,
    ) -> Self::Eligible {
        let provider_api_format = transport.endpoint.api_format.trim().to_ascii_lowercase();
        EligibleLocalExecutionCandidate {
            candidate,
            transport,
            provider_api_format,
            orchestration: LocalExecutionCandidateMetadata::default(),
            ranking: None,
        }
    }

    async fn rank_eligible_candidates(
        &self,
        candidates: Vec<Self::Eligible>,
        normalized_client_api_format: &str,
    ) -> Result<Vec<Self::Eligible>, Self::Error> {
        let started_at = Instant::now();
        let ranked = rank_eligible_local_execution_candidates(
            self.state,
            candidates,
            normalized_client_api_format,
            self.requested_model,
            self.auth_snapshot,
            self.client_session_affinity,
            self.required_capabilities,
            self.routing_policy,
        )
        .await;
        observe_gateway_stage_ms(
            "candidate_resolution_rank",
            started_at.elapsed().as_millis() as u64,
        );
        Ok(ranked)
    }
}

pub(crate) async fn resolve_and_rank_local_execution_candidates(
    state: PlannerAppState<'_>,
    candidates: Vec<SchedulerMinimalCandidateSelectionCandidate>,
    client_api_format: &str,
    requested_model: &str,
    auth_snapshot: Option<&GatewayAuthApiKeySnapshot>,
    client_session_affinity: Option<&ClientSessionAffinity>,
    required_capabilities: Option<&serde_json::Value>,
    routing_policy: Option<&ResolvedRoutingPolicy>,
) -> (
    Vec<EligibleLocalExecutionCandidate>,
    Vec<SkippedLocalExecutionCandidate>,
) {
    let requested_model = requested_model.trim();
    resolve_and_rank_local_execution_candidates_with_optional_model(
        state,
        candidates,
        client_api_format,
        Some(requested_model),
        auth_snapshot,
        client_session_affinity,
        required_capabilities,
        routing_policy,
    )
    .await
}

pub(crate) async fn resolve_and_rank_local_execution_candidates_with_optional_model(
    state: PlannerAppState<'_>,
    candidates: Vec<SchedulerMinimalCandidateSelectionCandidate>,
    client_api_format: &str,
    requested_model: Option<&str>,
    auth_snapshot: Option<&GatewayAuthApiKeySnapshot>,
    client_session_affinity: Option<&ClientSessionAffinity>,
    required_capabilities: Option<&serde_json::Value>,
    routing_policy: Option<&ResolvedRoutingPolicy>,
) -> (
    Vec<EligibleLocalExecutionCandidate>,
    Vec<SkippedLocalExecutionCandidate>,
) {
    resolve_and_rank_local_execution_candidates_inner(
        state,
        candidates,
        client_api_format,
        requested_model,
        auth_snapshot,
        client_session_affinity,
        required_capabilities,
        routing_policy,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn resolve_and_rank_local_execution_candidates_inner(
    state: PlannerAppState<'_>,
    candidates: Vec<SchedulerMinimalCandidateSelectionCandidate>,
    client_api_format: &str,
    requested_model: Option<&str>,
    auth_snapshot: Option<&GatewayAuthApiKeySnapshot>,
    client_session_affinity: Option<&ClientSessionAffinity>,
    required_capabilities: Option<&serde_json::Value>,
    routing_policy: Option<&ResolvedRoutingPolicy>,
) -> (
    Vec<EligibleLocalExecutionCandidate>,
    Vec<SkippedLocalExecutionCandidate>,
) {
    let candidates = candidates
        .into_iter()
        .filter(|candidate| {
            matches!(
                candidate.key_auth_type.trim().to_ascii_lowercase().as_str(),
                "api_key" | "bearer"
            )
        })
        .filter(|candidate| {
            crate::ai_serving::api_format_alias_matches(
                &candidate.endpoint_api_format,
                client_api_format,
            )
        })
        .collect();
    let scheduler_affinity_epoch = state.app().scheduler_affinity_epoch();
    let port = GatewayLocalCandidateResolutionPort {
        state,
        requested_model,
        auth_snapshot,
        client_session_affinity,
        required_capabilities,
        routing_policy,
    };

    let request = AiCandidateResolutionRequest::standard(client_api_format, requested_model);

    let started_at = Instant::now();
    match run_ai_candidate_resolution(&port, candidates, request).await {
        Ok(mut outcome) => {
            observe_gateway_stage_ms(
                "candidate_resolution_core",
                started_at.elapsed().as_millis() as u64,
            );
            for candidate in &mut outcome.eligible_candidates {
                candidate.orchestration.scheduler_affinity_epoch = Some(scheduler_affinity_epoch);
            }
            (outcome.eligible_candidates, outcome.skipped_candidates)
        }
        Err(error) => match error {},
    }
}

fn candidate_transport_policy_facts(
    candidate: &SchedulerMinimalCandidateSelectionCandidate,
) -> CandidateTransportPolicyFacts<'_> {
    CandidateTransportPolicyFacts {
        endpoint_api_format: candidate.endpoint_api_format.as_str(),
        global_model_name: candidate.global_model_name.as_str(),
        selected_provider_model_name: candidate.selected_provider_model_name.as_str(),
        mapping_matched_model: candidate.mapping_matched_model.as_deref(),
    }
}

fn routing_policy_candidate_skip_reason(
    routing_policy: Option<&ResolvedRoutingPolicy>,
    candidate: &SchedulerMinimalCandidateSelectionCandidate,
    transport: &GatewayProviderTransportSnapshot,
) -> Option<&'static str> {
    let policy = routing_policy?;
    if !policy
        .ranking_overlay
        .provider_allowed(candidate.provider_id.as_str())
    {
        return Some("routing_profile_disallowed_provider");
    }
    if !policy
        .ranking_overlay
        .key_allowed(candidate.key_id.as_str())
    {
        return Some("routing_profile_disallowed_key");
    }
    None
}

pub(crate) async fn read_candidate_transport_snapshot(
    state: PlannerAppState<'_>,
    candidate: &SchedulerMinimalCandidateSelectionCandidate,
) -> Option<GatewayProviderTransportSnapshot> {
    read_candidate_transport_snapshot_arc(state, candidate)
        .await
        .map(|transport| (*transport).clone())
}

pub(crate) async fn read_candidate_transport_snapshot_arc(
    state: PlannerAppState<'_>,
    candidate: &SchedulerMinimalCandidateSelectionCandidate,
) -> Option<Arc<GatewayProviderTransportSnapshot>> {
    match state
        .read_provider_transport_snapshot_arc(
            &candidate.provider_id,
            &candidate.endpoint_id,
            &candidate.key_id,
        )
        .await
    {
        Ok(Some(transport)) => Some(transport),
        Ok(None) => None,
        Err(error) => {
            warn!(
                event_name = "candidate_resolution_transport_load_failed",
                log_type = "event",
                provider_id = %candidate.provider_id,
                endpoint_id = %candidate.endpoint_id,
                key_id = %candidate.key_id,
                error = ?error,
                "failed to load provider transport while evaluating local candidate eligibility"
            );
            None
        }
    }
}
