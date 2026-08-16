use aether_ai_serving::AiCandidatePreselectionOutcome;
use aether_data_contracts::repository::candidate_selection::StoredMinimalCandidateSelectionRow;
use aether_routing_core::ResolvedRoutingPolicy;
use aether_runtime::ConcurrencyPermit;
use aether_scheduler_core::{
    enumerate_minimal_candidate_selection_with_model_directives, normalize_api_format,
    resolve_requested_global_model_name_with_model_directives_and_request_operation,
    row_supports_requested_model_with_model_directives_and_request_operation,
    ClientSessionAffinity, EnumerateMinimalCandidateSelectionInput,
    SchedulerMinimalCandidateSelectionCandidate,
};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use crate::ai_serving::planner::candidate_affinity_cache::has_explicit_session_affinity;
use crate::ai_serving::planner::candidate_resolution::SkippedLocalExecutionCandidate;
use crate::ai_serving::{GatewayAuthApiKeySnapshot, PlannerAppState};
use crate::cache::{
    candidate_page_cache_stale_ttl, candidate_page_cache_ttl_from_env,
    record_candidate_page_cache_follower_wait, record_candidate_page_cache_hit,
    record_candidate_page_cache_load, record_candidate_page_cache_miss,
    record_candidate_page_cache_none, record_candidate_row_page_cache_follower_wait,
    record_candidate_row_page_cache_hit, record_candidate_row_page_cache_load,
    record_candidate_row_page_cache_miss, record_candidate_row_page_cache_none, CacheLoadObserver,
    CandidatePageCacheKey, CandidatePageSnapshot, CandidateRowPageCacheKey,
};
use crate::clock::request_distribution_seed;
use crate::data::candidate_selection::{
    read_api_format_rows_fallback_page, read_requested_model_rows_fast_path_page,
    requested_model_candidate_names, MinimalCandidateSelectionRowSource,
    RequestedModelCandidateRowsPage, REQUESTED_MODEL_CANDIDATE_PAGE_SIZE,
    REQUESTED_MODEL_MAX_SCANNED_ROWS,
};
use crate::scheduler::candidate::SchedulerSkippedCandidate;
use crate::scheduler::config::{SchedulerOrderingConfig, SchedulerSchedulingMode};
use crate::stage_metrics::observe_gateway_stage_ms;
use crate::GatewayError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LocalCandidatePreselectionKeyMode {
    ProviderEndpointKeyModel,
    ProviderEndpointKeyModelAndApiFormat,
}

impl LocalCandidatePreselectionKeyMode {
    pub(crate) fn cache_key_name(self) -> &'static str {
        match self {
            Self::ProviderEndpointKeyModel => "provider_endpoint_key_model",
            Self::ProviderEndpointKeyModelAndApiFormat => "provider_endpoint_key_model_api_format",
        }
    }
}

fn resolve_model_directive_routing_models(
    policy: &crate::system_features::ModelDirectivePolicySnapshot,
    candidate_api_formats: &[String],
    requested_model: &str,
) -> BTreeMap<String, String> {
    candidate_api_formats
        .iter()
        .filter_map(|api_format| {
            let api_format = crate::ai_serving::normalize_api_format_alias(api_format);
            let resolution = policy.resolve_reasoning(&api_format, Some(requested_model));
            resolution
                .base_model()
                .map(|base_model| (api_format, base_model.to_string()))
        })
        .collect()
}

