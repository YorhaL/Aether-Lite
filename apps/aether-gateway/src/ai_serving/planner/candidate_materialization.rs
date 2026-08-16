use aether_ai_serving::{
    ai_candidate_extra_data_with_ranking, run_ai_available_candidate_persistence,
    run_ai_candidate_materialization, run_ai_skipped_candidate_persistence,
    AiAvailableCandidatePersistencePort, AiCandidateMaterializationOutcome,
    AiCandidateMaterializationPort, AiCandidatePreselectionOutcome,
    AiSkippedCandidatePersistencePort,
};
use aether_dispatch_core::{DispatchSequence, DispatchSequenceItem};
use aether_routing_core::{
    rank_vector_for_candidate, CandidateKind, ResolvedRoutingPolicy, RoutingCandidateFacts,
    RoutingCandidateTrace, RoutingDecisionTrace,
};
use aether_scheduler_core::{
    ClientSessionAffinity, SchedulerMinimalCandidateSelectionCandidate, SchedulerRankingOutcome,
};
use async_trait::async_trait;
use serde_json::Value;
use std::collections::{BTreeSet, VecDeque};
use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::Instant;
use tracing::warn;
use uuid::Uuid;

use crate::ai_serving::planner::candidate_affinity_cache::remember_scheduler_affinity_for_candidate_with_routing_policy_at_epoch;
use crate::ai_serving::planner::candidate_ranking::scheduler_ordering_config_for_routing_policy;
use crate::ai_serving::planner::candidate_resolution::{
    resolve_and_rank_local_execution_candidates_with_optional_model,
    EligibleLocalExecutionCandidate, SkippedLocalExecutionCandidate,
};
use crate::ai_serving::planner::candidate_source::{
    LocalCandidatePreselectionKeyMode, LocalCandidatePreselectionPageCursor,
};
use crate::ai_serving::planner::materialization_policy::LocalCandidatePersistencePolicy;
use crate::ai_serving::planner::runtime_miss::record_local_runtime_candidate_skip_reason;
use crate::ai_serving::planner::CandidateFailureDiagnostic;
use crate::ai_serving::{GatewayAuthApiKeySnapshot, PlannerAppState};
use crate::cache::{
    candidate_page_cache_stale_ttl, candidate_page_cache_ttl_from_env,
    record_candidate_page_resolve_cache_follower_wait, record_candidate_page_resolve_cache_hit,
    record_candidate_page_resolve_cache_load, record_candidate_page_resolve_cache_miss,
    CacheLoadObserver, CandidateResolvedPageCacheKey, CandidateResolvedPageSnapshot,
};
use crate::clock::current_unix_ms;
use crate::dispatch::refs::dispatch_ref_for_local_candidate;
use crate::orchestration::{local_attempt_slot_count, ExecutionAttemptIdentity};
use crate::scheduler::candidate::is_auth_api_key_concurrency_limit_skip_reason;
use crate::scheduler::config::SchedulerSchedulingMode;
use crate::stage_metrics::observe_gateway_stage_ms;
use crate::{AppState, GatewayError};

const AUTH_API_KEY_CONCURRENCY_WAIT_BUDGET: Duration = Duration::from_millis(100);
const AUTH_API_KEY_CONCURRENCY_RETRY_DELAY: Duration = Duration::from_millis(10);

#[derive(Debug, Clone)]
pub(crate) struct LocalExecutionCandidateAttempt {
    pub(crate) eligible: EligibleLocalExecutionCandidate,
    pub(crate) candidate_index: u32,
    pub(crate) retry_index: u32,
    pub(crate) candidate_id: String,
}

pub(crate) struct LocalExecutionCandidateAttemptSource<'a> {
    items: VecDeque<LocalExecutionCandidateAttemptSourceItem<'a>>,
    skipped_provider_ids: BTreeSet<String>,
    skipped_endpoint_ids: BTreeSet<String>,
    skipped_credential_ids: BTreeSet<String>,
}

type DecorateSkippedCandidateFn<'a> = Arc<
    dyn Fn(SkippedLocalExecutionCandidate) -> SkippedLocalExecutionCandidate + Send + Sync + 'a,
>;

#[async_trait]
pub(crate) trait LocalExecutionAttemptSource<T>: Send {
    async fn next_execution_attempt(&mut self) -> Result<Option<T>, GatewayError>;

    async fn drain_execution_attempts(&mut self) -> Result<Vec<T>, GatewayError>;

    async fn skip_credential(&mut self, key_id: &str) -> Result<(), GatewayError>;

    async fn skip_endpoint(&mut self, endpoint_id: &str) -> Result<(), GatewayError>;

    async fn skip_provider(&mut self, provider_id: &str) -> Result<(), GatewayError>;
}

enum LocalExecutionCandidateAttemptSourceItem<'a> {
    Static {
        attempts: DispatchSequence<LocalExecutionCandidateAttempt>,
    },
    RequestedModelPage {
        cursor: Box<RequestedModelAttemptPageCursor<'a>>,
    },
}

impl<'a> LocalExecutionCandidateAttemptSource<'a> {
    pub(crate) fn from_static_attempts_for_image_bridge(
        attempts: Vec<LocalExecutionCandidateAttempt>,
    ) -> Self {
        let mut items = VecDeque::new();
        if !attempts.is_empty() {
            items.push_back(LocalExecutionCandidateAttemptSourceItem::Static {
                attempts: dispatch_sequence_from_attempts(attempts),
            });
        }
        Self {
            items,
            skipped_provider_ids: BTreeSet::new(),
            skipped_endpoint_ids: BTreeSet::new(),
            skipped_credential_ids: BTreeSet::new(),
        }
    }

    pub(crate) async fn next_attempt(
        &mut self,
    ) -> Result<Option<LocalExecutionCandidateAttempt>, GatewayError> {
        loop {
            let Some(front) = self.items.front_mut() else {
                return Ok(None);
            };
            match front {
                LocalExecutionCandidateAttemptSourceItem::Static { attempts } => {
                    if dispatch_sequence_candidate_is_skipped(
                        attempts,
                        &self.skipped_provider_ids,
                        &self.skipped_endpoint_ids,
                        &self.skipped_credential_ids,
                    ) {
                        self.items.pop_front();
                        continue;
                    }
                    if let Some(attempt) = next_attempt_from_dispatch_sequence(attempts) {
                        if dispatch_sequence_exhausted(attempts) {
                            self.items.pop_front();
                        }
                        return Ok(Some(attempt));
                    }
                    self.items.pop_front();
                }
                LocalExecutionCandidateAttemptSourceItem::RequestedModelPage { cursor } => {
                    for provider_id in &self.skipped_provider_ids {
                        cursor.skip_provider(provider_id);
                    }
                    for endpoint_id in &self.skipped_endpoint_ids {
                        cursor.skip_endpoint(endpoint_id);
                    }
                    for key_id in &self.skipped_credential_ids {
                        cursor.skip_credential(key_id);
                    }
                    let Some(attempt) = cursor.next_attempt().await? else {
                        self.items.pop_front();
                        continue;
                    };
                    return Ok(Some(attempt));
                }
            }
        }
    }

    pub(crate) fn drain_static_attempts(&mut self) -> Vec<LocalExecutionCandidateAttempt> {
        self.items.clear();
        Vec::new()
    }

    pub(crate) fn skip_provider(&mut self, provider_id: &str) {
        let provider_id = provider_id.trim();
        if provider_id.is_empty() {
            return;
        }
        self.skipped_provider_ids.insert(provider_id.to_string());
        for item in &mut self.items {
            if let LocalExecutionCandidateAttemptSourceItem::RequestedModelPage { cursor } = item {
                cursor.skip_provider(provider_id);
            }
        }
    }

