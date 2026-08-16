use aether_contracts::ExecutionPlan;
use aether_data_contracts::repository::candidates::{
    RequestCandidateStatus, StoredRequestCandidate, UpsertRequestCandidateRecord,
};
use aether_scheduler_core::{
    build_execution_request_candidate_seed, build_local_request_candidate_status_record,
    build_report_request_candidate_status_record,
    finalize_execution_request_candidate_report_context, parse_request_candidate_report_context,
    resolve_report_request_candidate_slot as resolve_report_request_candidate_slot_from_candidates,
    LocalRequestCandidateStatusRecordInput, ReportRequestCandidateStatusRecordInput,
    SchedulerMinimalCandidateSelectionCandidate, SchedulerRequestCandidateStatusUpdate,
    SchedulerResolvedReportRequestCandidateSlot,
};
use aether_usage_runtime::build_locally_actionable_report_context_from_request_candidate;
use async_trait::async_trait;
use serde_json::Value;
use std::sync::OnceLock;
use std::time::Duration;
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::clock::current_unix_ms;
use crate::GatewayError;
use aether_gateway_frontdoor::short_request_id;

const REQUEST_CANDIDATE_PERSISTENCE_ENV: &str = "AETHER_GATEWAY_REQUEST_CANDIDATE_PERSISTENCE";
const REQUEST_CANDIDATE_SEED_WRITE_TIMEOUT_ENV: &str =
    "AETHER_GATEWAY_REQUEST_CANDIDATE_SEED_WRITE_TIMEOUT_MS";
const DEFAULT_REQUEST_CANDIDATE_SEED_WRITE_TIMEOUT_MS: u64 = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RequestCandidatePersistenceMode {
    Full,
    Terminal,
    None,
}

fn request_candidate_persistence_mode() -> RequestCandidatePersistenceMode {
    static MODE: OnceLock<RequestCandidatePersistenceMode> = OnceLock::new();
    *MODE.get_or_init(|| {
        match std::env::var(REQUEST_CANDIDATE_PERSISTENCE_ENV)
            .ok()
            .map(|value| value.trim().to_ascii_lowercase())
            .as_deref()
        {
            Some("terminal") | Some("final") | Some("final_only") | Some("final-only") => {
                RequestCandidatePersistenceMode::Terminal
            }
            Some("none") | Some("off") | Some("disabled") | Some("false") | Some("0") => {
                RequestCandidatePersistenceMode::None
            }
            _ => RequestCandidatePersistenceMode::Full,
        }
    })
}

fn request_candidate_status_is_terminal(status: RequestCandidateStatus) -> bool {
    matches!(
        status,
        RequestCandidateStatus::Success
            | RequestCandidateStatus::Failed
            | RequestCandidateStatus::Cancelled
    )
}

fn should_persist_request_candidate_status(status: RequestCandidateStatus) -> bool {
    match request_candidate_persistence_mode() {
        RequestCandidatePersistenceMode::Full => true,
        RequestCandidatePersistenceMode::Terminal => request_candidate_status_is_terminal(status),
        RequestCandidatePersistenceMode::None => false,
    }
}

fn request_candidate_seed_write_timeout() -> Duration {
    static TIMEOUT: OnceLock<Duration> = OnceLock::new();
    *TIMEOUT.get_or_init(|| {
        let millis = std::env::var(REQUEST_CANDIDATE_SEED_WRITE_TIMEOUT_ENV)
            .ok()
            .and_then(|value| value.trim().parse::<u64>().ok())
            .unwrap_or(DEFAULT_REQUEST_CANDIDATE_SEED_WRITE_TIMEOUT_MS);
        Duration::from_millis(millis)
    })
}

#[derive(Debug, Clone)]
pub(crate) struct LocalRequestCandidateStatusSnapshot {
    candidate_id: String,
    request_id: String,
    user_id: Option<String>,
    api_key_id: Option<String>,
    candidate_index: u32,
    retry_index: u32,
    provider_id: String,
    endpoint_id: String,
    key_id: String,
}

#[async_trait]
pub(crate) trait RequestCandidateRuntimeReader {
    async fn read_request_candidates_by_request_id(
        &self,
        request_id: &str,
    ) -> Result<Vec<StoredRequestCandidate>, GatewayError>;
}