pub(crate) struct LocalCandidatePreselectionPageCursor<'a> {
    state: PlannerAppState<'a>,
    trace_id: String,
    client_api_format: String,
    requested_model: String,
    request_operation: Option<String>,
    require_streaming: bool,
    required_capabilities: Option<serde_json::Value>,
    auth_snapshot: GatewayAuthApiKeySnapshot,
    routing_policy: Option<ResolvedRoutingPolicy>,
    client_session_affinity: Option<ClientSessionAffinity>,
    request_auth_channel: Option<String>,
    use_api_format_alias_match: bool,
    key_mode: LocalCandidatePreselectionKeyMode,
    allow_priority_page_cache: bool,
    candidate_api_format: String,
    model_directive_routing_models: BTreeMap<String, String>,
    model_directive_policy_cache_key: String,
    ordering_config: SchedulerOrderingConfig,
    ranking_seed: u64,
    priority_page_emitted: bool,
    requested_name_indexes: BTreeMap<String, usize>,
    requested_name_offsets: BTreeMap<String, u32>,
    scanned_rows_by_format: BTreeMap<String, u32>,
    resolved_global_model_names: BTreeMap<String, String>,
    fallback_offsets: BTreeMap<String, u32>,
    fallback_scan_epoch: u32,
    exhausted_api_formats: BTreeSet<String>,
    seen_candidate_keys: BTreeSet<String>,
}

impl<'a> LocalCandidatePreselectionPageCursor<'a> {
    fn model_directive_base_model(&self, candidate_api_format: &str) -> Option<&str> {
        self.model_directive_routing_models
            .get(&crate::ai_serving::normalize_api_format_alias(
                candidate_api_format,
            ))
            .map(String::as_str)
    }

    fn routing_model(&self, candidate_api_format: &str) -> &str {
        self.model_directive_base_model(candidate_api_format)
            .unwrap_or(&self.requested_model)
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn new(
        state: PlannerAppState<'a>,
        model_directive_policy: &crate::system_features::ModelDirectivePolicySnapshot,
        client_api_format: &str,
        requested_model: &str,
        request_operation: Option<&str>,
        require_streaming: bool,
        required_capabilities: Option<&serde_json::Value>,
        auth_snapshot: &GatewayAuthApiKeySnapshot,
        routing_policy: Option<&ResolvedRoutingPolicy>,
        client_session_affinity: Option<&ClientSessionAffinity>,
        request_auth_channel: Option<&str>,
        use_api_format_alias_match: bool,
        key_mode: LocalCandidatePreselectionKeyMode,
        allow_priority_page_cache: bool,
        trace_id: Option<&str>,
    ) -> Self {
        let candidate_api_format = crate::ai_serving::normalize_api_format_alias(client_api_format);
        let model_directive_routing_models = resolve_model_directive_routing_models(
            model_directive_policy,
            std::slice::from_ref(&candidate_api_format),
            requested_model,
        );

        let ordering_config =
            super::candidate_ranking::scheduler_ordering_config_for_routing_policy(
                state,
                routing_policy,
            )
            .await;

        Self {
            state,
            trace_id: trace_id.unwrap_or_default().to_string(),
            client_api_format: client_api_format.to_string(),
            requested_model: requested_model.to_string(),
            request_operation: request_operation.map(str::to_string),
            require_streaming,
            required_capabilities: required_capabilities.cloned(),
            auth_snapshot: auth_snapshot.clone(),
            routing_policy: routing_policy.cloned(),
            client_session_affinity: client_session_affinity.cloned(),
            request_auth_channel: request_auth_channel.map(str::to_string),
            use_api_format_alias_match,
            key_mode,
            allow_priority_page_cache,
            candidate_api_format,
            model_directive_routing_models,
            model_directive_policy_cache_key: model_directive_policy.cache_key().to_string(),
            ordering_config,
            ranking_seed: request_distribution_seed(),
            priority_page_emitted: false,
            requested_name_indexes: BTreeMap::new(),
            requested_name_offsets: BTreeMap::new(),
            scanned_rows_by_format: BTreeMap::new(),
            resolved_global_model_names: BTreeMap::new(),
            fallback_offsets: BTreeMap::new(),
            fallback_scan_epoch: 0,
            exhausted_api_formats: BTreeSet::new(),
            seen_candidate_keys: BTreeSet::new(),
        }
    }

    pub(crate) async fn next_page(
        &mut self,
    ) -> Result<
        Option<
            AiCandidatePreselectionOutcome<
                SchedulerMinimalCandidateSelectionCandidate,
                SkippedLocalExecutionCandidate,
            >,
        >,
        GatewayError,
    > {
        if !self.priority_page_emitted {
            self.priority_page_emitted = true;
            let mut priority_page = self.cached_next_priority_page().await?;
            if self.routing_policy.is_some() {
                while let Some(mut page) = self.next_page_after_priority().await? {
                    priority_page.candidates.append(&mut page.candidates);
                    priority_page
                        .skipped_candidates
                        .append(&mut page.skipped_candidates);
                }
            }
            if !priority_page.candidates.is_empty() || !priority_page.skipped_candidates.is_empty()
            {
                return Ok(Some(priority_page));
            }
        }

        self.next_page_after_priority().await
    }

    async fn next_page_after_priority(
        &mut self,
    ) -> Result<
        Option<
            AiCandidatePreselectionOutcome<
                SchedulerMinimalCandidateSelectionCandidate,
                SkippedLocalExecutionCandidate,
            >,
        >,
        GatewayError,
    > {
        if self.api_format_is_exhausted(&self.candidate_api_format) {
            return Ok(None);
        }

        let _permit = acquire_candidate_planning_gate(self.state, &self.trace_id).await?;
        loop {
            let candidate_api_format = self.candidate_api_format.clone();
            let Some(outcome) = self.next_page_for_api_format(&candidate_api_format).await? else {
                return Ok(None);
            };
            if outcome.candidates.is_empty() && outcome.skipped_candidates.is_empty() {
                continue;
            }
            return Ok(Some(outcome));
        }
    }

    pub(crate) fn restart_scan(&mut self) {
        self.requested_name_indexes.clear();
        self.requested_name_offsets.clear();
        self.scanned_rows_by_format.clear();
        self.resolved_global_model_names.clear();
        self.fallback_offsets.clear();
        self.fallback_scan_epoch = self.fallback_scan_epoch.wrapping_add(1);
        self.exhausted_api_formats.clear();
        self.seen_candidate_keys.clear();
        self.priority_page_emitted = false;
    }

    pub(crate) fn resolved_page_cache_preselection_mode(&self) -> &'static str {
        self.key_mode.cache_key_name()
    }

