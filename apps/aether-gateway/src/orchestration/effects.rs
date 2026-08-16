use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex as StdMutex, Weak};
use std::time::Duration;

use aether_cache::ExpiringMap;
use aether_contracts::ExecutionPlan;
use aether_data_contracts::repository::provider_catalog::{
    ProviderCatalogKeyAdaptiveState, ProviderCatalogKeyAdaptiveStateUpdate,
    ProviderCatalogKeyHealthStateUpdate,
};
use aether_scheduler_core::{
    build_scheduler_affinity_cache_key_for_api_key_id_with_client_session_and_scope,
    count_recent_rpm_requests_for_provider_key, ClientSessionAffinity, SchedulerAffinityTarget,
};
use serde_json::Value;
use tokio::sync::Mutex as TokioMutex;
use tracing::warn;

use super::{
    classify_failure_disposition, project_local_adaptive_rate_limit,
    project_local_adaptive_success, project_local_failure_health, project_local_key_circuit_closed,
    project_local_key_circuit_failure, project_local_success_health, FailureScope,
    LocalFailoverClassification,
};
use crate::client_session_affinity::{
    client_session_affinity_from_report_context_value, CLIENT_SESSION_AFFINITY_REPORT_CONTEXT_FIELD,
};
use crate::clock::current_unix_secs;
use crate::orchestration::local_execution_candidate_metadata_from_report_context;
use crate::scheduler::affinity::{
    scheduler_affinity_policy_context_from_report_context, SCHEDULER_AFFINITY_POLICY_REPORT_FIELD,
    SCHEDULER_AFFINITY_TTL,
};
use crate::scheduler::config::{read_scheduler_ordering_config, SchedulerSchedulingMode};
use crate::AppState;

const HEALTH_SUCCESS_PERSIST_GATE_MAX_ENTRIES: usize = 50_000;
const ADAPTIVE_SUCCESS_PERSIST_GATE_MAX_ENTRIES: usize = 50_000;
const HEALTH_SUCCESS_PERSIST_MIN_INTERVAL_ENV: &str =
    "AETHER_GATEWAY_PROVIDER_KEY_HEALTH_SUCCESS_PERSIST_MIN_INTERVAL_SECS";
const ADAPTIVE_SUCCESS_PERSIST_MIN_INTERVAL_ENV: &str =
    "AETHER_GATEWAY_PROVIDER_KEY_ADAPTIVE_SUCCESS_PERSIST_MIN_INTERVAL_SECS";
const DEFAULT_HEALTH_SUCCESS_PERSIST_MIN_INTERVAL_SECS: u64 = 5;
const DEFAULT_ADAPTIVE_SUCCESS_PERSIST_MIN_INTERVAL_SECS: u64 = 5;
const MAX_EFFECT_PERSIST_MIN_INTERVAL_SECS: u64 = 300;
const PROVIDER_KEY_EFFECT_LOCK_PRUNE_THRESHOLD: usize = 8_192;
// Same-process writers are serialized by the per-key lock. Keep remote-writer
// retries bounded so request/report completion cannot accumulate a long DB tail.
const PROVIDER_KEY_STATE_CAS_MAX_ATTEMPTS: usize = 4;

#[derive(Debug)]
struct ProviderKeyEffectLockRegistryState {
    entries: HashMap<String, Weak<TokioMutex<()>>>,
    accesses_since_prune: usize,
    next_growth_prune_at: usize,
    #[cfg(test)]
    prune_count: usize,
}

impl ProviderKeyEffectLockRegistryState {
    fn new(min_prune_threshold: usize) -> Self {
        Self {
            entries: HashMap::new(),
            accesses_since_prune: 0,
            next_growth_prune_at: min_prune_threshold,
            #[cfg(test)]
            prune_count: 0,
        }
    }
}

#[derive(Debug)]
struct ProviderKeyEffectLockRegistry {
    state: StdMutex<ProviderKeyEffectLockRegistryState>,
    min_prune_threshold: usize,
}

impl Default for ProviderKeyEffectLockRegistry {
    fn default() -> Self {
        Self::new(PROVIDER_KEY_EFFECT_LOCK_PRUNE_THRESHOLD)
    }
}