#[async_trait]
pub(crate) trait RequestCandidateRuntimeWriter: Sync {
    fn has_request_candidate_data_writer(&self) -> bool;

    async fn upsert_request_candidate(
        &self,
        candidate: UpsertRequestCandidateRecord,
    ) -> Result<Option<StoredRequestCandidate>, GatewayError>;

    async fn enqueue_request_candidate_status(
        &self,
        candidate: UpsertRequestCandidateRecord,
    ) -> Result<Option<()>, GatewayError> {
        self.upsert_request_candidate(candidate)
            .await
            .map(|stored| stored.map(|_| ()))
    }

    fn try_enqueue_request_candidate_status(
        &self,
        candidate: UpsertRequestCandidateRecord,
    ) -> Result<(), UpsertRequestCandidateRecord> {
        Err(candidate)
    }
}

#[async_trait]
pub(crate) trait RequestCandidateRuntimeCapabilityReader {
    async fn read_request_candidate_user_model_capability_settings(
        &self,
        user_id: &str,
    ) -> Result<Option<Value>, GatewayError>;

    async fn read_request_candidate_api_key_force_capabilities(
        &self,
        user_id: &str,
        api_key_id: &str,
    ) -> Result<Option<Value>, GatewayError>;
}

pub(crate) async fn resolve_request_candidate_required_capabilities(
    state: &(impl RequestCandidateRuntimeCapabilityReader + ?Sized),
    user_id: &str,
    api_key_id: &str,
    requested_model: Option<&str>,
    explicit_required_capabilities: Option<&Value>,
    model_directive_base_model: Option<&str>,
) -> Option<Value> {
    let mut merged = serde_json::Map::new();

    match state
        .read_request_candidate_user_model_capability_settings(user_id)
        .await
    {
        Ok(settings) => merge_capability_object(
            &mut merged,
            select_requested_model_capabilities(
                settings.as_ref(),
                requested_model,
                model_directive_base_model,
            ),
        ),
        Err(error) => {
            warn!(
                user_id = %user_id,
                api_key_id = %api_key_id,
                requested_model = requested_model.unwrap_or_default(),
                error = ?error,
                "gateway request candidate user model capabilities lookup failed"
            );
        }
    }

    match state
        .read_request_candidate_api_key_force_capabilities(user_id, api_key_id)
        .await
    {
        Ok(force_capabilities) => {
            merge_capability_object(&mut merged, force_capabilities.as_ref());
        }
        Err(error) => {
            warn!(
                user_id = %user_id,
                api_key_id = %api_key_id,
                requested_model = requested_model.unwrap_or_default(),
                error = ?error,
                "gateway request candidate api key capabilities lookup failed"
            );
        }
    }

    merge_capability_object(&mut merged, explicit_required_capabilities);

    (!merged.is_empty()).then_some(Value::Object(merged))
}

fn merge_capability_object(target: &mut serde_json::Map<String, Value>, source: Option<&Value>) {
    let Some(source) = source.and_then(Value::as_object) else {
        return;
    };

    for (capability, value) in source {
        if capability.trim().is_empty() {
            continue;
        }
        target.insert(capability.clone(), value.clone());
    }
}

fn select_requested_model_capabilities<'a>(
    settings: Option<&'a Value>,
    requested_model: Option<&str>,
    model_directive_base_model: Option<&str>,
) -> Option<&'a Value> {
    let requested_model = requested_model
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    let settings = settings?.as_object()?;

    find_model_capabilities(settings, requested_model).or_else(|| {
        model_directive_base_model
            .map(str::trim)
            .filter(|base_model| !base_model.is_empty() && *base_model != requested_model)
            .and_then(|base_model| find_model_capabilities(settings, base_model))
    })
}

fn find_model_capabilities<'a>(
    settings: &'a serde_json::Map<String, Value>,
    requested_model: &str,
) -> Option<&'a Value> {
    settings.get(requested_model).or_else(|| {
        settings.iter().find_map(|(model_name, capabilities)| {
            model_name
                .trim()
                .eq_ignore_ascii_case(requested_model)
                .then_some(capabilities)
        })
    })
}