    pub(crate) fn resolved_page_cache_request_operation(&self) -> Option<&str> {
        self.request_operation.as_deref()
    }

    pub(crate) fn resolved_page_cache_use_api_format_alias_match(&self) -> bool {
        self.use_api_format_alias_match
    }

    pub(crate) fn resolved_page_cache_model_directive_policy_hash(&self) -> &str {
        &self.model_directive_policy_cache_key
    }

    pub(crate) fn should_cache_current_priority_resolved_page(&self) -> bool {
        if !self.priority_page_emitted {
            return false;
        }

        match self.ordering_config.scheduling_mode {
            SchedulerSchedulingMode::FixedOrder => true,
            SchedulerSchedulingMode::CacheAffinity => {
                has_explicit_session_affinity(self.client_session_affinity.as_ref())
            }
            SchedulerSchedulingMode::LoadBalance => false,
        }
    }

    fn should_cache_current_priority_page(&self) -> bool {
        self.allow_priority_page_cache && self.should_cache_current_priority_resolved_page()
    }

    #[cfg(test)]
    pub(crate) fn mark_priority_page_emitted_for_tests(&mut self) {
        self.priority_page_emitted = true;
    }

    async fn cached_next_priority_page(
        &mut self,
    ) -> Result<
        AiCandidatePreselectionOutcome<
            SchedulerMinimalCandidateSelectionCandidate,
            SkippedLocalExecutionCandidate,
        >,
        GatewayError,
    > {
        let page = if self.should_cache_current_priority_page() {
            self.cached_next_priority_page_snapshot().await?
        } else {
            self.next_priority_page_with_planning_gate().await?
        };
        self.remember_seen_candidates_from_page(&page);
        Ok(page)
    }