impl ProviderKeyEffectLockRegistry {
    fn new(min_prune_threshold: usize) -> Self {
        let min_prune_threshold = min_prune_threshold.max(1);
        Self {
            state: StdMutex::new(ProviderKeyEffectLockRegistryState::new(min_prune_threshold)),
            min_prune_threshold,
        }
    }

    fn lock_for(&self, key_id: &str) -> Arc<TokioMutex<()>> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.accesses_since_prune = state.accesses_since_prune.saturating_add(1);
        let entry_count = state.entries.len();
        let growth_prune_due = entry_count >= state.next_growth_prune_at;
        let maintenance_prune_due = entry_count >= self.min_prune_threshold
            && state.accesses_since_prune >= entry_count.max(self.min_prune_threshold);
        if growth_prune_due || maintenance_prune_due {
            self.prune_inactive_locks(&mut state);
        }

        if let Some(existing) = state.entries.get(key_id).and_then(Weak::upgrade) {
            return existing;
        }
        let lock = Arc::new(TokioMutex::new(()));
        state
            .entries
            .insert(key_id.to_string(), Arc::downgrade(&lock));
        lock
    }

    fn prune_inactive_locks(&self, state: &mut ProviderKeyEffectLockRegistryState) {
        state.entries.retain(|_, lock| lock.strong_count() > 0);
        let active_entries = state.entries.len();
        state.next_growth_prune_at = if active_entries < self.min_prune_threshold {
            self.min_prune_threshold
        } else {
            active_entries.saturating_mul(2)
        };
        state.accesses_since_prune = 0;
        #[cfg(test)]
        {
            state.prune_count = state.prune_count.saturating_add(1);
        }
    }
}

static HEALTH_SUCCESS_PERSIST_GATE: LazyLock<ExpiringMap<String, ()>> =
    LazyLock::new(ExpiringMap::new);
static ADAPTIVE_SUCCESS_PERSIST_GATE: LazyLock<ExpiringMap<String, u64>> =
    LazyLock::new(ExpiringMap::new);
static ADAPTIVE_SUCCESS_PERSIST_GATE_NEXT_TOKEN: AtomicU64 = AtomicU64::new(1);
static PROVIDER_KEY_EFFECT_LOCKS: LazyLock<ProviderKeyEffectLockRegistry> =
    LazyLock::new(ProviderKeyEffectLockRegistry::default);
static HEALTH_SUCCESS_PERSIST_MIN_INTERVAL: LazyLock<Duration> = LazyLock::new(|| {
    effect_persist_interval_from_env(
        HEALTH_SUCCESS_PERSIST_MIN_INTERVAL_ENV,
        DEFAULT_HEALTH_SUCCESS_PERSIST_MIN_INTERVAL_SECS,
    )
});
static ADAPTIVE_SUCCESS_PERSIST_MIN_INTERVAL: LazyLock<Duration> = LazyLock::new(|| {
    effect_persist_interval_from_env(
        ADAPTIVE_SUCCESS_PERSIST_MIN_INTERVAL_ENV,
        DEFAULT_ADAPTIVE_SUCCESS_PERSIST_MIN_INTERVAL_SECS,
    )
});

