use aether_ai_serving::{
    ai_ranking_context, build_ai_rankable_candidate, run_ai_candidate_ranking,
    AiCandidateRankingPort, AiRankableCandidateParts, AiRankingContextConfig,
    AiRankingSchedulingMode,
};
use aether_routing_core::{ResolvedRoutingPolicy, RoutingSchedulingMode, RoutingSetPriorityMode};
use async_trait::async_trait;
use tracing::warn;

use crate::ai_serving::{GatewayAuthApiKeySnapshot, PlannerAppState};
use crate::clock::current_unix_ms;
use crate::scheduler::config::{
    read_scheduler_ordering_config, SchedulerOrderingConfig, SchedulerSchedulingMode,
};
use aether_scheduler_core::{
    matches_affinity_target, ClientSessionAffinity, SchedulerAffinityTarget,
    SchedulerMinimalCandidateSelectionCandidate, SchedulerPriorityMode, SchedulerRankableCandidate,
    SchedulerRankingContext, SchedulerRankingOutcome,
};

use super::candidate_affinity_cache::read_cached_scheduler_affinity_target;
use super::candidate_resolution::EligibleLocalExecutionCandidate;

struct GatewayLocalCandidateRankingPort<'a> {
    state: PlannerAppState<'a>,
    requested_model: Option<&'a str>,
    auth_snapshot: Option<&'a GatewayAuthApiKeySnapshot>,
    client_session_affinity: Option<&'a ClientSessionAffinity>,
    required_capabilities: Option<&'a serde_json::Value>,
    ordering_config: SchedulerOrderingConfig,
    routing_policy: Option<&'a ResolvedRoutingPolicy>,
}

#[async_trait]
impl AiCandidateRankingPort for GatewayLocalCandidateRankingPort<'_> {
    type Candidate = EligibleLocalExecutionCandidate;
    type AffinityTarget = SchedulerAffinityTarget;
    type Error = std::convert::Infallible;

    fn affinity_requested_model(&self, candidates: &[Self::Candidate]) -> Option<String> {
        self.requested_model
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .or_else(|| {
                candidates
                    .first()
                    .map(|candidate| candidate.candidate.global_model_name.clone())
            })
    }

    async fn read_cached_affinity_target(
        &self,
        normalized_client_api_format: &str,
        affinity_requested_model: Option<&str>,
    ) -> Result<Option<Self::AffinityTarget>, Self::Error> {
        Ok(read_cached_scheduler_affinity_target(
            self.state,
            self.auth_snapshot,
            self.client_session_affinity,
            normalized_client_api_format,
            affinity_requested_model,
            self.routing_policy,
        ))
    }

    fn cached_affinity_matches(
        &self,
        candidate: &Self::Candidate,
        target: &Self::AffinityTarget,
    ) -> bool {
        cached_affinity_matches_local_execution_scope(candidate, target)
    }

    async fn build_rankable_candidate(
        &self,
        candidate: &Self::Candidate,
        original_index: usize,
        normalized_client_api_format: &str,
        cached_affinity_match: bool,
    ) -> Result<SchedulerRankableCandidate, Self::Error> {
        let routing_overlaid_candidate =
            routing_overlaid_candidate(self.routing_policy, &candidate.candidate);
        Ok(build_ai_rankable_candidate(AiRankableCandidateParts {
            candidate: &routing_overlaid_candidate,
            original_index,
            required_capabilities: self.required_capabilities,
            cached_affinity_match,
        }))
    }

    fn ranking_context(&self) -> SchedulerRankingContext {
        ai_ranking_context(ai_ranking_context_config(self.ordering_config))
    }

    fn apply_ranking_outcome(
        &self,
        candidate: &mut Self::Candidate,
        outcome: SchedulerRankingOutcome,
    ) {
        candidate.ranking = Some(outcome);
    }
}