    async fn cached_next_priority_page_snapshot(
        &mut self,
    ) -> Result<
        AiCandidatePreselectionOutcome<
            SchedulerMinimalCandidateSelectionCandidate,
            SkippedLocalExecutionCandidate,
        >,
        GatewayError,
    > {
        let key = CandidatePageCacheKey::new(
            &self.requested_model,
            self.request_operation.as_deref(),
            &self.client_api_format,
            self.require_streaming,
            &self.auth_snapshot,
            self.required_capabilities.as_ref(),
            self.routing_policy.as_ref(),
            self.request_auth_channel.as_deref(),
            self.state.app().scheduler_affinity_epoch(),
            self.key_mode.cache_key_name(),
            self.use_api_format_alias_match,
            self.client_session_affinity.as_ref(),
            &self.model_directive_policy_cache_key,
        );
        let cache = self.state.app().candidate_page_cache.clone();
        let ttl = candidate_page_cache_ttl_from_env();
        let stale_ttl = candidate_page_cache_stale_ttl(ttl);
        let cached = cache
            .get_or_load_once_stale_while_refreshing(
                key,
                ttl,
                stale_ttl,
                || async {
                    let page = self.next_priority_page_with_planning_gate().await?;
                    Ok::<_, GatewayError>(Some(Arc::new(page) as Arc<CandidatePageSnapshot>))
                },
                CacheLoadObserver::new()
                    .on_hit(record_candidate_page_cache_hit)
                    .on_miss(record_candidate_page_cache_miss)
                    .on_load(record_candidate_page_cache_load)
                    .on_follower_wait(record_candidate_page_cache_follower_wait),
            )
            .await?;

        match cached {
            Some(snapshot) => {
                let page = snapshot.as_ref().clone();
                if page.candidates.is_empty() && page.skipped_candidates.is_empty() {
                    record_candidate_page_cache_none();
                }
                Ok(page)
            }
            None => {
                record_candidate_page_cache_none();
                Ok(AiCandidatePreselectionOutcome {
                    candidates: Vec::new(),
                    skipped_candidates: Vec::new(),
                })
            }
        }
    }

    fn remember_seen_candidates_from_page(
        &mut self,
        page: &AiCandidatePreselectionOutcome<
            SchedulerMinimalCandidateSelectionCandidate,
            SkippedLocalExecutionCandidate,
        >,
    ) {
        for candidate in &page.candidates {
            self.seen_candidate_keys
                .insert(local_candidate_preselection_key(candidate, self.key_mode));
        }
        for skipped_candidate in &page.skipped_candidates {
            self.seen_candidate_keys
                .insert(local_candidate_preselection_key(
                    &skipped_candidate.candidate,
                    self.key_mode,
                ));
        }
    }

    async fn next_priority_page(
        &mut self,
    ) -> Result<
        AiCandidatePreselectionOutcome<
            SchedulerMinimalCandidateSelectionCandidate,
            SkippedLocalExecutionCandidate,
        >,
        GatewayError,
    > {
        let mut priority_page = AiCandidatePreselectionOutcome {
            candidates: Vec::new(),
            skipped_candidates: Vec::new(),
        };

        let candidate_api_format = self.candidate_api_format.clone();
        if let Some(outcome) = self.next_page_for_api_format(&candidate_api_format).await? {
            priority_page.candidates.extend(outcome.candidates);
            priority_page
                .skipped_candidates
                .extend(outcome.skipped_candidates);
        }

        Ok(priority_page)
    }

    async fn next_priority_page_with_planning_gate(
        &mut self,
    ) -> Result<
        AiCandidatePreselectionOutcome<
            SchedulerMinimalCandidateSelectionCandidate,
            SkippedLocalExecutionCandidate,
        >,
        GatewayError,
    > {
        let _permit = acquire_candidate_planning_gate(self.state, &self.trace_id).await?;
        self.next_priority_page().await
    }