fn effect_persist_interval_from_env(key: &str, default_secs: u64) -> Duration {
    let secs = std::env::var(key)
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .unwrap_or(default_secs)
        .min(MAX_EFFECT_PERSIST_MIN_INTERVAL_SECS);
    Duration::from_secs(secs)
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct LocalExecutionEffectContext<'a> {
    pub(crate) plan: &'a ExecutionPlan,
    pub(crate) report_context: Option<&'a Value>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct LocalAttemptFailureEffect {
    pub(crate) status_code: u16,
    pub(crate) classification: LocalFailoverClassification,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct LocalAdaptiveRateLimitEffect<'a> {
    pub(crate) status_code: u16,
    pub(crate) classification: LocalFailoverClassification,
    pub(crate) headers: Option<&'a BTreeMap<String, String>>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct LocalHealthFailureEffect {
    pub(crate) status_code: u16,
    pub(crate) classification: LocalFailoverClassification,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct LocalHealthSuccessEffect;

#[derive(Debug, Clone, Copy)]
pub(crate) struct LocalAdaptiveSuccessEffect;

#[derive(Debug, Clone, Copy)]
pub(crate) enum LocalExecutionEffect<'a> {
    AttemptFailure(LocalAttemptFailureEffect),
    AdaptiveRateLimit(LocalAdaptiveRateLimitEffect<'a>),
    HealthFailure(LocalHealthFailureEffect),
    HealthSuccess(LocalHealthSuccessEffect),
    AdaptiveSuccess(LocalAdaptiveSuccessEffect),
}

const ADAPTIVE_RPM_RECENT_CANDIDATE_LIMIT: usize = 512;
const LOCAL_EXECUTION_SCHEDULER_AFFINITY_MAX_ENTRIES: usize = 10_000;

pub(crate) async fn apply_local_execution_effect(
    state: &AppState,
    context: LocalExecutionEffectContext<'_>,
    effect: LocalExecutionEffect<'_>,
) {
    match effect {
        LocalExecutionEffect::AttemptFailure(effect) => {
            record_attempt_failure_effect(state, context, effect).await;
        }
        LocalExecutionEffect::AdaptiveRateLimit(effect) => {
            record_adaptive_rate_limit_effect(state, context, effect).await;
        }
        LocalExecutionEffect::HealthFailure(effect) => {
            record_health_failure_effect(state, context, effect).await;
        }
        LocalExecutionEffect::HealthSuccess(effect) => {
            record_health_success_effect(state, context, effect).await;
        }
        LocalExecutionEffect::AdaptiveSuccess(effect) => {
            record_adaptive_success_effect(state, context, effect).await;
        }
    }
}

fn report_context_string_field<'a>(
    report_context: Option<&'a Value>,
    field: &str,
) -> Option<&'a str> {
    report_context
        .and_then(|context| context.get(field))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn report_context_u64_field(report_context: Option<&Value>, field: &str) -> Option<u64> {
    report_context
        .and_then(|context| context.get(field))
        .and_then(|value| {
            value
                .as_u64()
                .or_else(|| value.as_str().and_then(|value| value.parse::<u64>().ok()))
        })
}

fn local_scheduler_affinity_cache_key(report_context: Option<&Value>) -> Option<String> {
    let client_session_affinity = local_client_session_affinity(report_context);
    let policy_context = scheduler_affinity_policy_context_from_report_context(report_context);
    if report_context
        .and_then(|context| context.get(SCHEDULER_AFFINITY_POLICY_REPORT_FIELD))
        .is_some()
        && policy_context.is_none()
    {
        return None;
    }
    build_scheduler_affinity_cache_key_for_api_key_id_with_client_session_and_scope(
        report_context_string_field(report_context, "api_key_id")?,
        report_context_string_field(report_context, "client_api_format")?,
        report_context_string_field(report_context, "model")?,
        client_session_affinity.as_ref(),
        policy_context
            .as_ref()
            .and_then(|context| context.scope.as_ref()),
    )
}

fn local_client_session_affinity(report_context: Option<&Value>) -> Option<ClientSessionAffinity> {
    let report_context = report_context?;
    if let Some(affinity) = client_session_affinity_from_report_context_value(
        report_context.get(CLIENT_SESSION_AFFINITY_REPORT_CONTEXT_FIELD),
    ) {
        return Some(affinity);
    }

    let headers = header_map_from_report_context(report_context.get("original_headers"));
    let body_json = report_context
        .get("original_request_body")
        .filter(|value| !value.is_null());

    crate::client_session_affinity::client_session_affinity_from_api_request(
        report_context_string_field(Some(report_context), "client_api_format").unwrap_or_default(),
        &headers,
        body_json,
    )
}

fn header_map_from_report_context(headers: Option<&Value>) -> http::HeaderMap {
    let mut header_map = http::HeaderMap::new();
    let Some(headers) = headers.and_then(Value::as_object) else {
        return header_map;
    };

    for (name, value) in headers {
        let Some(value) = value.as_str() else {
            continue;
        };
        let Ok(name) = http::header::HeaderName::from_bytes(name.as_bytes()) else {
            continue;
        };
        let Ok(value) = http::HeaderValue::from_str(value) else {
            continue;
        };
        header_map.insert(name, value);
    }

    header_map
}