fn request_candidate_status_label(status: RequestCandidateStatus) -> &'static str {
    match status {
        RequestCandidateStatus::Available => "available",
        RequestCandidateStatus::Unused => "unused",
        RequestCandidateStatus::Pending => "pending",
        RequestCandidateStatus::Streaming => "streaming",
        RequestCandidateStatus::Success => "success",
        RequestCandidateStatus::Failed => "failed",
        RequestCandidateStatus::Cancelled => "cancelled",
        RequestCandidateStatus::Skipped => "skipped",
    }
}

pub(crate) fn snapshot_local_request_candidate_status(
    plan: &ExecutionPlan,
    report_context: Option<&Value>,
) -> Option<LocalRequestCandidateStatusSnapshot> {
    let candidate_id = plan
        .candidate_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    let metadata = parse_request_candidate_report_context(report_context);
    let candidate_index = metadata
        .as_ref()
        .and_then(|metadata| metadata.candidate_index)
        .unwrap_or(0);

    Some(LocalRequestCandidateStatusSnapshot {
        candidate_id: candidate_id.to_string(),
        request_id: plan.request_id.clone(),
        user_id: metadata
            .as_ref()
            .and_then(|metadata| metadata.user_id.clone()),
        api_key_id: metadata
            .as_ref()
            .and_then(|metadata| metadata.api_key_id.clone()),
        candidate_index,
        retry_index: metadata
            .as_ref()
            .map(|metadata| metadata.retry_index)
            .unwrap_or(0),
        provider_id: plan.provider_id.clone(),
        endpoint_id: plan.endpoint_id.clone(),
        key_id: plan.key_id.clone(),
    })
}

pub(crate) async fn persist_local_request_candidate_status_record(
    state: &(impl RequestCandidateRuntimeWriter + ?Sized),
    record: UpsertRequestCandidateRecord,
) {
    let candidate_id = record.id.clone();
    let request_id = short_request_id(record.request_id.as_str());
    let candidate_index = record.candidate_index;
    let retry_index = record.retry_index;
    let status = record.status;

    if !should_persist_request_candidate_status(status) {
        debug!(
            event_name = "request_candidate_status_persistence_skipped",
            log_type = "event",
            request_id = %request_id,
            candidate_id = %candidate_id,
            candidate_index,
            retry_index,
            status = request_candidate_status_label(status),
            source = "local_status",
            "gateway skipped request candidate status update due to persistence mode"
        );
        return;
    }

    match state.enqueue_request_candidate_status(record).await {
        Ok(Some(())) => {
            debug!(
                event_name = "request_candidate_status_persisted",
                log_type = "event",
                request_id = %request_id,
                candidate_id = %candidate_id,
                candidate_index,
                retry_index,
                status = request_candidate_status_label(status),
                source = "local_status",
                "gateway persisted request candidate status update"
            );
        }
        Ok(None) => {
            warn!(
                event_name = "request_candidate_writer_unavailable",
                log_type = "event",
                request_id = %request_id,
                candidate_id = %candidate_id,
                candidate_index,
                retry_index,
                status = request_candidate_status_label(status),
                source = "local_status",
                "gateway skipped request candidate persistence because writer is unavailable"
            );
        }
        Err(err) => {
            warn!(
                event_name = "request_candidate_status_persist_failed",
                log_type = "event",
                request_id = %request_id,
                candidate_id = %candidate_id,
                error = ?err,
                "gateway failed to persist request candidate status update"
            );
        }
    }
}

pub(crate) async fn record_local_request_candidate_status(
    state: &(impl RequestCandidateRuntimeWriter + ?Sized),
    plan: &ExecutionPlan,
    report_context: Option<&Value>,
    status_update: SchedulerRequestCandidateStatusUpdate,
) {
    let Some(record) =
        build_local_request_candidate_status_record(LocalRequestCandidateStatusRecordInput {
            plan,
            report_context,
            status_update,
        })
    else {
        return;
    };
    persist_local_request_candidate_status_record(state, record).await;
}