    async fn next_page_for_api_format(
        &mut self,
        candidate_api_format: &str,
    ) -> Result<
        Option<
            AiCandidatePreselectionOutcome<
                SchedulerMinimalCandidateSelectionCandidate,
                SkippedLocalExecutionCandidate,
            >,
        >,
        GatewayError,
    > {
        let normalized_api_format = normalize_api_format(candidate_api_format);
        if normalized_api_format.is_empty() {
            return Ok(None);
        }
        if self.exhausted_api_formats.contains(&normalized_api_format) {
            return Ok(None);
        }
        let routing_model = self.routing_model(candidate_api_format).to_string();
        let requested_names = requested_model_candidate_names(&routing_model, false);
        let scanned = *self
            .scanned_rows_by_format
            .get(&normalized_api_format)
            .unwrap_or(&0);
        if scanned >= REQUESTED_MODEL_MAX_SCANNED_ROWS {
            self.exhausted_api_formats
                .insert(normalized_api_format.clone());
            return Ok(None);
        }

        loop {
            let requested_name_index = *self
                .requested_name_indexes
                .entry(normalized_api_format.clone())
                .or_insert(0);
            let Some(requested_name) = requested_names.get(requested_name_index) else {
                return self
                    .next_fallback_page_for_api_format(candidate_api_format, &normalized_api_format)
                    .await;
            };
            if requested_name.trim().is_empty() {
                self.requested_name_indexes
                    .insert(normalized_api_format.clone(), requested_name_index + 1);
                continue;
            }

            let offset_key = format!("{normalized_api_format}:{requested_name_index}");
            let offset = *self
                .requested_name_offsets
                .entry(offset_key.clone())
                .or_insert(0);
            let scanned = *self
                .scanned_rows_by_format
                .get(&normalized_api_format)
                .unwrap_or(&0);
            let remaining = REQUESTED_MODEL_MAX_SCANNED_ROWS.saturating_sub(scanned);
            if remaining == 0 {
                self.exhausted_api_formats
                    .insert(normalized_api_format.clone());
                return Ok(None);
            }
            let limit = REQUESTED_MODEL_CANDIDATE_PAGE_SIZE.min(remaining);
            let page = self
                .read_requested_model_rows_fast_path_page_cached(
                    &normalized_api_format,
                    requested_name,
                    &routing_model,
                    offset,
                    limit,
                )
                .await?;
            self.scanned_rows_by_format.insert(
                normalized_api_format.clone(),
                scanned.saturating_add(page.scanned_rows),
            );
            self.requested_name_offsets
                .insert(offset_key, offset.saturating_add(limit));
            if page.end_of_requested_name {
                self.requested_name_indexes
                    .insert(normalized_api_format.clone(), requested_name_index + 1);
            }
            if page.scanned_rows == 0 {
                if requested_name_index + 1 >= requested_names.len() {
                    return self
                        .next_fallback_page_for_api_format(
                            candidate_api_format,
                            &normalized_api_format,
                        )
                        .await;
                }
                continue;
            }

            if let Some(outcome) = self
                .build_page_outcome_from_rows(
                    candidate_api_format,
                    &normalized_api_format,
                    page.rows,
                )
                .await?
            {
                return Ok(Some(outcome));
            }
        }
    }

    async fn read_requested_model_rows_fast_path_page_cached(
        &self,
        normalized_api_format: &str,
        requested_name: &str,
        routing_model: &str,
        offset: u32,
        limit: u32,
    ) -> Result<RequestedModelCandidateRowsPage, GatewayError> {
        let key = CandidateRowPageCacheKey::new(
            normalized_api_format,
            routing_model,
            requested_name,
            offset,
            limit,
            false,
        );
        let cache = self.state.app().candidate_row_page_cache.clone();
        let ttl = candidate_page_cache_ttl_from_env();
        let stale_ttl = candidate_page_cache_stale_ttl(ttl);
        let cached = cache
            .get_or_load_once_stale_while_refreshing(
                key,
                ttl,
                stale_ttl,
                || async {
                    let page = read_requested_model_rows_fast_path_page(
                        self.state.app().data.as_ref(),
                        normalized_api_format,
                        routing_model,
                        requested_name,
                        offset,
                        limit,
                        false,
                    )
                    .await
                    .map_err(|err| GatewayError::Internal(err.to_string()))?;
                    Ok::<_, GatewayError>(Some(Arc::new(page)))
                },
                CacheLoadObserver::new()
                    .on_hit(record_candidate_row_page_cache_hit)
                    .on_miss(record_candidate_row_page_cache_miss)
                    .on_load(record_candidate_row_page_cache_load)
                    .on_follower_wait(record_candidate_row_page_cache_follower_wait),
            )
            .await?;

        match cached {
            Some(page) => {
                if page.rows.is_empty() {
                    record_candidate_row_page_cache_none();
                }
                Ok(page.as_ref().clone())
            }
            None => {
                record_candidate_row_page_cache_none();
                Ok(RequestedModelCandidateRowsPage {
                    rows: Vec::new(),
                    scanned_rows: 0,
                    end_of_requested_name: true,
                })
            }
        }
    }