fn local_scheduler_affinity_target(plan: &ExecutionPlan) -> Option<SchedulerAffinityTarget> {
    let provider_id = plan.provider_id.trim();
    let endpoint_id = plan.endpoint_id.trim();
    let key_id = plan.key_id.trim();
    if provider_id.is_empty() || endpoint_id.is_empty() || key_id.is_empty() {
        return None;
    }

    Some(SchedulerAffinityTarget {
        provider_id: provider_id.to_string(),
        endpoint_id: endpoint_id.to_string(),
        key_id: key_id.to_string(),
    })
}

fn local_scheduler_affinity_matches_failed_target(
    cached_target: &SchedulerAffinityTarget,
    failed_target: &SchedulerAffinityTarget,
) -> bool {
    cached_target == failed_target
}

async fn scheduler_cache_affinity_enabled(
    state: &AppState,
    report_context: Option<&Value>,
) -> bool {
    if report_context
        .and_then(|context| context.get(SCHEDULER_AFFINITY_POLICY_REPORT_FIELD))
        .is_some()
    {
        return scheduler_affinity_policy_context_from_report_context(report_context)
            .is_some_and(|context| context.cache_affinity_enabled());
    }
    match read_scheduler_ordering_config(state).await {
        Ok(config) => config.scheduling_mode == SchedulerSchedulingMode::CacheAffinity,
        Err(error) => {
            warn!(
                event_name = "orchestration_scheduler_affinity_config_load_failed",
                log_type = "event",
                error = ?error,
                "failed to load scheduler config while checking cache affinity mode"
            );
            SchedulerSchedulingMode::default() == SchedulerSchedulingMode::CacheAffinity
        }
    }
}

async fn remember_successful_local_scheduler_affinity(
    state: &AppState,
    context: LocalExecutionEffectContext<'_>,
) {
    if !scheduler_cache_affinity_enabled(state, context.report_context).await {
        return;
    }
    let Some(cache_key) = local_scheduler_affinity_cache_key(context.report_context) else {
        return;
    };
    let Some(target) = local_scheduler_affinity_target(context.plan) else {
        return;
    };
    let expected_epoch =
        local_execution_candidate_metadata_from_report_context(context.report_context)
            .scheduler_affinity_epoch;

    let _ = state.remember_scheduler_affinity_target_for_epoch(
        &cache_key,
        target,
        SCHEDULER_AFFINITY_TTL,
        LOCAL_EXECUTION_SCHEDULER_AFFINITY_MAX_ENTRIES,
        expected_epoch,
    );
}

async fn record_attempt_failure_effect(
    state: &AppState,
    context: LocalExecutionEffectContext<'_>,
    effect: LocalAttemptFailureEffect,
) {
    if !local_candidate_failure_should_invalidate_affinity_for_provider(
        &context.plan.provider_api_format,
        effect.classification,
        effect.status_code,
    ) {
        return;
    }

    if let Some(cache_key) = local_scheduler_affinity_cache_key(context.report_context) {
        let Some(failed_target) = local_scheduler_affinity_target(context.plan) else {
            return;
        };
        let Some(cached_target) =
            state.read_scheduler_affinity_target(&cache_key, SCHEDULER_AFFINITY_TTL)
        else {
            return;
        };
        if local_scheduler_affinity_matches_failed_target(&cached_target, &failed_target) {
            let _ = state.remove_scheduler_affinity_cache_entry(&cache_key);
        }
    }
}