pub(crate) async fn record_local_request_candidate_extra_data(
    state: &(impl RequestCandidateRuntimeWriter + ?Sized),
    plan: &ExecutionPlan,
    report_context: Option<&Value>,
    status: RequestCandidateStatus,
    status_code: Option<u16>,
    latency_ms: Option<u64>,
    extra_data: Value,
) {
    let Some(snapshot) = snapshot_local_request_candidate_status(plan, report_context) else {
        return;
    };
    let record = UpsertRequestCandidateRecord {
        id: snapshot.candidate_id.clone(),
        request_id: snapshot.request_id.clone(),
        user_id: snapshot.user_id.clone(),
        api_key_id: snapshot.api_key_id.clone(),
        username: None,
        api_key_name: None,
        candidate_index: snapshot.candidate_index,
        retry_index: snapshot.retry_index,
        provider_id: Some(snapshot.provider_id.clone()),
        endpoint_id: Some(snapshot.endpoint_id.clone()),
        key_id: Some(snapshot.key_id.clone()),
        status,
        skip_reason: None,
        is_cached: None,
        status_code,
        error_type: None,
        error_message: None,
        latency_ms,
        concurrent_requests: None,
        extra_data: Some(extra_data),
        required_capabilities: None,
        created_at_unix_ms: None,
        started_at_unix_ms: None,
        finished_at_unix_ms: None,
    };
    persist_local_request_candidate_status_record(state, record).await;
}

fn build_local_request_candidate_status_snapshot_record(
    snapshot: &LocalRequestCandidateStatusSnapshot,
    status_update: SchedulerRequestCandidateStatusUpdate,
) -> UpsertRequestCandidateRecord {
    let SchedulerRequestCandidateStatusUpdate {
        status,
        status_code,
        error_type,
        error_message,
        latency_ms,
        started_at_unix_ms,
        finished_at_unix_ms,
    } = status_update;
    UpsertRequestCandidateRecord {
        id: snapshot.candidate_id.clone(),
        request_id: snapshot.request_id.clone(),
        user_id: snapshot.user_id.clone(),
        api_key_id: snapshot.api_key_id.clone(),
        username: None,
        api_key_name: None,
        candidate_index: snapshot.candidate_index,
        retry_index: snapshot.retry_index,
        provider_id: Some(snapshot.provider_id.clone()),
        endpoint_id: Some(snapshot.endpoint_id.clone()),
        key_id: Some(snapshot.key_id.clone()),
        status,
        skip_reason: None,
        is_cached: None,
        status_code,
        error_type,
        error_message,
        latency_ms,
        concurrent_requests: None,
        extra_data: None,
        required_capabilities: None,
        created_at_unix_ms: None,
        started_at_unix_ms,
        finished_at_unix_ms,
    }
}

pub(crate) fn try_enqueue_local_request_candidate_status_snapshot(
    state: &(impl RequestCandidateRuntimeWriter + ?Sized),
    snapshot: &LocalRequestCandidateStatusSnapshot,
    status_update: SchedulerRequestCandidateStatusUpdate,
) -> Result<(), UpsertRequestCandidateRecord> {
    let record = build_local_request_candidate_status_snapshot_record(snapshot, status_update);
    if !should_persist_request_candidate_status(record.status) {
        return Ok(());
    }
    state.try_enqueue_request_candidate_status(record)
}

pub(crate) async fn record_local_request_candidate_status_snapshot(
    state: &(impl RequestCandidateRuntimeWriter + ?Sized),
    snapshot: &LocalRequestCandidateStatusSnapshot,
    status_update: SchedulerRequestCandidateStatusUpdate,
) {
    let record = build_local_request_candidate_status_snapshot_record(snapshot, status_update);
    persist_local_request_candidate_status_record(state, record).await;
}