    async fn read_api_format_rows_fallback_page_cached(
        &self,
        normalized_api_format: &str,
        offset: u32,
        limit: u32,
    ) -> Result<RequestedModelCandidateRowsPage, GatewayError> {
        let key = CandidateRowPageCacheKey::for_api_format_fallback(
            normalized_api_format,
            offset,
            limit,
            self.fallback_scan_epoch,
        );
        let cache = self.state.app().candidate_row_page_cache.clone();
        let ttl = candidate_page_cache_ttl_from_env();
        let stale_ttl = candidate_page_cache_stale_ttl(ttl);
        let cached = cache
            .get_or_load_once_stale_while_refreshing(
                key,
                ttl,
                stale_ttl,
                || async {
                    let page = read_api_format_rows_fallback_page(
                        self.state.app().data.as_ref(),
                        normalized_api_format,
                        offset,
                        limit,
                    )
                    .await
                    .map_err(|err| GatewayError::Internal(err.to_string()))?;
                    Ok::<_, GatewayError>(Some(Arc::new(page)))
                },
                CacheLoadObserver::new()
                    .on_hit(record_candidate_row_page_cache_hit)
                    .on_miss(record_candidate_row_page_cache_miss)
                    .on_load(record_candidate_row_page_cache_load)
                    .on_follower_wait(record_candidate_row_page_cache_follower_wait),
            )
            .await?;

        match cached {
            Some(page) => {
                if page.rows.is_empty() {
                    record_candidate_row_page_cache_none();
                }
                Ok(page.as_ref().clone())
            }
            None => {
                record_candidate_row_page_cache_none();
                Ok(RequestedModelCandidateRowsPage {
                    rows: Vec::new(),
                    scanned_rows: 0,
                    end_of_requested_name: true,
                })
            }
        }
    }

    async fn next_fallback_page_for_api_format(
        &mut self,
        candidate_api_format: &str,
        normalized_api_format: &str,
    ) -> Result<
        Option<
            AiCandidatePreselectionOutcome<
                SchedulerMinimalCandidateSelectionCandidate,
                SkippedLocalExecutionCandidate,
            >,
        >,
        GatewayError,
    > {
        let routing_model = self.routing_model(candidate_api_format).to_string();
        loop {
            let scanned = *self
                .scanned_rows_by_format
                .get(normalized_api_format)
                .unwrap_or(&0);
            let remaining = REQUESTED_MODEL_MAX_SCANNED_ROWS.saturating_sub(scanned);
            if remaining == 0 {
                self.exhausted_api_formats
                    .insert(normalized_api_format.to_string());
                return Ok(None);
            }
            let limit = REQUESTED_MODEL_CANDIDATE_PAGE_SIZE.min(remaining);
            let offset = *self
                .fallback_offsets
                .get(normalized_api_format)
                .unwrap_or(&0);
            let page = self
                .read_api_format_rows_fallback_page_cached(normalized_api_format, offset, limit)
                .await?;
            let page_scanned = page.scanned_rows.min(limit);
            let end_of_format = page.end_of_requested_name || page_scanned < limit;
            self.fallback_offsets.insert(
                normalized_api_format.to_string(),
                offset.saturating_add(page_scanned),
            );
            let total_scanned = scanned.saturating_add(page_scanned);
            self.scanned_rows_by_format
                .insert(normalized_api_format.to_string(), total_scanned);
            if end_of_format || total_scanned >= REQUESTED_MODEL_MAX_SCANNED_ROWS {
                self.exhausted_api_formats
                    .insert(normalized_api_format.to_string());
            }
            if page_scanned == 0 {
                return Ok(None);
            }

            let rows = page
                .rows
                .into_iter()
                .take(page_scanned as usize)
                .filter(|row| {
                    row_supports_requested_model_with_model_directives_and_request_operation(
                        row,
                        &routing_model,
                        normalized_api_format,
                        false,
                        self.request_operation.as_deref(),
                    )
                })
                .collect::<Vec<_>>();
            if let Some(outcome) = self
                .build_page_outcome_from_rows(candidate_api_format, normalized_api_format, rows)
                .await?
            {
                return Ok(Some(outcome));
            }
            if self.exhausted_api_formats.contains(normalized_api_format) {
                return Ok(None);
            }
        }
    }