async fn record_adaptive_rate_limit_effect(
    state: &AppState,
    context: LocalExecutionEffectContext<'_>,
    effect: LocalAdaptiveRateLimitEffect<'_>,
) {
    if !local_candidate_failure_should_apply_key_effects(
        &context.plan.provider_api_format,
        effect.classification,
        effect.status_code,
    ) {
        return;
    }
    let effect_lock = PROVIDER_KEY_EFFECT_LOCKS.lock_for(&context.plan.key_id);
    let _effect_guard = effect_lock.lock().await;
    let observed_at_unix_secs = current_unix_secs();
    let current_rpm = state
        .read_recent_request_candidates(ADAPTIVE_RPM_RECENT_CANDIDATE_LIMIT)
        .await
        .ok()
        .map(|recent_candidates| {
            count_recent_rpm_requests_for_provider_key(
                &recent_candidates,
                &context.plan.key_id,
                observed_at_unix_secs,
            ) as u32
        });

    for _ in 0..PROVIDER_KEY_STATE_CAS_MAX_ATTEMPTS {
        let Some(current_key) = state
            .read_provider_catalog_keys_by_ids(std::slice::from_ref(&context.plan.key_id))
            .await
            .ok()
            .and_then(|mut keys| keys.drain(..).next())
        else {
            return;
        };
        let Some(projection) = project_local_adaptive_rate_limit(
            &current_key,
            effect.classification,
            effect.status_code,
            current_rpm,
            effect.headers,
            observed_at_unix_secs,
        ) else {
            return;
        };
        let expected = ProviderCatalogKeyAdaptiveState::from(&current_key);
        let mut next = expected.clone();
        next.rpm_429_count = Some(projection.rpm_429_count);
        next.learned_rpm_limit = projection.learned_rpm_limit;
        next.last_429_at_unix_secs = Some(projection.last_429_at_unix_secs);
        next.last_429_type = Some(projection.last_429_type);
        next.adjustment_history = projection.adjustment_history;
        next.utilization_samples = projection.utilization_samples;
        next.last_probe_increase_at_unix_secs = projection.last_probe_increase_at_unix_secs;
        next.last_rpm_peak = projection.last_rpm_peak;
        let update = ProviderCatalogKeyAdaptiveStateUpdate {
            key_id: context.plan.key_id.clone(),
            expected,
            next,
            status_snapshot_patch: adaptive_status_snapshot_patch(&projection.status_snapshot),
            updated_at_unix_secs: Some(observed_at_unix_secs),
        };
        provider_key_adaptive_success_persist_gate_reset(&context.plan.key_id);
        match state
            .compare_and_update_provider_catalog_key_adaptive_state(&update)
            .await
        {
            Ok(true) => return,
            Ok(false) => tokio::task::yield_now().await,
            Err(err) => {
                warn!(
                    "gateway orchestration effects: failed to persist adaptive rate-limit projection for provider {} endpoint {} key {}: {:?}",
                    context.plan.provider_id, context.plan.endpoint_id, context.plan.key_id, err
                );
                return;
            }
        }
    }
    warn!(
        "gateway orchestration effects: adaptive rate-limit CAS retries exhausted for provider {} endpoint {} key {}",
        context.plan.provider_id, context.plan.endpoint_id, context.plan.key_id
    );
}

fn adaptive_status_snapshot_patch(status_snapshot: &Value) -> Value {
    const OWNED_FIELDS: [&str; 6] = [
        "observation_count",
        "header_observation_count",
        "latest_upstream_limit",
        "learning_confidence",
        "enforcement_active",
        "known_boundary",
    ];
    let Some(snapshot) = status_snapshot.as_object() else {
        return serde_json::json!({});
    };
    Value::Object(
        OWNED_FIELDS
            .into_iter()
            .filter_map(|field| {
                snapshot
                    .get(field)
                    .cloned()
                    .map(|value| (field.to_string(), value))
            })
            .collect(),
    )
}

