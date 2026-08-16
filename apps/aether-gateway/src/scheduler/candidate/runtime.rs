use std::collections::{BTreeMap, BTreeSet};

use aether_data_contracts::repository::candidates::StoredRequestCandidate;
use aether_data_contracts::repository::provider_catalog::StoredProviderCatalogKey;
use aether_scheduler_core::{
    auth_api_key_concurrency_limit_reached, build_provider_concurrent_limit_map,
    candidate_is_selectable_with_runtime_state, candidate_runtime_skip_reason_with_state,
    count_recent_active_requests_for_user, effective_provider_key_rpm_limit,
    CandidateRuntimeSelectabilityInput,
};

use crate::data::auth::GatewayAuthApiKeySnapshot;
use crate::GatewayError;

use super::{SchedulerMinimalCandidateSelectionCandidate, SchedulerRuntimeState};

pub(super) struct CandidateRuntimeSelectionSnapshot {
    pub(super) recent_candidates: Vec<StoredRequestCandidate>,
    pub(super) provider_concurrent_limits: BTreeMap<String, usize>,
    pub(super) provider_key_rpm_states: BTreeMap<String, StoredProviderCatalogKey>,
    provider_key_rpm_reset_ats: BTreeMap<String, Option<u64>>,
}

pub(super) async fn read_candidate_runtime_selection_snapshot(
    state: &(impl SchedulerRuntimeState + ?Sized),
    candidates: &[SchedulerMinimalCandidateSelectionCandidate],
    auth_snapshot: Option<&GatewayAuthApiKeySnapshot>,
    now_unix_secs: u64,
) -> Result<CandidateRuntimeSelectionSnapshot, GatewayError> {
    let provider_concurrent_limits = read_provider_concurrent_limits(state, candidates).await?;
    let provider_key_rpm_states = read_provider_key_rpm_states(state, candidates).await?;
    let recent_candidates = if runtime_snapshot_requires_recent_candidates(
        auth_snapshot,
        &provider_concurrent_limits,
        &provider_key_rpm_states,
        now_unix_secs,
    ) {
        state.read_recent_request_candidates(128).await?
    } else {
        Vec::new()
    };
    let provider_key_rpm_reset_ats =
        read_provider_key_rpm_reset_at_map(state, candidates, now_unix_secs);

    Ok(CandidateRuntimeSelectionSnapshot {
        recent_candidates,
        provider_concurrent_limits,
        provider_key_rpm_states,
        provider_key_rpm_reset_ats,
    })
}

fn runtime_snapshot_requires_recent_candidates(
    auth_snapshot: Option<&GatewayAuthApiKeySnapshot>,
    provider_concurrent_limits: &BTreeMap<String, usize>,
    provider_key_rpm_states: &BTreeMap<String, StoredProviderCatalogKey>,
    now_unix_secs: u64,
) -> bool {
    if auth_snapshot.is_some_and(auth_snapshot_has_concurrency_limit) {
        return true;
    }

    if provider_concurrent_limits.values().any(|limit| *limit > 0) {
        return true;
    }

    provider_key_rpm_states.values().any(|key| {
        key.concurrent_limit.is_some_and(|limit| limit > 0)
            || effective_provider_key_rpm_limit(key, now_unix_secs).is_some()
    })
}

pub(super) fn auth_snapshot_concurrency_limit_reached(
    auth_snapshot: Option<&GatewayAuthApiKeySnapshot>,
    snapshot: &CandidateRuntimeSelectionSnapshot,
    now_unix_secs: u64,
) -> bool {
    let Some(auth) = auth_snapshot else {
        return false;
    };
    let principal_limit = auth
        .admission_policy
        .principal
        .concurrent_requests()
        .unwrap_or_default();
    let key_limit = auth.admission_policy.api_key.concurrent_requests();

    if auth.api_key_is_standalone {
        let limit = key_limit.unwrap_or(principal_limit) as usize;
        return limit > 0
            && auth_api_key_concurrency_limit_reached(
                &snapshot.recent_candidates,
                now_unix_secs,
                auth.api_key_id.as_str(),
                limit,
            );
    }

    if principal_limit > 0
        && count_recent_active_requests_for_user(
            &snapshot.recent_candidates,
            auth.user_id.as_str(),
            now_unix_secs,
        ) >= principal_limit as usize
    {
        return true;
    }

    key_limit.filter(|limit| *limit > 0).is_some_and(|limit| {
        auth_api_key_concurrency_limit_reached(
            &snapshot.recent_candidates,
            now_unix_secs,
            auth.api_key_id.as_str(),
            limit as usize,
        )
    })
}