    fn api_format_is_exhausted(&self, candidate_api_format: &str) -> bool {
        let normalized_api_format = normalize_api_format(candidate_api_format);
        normalized_api_format.is_empty()
            || self.exhausted_api_formats.contains(&normalized_api_format)
    }

    async fn build_page_outcome_from_rows(
        &mut self,
        candidate_api_format: &str,
        normalized_api_format: &str,
        rows: Vec<StoredMinimalCandidateSelectionRow>,
    ) -> Result<
        Option<
            AiCandidatePreselectionOutcome<
                SchedulerMinimalCandidateSelectionCandidate,
                SkippedLocalExecutionCandidate,
            >,
        >,
        GatewayError,
    > {
        let mut rows = rows
            .into_iter()
            .filter(|row| {
                self.seen_candidate_keys.insert(format!(
                    "{}:{}:{}:{}",
                    row.endpoint_id, row.key_id, row.model_id, row.endpoint_api_format
                ))
            })
            .collect::<Vec<_>>();
        if rows.is_empty() {
            return Ok(None);
        }
        let routing_model = self.routing_model(candidate_api_format).to_string();
        let resolved_global_model_name =
            if let Some(value) = self.resolved_global_model_names.get(normalized_api_format) {
                value.clone()
            } else {
                let Some(value) =
                    resolve_requested_global_model_name_with_model_directives_and_request_operation(
                        &rows,
                        &routing_model,
                        normalized_api_format,
                        false,
                        self.request_operation.as_deref(),
                    )
                else {
                    return Ok(None);
                };
                self.resolved_global_model_names
                    .insert(normalized_api_format.to_string(), value.clone());
                value
            };
        rows.retain(|row| row.global_model_name == resolved_global_model_name);
        if rows.is_empty() {
            return Ok(None);
        }

        let auth_constraints = Some(crate::data::candidate_selection::auth_snapshot_constraints(
            &self.auth_snapshot,
        ));
        let enumerated_candidates = enumerate_minimal_candidate_selection_with_model_directives(
            EnumerateMinimalCandidateSelectionInput {
                rows,
                normalized_api_format,
                request_operation: self.request_operation.as_deref(),
                requested_model_name: &routing_model,
                resolved_global_model_name: resolved_global_model_name.as_str(),
                require_streaming: self.require_streaming,
                required_capabilities: self.required_capabilities.as_ref(),
                auth_constraints: auth_constraints.as_ref(),
            },
            false,
        )
        .map_err(|err| GatewayError::Internal(err.to_string()))?;
        let mut candidates = Vec::new();
        for candidate in enumerated_candidates {
            if !self.candidate_allowed_for_page(&candidate, candidate_api_format) {
                continue;
            }
            if !self
                .seen_candidate_keys
                .insert(local_candidate_preselection_key(&candidate, self.key_mode))
            {
                continue;
            }
            candidates.push(candidate);
        }

        let (candidates, skipped_candidates) = self
            .state
            .list_selectable_enumerated_candidates_with_skip_reasons(
                candidate_api_format,
                &resolved_global_model_name,
                candidates,
                self.required_capabilities.as_ref(),
                Some(&self.auth_snapshot),
                self.routing_policy
                    .is_none()
                    .then_some(self.client_session_affinity.as_ref())
                    .flatten(),
                self.ranking_seed,
            )
            .await?;
        let skipped_candidates = skipped_candidates
            .into_iter()
            .map(skipped_local_execution_candidate_from_scheduler_skip)
            .filter(|skipped_candidate| {
                self.skipped_candidate_allowed_for_page(skipped_candidate, candidate_api_format)
            })
            .collect::<Vec<_>>();

        Ok(Some(AiCandidatePreselectionOutcome {
            candidates,
            skipped_candidates,
        }))
    }