pub(crate) async fn record_report_request_candidate_status(
    state: &(impl RequestCandidateRuntimeReader + RequestCandidateRuntimeWriter + ?Sized),
    report_context: Option<&Value>,
    status_update: SchedulerRequestCandidateStatusUpdate,
) {
    if matches!(
        request_candidate_persistence_mode(),
        RequestCandidatePersistenceMode::None
    ) {
        return;
    }
    let Some(slot) = resolve_report_request_candidate_slot(state, report_context).await else {
        return;
    };
    let request_id = slot.request_id.clone();
    let request_id_for_log = short_request_id(request_id.as_str());
    let candidate_index = slot.candidate_index;
    let retry_index = slot.retry_index;
    let record =
        build_report_request_candidate_status_record(ReportRequestCandidateStatusRecordInput {
            slot,
            status_update,
            now_unix_ms: current_unix_ms(),
        });
    let candidate_id = record.id.clone();
    let status = record.status;

    if !should_persist_request_candidate_status(status) {
        debug!(
            event_name = "request_candidate_report_status_persistence_skipped",
            log_type = "event",
            request_id = %request_id_for_log,
            candidate_id = %candidate_id,
            candidate_index,
            retry_index,
            status = request_candidate_status_label(status),
            source = "report_status",
            "gateway skipped report-driven request candidate status update due to persistence mode"
        );
        return;
    }

    match state.enqueue_request_candidate_status(record).await {
        Ok(Some(())) => {
            debug!(
                event_name = "request_candidate_report_status_persisted",
                log_type = "event",
                request_id = %request_id_for_log,
                candidate_id = %candidate_id,
                candidate_index,
                retry_index,
                status = request_candidate_status_label(status),
                source = "report_status",
                "gateway persisted report-driven request candidate status update"
            );
        }
        Ok(None) => {
            warn!(
                event_name = "request_candidate_writer_unavailable",
                log_type = "event",
                request_id = %request_id_for_log,
                candidate_id = %candidate_id,
                candidate_index,
                retry_index,
                status = request_candidate_status_label(status),
                source = "report_status",
                "gateway skipped request candidate persistence because writer is unavailable"
            );
        }
        Err(err) => {
            warn!(
                event_name = "request_candidate_report_status_persist_failed",
                log_type = "event",
                request_id = %request_id_for_log,
                candidate_index,
                retry_index,
                error = ?err,
                "gateway failed to persist report-driven request candidate status update"
            );
        }
    }
}

pub(crate) async fn ensure_execution_request_candidate_slot(
    state: &(impl RequestCandidateRuntimeWriter + ?Sized),
    plan: &mut ExecutionPlan,
    report_context: &mut Option<Value>,
) {
    if !state.has_request_candidate_data_writer() {
        warn!(
            event_name = "request_candidate_writer_unavailable",
            log_type = "event",
            request_id = %short_request_id(plan.request_id.as_str()),
            provider_id = %plan.provider_id,
            endpoint_id = %plan.endpoint_id,
            key_id = %plan.key_id,
            source = "seed",
            "gateway skipped request candidate seed because writer is unavailable"
        );
        return;
    }
    let existing_candidate_id = plan
        .candidate_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    let report_candidate_id = parse_request_candidate_report_context(report_context.as_ref())
        .and_then(|metadata| metadata.candidate_id);
    if existing_candidate_id.as_deref().is_some()
        && report_candidate_id.as_deref() == existing_candidate_id.as_deref()
    {
        return;
    }

    let seed = build_execution_request_candidate_seed(
        plan,
        report_context.as_ref(),
        current_unix_ms(),
        existing_candidate_id.unwrap_or_else(|| Uuid::new_v4().to_string()),
    );
    let generated_candidate_id = seed.upsert_record.id.clone();
    let request_id = short_request_id(plan.request_id.as_str());

    if !should_persist_request_candidate_status(seed.upsert_record.status) {
        plan.candidate_id = Some(generated_candidate_id.clone());
        *report_context = Some(finalize_execution_request_candidate_report_context(
            seed.report_context,
            &generated_candidate_id,
        ));
        debug!(
            event_name = "request_candidate_slot_seed_persistence_skipped",
            log_type = "event",
            request_id = %request_id,
            candidate_id = %generated_candidate_id,
            provider_id = %plan.provider_id,
            endpoint_id = %plan.endpoint_id,
            key_id = %plan.key_id,
            source = "seed",
            "gateway skipped request candidate seed due to persistence mode"
        );
        return;
    }

    let seed_upsert_record = seed.upsert_record;
    let generated_candidate_id = generated_candidate_id.clone();
    let candidate_id = match tokio::time::timeout(
        request_candidate_seed_write_timeout(),
        state.upsert_request_candidate(seed_upsert_record),
    )
    .await
    {
        Ok(Ok(Some(stored))) => {
            info!(
                event_name = "request_candidate_slot_seeded",
                log_type = "event",
                request_id = %request_id,
                candidate_id = %stored.id,
                provider_id = %plan.provider_id,
                endpoint_id = %plan.endpoint_id,
                key_id = %plan.key_id,
                source = "seed",
                "gateway seeded execution request candidate slot"
            );
            stored.id
        }
        Ok(Ok(None)) => {
            warn!(
                event_name = "request_candidate_writer_unavailable",
                log_type = "event",
                request_id = %request_id,
                candidate_id = %generated_candidate_id,
                provider_id = %plan.provider_id,
                endpoint_id = %plan.endpoint_id,
                key_id = %plan.key_id,
                source = "seed",
                "gateway skipped request candidate seed because writer is unavailable"
            );
            generated_candidate_id
        }
        Ok(Err(err)) => {
            warn!(
                event_name = "request_candidate_slot_seed_failed",
                log_type = "event",
                request_id = %request_id,
                error = ?err,
                "gateway failed to seed execution request candidate slot"
            );
            generated_candidate_id
        }
        Err(_) => {
            let timeout_ms = request_candidate_seed_write_timeout().as_millis() as u64;
            warn!(
                event_name = "request_candidate_slot_seed_timed_out",
                log_type = "event",
                request_id = %request_id,
                candidate_id = %generated_candidate_id,
                provider_id = %plan.provider_id,
                endpoint_id = %plan.endpoint_id,
                key_id = %plan.key_id,
                source = "seed",
                timeout_ms,
                "gateway skipped blocking request candidate seed after timeout"
            );
            generated_candidate_id
        }
    };

    plan.candidate_id = Some(candidate_id.clone());
    *report_context = Some(finalize_execution_request_candidate_report_context(
        seed.report_context,
        &candidate_id,
    ));
}