    pub(crate) fn skip_endpoint(&mut self, endpoint_id: &str) {
        let endpoint_id = endpoint_id.trim();
        if endpoint_id.is_empty() {
            return;
        }
        self.skipped_endpoint_ids.insert(endpoint_id.to_string());
        for item in &mut self.items {
            if let LocalExecutionCandidateAttemptSourceItem::RequestedModelPage { cursor } = item {
                cursor.skip_endpoint(endpoint_id);
            }
        }
    }

    pub(crate) fn skip_credential(&mut self, key_id: &str) {
        let key_id = key_id.trim();
        if key_id.is_empty() {
            return;
        }
        self.skipped_credential_ids.insert(key_id.to_string());
        for item in &mut self.items {
            if let LocalExecutionCandidateAttemptSourceItem::RequestedModelPage { cursor } = item {
                cursor.skip_credential(key_id);
            }
        }
    }
}

impl LocalExecutionCandidateAttempt {
    pub(crate) fn attempt_identity(&self) -> ExecutionAttemptIdentity {
        ExecutionAttemptIdentity::new(self.candidate_index, self.retry_index)
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct LocalAvailableCandidatePersistenceContext<'a> {
    pub(crate) user_id: &'a str,
    pub(crate) api_key_id: &'a str,
    pub(crate) required_capabilities: Option<&'a Value>,
    pub(crate) error_context: &'static str,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct LocalSkippedCandidatePersistenceContext<'a> {
    pub(crate) user_id: &'a str,
    pub(crate) api_key_id: &'a str,
    pub(crate) required_capabilities: Option<&'a Value>,
    pub(crate) error_context: &'static str,
    pub(crate) record_runtime_miss_diagnostic: bool,
}

struct GatewayLocalCandidateMaterializationPort<'a, F, G> {
    state: PlannerAppState<'a>,
    trace_id: &'a str,
    client_api_format: &'a str,
    requested_model: Option<&'a str>,
    auth_snapshot: Option<&'a GatewayAuthApiKeySnapshot>,
    client_session_affinity: Option<&'a ClientSessionAffinity>,
    required_capabilities: Option<&'a Value>,
    routing_policy: Option<&'a ResolvedRoutingPolicy>,
    persistence_policy: LocalCandidatePersistencePolicy<'a>,
    scheduler_cache_affinity_enabled: bool,
    build_available_extra_data: F,
    decorate_skipped_candidate: G,
}

struct GatewayAvailableCandidatePersistencePort<'a, F> {
    state: PlannerAppState<'a>,
    trace_id: &'a str,
    user_id: &'a str,
    api_key_id: &'a str,
    required_capabilities: Option<&'a Value>,
    error_context: &'static str,
    created_at_unix_ms: u64,
    build_extra_data: F,
}

struct GatewaySkippedCandidatePersistencePort<'a> {
    state: &'a AppState,
    trace_id: &'a str,
    user_id: &'a str,
    api_key_id: &'a str,
    required_capabilities: Option<&'a Value>,
    error_context: &'static str,
    record_runtime_miss_diagnostic: bool,
}

#[async_trait]
impl<F, G> AiCandidateMaterializationPort for GatewayLocalCandidateMaterializationPort<'_, F, G>
where
    F: Fn(&EligibleLocalExecutionCandidate) -> Option<Value> + Send + Sync,
    G: Fn(SkippedLocalExecutionCandidate) -> SkippedLocalExecutionCandidate + Send + Sync,
{
    type Candidate = SchedulerMinimalCandidateSelectionCandidate;
    type Eligible = EligibleLocalExecutionCandidate;
    type Skipped = SkippedLocalExecutionCandidate;
    type Attempt = LocalExecutionCandidateAttempt;
    type Error = Infallible;

    async fn resolve_and_rank_candidates(
        &self,
        candidates: Vec<Self::Candidate>,
    ) -> Result<(Vec<Self::Eligible>, Vec<Self::Skipped>), Self::Error> {
        let resolved = resolve_and_rank_local_execution_candidates_with_optional_model(
            self.state,
            candidates,
            self.client_api_format,
            self.requested_model,
            self.auth_snapshot,
            self.client_session_affinity,
            self.required_capabilities,
            self.routing_policy,
        )
        .await;
        Ok(resolved)
    }

    fn decorate_skipped_candidate(&self, skipped: Self::Skipped) -> Self::Skipped {
        (self.decorate_skipped_candidate)(skipped)
    }

    fn remember_first_candidate_affinity(&self, candidates: &[Self::Eligible]) {
        if !self.scheduler_cache_affinity_enabled {
            return;
        }
        remember_first_local_candidate_affinity(
            self.state,
            self.auth_snapshot,
            self.client_session_affinity,
            self.client_api_format,
            self.requested_model,
            self.routing_policy,
            candidates,
        );
    }

    async fn persist_available_candidates(
        &self,
        candidates: Vec<Self::Eligible>,
    ) -> Result<Vec<Self::Attempt>, Self::Error> {
        Ok(materialize_local_execution_candidate_attempts(
            self.state,
            self.trace_id,
            self.persistence_policy.available,
            candidates,
            self.routing_policy,
            self.client_api_format,
            &self.build_available_extra_data,
        )
        .await)
    }

    async fn persist_skipped_candidates(
        &self,
        starting_candidate_index: u32,
        skipped_candidates: Vec<Self::Skipped>,
    ) -> Result<(), Self::Error> {
        let skipped_candidates = attach_routing_trace_to_skipped_candidates(
            self.routing_policy,
            self.client_api_format,
            starting_candidate_index,
            skipped_candidates,
        );
        persist_skipped_local_execution_candidates_with_context(
            self.state.app(),
            self.trace_id,
            self.persistence_policy.skipped,
            starting_candidate_index,
            skipped_candidates,
        )
        .await;
        Ok(())
    }
}

#[async_trait]
impl<F> AiAvailableCandidatePersistencePort for GatewayAvailableCandidatePersistencePort<'_, F>
where
    F: Fn(&EligibleLocalExecutionCandidate) -> Option<Value> + Send + Sync,
{
    type Candidate = EligibleLocalExecutionCandidate;
    type Attempt = LocalExecutionCandidateAttempt;
    type ExtraData = Value;
    type Error = Infallible;

    fn attempt_slot_count(&self, candidate: &Self::Candidate) -> u32 {
        local_attempt_slot_count(&candidate.transport)
    }

    fn build_extra_data(&self, candidate: &Self::Candidate) -> Option<Self::ExtraData> {
        available_candidate_extra_data_with_dispatch_ref(candidate, &self.build_extra_data)
    }

    fn generate_candidate_id(&self) -> String {
        Uuid::new_v4().to_string()
    }

    fn should_persist_available_candidate(&self, candidate: &Self::Candidate) -> bool {
        should_persist_available_local_candidate(candidate)
    }

    async fn persist_available_candidate(
        &self,
        candidate: &Self::Candidate,
        candidate_index: u32,
        retry_index: u32,
        generated_candidate_id: &str,
        extra_data: Option<Self::ExtraData>,
    ) -> Result<String, Self::Error> {
        Ok(self
            .state
            .persist_available_local_candidate(
                self.trace_id,
                self.user_id,
                self.api_key_id,
                &candidate.candidate,
                candidate_index,
                retry_index,
                generated_candidate_id,
                self.required_capabilities,
                extra_data,
                self.created_at_unix_ms,
                self.error_context,
            )
            .await)
    }

    fn build_attempt(
        &self,
        candidate: Self::Candidate,
        candidate_index: u32,
        retry_index: u32,
        candidate_id: String,
    ) -> Self::Attempt {
        LocalExecutionCandidateAttempt {
            eligible: candidate,
            candidate_index,
            retry_index,
            candidate_id,
        }
    }
}