async fn record_adaptive_success_effect(
    state: &AppState,
    context: LocalExecutionEffectContext<'_>,
    _effect: LocalAdaptiveSuccessEffect,
) {
    let observed_at_unix_secs = current_unix_secs();
    let Some(current_key) = state
        .read_provider_catalog_keys_by_ids(std::slice::from_ref(&context.plan.key_id))
        .await
        .ok()
        .and_then(|mut keys| keys.drain(..).next())
    else {
        return;
    };
    if current_key.rpm_limit.is_some()
        || current_key
            .learned_rpm_limit
            .filter(|value| *value > 0)
            .is_none()
    {
        return;
    }
    let Some(gate_token) = provider_key_adaptive_success_persist_gate_admit(&context.plan.key_id)
    else {
        return;
    };

    let effect_lock = PROVIDER_KEY_EFFECT_LOCKS.lock_for(&context.plan.key_id);
    let _effect_guard = effect_lock.lock().await;
    if !provider_key_adaptive_success_persist_gate_admission_is_current(
        &context.plan.key_id,
        gate_token,
    ) {
        return;
    }
    let Some(recent_candidates) = state
        .read_recent_request_candidates(ADAPTIVE_RPM_RECENT_CANDIDATE_LIMIT)
        .await
        .ok()
    else {
        return;
    };
    let current_rpm = count_recent_rpm_requests_for_provider_key(
        &recent_candidates,
        &context.plan.key_id,
        observed_at_unix_secs,
    ) as u32;

    for _ in 0..PROVIDER_KEY_STATE_CAS_MAX_ATTEMPTS {
        let Some(current_key) = state
            .read_provider_catalog_keys_by_ids(std::slice::from_ref(&context.plan.key_id))
            .await
            .ok()
            .and_then(|mut keys| keys.drain(..).next())
        else {
            return;
        };
        if current_key.rpm_limit.is_some()
            || current_key
                .learned_rpm_limit
                .filter(|value| *value > 0)
                .is_none()
        {
            return;
        }
        let Some(projection) =
            project_local_adaptive_success(&current_key, current_rpm, observed_at_unix_secs)
        else {
            return;
        };
        let expected = ProviderCatalogKeyAdaptiveState::from(&current_key);
        let mut next = expected.clone();
        next.learned_rpm_limit = projection.learned_rpm_limit;
        next.adjustment_history = projection.adjustment_history;
        next.utilization_samples = projection.utilization_samples;
        next.last_probe_increase_at_unix_secs = projection.last_probe_increase_at_unix_secs;
        let update = ProviderCatalogKeyAdaptiveStateUpdate {
            key_id: context.plan.key_id.clone(),
            expected,
            next,
            status_snapshot_patch: adaptive_status_snapshot_patch(&projection.status_snapshot),
            updated_at_unix_secs: Some(observed_at_unix_secs),
        };
        match state
            .compare_and_update_provider_catalog_key_adaptive_state(&update)
            .await
        {
            Ok(true) => return,
            Ok(false) => tokio::task::yield_now().await,
            Err(err) => {
                warn!(
                    "gateway orchestration effects: failed to persist adaptive success projection for provider {} endpoint {} key {}: {:?}",
                    context.plan.provider_id, context.plan.endpoint_id, context.plan.key_id, err
                );
                return;
            }
        }
    }
    warn!(
        "gateway orchestration effects: adaptive success CAS retries exhausted for provider {} endpoint {} key {}",
        context.plan.provider_id, context.plan.endpoint_id, context.plan.key_id
    );
}

fn provider_key_adaptive_success_persist_gate_admit(key_id: &str) -> Option<u64> {
    if cfg!(test) {
        return Some(0);
    }
    let interval = *ADAPTIVE_SUCCESS_PERSIST_MIN_INTERVAL;
    if interval.is_zero() {
        return Some(0);
    }
    let token = ADAPTIVE_SUCCESS_PERSIST_GATE_NEXT_TOKEN.fetch_add(1, Ordering::Relaxed);
    ADAPTIVE_SUCCESS_PERSIST_GATE
        .insert_if_absent_fresh(
            provider_key_adaptive_success_persist_gate_key(key_id),
            token,
            interval,
            ADAPTIVE_SUCCESS_PERSIST_GATE_MAX_ENTRIES,
        )
        .then_some(token)
}

fn provider_key_adaptive_success_persist_gate_admission_is_current(
    key_id: &str,
    token: u64,
) -> bool {
    if cfg!(test) || ADAPTIVE_SUCCESS_PERSIST_MIN_INTERVAL.is_zero() {
        return true;
    }
    ADAPTIVE_SUCCESS_PERSIST_GATE.get_fresh(
        &provider_key_adaptive_success_persist_gate_key(key_id),
        *ADAPTIVE_SUCCESS_PERSIST_MIN_INTERVAL,
    ) == Some(token)
}

fn provider_key_adaptive_success_persist_gate_reset(key_id: &str) {
    ADAPTIVE_SUCCESS_PERSIST_GATE.remove(&provider_key_adaptive_success_persist_gate_key(key_id));
}

fn provider_key_adaptive_success_persist_gate_key(key_id: &str) -> String {
    format!("adaptive-success:{key_id}")
}