pub(crate) async fn persist_available_local_candidate(
    state: &(impl RequestCandidateRuntimeWriter + ?Sized),
    trace_id: &str,
    user_id: &str,
    api_key_id: &str,
    candidate: &SchedulerMinimalCandidateSelectionCandidate,
    candidate_index: u32,
    retry_index: u32,
    candidate_id: &str,
    required_capabilities: Option<&Value>,
    extra_data: Option<serde_json::Value>,
    created_at_unix_ms: u64,
    error_context: &'static str,
) -> String {
    if !should_persist_request_candidate_status(RequestCandidateStatus::Available) {
        return candidate_id.to_string();
    }
    match state
        .upsert_request_candidate(UpsertRequestCandidateRecord {
            id: candidate_id.to_string(),
            request_id: trace_id.to_string(),
            user_id: Some(user_id.to_string()),
            api_key_id: Some(api_key_id.to_string()),
            username: None,
            api_key_name: None,
            candidate_index,
            retry_index,
            provider_id: Some(candidate.provider_id.clone()),
            endpoint_id: Some(candidate.endpoint_id.clone()),
            key_id: Some(candidate.key_id.clone()),
            status: RequestCandidateStatus::Available,
            skip_reason: None,
            is_cached: Some(false),
            status_code: None,
            error_type: None,
            error_message: None,
            latency_ms: None,
            concurrent_requests: None,
            extra_data,
            required_capabilities: required_capabilities.cloned(),
            created_at_unix_ms: Some(created_at_unix_ms),
            started_at_unix_ms: None,
            finished_at_unix_ms: None,
        })
        .await
    {
        Ok(Some(stored)) => {
            debug!(
                event_name = "request_candidate_status_persisted",
                log_type = "event",
                request_id = %short_request_id(trace_id),
                candidate_id = %stored.id,
                candidate_index,
                retry_index,
                status = "available",
                source = "planner_available",
                provider_id = %candidate.provider_id,
                endpoint_id = %candidate.endpoint_id,
                key_id = %candidate.key_id,
                has_required_capabilities = required_capabilities.is_some(),
                "gateway persisted available local request candidate"
            );
            stored.id
        }
        Ok(None) => {
            warn!(
                event_name = "request_candidate_writer_unavailable",
                log_type = "event",
                request_id = %short_request_id(trace_id),
                candidate_id = %candidate_id,
                candidate_index,
                retry_index,
                status = "available",
                source = "planner_available",
                provider_id = %candidate.provider_id,
                endpoint_id = %candidate.endpoint_id,
                key_id = %candidate.key_id,
                "gateway skipped request candidate persistence because writer is unavailable"
            );
            candidate_id.to_string()
        }
        Err(err) => {
            warn!(
                trace_id = %trace_id,
                candidate_id = %candidate_id,
                error = ?err,
                "{error_context}"
            );
            candidate_id.to_string()
        }
    }
}