#[async_trait]
impl AiSkippedCandidatePersistencePort for GatewaySkippedCandidatePersistencePort<'_> {
    type Skipped = SkippedLocalExecutionCandidate;
    type ExtraData = Value;
    type Error = Infallible;

    fn should_persist_skipped_candidate(&self, candidate: &Self::Skipped) -> bool {
        should_persist_skipped_local_candidate(candidate)
    }

    fn build_extra_data(&self, candidate: &Self::Skipped) -> Option<Self::ExtraData> {
        ai_candidate_extra_data_with_ranking(
            candidate.extra_data.clone(),
            candidate.ranking.as_ref(),
        )
    }

    fn generate_candidate_id(&self) -> String {
        Uuid::new_v4().to_string()
    }

    async fn persist_skipped_candidate(
        &self,
        candidate: &Self::Skipped,
        candidate_index: u32,
        generated_candidate_id: &str,
        extra_data: Option<Self::ExtraData>,
    ) -> Result<(), Self::Error> {
        persist_skipped_local_execution_candidate(
            self.state,
            self.trace_id,
            self.user_id,
            self.api_key_id,
            &candidate.candidate,
            candidate_index,
            generated_candidate_id,
            self.required_capabilities,
            candidate.skip_reason,
            extra_data,
            self.error_context,
            self.record_runtime_miss_diagnostic,
        )
        .await;
        Ok(())
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn materialize_local_execution_candidates_with_serving<F, G>(
    state: PlannerAppState<'_>,
    trace_id: &str,
    client_api_format: &str,
    requested_model: Option<&str>,
    auth_snapshot: Option<&GatewayAuthApiKeySnapshot>,
    client_session_affinity: Option<&ClientSessionAffinity>,
    required_capabilities: Option<&Value>,
    routing_policy: Option<&ResolvedRoutingPolicy>,
    _request_auth_channel: Option<&str>,
    persistence_policy: LocalCandidatePersistencePolicy<'_>,
    candidates: Vec<SchedulerMinimalCandidateSelectionCandidate>,
    preselection_skipped: Vec<SkippedLocalExecutionCandidate>,
    build_available_extra_data: F,
    decorate_skipped_candidate: G,
) -> AiCandidateMaterializationOutcome<LocalExecutionCandidateAttempt>
where
    F: Fn(&EligibleLocalExecutionCandidate) -> Option<Value> + Send + Sync,
    G: Fn(SkippedLocalExecutionCandidate) -> SkippedLocalExecutionCandidate + Send + Sync,
{
    let scheduler_cache_affinity_enabled =
        scheduler_cache_affinity_enabled(state, routing_policy).await;
    let port = GatewayLocalCandidateMaterializationPort {
        state,
        trace_id,
        client_api_format,
        requested_model,
        auth_snapshot,
        client_session_affinity,
        required_capabilities,
        routing_policy,
        persistence_policy,
        scheduler_cache_affinity_enabled,
        build_available_extra_data,
        decorate_skipped_candidate,
    };

    match run_ai_candidate_materialization(&port, candidates, preselection_skipped).await {
        Ok(outcome) => outcome,
        Err(error) => match error {},
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn build_local_execution_candidate_attempt_source_with_serving<'a, F, G>(
    state: PlannerAppState<'a>,
    trace_id: &str,
    client_api_format: &str,
    requested_model: Option<&str>,
    auth_snapshot: Option<&GatewayAuthApiKeySnapshot>,
    client_session_affinity: Option<&ClientSessionAffinity>,
    required_capabilities: Option<&Value>,
    routing_policy: Option<&ResolvedRoutingPolicy>,
    _request_auth_channel: Option<&str>,
    persistence_policy: LocalCandidatePersistencePolicy<'_>,
    candidates: Vec<SchedulerMinimalCandidateSelectionCandidate>,
    preselection_skipped: Vec<SkippedLocalExecutionCandidate>,
    build_available_extra_data: F,
    decorate_skipped_candidate: G,
) -> (LocalExecutionCandidateAttemptSource<'a>, usize)
where
    F: Fn(&EligibleLocalExecutionCandidate) -> Option<Value> + Send + Sync,
    G: Fn(SkippedLocalExecutionCandidate) -> SkippedLocalExecutionCandidate + Send + Sync,
{
    let scheduler_cache_affinity_enabled =
        scheduler_cache_affinity_enabled(state, routing_policy).await;
    let _ = build_available_extra_data;
    let (candidates, resolved_skipped) =
        resolve_and_rank_local_execution_candidates_with_optional_model(
            state,
            candidates,
            client_api_format,
            requested_model,
            auth_snapshot,
            client_session_affinity,
            required_capabilities,
            routing_policy,
        )
        .await;
    let skipped_candidate_count = preselection_skipped.len() + resolved_skipped.len();
    let skipped_candidates = preselection_skipped
        .into_iter()
        .chain(resolved_skipped)
        .map(decorate_skipped_candidate)
        .collect::<Vec<_>>();
    let candidate_count = candidates.len() + skipped_candidate_count;

    if scheduler_cache_affinity_enabled {
        remember_first_local_candidate_affinity(
            state,
            auth_snapshot,
            client_session_affinity,
            client_api_format,
            requested_model,
            routing_policy,
            &candidates,
        );
    }
    persist_skipped_local_execution_candidates_with_context(
        state.app(),
        trace_id,
        persistence_policy.skipped,
        u32::try_from(candidates.len()).unwrap_or(u32::MAX),
        attach_routing_trace_to_skipped_candidates(
            routing_policy,
            client_api_format,
            u32::try_from(candidates.len()).unwrap_or(u32::MAX),
            skipped_candidates,
        ),
    )
    .await;

    let (items, _) = build_candidate_items(candidates, 0);

    (
        LocalExecutionCandidateAttemptSource {
            items,
            skipped_provider_ids: BTreeSet::new(),
            skipped_endpoint_ids: BTreeSet::new(),
            skipped_credential_ids: BTreeSet::new(),
        },
        candidate_count,
    )
}

fn build_candidate_items<'a>(
    candidates: Vec<EligibleLocalExecutionCandidate>,
    starting_candidate_index: u32,
) -> (VecDeque<LocalExecutionCandidateAttemptSourceItem<'a>>, u32) {
    let mut items = VecDeque::new();
    let mut next_candidate_index = starting_candidate_index;
    for candidate in candidates {
        let candidate_index = next_candidate_index;
        next_candidate_index = next_candidate_index.saturating_add(1);
        let attempts =
            build_unpersisted_local_execution_candidate_attempts(candidate, candidate_index);
        if !attempts.is_empty() {
            items.push_back(LocalExecutionCandidateAttemptSourceItem::Static {
                attempts: dispatch_sequence_from_attempts(attempts.into()),
            });
        }
    }
    (items, next_candidate_index)
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn build_lazy_requested_model_execution_candidate_attempt_source_with_serving<
    'a,
    F,
    G,
>(
    state: PlannerAppState<'a>,
    model_directive_policy: &crate::system_features::ModelDirectivePolicySnapshot,
    trace_id: &str,
    client_api_format: &str,
    requested_model: &str,
    request_operation: Option<&str>,
    require_streaming: bool,
    auth_snapshot: &GatewayAuthApiKeySnapshot,
    client_session_affinity: Option<&ClientSessionAffinity>,
    required_capabilities: Option<&Value>,
    routing_policy: Option<&ResolvedRoutingPolicy>,
    request_auth_channel: Option<&str>,
    persistence_policy: LocalCandidatePersistencePolicy<'_>,
    use_api_format_alias_match: bool,
    key_mode: LocalCandidatePreselectionKeyMode,
    build_available_extra_data: F,
    decorate_skipped_candidate: G,
) -> (LocalExecutionCandidateAttemptSource<'a>, usize)
where
    F: Fn(&EligibleLocalExecutionCandidate) -> Option<Value> + Send + Sync + 'a,
    G: Fn(SkippedLocalExecutionCandidate) -> SkippedLocalExecutionCandidate + Send + Sync + 'a,
{
    let scheduler_cache_affinity_enabled =
        scheduler_cache_affinity_enabled(state, routing_policy).await;
    let _ = build_available_extra_data;
    let decorate_skipped_candidate = Arc::new(decorate_skipped_candidate);
    let record_runtime_miss_diagnostic = persistence_policy.skipped.record_runtime_miss_diagnostic;
    let page_cursor = LocalCandidatePreselectionPageCursor::new(
        state,
        model_directive_policy,
        client_api_format,
        requested_model,
        request_operation,
        require_streaming,
        required_capabilities,
        auth_snapshot,
        routing_policy,
        client_session_affinity,
        request_auth_channel,
        use_api_format_alias_match,
        key_mode,
        true,
        Some(trace_id),
    )
    .await;
    let mut cursor = RequestedModelAttemptPageCursor {
        state,
        trace_id: trace_id.to_string(),
        client_api_format: client_api_format.to_string(),
        requested_model: requested_model.to_string(),
        auth_snapshot: auth_snapshot.clone(),
        client_session_affinity: client_session_affinity.cloned(),
        required_capabilities: required_capabilities.cloned(),
        routing_policy: routing_policy.cloned(),
        request_auth_channel: request_auth_channel.map(str::to_string),
        skipped_user_id: persistence_policy.skipped.user_id.to_string(),
        skipped_api_key_id: persistence_policy.skipped.api_key_id.to_string(),
        skipped_required_capabilities: persistence_policy.skipped.required_capabilities.cloned(),
        skipped_error_context: persistence_policy.skipped.error_context,
        record_runtime_miss_diagnostic,
        decorate_skipped_candidate,
        page_cursor,
        pending_items: VecDeque::new(),
        skipped_provider_ids: BTreeSet::new(),
        skipped_endpoint_ids: BTreeSet::new(),
        skipped_credential_ids: BTreeSet::new(),
        candidate_count: 0,
        next_candidate_index: 0,
        remembered_affinity: false,
        scheduler_cache_affinity_enabled,
        auth_api_key_concurrency_wait_deadline: None,
        deferred_error: None,
    };
    if let Err(error) = cursor.load_next_page().await {
        cursor.deferred_error = Some(error);
    }
    let candidate_count = cursor.candidate_count;
    let mut items = VecDeque::new();
    if !cursor.pending_items.is_empty() || cursor.deferred_error.is_some() {
        items.push_back(
            LocalExecutionCandidateAttemptSourceItem::RequestedModelPage {
                cursor: Box::new(cursor),
            },
        );
    }
    (
        LocalExecutionCandidateAttemptSource {
            items,
            skipped_provider_ids: BTreeSet::new(),
            skipped_endpoint_ids: BTreeSet::new(),
            skipped_credential_ids: BTreeSet::new(),
        },
        candidate_count,
    )
}

struct RequestedModelAttemptPageCursor<'a> {
    state: PlannerAppState<'a>,
    trace_id: String,
    client_api_format: String,
    requested_model: String,
    auth_snapshot: GatewayAuthApiKeySnapshot,
    client_session_affinity: Option<ClientSessionAffinity>,
    required_capabilities: Option<Value>,
    routing_policy: Option<ResolvedRoutingPolicy>,
    request_auth_channel: Option<String>,
    skipped_user_id: String,
    skipped_api_key_id: String,
    skipped_required_capabilities: Option<Value>,
    skipped_error_context: &'static str,
    record_runtime_miss_diagnostic: bool,
    decorate_skipped_candidate: DecorateSkippedCandidateFn<'a>,
    page_cursor: LocalCandidatePreselectionPageCursor<'a>,
    pending_items: VecDeque<LocalExecutionCandidateAttemptSourceItem<'a>>,
    skipped_provider_ids: BTreeSet<String>,
    skipped_endpoint_ids: BTreeSet<String>,
    skipped_credential_ids: BTreeSet<String>,
    candidate_count: usize,
    next_candidate_index: u32,
    remembered_affinity: bool,
    scheduler_cache_affinity_enabled: bool,
    auth_api_key_concurrency_wait_deadline: Option<Instant>,
    deferred_error: Option<GatewayError>,
}

impl<'a> RequestedModelAttemptPageCursor<'a> {
    fn skip_provider(&mut self, provider_id: &str) {
        self.skipped_provider_ids.insert(provider_id.to_string());
    }

    fn skip_endpoint(&mut self, endpoint_id: &str) {
        self.skipped_endpoint_ids.insert(endpoint_id.to_string());
    }

    fn skip_credential(&mut self, key_id: &str) {
        self.skipped_credential_ids.insert(key_id.to_string());
    }

    async fn next_attempt(
        &mut self,
    ) -> Result<Option<LocalExecutionCandidateAttempt>, GatewayError> {
        if let Some(error) = self.deferred_error.take() {
            return Err(error);
        }
        loop {
            if let Some(attempt) = pop_attempt_from_items(
                &mut self.pending_items,
                &self.skipped_provider_ids,
                &self.skipped_endpoint_ids,
                &self.skipped_credential_ids,
            )
            .await
            {
                return Ok(Some(attempt));
            }
            if !self.load_next_page().await? {
                return Ok(None);
            }
        }
    }

    async fn load_next_page(&mut self) -> Result<bool, GatewayError> {
        loop {
            let page_started_at = std::time::Instant::now();
            let page = match self.page_cursor.next_page().await {
                Ok(Some(page)) => page,
                Ok(None) => {
                    observe_gateway_stage_ms(
                        "candidate_page_load",
                        page_started_at.elapsed().as_millis() as u64,
                    );
                    return Ok(false);
                }
                Err(error) => {
                    observe_gateway_stage_ms(
                        "candidate_page_load",
                        page_started_at.elapsed().as_millis() as u64,
                    );
                    return Err(error);
                }
            };
            observe_gateway_stage_ms(
                "candidate_page_load",
                page_started_at.elapsed().as_millis() as u64,
            );

            if page_is_exact_auth_api_key_concurrency_limited(&page) {
                if self.wait_for_auth_api_key_concurrency_retry().await {
                    continue;
                }
                self.persist_final_auth_api_key_concurrency_skips(page.skipped_candidates)
                    .await;
                return Ok(false);
            }

            let resolve_started_at = std::time::Instant::now();
            let (candidates, resolved_skipped) =
                resolve_priority_candidate_page_with_cache(self, page.candidates).await;
            observe_gateway_stage_ms(
                "candidate_page_resolve",
                resolve_started_at.elapsed().as_millis() as u64,
            );
            let skipped_candidates = page
                .skipped_candidates
                .into_iter()
                .chain(resolved_skipped)
                .map(|skipped| (self.decorate_skipped_candidate)(skipped))
                .collect::<Vec<_>>();
            let skipped_candidate_count = skipped_candidates.len();
            self.candidate_count = self
                .candidate_count
                .saturating_add(candidates.len() + skipped_candidate_count);
            if self.scheduler_cache_affinity_enabled
                && !self.remembered_affinity
                && !candidates.is_empty()
            {
                remember_first_local_candidate_affinity(
                    self.state,
                    Some(&self.auth_snapshot),
                    self.client_session_affinity.as_ref(),
                    &self.client_api_format,
                    Some(&self.requested_model),
                    self.routing_policy.as_ref(),
                    &candidates,
                );
                self.remembered_affinity = true;
            }
            let (items, next_candidate_index) =
                build_candidate_items(candidates, self.next_candidate_index);
            self.next_candidate_index = next_candidate_index
                .saturating_add(u32::try_from(skipped_candidate_count).unwrap_or(u32::MAX));
            if !items.is_empty() {
                self.pending_items = items;
                return Ok(true);
            }
            let skipped_starting_candidate_index = next_candidate_index;
            let skipped_persistence = LocalSkippedCandidatePersistenceContext {
                user_id: self.skipped_user_id.as_str(),
                api_key_id: self.skipped_api_key_id.as_str(),
                required_capabilities: self.skipped_required_capabilities.as_ref(),
                error_context: self.skipped_error_context,
                record_runtime_miss_diagnostic: self.record_runtime_miss_diagnostic,
            };
            persist_skipped_local_execution_candidates_with_context(
                self.state.app(),
                &self.trace_id,
                skipped_persistence,
                skipped_starting_candidate_index,
                attach_routing_trace_to_skipped_candidates(
                    self.routing_policy.as_ref(),
                    &self.client_api_format,
                    skipped_starting_candidate_index,
                    skipped_candidates,
                ),
            )
            .await;
        }
    }

    async fn wait_for_auth_api_key_concurrency_retry(&mut self) -> bool {
        let now = Instant::now();
        let deadline = *self
            .auth_api_key_concurrency_wait_deadline
            .get_or_insert(now + AUTH_API_KEY_CONCURRENCY_WAIT_BUDGET);
        if now >= deadline {
            return false;
        }

        let sleep_duration =
            AUTH_API_KEY_CONCURRENCY_RETRY_DELAY.min(deadline.saturating_duration_since(now));
        tokio::time::sleep(sleep_duration).await;
        self.page_cursor.restart_scan();
        true
    }

    async fn persist_final_auth_api_key_concurrency_skips(
        &mut self,
        skipped_candidates: Vec<SkippedLocalExecutionCandidate>,
    ) {
        let skipped_candidates = skipped_candidates
            .into_iter()
            .map(|skipped| (self.decorate_skipped_candidate)(skipped))
            .collect::<Vec<_>>();
        let skipped_candidate_count = skipped_candidates.len();
        self.candidate_count = self.candidate_count.saturating_add(skipped_candidate_count);
        let skipped_persistence = LocalSkippedCandidatePersistenceContext {
            user_id: self.skipped_user_id.as_str(),
            api_key_id: self.skipped_api_key_id.as_str(),
            required_capabilities: self.skipped_required_capabilities.as_ref(),
            error_context: self.skipped_error_context,
            record_runtime_miss_diagnostic: self.record_runtime_miss_diagnostic,
        };
        persist_skipped_local_execution_candidates_with_context(
            self.state.app(),
            &self.trace_id,
            skipped_persistence,
            self.next_candidate_index,
            attach_routing_trace_to_skipped_candidates(
                self.routing_policy.as_ref(),
                &self.client_api_format,
                self.next_candidate_index,
                skipped_candidates,
            ),
        )
        .await;
        self.next_candidate_index = self
            .next_candidate_index
            .saturating_add(u32::try_from(skipped_candidate_count).unwrap_or(u32::MAX));
    }
}

fn page_is_exact_auth_api_key_concurrency_limited(
    page: &AiCandidatePreselectionOutcome<
        SchedulerMinimalCandidateSelectionCandidate,
        SkippedLocalExecutionCandidate,
    >,
) -> bool {
    page.candidates.is_empty()
        && !page.skipped_candidates.is_empty()
        && page
            .skipped_candidates
            .iter()
            .all(|skipped| is_auth_api_key_concurrency_limit_skip_reason(skipped.skip_reason))
}

async fn pop_attempt_from_items(
    items: &mut VecDeque<LocalExecutionCandidateAttemptSourceItem<'_>>,
    skipped_provider_ids: &BTreeSet<String>,
    skipped_endpoint_ids: &BTreeSet<String>,
    skipped_credential_ids: &BTreeSet<String>,
) -> Option<LocalExecutionCandidateAttempt> {
    loop {
        let front = items.front_mut()?;
        match front {
            LocalExecutionCandidateAttemptSourceItem::Static { attempts } => {
                if dispatch_sequence_candidate_is_skipped(
                    attempts,
                    skipped_provider_ids,
                    skipped_endpoint_ids,
                    skipped_credential_ids,
                ) {
                    items.pop_front();
                    continue;
                }
                if let Some(attempt) = next_attempt_from_dispatch_sequence(attempts) {
                    if dispatch_sequence_exhausted(attempts) {
                        items.pop_front();
                    }
                    return Some(attempt);
                }
                items.pop_front();
            }
            LocalExecutionCandidateAttemptSourceItem::RequestedModelPage { .. } => {
                items.pop_front();
            }
        }
    }
}

async fn scheduler_cache_affinity_enabled(
    state: PlannerAppState<'_>,
    routing_policy: Option<&ResolvedRoutingPolicy>,
) -> bool {
    scheduler_ordering_config_for_routing_policy(state, routing_policy)
        .await
        .scheduling_mode
        == SchedulerSchedulingMode::CacheAffinity
}

pub(crate) fn remember_first_local_candidate_affinity(
    state: PlannerAppState<'_>,
    auth_snapshot: Option<&GatewayAuthApiKeySnapshot>,
    client_session_affinity: Option<&ClientSessionAffinity>,
    client_api_format: &str,
    requested_model: Option<&str>,
    routing_policy: Option<&ResolvedRoutingPolicy>,
    candidates: &[EligibleLocalExecutionCandidate],
) {
    let Some(first_candidate) = candidates.first() else {
        return;
    };
    let affinity_requested_model = requested_model
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(first_candidate.candidate.global_model_name.as_str());
    remember_scheduler_affinity_for_candidate_with_routing_policy_at_epoch(
        state,
        auth_snapshot,
        client_session_affinity,
        client_api_format,
        affinity_requested_model,
        &first_candidate.candidate,
        routing_policy,
        first_candidate.orchestration.scheduler_affinity_epoch,
    );
}

async fn resolve_priority_candidate_page_with_cache(
    cursor: &RequestedModelAttemptPageCursor<'_>,
    page_candidates: Vec<SchedulerMinimalCandidateSelectionCandidate>,
) -> (
    Vec<EligibleLocalExecutionCandidate>,
    Vec<SkippedLocalExecutionCandidate>,
) {
    if !should_cache_resolved_candidate_page(cursor) {
        return resolve_and_rank_local_execution_candidates_with_optional_model(
            cursor.state,
            page_candidates,
            &cursor.client_api_format,
            Some(&cursor.requested_model),
            Some(&cursor.auth_snapshot),
            cursor.client_session_affinity.as_ref(),
            cursor.required_capabilities.as_ref(),
            cursor.routing_policy.as_ref(),
        )
        .await;
    }

    let key = CandidateResolvedPageCacheKey::new(
        &cursor.requested_model,
        cursor.page_cursor.resolved_page_cache_request_operation(),
        &cursor.client_api_format,
        true,
        &cursor.auth_snapshot,
        cursor.required_capabilities.as_ref(),
        cursor.routing_policy.as_ref(),
        cursor.request_auth_channel.as_deref(),
        cursor.state.app().scheduler_affinity_epoch(),
        cursor.page_cursor.resolved_page_cache_preselection_mode(),
        cursor
            .page_cursor
            .resolved_page_cache_use_api_format_alias_match(),
        cursor.client_session_affinity.as_ref(),
        cursor
            .page_cursor
            .resolved_page_cache_model_directive_policy_hash(),
    );
    let page_candidates_for_fallback = page_candidates;
    let app = cursor.state.app();
    let cache = app.candidate_resolved_page_cache.clone();
    let ttl = candidate_page_cache_ttl_from_env();
    let stale_ttl = candidate_page_cache_stale_ttl(ttl);
    // Keep request-owned planner inputs borrowed until the cache tells us a
    // cold load or stale refresh is actually needed. Fresh hits must not pay
    // for deep copies of candidate pages and auth/routing snapshots.
    let client_api_format = cursor.client_api_format.as_str();
    let requested_model = cursor.requested_model.as_str();
    let auth_snapshot = &cursor.auth_snapshot;
    let client_session_affinity = cursor.client_session_affinity.as_ref();
    let required_capabilities = cursor.required_capabilities.as_ref();
    let routing_policy = cursor.routing_policy.as_ref();
    let request_auth_channel = cursor.request_auth_channel.as_deref();
    let cached = cache
        .get_or_load_once_stale_while_revalidating(
            key,
            ttl,
            stale_ttl,
            || {
                resolve_candidate_page_snapshot(
                    (*app).clone(),
                    page_candidates_for_fallback.clone(),
                    client_api_format.to_owned(),
                    requested_model.to_owned(),
                    auth_snapshot.clone(),
                    client_session_affinity.cloned(),
                    required_capabilities.cloned(),
                    routing_policy.cloned(),
                    request_auth_channel.map(ToOwned::to_owned),
                )
            },
            || {
                resolve_candidate_page_snapshot(
                    (*app).clone(),
                    page_candidates_for_fallback.clone(),
                    client_api_format.to_owned(),
                    requested_model.to_owned(),
                    auth_snapshot.clone(),
                    client_session_affinity.cloned(),
                    required_capabilities.cloned(),
                    routing_policy.cloned(),
                    request_auth_channel.map(ToOwned::to_owned),
                )
            },
            CacheLoadObserver::new()
                .on_hit(record_candidate_page_resolve_cache_hit)
                .on_miss(record_candidate_page_resolve_cache_miss)
                .on_load(record_candidate_page_resolve_cache_load)
                .on_follower_wait(record_candidate_page_resolve_cache_follower_wait),
        )
        .await
        .unwrap_or(None);

    match cached {
        Some(snapshot) => (
            snapshot.candidates.clone(),
            snapshot.resolved_skipped.clone(),
        ),
        None => {
            if page_candidates_for_fallback.is_empty() {
                return (Vec::new(), Vec::new());
            }
            resolve_and_rank_local_execution_candidates_with_optional_model(
                cursor.state,
                page_candidates_for_fallback,
                &cursor.client_api_format,
                Some(&cursor.requested_model),
                Some(&cursor.auth_snapshot),
                cursor.client_session_affinity.as_ref(),
                cursor.required_capabilities.as_ref(),
                cursor.routing_policy.as_ref(),
            )
            .await
        }
    }
}

async fn resolve_candidate_page_snapshot(
    app: AppState,
    page_candidates: Vec<SchedulerMinimalCandidateSelectionCandidate>,
    client_api_format: String,
    requested_model: String,
    auth_snapshot: GatewayAuthApiKeySnapshot,
    client_session_affinity: Option<ClientSessionAffinity>,
    required_capabilities: Option<Value>,
    routing_policy: Option<ResolvedRoutingPolicy>,
    _request_auth_channel: Option<String>,
) -> Result<Option<Arc<CandidateResolvedPageSnapshot>>, GatewayError> {
    let state = PlannerAppState::new(&app);
    let (candidates, resolved_skipped) =
        resolve_and_rank_local_execution_candidates_with_optional_model(
            state,
            page_candidates,
            &client_api_format,
            Some(&requested_model),
            Some(&auth_snapshot),
            client_session_affinity.as_ref(),
            required_capabilities.as_ref(),
            routing_policy.as_ref(),
        )
        .await;
    Ok(Some(Arc::new(CandidateResolvedPageSnapshot {
        candidates,
        resolved_skipped,
    })))
}

fn should_cache_resolved_candidate_page(cursor: &RequestedModelAttemptPageCursor<'_>) -> bool {
    cursor
        .page_cursor
        .should_cache_current_priority_resolved_page()
}

fn should_persist_available_local_candidate(_eligible: &EligibleLocalExecutionCandidate) -> bool {
    true
}

fn should_persist_skipped_local_candidate(_candidate: &SkippedLocalExecutionCandidate) -> bool {
    true
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn persist_available_local_execution_candidates<F>(
    state: PlannerAppState<'_>,
    trace_id: &str,
    user_id: &str,
    api_key_id: &str,
    required_capabilities: Option<&Value>,
    candidates: Vec<EligibleLocalExecutionCandidate>,
    error_context: &'static str,
    build_extra_data: F,
) -> Vec<LocalExecutionCandidateAttempt>
where
    F: Fn(&EligibleLocalExecutionCandidate) -> Option<Value> + Send + Sync,
{
    let port = GatewayAvailableCandidatePersistencePort {
        state,
        trace_id,
        user_id,
        api_key_id,
        required_capabilities,
        error_context,
        created_at_unix_ms: current_unix_ms(),
        build_extra_data,
    };

    match run_ai_available_candidate_persistence(&port, candidates).await {
        Ok(attempts) => attempts,
        Err(error) => match error {},
    }
}

pub(crate) async fn persist_available_local_execution_candidates_with_context<F>(
    state: PlannerAppState<'_>,
    trace_id: &str,
    context: LocalAvailableCandidatePersistenceContext<'_>,
    candidates: Vec<EligibleLocalExecutionCandidate>,
    build_extra_data: F,
) -> Vec<LocalExecutionCandidateAttempt>
where
    F: Fn(&EligibleLocalExecutionCandidate) -> Option<Value> + Send + Sync,
{
    persist_available_local_execution_candidates(
        state,
        trace_id,
        context.user_id,
        context.api_key_id,
        context.required_capabilities,
        candidates,
        context.error_context,
        build_extra_data,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn materialize_local_execution_candidate_attempts<F>(
    state: PlannerAppState<'_>,
    trace_id: &str,
    context: LocalAvailableCandidatePersistenceContext<'_>,
    candidates: Vec<EligibleLocalExecutionCandidate>,
    routing_policy: Option<&ResolvedRoutingPolicy>,
    client_api_format: &str,
    build_extra_data: &F,
) -> Vec<LocalExecutionCandidateAttempt>
where
    F: Fn(&EligibleLocalExecutionCandidate) -> Option<Value> + Send + Sync,
{
    let mut attempts = Vec::new();

    for (candidate_index, candidate) in candidates.into_iter().enumerate() {
        let candidate_index = u32::try_from(candidate_index).unwrap_or(u32::MAX);
        attempts.extend(
            persist_available_local_execution_candidate_at_index(
                state,
                trace_id,
                context,
                candidate,
                candidate_index,
                routing_policy,
                client_api_format,
                build_extra_data,
            )
            .await,
        );
    }

    attempts
}

async fn persist_available_local_execution_candidate_at_index<F>(
    state: PlannerAppState<'_>,
    trace_id: &str,
    context: LocalAvailableCandidatePersistenceContext<'_>,
    candidate: EligibleLocalExecutionCandidate,
    candidate_index: u32,
    routing_policy: Option<&ResolvedRoutingPolicy>,
    client_api_format: &str,
    build_extra_data: &F,
) -> Vec<LocalExecutionCandidateAttempt>
where
    F: Fn(&EligibleLocalExecutionCandidate) -> Option<Value> + Send + Sync,
{
    let attempt_slots = local_attempt_slot_count(&candidate.transport).max(1);
    let extra_data = ai_candidate_extra_data_with_ranking(
        available_candidate_base_extra_data_with_dispatch_ref(&candidate, build_extra_data),
        candidate.ranking.as_ref(),
    );
    let extra_data = attach_routing_trace_to_extra_data(
        routing_policy,
        client_api_format,
        &candidate.candidate,
        candidate.ranking.as_ref(),
        None,
        Some(candidate_index),
        extra_data,
    );
    let should_persist = should_persist_available_local_candidate(&candidate);
    let mut attempts = Vec::with_capacity(attempt_slots as usize);
    let mut owned_candidate = Some(candidate);

    for retry_index in 0..attempt_slots {
        let candidate_ref = owned_candidate
            .as_ref()
            .expect("candidate should remain available until final retry");
        let generated_candidate_id = Uuid::new_v4().to_string();
        let candidate_id = if should_persist {
            state
                .persist_available_local_candidate(
                    trace_id,
                    context.user_id,
                    context.api_key_id,
                    &candidate_ref.candidate,
                    candidate_index,
                    retry_index,
                    generated_candidate_id.as_str(),
                    context.required_capabilities,
                    extra_data.clone(),
                    current_unix_ms(),
                    context.error_context,
                )
                .await
        } else {
            generated_candidate_id
        };

        let candidate = if retry_index + 1 == attempt_slots {
            owned_candidate
                .take()
                .expect("final retry should consume owned candidate")
        } else {
            candidate_ref.clone()
        };
        attempts.push(LocalExecutionCandidateAttempt {
            eligible: candidate,
            candidate_index,
            retry_index,
            candidate_id,
        });
    }

    attempts
}

fn available_candidate_extra_data_with_dispatch_ref<F>(
    candidate: &EligibleLocalExecutionCandidate,
    build_extra_data: &F,
) -> Option<Value>
where
    F: Fn(&EligibleLocalExecutionCandidate) -> Option<Value> + Send + Sync,
{
    ai_candidate_extra_data_with_ranking(
        available_candidate_base_extra_data_with_dispatch_ref(candidate, build_extra_data),
        candidate.ranking.as_ref(),
    )
}

fn available_candidate_base_extra_data_with_dispatch_ref<F>(
    candidate: &EligibleLocalExecutionCandidate,
    build_extra_data: &F,
) -> Option<Value>
where
    F: Fn(&EligibleLocalExecutionCandidate) -> Option<Value> + Send + Sync,
{
    let dispatch_ref = serde_json::to_value(dispatch_ref_for_local_candidate(candidate)).ok()?;
    let mut object = match build_extra_data(candidate) {
        Some(Value::Object(object)) => object,
        Some(value) => {
            let mut object = serde_json::Map::new();
            object.insert("extra".to_string(), value);
            object
        }
        None => serde_json::Map::new(),
    };
    object.insert("dispatch_ref".to_string(), dispatch_ref);
    Some(Value::Object(object))
}

fn attach_routing_trace_to_skipped_candidates(
    routing_policy: Option<&ResolvedRoutingPolicy>,
    client_api_format: &str,
    starting_candidate_index: u32,
    skipped_candidates: Vec<SkippedLocalExecutionCandidate>,
) -> Vec<SkippedLocalExecutionCandidate> {
    skipped_candidates
        .into_iter()
        .enumerate()
        .map(|(offset, skipped)| {
            let selected_order =
                starting_candidate_index.saturating_add(u32::try_from(offset).unwrap_or(u32::MAX));
            attach_routing_trace_to_skipped_candidate(
                routing_policy,
                client_api_format,
                selected_order,
                skipped,
            )
        })
        .collect()
}

fn attach_routing_trace_to_skipped_candidate(
    routing_policy: Option<&ResolvedRoutingPolicy>,
    client_api_format: &str,
    selected_order: u32,
    mut skipped_candidate: SkippedLocalExecutionCandidate,
) -> SkippedLocalExecutionCandidate {
    skipped_candidate.extra_data = attach_routing_trace_to_extra_data(
        routing_policy,
        client_api_format,
        &skipped_candidate.candidate,
        skipped_candidate.ranking.as_ref(),
        Some(skipped_candidate.skip_reason),
        Some(selected_order),
        skipped_candidate.extra_data,
    );
    skipped_candidate
}

#[allow(clippy::too_many_arguments)]
fn attach_routing_trace_to_extra_data(
    routing_policy: Option<&ResolvedRoutingPolicy>,
    client_api_format: &str,
    candidate: &SchedulerMinimalCandidateSelectionCandidate,
    ranking: Option<&SchedulerRankingOutcome>,
    skip_reason: Option<&'static str>,
    selected_order: Option<u32>,
    extra_data: Option<Value>,
) -> Option<Value> {
    let Some(policy) = routing_policy else {
        return extra_data;
    };
    let routing_trace = routing_trace_for_candidate(
        policy,
        client_api_format,
        candidate,
        ranking,
        skip_reason,
        selected_order,
    );
    Some(merge_routing_trace_into_extra_data(
        extra_data,
        routing_trace,
    ))
}

fn merge_routing_trace_into_extra_data(
    extra_data: Option<Value>,
    routing_trace: RoutingDecisionTrace,
) -> Value {
    let mut object = match extra_data {
        Some(Value::Object(object)) => object,
        Some(value) => {
            let mut object = serde_json::Map::new();
            object.insert("extra".to_string(), value);
            object
        }
        None => serde_json::Map::new(),
    };
    object.insert(
        "routing_trace".to_string(),
        serde_json::json!(routing_trace),
    );
    Value::Object(object)
}

fn routing_trace_for_candidate(
    policy: &ResolvedRoutingPolicy,
    client_api_format: &str,
    candidate: &SchedulerMinimalCandidateSelectionCandidate,
    ranking: Option<&SchedulerRankingOutcome>,
    skip_reason: Option<&'static str>,
    selected_order: Option<u32>,
) -> RoutingDecisionTrace {
    let candidate_kind = CandidateKind::Provider;
    let mut trace = crate::routing::build_routing_trace_seed(policy, client_api_format);
    trace.global_candidates.push(RoutingCandidateTrace {
        candidate_kind,
        provider_id: candidate.provider_id.clone(),
        endpoint_id: candidate.endpoint_id.clone(),
        model_id: candidate.model_id.clone(),
        key_id: Some(candidate.key_id.clone()),
        ranking_vector: rank_vector_for_candidate(
            &policy.ranking_overlay,
            &RoutingCandidateFacts {
                candidate_kind,
                provider_id: candidate.provider_id.clone(),
                endpoint_id: candidate.endpoint_id.clone(),
                model_id: candidate.model_id.clone(),
                key_id: Some(candidate.key_id.clone()),
                provider_priority: candidate.provider_priority,
                key_priority: candidate
                    .key_global_priority_for_format
                    .unwrap_or(candidate.key_internal_priority),
            },
        ),
        skip_reason: skip_reason.map(str::to_string),
        selected_order,
    });
    if let Some(ranking) = ranking {
        trace.runtime_facts.cache_affinity_hit = ranking.promoted_by == Some("cached_affinity");
    }
    trace
}

fn dispatch_sequence_from_attempts(
    attempts: Vec<LocalExecutionCandidateAttempt>,
) -> DispatchSequence<LocalExecutionCandidateAttempt> {
    DispatchSequence::new(
        attempts
            .into_iter()
            .map(|attempt| DispatchSequenceItem {
                candidate_index: attempt.candidate_index,
                retry_index: attempt.retry_index,
                candidate: attempt,
                mark: aether_dispatch_core::DispatchSequenceMark::Pending,
            })
            .collect(),
    )
}

fn next_attempt_from_dispatch_sequence(
    sequence: &mut DispatchSequence<LocalExecutionCandidateAttempt>,
) -> Option<LocalExecutionCandidateAttempt> {
    let attempt = sequence.next()?.candidate.clone();
    let _ = sequence.mark_succeeded();
    Some(attempt)
}

fn dispatch_sequence_candidate_is_skipped(
    sequence: &DispatchSequence<LocalExecutionCandidateAttempt>,
    skipped_provider_ids: &BTreeSet<String>,
    skipped_endpoint_ids: &BTreeSet<String>,
    skipped_credential_ids: &BTreeSet<String>,
) -> bool {
    sequence.peek_current().is_some_and(|item| {
        candidate_is_skipped(
            &item.candidate.eligible,
            skipped_provider_ids,
            skipped_endpoint_ids,
            skipped_credential_ids,
        )
    })
}

fn candidate_is_skipped(
    candidate: &EligibleLocalExecutionCandidate,
    skipped_provider_ids: &BTreeSet<String>,
    skipped_endpoint_ids: &BTreeSet<String>,
    skipped_credential_ids: &BTreeSet<String>,
) -> bool {
    skipped_provider_ids.contains(&candidate.candidate.provider_id)
        || skipped_endpoint_ids.contains(&candidate.candidate.endpoint_id)
        || skipped_credential_ids.contains(&candidate.candidate.key_id)
}

fn dispatch_sequence_exhausted(
    sequence: &mut DispatchSequence<LocalExecutionCandidateAttempt>,
) -> bool {
    sequence.next().is_none()
}

fn build_unpersisted_local_execution_candidate_attempts(
    candidate: EligibleLocalExecutionCandidate,
    candidate_index: u32,
) -> VecDeque<LocalExecutionCandidateAttempt> {
    let attempt_slots = local_attempt_slot_count(&candidate.transport).max(1);
    let mut attempts = VecDeque::with_capacity(attempt_slots as usize);
    let mut owned_candidate = Some(candidate);

    for retry_index in 0..attempt_slots {
        let candidate = if retry_index + 1 == attempt_slots {
            owned_candidate
                .take()
                .expect("final retry should consume owned candidate")
        } else {
            owned_candidate
                .as_ref()
                .expect("candidate should remain available until final retry")
                .clone()
        };
        attempts.push_back(LocalExecutionCandidateAttempt {
            eligible: candidate,
            candidate_index,
            retry_index,
            candidate_id: Uuid::new_v4().to_string(),
        });
    }

    attempts
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn persist_skipped_local_execution_candidate(
    state: &AppState,
    trace_id: &str,
    user_id: &str,
    api_key_id: &str,
    candidate: &SchedulerMinimalCandidateSelectionCandidate,
    candidate_index: u32,
    candidate_id: &str,
    required_capabilities: Option<&Value>,
    skip_reason: &'static str,
    extra_data: Option<Value>,
    error_context: &'static str,
    record_runtime_miss_diagnostic: bool,
) {
    if record_runtime_miss_diagnostic {
        record_local_runtime_candidate_skip_reason(state, trace_id, skip_reason);
    }

    PlannerAppState::new(state)
        .persist_skipped_local_candidate(
            trace_id,
            user_id,
            api_key_id,
            candidate,
            candidate_index,
            0,
            candidate_id,
            required_capabilities,
            skip_reason,
            extra_data,
            current_unix_ms(),
            error_context,
        )
        .await;
}

pub(crate) async fn mark_skipped_local_execution_candidate(
    state: &AppState,
    trace_id: &str,
    context: LocalSkippedCandidatePersistenceContext<'_>,
    candidate: &SchedulerMinimalCandidateSelectionCandidate,
    candidate_index: u32,
    candidate_id: &str,
    skip_reason: &'static str,
) {
    persist_skipped_local_execution_candidate(
        state,
        trace_id,
        context.user_id,
        context.api_key_id,
        candidate,
        candidate_index,
        candidate_id,
        context.required_capabilities,
        skip_reason,
        None,
        context.error_context,
        context.record_runtime_miss_diagnostic,
    )
    .await;
}

pub(crate) async fn mark_skipped_local_execution_candidate_with_extra_data(
    state: &AppState,
    trace_id: &str,
    context: LocalSkippedCandidatePersistenceContext<'_>,
    candidate: &SchedulerMinimalCandidateSelectionCandidate,
    candidate_index: u32,
    candidate_id: &str,
    skip_reason: &'static str,
    extra_data: Option<Value>,
) {
    persist_skipped_local_execution_candidate(
        state,
        trace_id,
        context.user_id,
        context.api_key_id,
        candidate,
        candidate_index,
        candidate_id,
        context.required_capabilities,
        skip_reason,
        extra_data,
        context.error_context,
        context.record_runtime_miss_diagnostic,
    )
    .await;
}

pub(crate) async fn mark_skipped_local_execution_candidate_with_failure_diagnostic(
    state: &AppState,
    trace_id: &str,
    context: LocalSkippedCandidatePersistenceContext<'_>,
    candidate: &SchedulerMinimalCandidateSelectionCandidate,
    candidate_index: u32,
    candidate_id: &str,
    skip_reason: &'static str,
    diagnostic: CandidateFailureDiagnostic,
) {
    mark_skipped_local_execution_candidate_with_extra_data(
        state,
        trace_id,
        context,
        candidate,
        candidate_index,
        candidate_id,
        skip_reason,
        Some(diagnostic.to_extra_data()),
    )
    .await;
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn persist_skipped_local_execution_candidates(
    state: &AppState,
    trace_id: &str,
    user_id: &str,
    api_key_id: &str,
    required_capabilities: Option<&Value>,
    starting_candidate_index: u32,
    skipped_candidates: Vec<SkippedLocalExecutionCandidate>,
    error_context: &'static str,
    record_runtime_miss_diagnostic: bool,
) {
    let port = GatewaySkippedCandidatePersistencePort {
        state,
        trace_id,
        user_id,
        api_key_id,
        required_capabilities,
        error_context,
        record_runtime_miss_diagnostic,
    };

    match run_ai_skipped_candidate_persistence(&port, starting_candidate_index, skipped_candidates)
        .await
    {
        Ok(()) => {}
        Err(error) => match error {},
    }
}

pub(crate) async fn persist_skipped_local_execution_candidates_with_context(
    state: &AppState,
    trace_id: &str,
    context: LocalSkippedCandidatePersistenceContext<'_>,
    starting_candidate_index: u32,
    skipped_candidates: Vec<SkippedLocalExecutionCandidate>,
) {
    persist_skipped_local_execution_candidates(
        state,
        trace_id,
        context.user_id,
        context.api_key_id,
        context.required_capabilities,
        starting_candidate_index,
        skipped_candidates,
        context.error_context,
        context.record_runtime_miss_diagnostic,
    )
    .await;
}