async fn record_health_failure_effect(
    state: &AppState,
    context: LocalExecutionEffectContext<'_>,
    effect: LocalHealthFailureEffect,
) {
    if !local_candidate_failure_should_apply_key_effects(
        &context.plan.provider_api_format,
        effect.classification,
        effect.status_code,
    ) {
        return;
    }
    let api_format = context.plan.provider_api_format.trim();
    if api_format.is_empty() {
        return;
    }
    let effect_lock = PROVIDER_KEY_EFFECT_LOCKS.lock_for(&context.plan.key_id);
    let _effect_guard = effect_lock.lock().await;
    let observed_at_unix_secs = current_unix_secs();
    provider_key_health_success_persist_gate_reset(&context.plan.key_id, api_format);

    for _ in 0..PROVIDER_KEY_STATE_CAS_MAX_ATTEMPTS {
        let Some(current_key) = state
            .read_provider_catalog_keys_by_ids(std::slice::from_ref(&context.plan.key_id))
            .await
            .ok()
            .and_then(|mut keys| keys.drain(..).next())
        else {
            return;
        };
        let Some(health_by_format) = project_local_failure_health(
            current_key.health_by_format.as_ref(),
            api_format,
            effect.classification,
            effect.status_code,
            observed_at_unix_secs,
        ) else {
            return;
        };
        let consecutive_failures = health_by_format
            .get(api_format)
            .and_then(|value| value.get("consecutive_failures"))
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let circuit_breaker_by_format = project_local_key_circuit_failure(
            current_key.circuit_breaker_by_format.as_ref(),
            api_format,
            observed_at_unix_secs,
            consecutive_failures,
            current_key.max_probe_interval_minutes,
        )
        .or_else(|| current_key.circuit_breaker_by_format.clone());
        let update = ProviderCatalogKeyHealthStateUpdate {
            key_id: context.plan.key_id.clone(),
            expected_health_by_format: current_key.health_by_format,
            expected_circuit_breaker_by_format: current_key.circuit_breaker_by_format,
            health_by_format: Some(health_by_format),
            circuit_breaker_by_format,
        };
        match state
            .compare_and_update_provider_catalog_key_health_state(&update)
            .await
        {
            Ok(true) => return,
            Ok(false) => tokio::task::yield_now().await,
            Err(err) => {
                warn!(
                    "gateway orchestration effects: failed to persist health failure projection for provider {} endpoint {} key {}: {:?}",
                    context.plan.provider_id, context.plan.endpoint_id, context.plan.key_id, err
                );
                return;
            }
        }
    }
    warn!(
        "gateway orchestration effects: health failure CAS retries exhausted for provider {} endpoint {} key {}",
        context.plan.provider_id, context.plan.endpoint_id, context.plan.key_id
    );
}

async fn record_health_success_effect(
    state: &AppState,
    context: LocalExecutionEffectContext<'_>,
    _effect: LocalHealthSuccessEffect,
) {
    remember_successful_local_scheduler_affinity(state, context).await;

    let api_format = context.plan.provider_api_format.trim();
    if api_format.is_empty() {
        return;
    }
    // Health updates replace both JSON snapshots in one write. Serialize the success
    // read/project/write with failure and circuit-clear effects for this provider key so a
    // stale success snapshot cannot overwrite a newer failure counter or open circuit.
    let effect_lock = PROVIDER_KEY_EFFECT_LOCKS.lock_for(&context.plan.key_id);
    let _effect_guard = effect_lock.lock().await;

    let mut persist_gate_checked = false;

    for _ in 0..PROVIDER_KEY_STATE_CAS_MAX_ATTEMPTS {
        let Some(current_key) = state
            .read_provider_catalog_keys_by_ids(std::slice::from_ref(&context.plan.key_id))
            .await
            .ok()
            .and_then(|mut keys| keys.drain(..).next())
        else {
            return;
        };
        let Some(health_by_format) =
            project_local_success_health(current_key.health_by_format.as_ref(), api_format)
        else {
            return;
        };
        let circuit_breaker_update_owned = current_key
            .circuit_breaker_by_format
            .as_ref()
            .and_then(|current| project_local_key_circuit_closed(Some(current), api_format));
        if current_key.health_by_format.as_ref() == Some(&health_by_format)
            && circuit_breaker_update_owned.as_ref()
                == current_key.circuit_breaker_by_format.as_ref()
        {
            return;
        }
        if !persist_gate_checked {
            if !provider_key_health_success_persist_gate_allows(
                &context.plan.key_id,
                api_format,
                circuit_breaker_update_owned.is_some(),
            ) {
                return;
            }
            persist_gate_checked = true;
        }
        let circuit_breaker_by_format =
            circuit_breaker_update_owned.or_else(|| current_key.circuit_breaker_by_format.clone());
        let update = ProviderCatalogKeyHealthStateUpdate {
            key_id: context.plan.key_id.clone(),
            expected_health_by_format: current_key.health_by_format,
            expected_circuit_breaker_by_format: current_key.circuit_breaker_by_format,
            health_by_format: Some(health_by_format),
            circuit_breaker_by_format,
        };
        match state
            .compare_and_update_provider_catalog_key_health_state(&update)
            .await
        {
            Ok(true) => return,
            Ok(false) => tokio::task::yield_now().await,
            Err(err) => {
                warn!(
                    "gateway orchestration effects: failed to persist health success projection for provider {} endpoint {} key {}: {:?}",
                    context.plan.provider_id, context.plan.endpoint_id, context.plan.key_id, err
                );
                return;
            }
        }
    }
    warn!(
        "gateway orchestration effects: health success CAS retries exhausted for provider {} endpoint {} key {}",
        context.plan.provider_id, context.plan.endpoint_id, context.plan.key_id
    );
}