pub(crate) async fn persist_skipped_local_candidate(
    state: &(impl RequestCandidateRuntimeWriter + ?Sized),
    trace_id: &str,
    user_id: &str,
    api_key_id: &str,
    candidate: &SchedulerMinimalCandidateSelectionCandidate,
    candidate_index: u32,
    retry_index: u32,
    candidate_id: &str,
    required_capabilities: Option<&Value>,
    skip_reason: &str,
    extra_data: Option<serde_json::Value>,
    finished_at_unix_ms: u64,
    error_context: &'static str,
) {
    if !should_persist_request_candidate_status(RequestCandidateStatus::Skipped) {
        return;
    }
    match state
        .upsert_request_candidate(UpsertRequestCandidateRecord {
            id: candidate_id.to_string(),
            request_id: trace_id.to_string(),
            user_id: Some(user_id.to_string()),
            api_key_id: Some(api_key_id.to_string()),
            username: None,
            api_key_name: None,
            candidate_index,
            retry_index,
            provider_id: Some(candidate.provider_id.clone()),
            endpoint_id: Some(candidate.endpoint_id.clone()),
            key_id: Some(candidate.key_id.clone()),
            status: RequestCandidateStatus::Skipped,
            skip_reason: Some(skip_reason.to_string()),
            is_cached: Some(false),
            status_code: None,
            error_type: None,
            error_message: None,
            latency_ms: None,
            concurrent_requests: None,
            extra_data,
            required_capabilities: required_capabilities.cloned(),
            created_at_unix_ms: None,
            started_at_unix_ms: None,
            finished_at_unix_ms: Some(finished_at_unix_ms),
        })
        .await
    {
        Ok(Some(stored)) => {
            debug!(
                event_name = "request_candidate_status_persisted",
                log_type = "event",
                request_id = %short_request_id(trace_id),
                candidate_id = %stored.id,
                candidate_index,
                retry_index,
                status = "skipped",
                skip_reason,
                source = "planner_skipped",
                provider_id = %candidate.provider_id,
                endpoint_id = %candidate.endpoint_id,
                key_id = %candidate.key_id,
                has_required_capabilities = required_capabilities.is_some(),
                "gateway persisted skipped local request candidate"
            );
        }
        Ok(None) => {
            warn!(
                event_name = "request_candidate_writer_unavailable",
                log_type = "event",
                request_id = %short_request_id(trace_id),
                candidate_id = %candidate_id,
                candidate_index,
                retry_index,
                status = "skipped",
                skip_reason,
                source = "planner_skipped",
                provider_id = %candidate.provider_id,
                endpoint_id = %candidate.endpoint_id,
                key_id = %candidate.key_id,
                "gateway skipped request candidate persistence because writer is unavailable"
            );
        }
        Err(err) => {
            warn!(
                trace_id = %trace_id,
                candidate_id = %candidate_id,
                skip_reason,
                error = ?err,
                "{error_context}"
            );
        }
    }
}

pub(crate) async fn resolve_locally_actionable_request_candidate_report_context(
    state: &(impl RequestCandidateRuntimeReader + ?Sized),
    context: &Value,
) -> Option<Value> {
    let request_id = context
        .get("request_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    let existing_candidates = state
        .read_request_candidates_by_request_id(request_id)
        .await
        .ok()?;
    if existing_candidates.len() != 1 {
        return None;
    }

    build_locally_actionable_report_context_from_request_candidate(context, &existing_candidates[0])
}

async fn resolve_report_request_candidate_slot(
    state: &(impl RequestCandidateRuntimeReader + ?Sized),
    report_context: Option<&Value>,
) -> Option<SchedulerResolvedReportRequestCandidateSlot> {
    let metadata = parse_request_candidate_report_context(report_context)?;
    if metadata
        .request_id
        .as_deref()
        .map(str::trim)
        .is_some_and(|value| !value.is_empty())
        && metadata
            .candidate_id
            .as_deref()
            .map(str::trim)
            .is_some_and(|value| !value.is_empty())
    {
        return resolve_report_request_candidate_slot_from_candidates(
            &[],
            metadata,
            current_unix_ms(),
            Uuid::new_v4().to_string(),
        );
    }

    let request_id = metadata.request_id.clone()?;
    let existing_candidates = state
        .read_request_candidates_by_request_id(request_id.as_str())
        .await
        .ok()
        .unwrap_or_default();
    resolve_report_request_candidate_slot_from_candidates(
        &existing_candidates,
        metadata,
        current_unix_ms(),
        Uuid::new_v4().to_string(),
    )
}