fn auth_snapshot_has_concurrency_limit(snapshot: &GatewayAuthApiKeySnapshot) -> bool {
    let principal = snapshot
        .admission_policy
        .principal
        .concurrent_requests()
        .unwrap_or_default();
    let key = snapshot.admission_policy.api_key.concurrent_requests();
    if snapshot.api_key_is_standalone {
        key.unwrap_or(principal) > 0
    } else {
        principal > 0 || key.is_some_and(|limit| limit > 0)
    }
}

pub(super) fn is_candidate_selectable(
    candidate: &SchedulerMinimalCandidateSelectionCandidate,
    snapshot: &CandidateRuntimeSelectionSnapshot,
    now_unix_secs: u64,
) -> bool {
    candidate_is_selectable_with_runtime_state(CandidateRuntimeSelectabilityInput {
        candidate,
        recent_candidates: &snapshot.recent_candidates,
        provider_concurrent_limits: &snapshot.provider_concurrent_limits,
        provider_key_rpm_states: &snapshot.provider_key_rpm_states,
        now_unix_secs,
        rpm_reset_at: snapshot
            .provider_key_rpm_reset_ats
            .get(candidate.key_id.as_str())
            .copied()
            .flatten(),
    })
}

pub(super) fn current_candidate_runtime_skip_reason(
    candidate: &SchedulerMinimalCandidateSelectionCandidate,
    snapshot: &CandidateRuntimeSelectionSnapshot,
    now_unix_secs: u64,
) -> Option<&'static str> {
    let rpm_reset_at = snapshot
        .provider_key_rpm_reset_ats
        .get(candidate.key_id.as_str())
        .copied()
        .flatten();

    candidate_runtime_skip_reason_with_state(CandidateRuntimeSelectabilityInput {
        candidate,
        recent_candidates: &snapshot.recent_candidates,
        provider_concurrent_limits: &snapshot.provider_concurrent_limits,
        provider_key_rpm_states: &snapshot.provider_key_rpm_states,
        now_unix_secs,
        rpm_reset_at,
    })
}

pub(super) async fn read_provider_concurrent_limits(
    state: &(impl SchedulerRuntimeState + ?Sized),
    candidates: &[SchedulerMinimalCandidateSelectionCandidate],
) -> Result<BTreeMap<String, usize>, GatewayError> {
    let provider_ids = candidates
        .iter()
        .map(|candidate| candidate.provider_id.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    if provider_ids.is_empty() {
        return Ok(BTreeMap::new());
    }

    let providers = state
        .read_provider_catalog_providers_by_ids(&provider_ids)
        .await?;
    Ok(build_provider_concurrent_limit_map(providers))
}

pub(super) async fn read_provider_key_rpm_states(
    state: &(impl SchedulerRuntimeState + ?Sized),
    candidates: &[SchedulerMinimalCandidateSelectionCandidate],
) -> Result<BTreeMap<String, StoredProviderCatalogKey>, GatewayError> {
    let key_ids = candidates
        .iter()
        .map(|candidate| candidate.key_id.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    if key_ids.is_empty() {
        return Ok(BTreeMap::new());
    }

    let keys = state.read_provider_catalog_keys_by_ids(&key_ids).await?;
    Ok(keys
        .into_iter()
        .map(|key| (key.id.clone(), key))
        .collect::<BTreeMap<_, _>>())
}

fn read_provider_key_rpm_reset_at_map(
    state: &(impl SchedulerRuntimeState + ?Sized),
    candidates: &[SchedulerMinimalCandidateSelectionCandidate],
    now_unix_secs: u64,
) -> BTreeMap<String, Option<u64>> {
    candidates
        .iter()
        .map(|candidate| {
            (
                candidate.key_id.clone(),
                state.provider_key_rpm_reset_at(candidate.key_id.as_str(), now_unix_secs),
            )
        })
        .collect::<BTreeMap<_, _>>()
}