fn provider_key_health_success_persist_gate_allows(
    key_id: &str,
    api_format: &str,
    closes_circuit: bool,
) -> bool {
    if closes_circuit {
        return true;
    }
    let min_interval = *HEALTH_SUCCESS_PERSIST_MIN_INTERVAL;
    if min_interval.is_zero() {
        return true;
    }
    let key = provider_key_health_success_persist_gate_key(key_id, api_format);
    HEALTH_SUCCESS_PERSIST_GATE.insert_if_absent_fresh(
        key,
        (),
        min_interval,
        HEALTH_SUCCESS_PERSIST_GATE_MAX_ENTRIES,
    )
}

fn provider_key_health_success_persist_gate_reset(key_id: &str, api_format: &str) {
    let key = provider_key_health_success_persist_gate_key(key_id, api_format);
    HEALTH_SUCCESS_PERSIST_GATE.remove(&key);
}

fn provider_key_health_success_persist_gate_key(key_id: &str, api_format: &str) -> String {
    format!("success:{key_id}:{api_format}")
}

fn local_candidate_failure_should_invalidate_affinity(
    classification: LocalFailoverClassification,
    status_code: u16,
) -> bool {
    if status_code < 400 {
        return false;
    }

    match classification {
        LocalFailoverClassification::RetrySuccessPattern
        | LocalFailoverClassification::RetryStatusCode
        | LocalFailoverClassification::RetryUpstreamFailure => true,
        LocalFailoverClassification::UseDefault | LocalFailoverClassification::StopStatusCode => {
            status_code >= 500
        }
        LocalFailoverClassification::StopErrorPattern
        | LocalFailoverClassification::StopExecutionError
        | LocalFailoverClassification::StopCyberPolicy => false,
    }
}

fn local_candidate_failure_should_invalidate_affinity_for_provider(
    provider_api_format: &str,
    classification: LocalFailoverClassification,
    status_code: u16,
) -> bool {
    if !local_candidate_failure_should_invalidate_affinity(classification, status_code) {
        return false;
    }
    if !provider_api_format
        .trim()
        .eq_ignore_ascii_case("claude:messages")
    {
        return true;
    }

    let disposition =
        classify_failure_disposition(provider_api_format, classification, status_code);
    !(disposition.retry_action == crate::orchestration::FailureRetryAction::Stop
        && disposition.failure_scope == FailureScope::None)
}

fn local_candidate_failure_should_apply_key_effects(
    provider_api_format: &str,
    classification: LocalFailoverClassification,
    status_code: u16,
) -> bool {
    if !provider_api_format
        .trim()
        .eq_ignore_ascii_case("claude:messages")
    {
        return true;
    }

    matches!(
        classify_failure_disposition(provider_api_format, classification, status_code)
            .failure_scope,
        FailureScope::Credential
    )
}