pub(crate) async fn rank_eligible_local_execution_candidates(
    state: PlannerAppState<'_>,
    candidates: Vec<EligibleLocalExecutionCandidate>,
    normalized_client_api_format: &str,
    requested_model: Option<&str>,
    auth_snapshot: Option<&GatewayAuthApiKeySnapshot>,
    client_session_affinity: Option<&ClientSessionAffinity>,
    required_capabilities: Option<&serde_json::Value>,
    routing_policy: Option<&ResolvedRoutingPolicy>,
) -> Vec<EligibleLocalExecutionCandidate> {
    let ordering_config = scheduler_ordering_config_for_routing_policy(state, routing_policy).await;
    let port = GatewayLocalCandidateRankingPort {
        state,
        requested_model,
        auth_snapshot,
        client_session_affinity,
        required_capabilities,
        ordering_config,
        routing_policy,
    };

    match run_ai_candidate_ranking(&port, candidates, normalized_client_api_format).await {
        Ok(candidates) => candidates,
        Err(error) => match error {},
    }
}

fn cached_affinity_matches_local_execution_scope(
    eligible: &EligibleLocalExecutionCandidate,
    target: &SchedulerAffinityTarget,
) -> bool {
    matches_affinity_target(&eligible.candidate, target)
}

fn ai_ranking_context_config(ordering_config: SchedulerOrderingConfig) -> AiRankingContextConfig {
    AiRankingContextConfig {
        priority_mode: ordering_config.priority_mode,
        scheduling_mode: ai_ranking_scheduling_mode(ordering_config.scheduling_mode),
        load_balance_seed: current_unix_ms(),
    }
}

fn ai_ranking_scheduling_mode(mode: SchedulerSchedulingMode) -> AiRankingSchedulingMode {
    match mode {
        SchedulerSchedulingMode::FixedOrder => AiRankingSchedulingMode::FixedOrder,
        SchedulerSchedulingMode::CacheAffinity => AiRankingSchedulingMode::CacheAffinity,
        SchedulerSchedulingMode::LoadBalance => AiRankingSchedulingMode::LoadBalance,
    }
}

pub(crate) async fn scheduler_ordering_config_for_routing_policy(
    state: PlannerAppState<'_>,
    routing_policy: Option<&ResolvedRoutingPolicy>,
) -> SchedulerOrderingConfig {
    match routing_policy {
        Some(policy) => scheduler_ordering_config_from_routing_policy(policy),
        None => read_scheduler_ordering_config_or_default(state).await,
    }
}

fn scheduler_ordering_config_from_routing_policy(
    policy: &ResolvedRoutingPolicy,
) -> SchedulerOrderingConfig {
    SchedulerOrderingConfig {
        priority_mode: match policy.priority_mode {
            RoutingSetPriorityMode::Provider => SchedulerPriorityMode::Provider,
            RoutingSetPriorityMode::GlobalKey => SchedulerPriorityMode::GlobalKey,
        },
        scheduling_mode: match policy.scheduling_mode {
            RoutingSchedulingMode::FixedOrder => SchedulerSchedulingMode::FixedOrder,
            RoutingSchedulingMode::CacheAffinity => SchedulerSchedulingMode::CacheAffinity,
            RoutingSchedulingMode::LoadBalance => SchedulerSchedulingMode::LoadBalance,
        },
    }
}

fn routing_overlaid_candidate(
    routing_policy: Option<&ResolvedRoutingPolicy>,
    candidate: &SchedulerMinimalCandidateSelectionCandidate,
) -> SchedulerMinimalCandidateSelectionCandidate {
    let Some(policy) = routing_policy else {
        return candidate.clone();
    };
    let mut overlaid = candidate.clone();
    overlaid.provider_priority = policy
        .ranking_overlay
        .provider_priority(candidate.provider_id.as_str(), candidate.provider_priority);
    let overlaid_key_priority = policy
        .ranking_overlay
        .key_priority_overrides
        .get(candidate.key_id.as_str());
    if let Some(overlaid_key_priority) = overlaid_key_priority.copied() {
        overlaid.key_internal_priority = overlaid_key_priority;
        overlaid.key_global_priority_for_format = Some(overlaid_key_priority);
    }
    overlaid
}

async fn read_scheduler_ordering_config_or_default(
    state: PlannerAppState<'_>,
) -> SchedulerOrderingConfig {
    match read_scheduler_ordering_config(state.app()).await {
        Ok(config) => config,
        Err(error) => {
            warn!(
                event_name = "planner_scheduler_ordering_config_load_failed",
                log_type = "event",
                error = ?error,
                "failed to load scheduler ordering config while ranking local execution candidates"
            );
            SchedulerOrderingConfig::default()
        }
    }
}