    fn candidate_allowed_for_page(
        &self,
        candidate: &SchedulerMinimalCandidateSelectionCandidate,
        candidate_api_format: &str,
    ) -> bool {
        let _ = candidate_api_format;
        routing_policy_allows_provider(self.routing_policy.as_ref(), candidate)
    }

    fn skipped_candidate_allowed_for_page(
        &self,
        skipped_candidate: &SkippedLocalExecutionCandidate,
        candidate_api_format: &str,
    ) -> bool {
        let _ = candidate_api_format;
        routing_policy_allows_provider(self.routing_policy.as_ref(), &skipped_candidate.candidate)
    }
}

fn skipped_local_execution_candidate_from_scheduler_skip(
    skipped_candidate: SchedulerSkippedCandidate,
) -> SkippedLocalExecutionCandidate {
    SkippedLocalExecutionCandidate {
        candidate: skipped_candidate.candidate,
        skip_reason: skipped_candidate.skip_reason,
        transport: None,
        ranking: None,
        extra_data: None,
    }
}

fn local_candidate_preselection_key(
    candidate: &SchedulerMinimalCandidateSelectionCandidate,
    mode: LocalCandidatePreselectionKeyMode,
) -> String {
    match mode {
        LocalCandidatePreselectionKeyMode::ProviderEndpointKeyModel => format!(
            "{}:{}:{}:{}:{}",
            candidate.provider_id,
            candidate.endpoint_id,
            candidate.key_id,
            candidate.model_id,
            candidate.selected_provider_model_name,
        ),
        LocalCandidatePreselectionKeyMode::ProviderEndpointKeyModelAndApiFormat => format!(
            "{}:{}:{}:{}:{}:{}",
            candidate.provider_id,
            candidate.endpoint_id,
            candidate.key_id,
            candidate.model_id,
            candidate.selected_provider_model_name,
            candidate.endpoint_api_format,
        ),
    }
}

async fn acquire_candidate_planning_gate(
    state: PlannerAppState<'_>,
    trace_id: &str,
) -> Result<Option<ConcurrencyPermit>, GatewayError> {
    let Some(gate) = state.app().candidate_planning_gate.as_ref() else {
        return Ok(None);
    };
    let budget = state
        .app()
        .frontdoor_runtime_guards
        .internal_gate_queue_budget;
    let gate_wait_started_at = std::time::Instant::now();
    match tokio::time::timeout(budget, gate.acquire()).await {
        Ok(Ok(permit)) => {
            observe_gateway_stage_ms(
                "candidate_planning_gate_wait",
                gate_wait_started_at.elapsed().as_millis() as u64,
            );
            Ok(Some(permit))
        }
        Ok(Err(err)) => Err(GatewayError::Internal(err.to_string())),
        Err(_) => Err(GatewayError::AdmissionTimeout {
            trace_id: trace_id.to_string(),
            gate: "gateway_candidate_planning",
            queue_budget_ms: budget.as_millis() as u64,
        }),
    }
}

fn routing_policy_allows_provider(
    routing_policy: Option<&ResolvedRoutingPolicy>,
    candidate: &SchedulerMinimalCandidateSelectionCandidate,
) -> bool {
    match routing_policy {
        Some(policy) => policy
            .ranking_overlay
            .provider_allowed(candidate.provider_id.as_str()),
        None => true,
    }
}
