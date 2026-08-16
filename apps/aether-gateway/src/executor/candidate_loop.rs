use aether_ai_serving::{
    run_ai_attempt_loop, AiAttemptExecutionOutcome, AiAttemptLoopOutcome, AiAttemptLoopPort,
    AiAttemptRetryScope, AiExecutionAttempt,
};
use aether_data_contracts::repository::candidates::RequestCandidateStatus;
use aether_runtime::ConcurrencyPermit;
use aether_scheduler_core::{
    parse_request_candidate_report_context, SchedulerRequestCandidateStatusUpdate,
};
use async_trait::async_trait;
use axum::body::Body;
use axum::http::Response;
use futures_util::StreamExt;
use tokio::sync::OnceCell;
use tokio::time::{timeout, Duration};
use tracing::{debug, info, warn, Instrument};

use crate::ai_serving::LocalExecutionAttemptSource;
use crate::clock::current_unix_ms;
use crate::control::GatewayControlDecision;
use crate::execution_runtime::{
    build_transport_error_stop_response, execute_execution_runtime_stream_with_retry_scope,
    execute_execution_runtime_sync_with_retry_scope,
    mark_stream_candidate_watchdog_terminal_started, StreamCandidateWatchdogProgress,
};
use crate::executor::{
    build_local_execution_exhaustion, mark_deferred_upstream_response, LocalExecutionRequestOutcome,
};
use crate::orchestration::{
    local_execution_candidate_metadata_from_report_context,
    resolve_local_transport_failover_analysis_for_attempt, LocalFailoverDecision,
};
use crate::privacy::RedactionExecutionCandidateId;
use crate::request_candidate_runtime::{
    record_local_request_candidate_status, RequestCandidateRuntimeWriter,
};
use crate::stage_metrics::observe_gateway_stage_ms;
use crate::{AppState, GatewayError};
use aether_gateway_frontdoor::short_request_id;

const DEFAULT_STREAM_FIRST_BYTE_WATCHDOG_TIMEOUT_MS: u64 = 30_000;
const UPSTREAM_EXECUTION_GATE_NAME: &str = "gateway_upstream_execution";
const UPSTREAM_TARGET_GATE_NAME: &str = "gateway_upstream_target";
const UPSTREAM_EXECUTION_GATE_HOLD_STREAM_RESPONSE_ENV: &str =
    "AETHER_GATEWAY_UPSTREAM_EXECUTION_GATE_HOLD_STREAM_RESPONSE";
const UPSTREAM_EXECUTION_GATE_STREAM_HOLD_MODE_ENV: &str =
    "AETHER_GATEWAY_UPSTREAM_EXECUTION_GATE_STREAM_HOLD_MODE";

fn attach_redaction_execution_candidate(response: &mut Response<Body>, candidate_id: Option<&str>) {
    if let Some(candidate_id) = candidate_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        response
            .extensions_mut()
            .insert(RedactionExecutionCandidateId::new(candidate_id));
    }
}

pub(crate) async fn execute_sync_plan_and_reports<T>(
    state: &AppState,
    parts: &http::request::Parts,
    trace_id: &str,
    decision: &GatewayControlDecision,
    plan_kind: &str,
    plan_and_reports: Vec<T>,
) -> Result<LocalExecutionRequestOutcome, GatewayError>
where
    T: AiExecutionAttempt + Send + Sync + 'static,
{
    let execution_context = CandidateExecutionContext::default();
    execute_sync_plan_and_reports_with_execution_context(
        state,
        parts,
        trace_id,
        decision,
        plan_kind,
        plan_and_reports,
        &execution_context,
    )
    .await
}

pub(crate) async fn execute_sync_plan_and_reports_with_execution_context<T>(
    state: &AppState,
    parts: &http::request::Parts,
    trace_id: &str,
    decision: &GatewayControlDecision,
    plan_kind: &str,
    plan_and_reports: Vec<T>,
    execution_context: &CandidateExecutionContext,
) -> Result<LocalExecutionRequestOutcome, GatewayError>
where
    T: AiExecutionAttempt + Send + Sync + 'static,
{
    let candidate_count = plan_and_reports.len();
    let first_provider = plan_and_reports
        .first()
        .and_then(|item| item.execution_plan().provider_name.as_deref())
        .unwrap_or("-")
        .to_string();
    let span = tracing::debug_span!(
        "candidates",
        trace_id = %trace_id,
        plan_kind,
        candidate_count,
    );

    async move {
        tracing::debug!(
            event_name = "candidate_loop_started",
            log_type = "event",
            trace_id = %trace_id,
            plan_kind,
            candidate_count,
            first_provider = first_provider.as_str(),
            "candidate loop started"
        );

        let port = SyncAttemptLoopPort {
            state,
            parts,
            trace_id,
            decision,
            plan_kind,
            execution_context,
        };
        match run_ai_attempt_loop(&port, plan_and_reports).await? {
            AiAttemptLoopOutcome::Responded(response) => {
                Ok(LocalExecutionRequestOutcome::responded(response))
            }
            AiAttemptLoopOutcome::Deferred(response) => Ok(
                LocalExecutionRequestOutcome::responded(mark_deferred_upstream_response(response)),
            ),
            AiAttemptLoopOutcome::Exhausted(exhaustion) => {
                Ok(LocalExecutionRequestOutcome::Exhausted(exhaustion))
            }
            AiAttemptLoopOutcome::NoPath => Ok(LocalExecutionRequestOutcome::NoPath),
        }
    }
    .instrument(span)
    .await
}

pub(crate) async fn execute_sync_attempt_source<T, S>(
    state: &AppState,
    parts: &http::request::Parts,
    trace_id: &str,
    decision: &GatewayControlDecision,
    plan_kind: &str,
    source: S,
) -> Result<LocalExecutionRequestOutcome, GatewayError>
where
    T: AiExecutionAttempt + Send + Sync + 'static,
    S: LocalExecutionAttemptSource<T>,
{
    let execution_context = CandidateExecutionContext::default();
    execute_sync_attempt_source_with_execution_context(
        state,
        parts,
        trace_id,
        decision,
        plan_kind,
        source,
        &execution_context,
    )
    .await
}

pub(crate) async fn execute_sync_attempt_source_with_execution_context<T, S>(
    state: &AppState,
    parts: &http::request::Parts,
    trace_id: &str,
    decision: &GatewayControlDecision,
    plan_kind: &str,
    mut source: S,
    execution_context: &CandidateExecutionContext,
) -> Result<LocalExecutionRequestOutcome, GatewayError>
where
    T: AiExecutionAttempt + Send + Sync + 'static,
    S: LocalExecutionAttemptSource<T>,
{
    let span = tracing::debug_span!("candidates", trace_id = %trace_id, plan_kind);

    async move {
        tracing::debug!(
            event_name = "candidate_loop_started",
            log_type = "event",
            trace_id = %trace_id,
            plan_kind,
            "dynamic candidate loop started"
        );

        let port = SyncAttemptLoopPort {
            state,
            parts,
            trace_id,
            decision,
            plan_kind,
            execution_context,
        };
        run_dynamic_attempt_loop(
            &port,
            &mut source,
            trace_id,
            plan_kind,
            state
                .frontdoor_runtime_guards
                .local_execution_planning_timeout,
        )
        .await
    }
    .instrument(span)
    .await
}

struct SyncAttemptLoopPort<'a> {
    state: &'a AppState,
    parts: &'a http::request::Parts,
    trace_id: &'a str,
    decision: &'a GatewayControlDecision,
    plan_kind: &'a str,
    execution_context: &'a CandidateExecutionContext,
}

#[async_trait]
impl<T> AiAttemptLoopPort<T> for SyncAttemptLoopPort<'_>
where
    T: AiExecutionAttempt + Send + Sync + 'static,
{
    type Response = Response<Body>;
    type Exhaustion = crate::executor::LocalExecutionExhaustion;
    type Error = GatewayError;

    async fn execute_attempt(
        &self,
        attempt: &T,
    ) -> Result<AiAttemptExecutionOutcome<Self::Response>, Self::Error> {
        let plan = attempt.execution_plan();
        let report_context = attempt.report_context();
        let daily_usage_outcome = self
            .execution_context
            .daily_usage_outcome
            .get_or_init(|| async {
                self.state
                    .frontdoor_daily_usage()
                    .check(self.state, self.decision)
                    .await
            })
            .await;
        if let Some(response) = execution_plan_balance_capacity_response(
            self.state,
            self.trace_id,
            self.decision,
            daily_usage_outcome,
            plan,
            report_context.as_ref(),
        )
        .await?
        {
            return Ok(AiAttemptExecutionOutcome::Responded(response));
        }
        prewarm_direct_reqwest_candidate_client(plan);
        let _permit = acquire_upstream_execution_gate(self.state, self.trace_id).await?;
        let upstream_execution_gate_held_started_at = std::time::Instant::now();
        let mut execution = execute_execution_runtime_sync_with_retry_scope(
            self.state,
            self.parts.uri.path(),
            plan.clone(),
            self.trace_id,
            self.decision,
            self.plan_kind,
            attempt.report_kind(),
            report_context,
        )
        .await?;
        observe_gateway_stage_ms(
            "upstream_execution_gate_held",
            upstream_execution_gate_held_started_at
                .elapsed()
                .as_millis() as u64,
        );
        match &mut execution {
            AiAttemptExecutionOutcome::Responded(response)
            | AiAttemptExecutionOutcome::Retry {
                fallback_response: Some(response),
                ..
            } => attach_redaction_execution_candidate(response, plan.candidate_id.as_deref()),
            AiAttemptExecutionOutcome::Retry {
                fallback_response: None,
                ..
            } => {}
        }
        Ok(execution)
    }

    async fn mark_unused_attempts(&self, attempts: Vec<T>) -> Result<(), Self::Error> {
        mark_unused_local_candidates(self.state, attempts).await;
        Ok(())
    }

    async fn build_exhaustion(
        &self,
        last_plan: aether_contracts::ExecutionPlan,
        last_report_context: Option<serde_json::Value>,
    ) -> Result<Self::Exhaustion, Self::Error> {
        warn!(
            event_name = "candidate_loop_exhausted",
            log_type = "ops",
            trace_id = %self.trace_id,
            plan_kind = self.plan_kind,
            request_id = %short_request_id(last_plan.request_id.as_str()),
            candidate_id = ?last_plan.candidate_id,
            provider_name = last_plan.provider_name.as_deref().unwrap_or("-"),
            endpoint_id = %last_plan.endpoint_id,
            key_id = %last_plan.key_id,
            model_name = last_plan.model_name.as_deref().unwrap_or("-"),
            "candidate loop exhausted local sync candidates"
        );
        Ok(
            build_local_execution_exhaustion(self.state, &last_plan, last_report_context.as_ref())
                .await,
        )
    }
}

pub(crate) async fn execute_stream_plan_and_reports<T>(
    state: &AppState,
    trace_id: &str,
    decision: &GatewayControlDecision,
    plan_kind: &str,
    plan_and_reports: Vec<T>,
) -> Result<LocalExecutionRequestOutcome, GatewayError>
where
    T: AiExecutionAttempt + Send + Sync + 'static,
{
    let execution_context = CandidateExecutionContext::default();
    execute_stream_plan_and_reports_with_execution_context(
        state,
        trace_id,
        decision,
        plan_kind,
        plan_and_reports,
        &execution_context,
    )
    .await
}

pub(crate) async fn execute_stream_plan_and_reports_with_execution_context<T>(
    state: &AppState,
    trace_id: &str,
    decision: &GatewayControlDecision,
    plan_kind: &str,
    plan_and_reports: Vec<T>,
    execution_context: &CandidateExecutionContext,
) -> Result<LocalExecutionRequestOutcome, GatewayError>
where
    T: AiExecutionAttempt + Send + Sync + 'static,
{
    let candidate_count = plan_and_reports.len();
    let first_provider = plan_and_reports
        .first()
        .and_then(|item| item.execution_plan().provider_name.as_deref())
        .unwrap_or("-")
        .to_string();
    let span = tracing::debug_span!(
        "candidates",
        trace_id = %trace_id,
        plan_kind,
        candidate_count,
    );

    async move {
        tracing::debug!(
            event_name = "candidate_loop_started",
            log_type = "event",
            trace_id = %trace_id,
            plan_kind,
            candidate_count,
            first_provider = first_provider.as_str(),
            "candidate loop started"
        );

        let port = StreamAttemptLoopPort {
            state,
            trace_id,
            decision,
            plan_kind,
            execution_context,
        };
        match run_ai_attempt_loop(&port, plan_and_reports).await? {
            AiAttemptLoopOutcome::Responded(response) => {
                Ok(LocalExecutionRequestOutcome::responded(response))
            }
            AiAttemptLoopOutcome::Deferred(response) => Ok(
                LocalExecutionRequestOutcome::responded(mark_deferred_upstream_response(response)),
            ),
            AiAttemptLoopOutcome::Exhausted(exhaustion) => {
                Ok(LocalExecutionRequestOutcome::Exhausted(exhaustion))
            }
            AiAttemptLoopOutcome::NoPath => Ok(LocalExecutionRequestOutcome::NoPath),
        }
    }
    .instrument(span)
    .await
}

pub(crate) async fn execute_stream_attempt_source<T, S>(
    state: &AppState,
    trace_id: &str,
    decision: &GatewayControlDecision,
    plan_kind: &str,
    source: S,
) -> Result<LocalExecutionRequestOutcome, GatewayError>
where
    T: AiExecutionAttempt + Send + Sync + 'static,
    S: LocalExecutionAttemptSource<T>,
{
    let execution_context = CandidateExecutionContext::default();
    execute_stream_attempt_source_with_execution_context(
        state,
        trace_id,
        decision,
        plan_kind,
        source,
        &execution_context,
    )
    .await
}

pub(crate) async fn execute_stream_attempt_source_with_execution_context<T, S>(
    state: &AppState,
    trace_id: &str,
    decision: &GatewayControlDecision,
    plan_kind: &str,
    mut source: S,
    execution_context: &CandidateExecutionContext,
) -> Result<LocalExecutionRequestOutcome, GatewayError>
where
    T: AiExecutionAttempt + Send + Sync + 'static,
    S: LocalExecutionAttemptSource<T>,
{
    let span = tracing::debug_span!("candidates", trace_id = %trace_id, plan_kind);

    async move {
        tracing::debug!(
            event_name = "candidate_loop_started",
            log_type = "event",
            trace_id = %trace_id,
            plan_kind,
            "dynamic candidate loop started"
        );

        let port = StreamAttemptLoopPort {
            state,
            trace_id,
            decision,
            plan_kind,
            execution_context,
        };
        run_dynamic_attempt_loop(
            &port,
            &mut source,
            trace_id,
            plan_kind,
            state
                .frontdoor_runtime_guards
                .local_execution_planning_timeout,
        )
        .await
    }
    .instrument(span)
    .await
}

#[derive(Clone, Debug, Default)]
pub(crate) struct CandidateExecutionContext {
    daily_usage_outcome:
        std::sync::Arc<OnceCell<crate::daily_usage_limit::FrontdoorDailyUsageOutcome>>,
}

async fn run_dynamic_attempt_loop<Port, Source, Attempt>(
    port: &Port,
    source: &mut Source,
    trace_id: &str,
    plan_kind: &str,
    planning_timeout: Duration,
) -> Result<LocalExecutionRequestOutcome, GatewayError>
where
    Port: AiAttemptLoopPort<
        Attempt,
        Response = Response<Body>,
        Exhaustion = crate::executor::LocalExecutionExhaustion,
        Error = GatewayError,
    >,
    Source: LocalExecutionAttemptSource<Attempt>,
    Attempt: AiExecutionAttempt + Send + Sync + 'static,
{
    let mut last_attempted = None;
    let mut fallback_response = None;

    loop {
        let next_started_at = std::time::Instant::now();
        let next_attempt =
            next_execution_attempt_with_timeout(source, trace_id, plan_kind, planning_timeout)
                .await?;
        observe_gateway_stage_ms(
            "stream_candidate_next",
            next_started_at.elapsed().as_millis() as u64,
        );
        let Some(attempt) = next_attempt else {
            break;
        };
        if port.should_skip_attempt(&attempt).await? {
            let provider_id = attempt.execution_plan().provider_id.clone();
            port.mark_unused_attempts(vec![attempt]).await?;
            source.skip_provider(provider_id.as_str()).await?;
            continue;
        }
        port.record_attempt_started(&attempt).await?;
        let execute_started_at = std::time::Instant::now();
        let execution = match port.execute_attempt(&attempt).await {
            Ok(execution) => execution,
            Err(err) => {
                let remaining = source.drain_execution_attempts().await?;
                port.mark_unused_attempts(remaining).await?;
                return Err(err);
            }
        };
        observe_gateway_stage_ms(
            "stream_candidate_execute",
            execute_started_at.elapsed().as_millis() as u64,
        );
        match execution {
            AiAttemptExecutionOutcome::Responded(response) => {
                let remaining = source.drain_execution_attempts().await?;
                let unused_started_at = std::time::Instant::now();
                port.mark_unused_attempts(remaining).await?;
                observe_gateway_stage_ms(
                    "stream_candidate_unused",
                    unused_started_at.elapsed().as_millis() as u64,
                );
                return Ok(LocalExecutionRequestOutcome::responded(response));
            }
            AiAttemptExecutionOutcome::Retry {
                scope,
                fallback_response: attempt_fallback_response,
            } => {
                if attempt_fallback_response.is_some() {
                    fallback_response = attempt_fallback_response;
                }
                apply_attempt_retry_scope(source, &attempt, scope).await?;
            }
        }

        port.record_attempt_failed(&attempt).await?;
        if port.should_skip_attempt(&attempt).await? {
            source
                .skip_provider(attempt.execution_plan().provider_id.as_str())
                .await?;
        }

        // Only retain a deep plan/context snapshot when this candidate really
        // failed and exhaustion reporting will need it.
        last_attempted = Some((attempt.execution_plan().clone(), attempt.report_context()));
    }

    if let Some(response) = fallback_response {
        return Ok(LocalExecutionRequestOutcome::responded(
            mark_deferred_upstream_response(response),
        ));
    }

    let Some((last_plan, last_report_context)) = last_attempted else {
        return Ok(LocalExecutionRequestOutcome::NoPath);
    };

    Ok(LocalExecutionRequestOutcome::Exhausted(
        port.build_exhaustion(last_plan, last_report_context)
            .await?,
    ))
}

async fn apply_attempt_retry_scope<Source, Attempt>(
    source: &mut Source,
    attempt: &Attempt,
    scope: AiAttemptRetryScope,
) -> Result<(), GatewayError>
where
    Source: LocalExecutionAttemptSource<Attempt>,
    Attempt: AiExecutionAttempt,
{
    let plan = attempt.execution_plan();
    match scope {
        AiAttemptRetryScope::Candidate => Ok(()),
        AiAttemptRetryScope::Credential => source.skip_credential(plan.key_id.as_str()).await,
        AiAttemptRetryScope::Endpoint => source.skip_endpoint(plan.endpoint_id.as_str()).await,
        AiAttemptRetryScope::Provider => source.skip_provider(plan.provider_id.as_str()).await,
    }
}

async fn next_execution_attempt_with_timeout<Source, Attempt>(
    source: &mut Source,
    trace_id: &str,
    plan_kind: &str,
    planning_timeout: Duration,
) -> Result<Option<Attempt>, GatewayError>
where
    Source: LocalExecutionAttemptSource<Attempt>,
{
    match timeout(planning_timeout, source.next_execution_attempt()).await {
        Ok(result) => result,
        Err(_) => {
            let timeout_ms = planning_timeout.as_millis() as u64;
            warn!(
                event_name = "local_execution_candidate_planning_timeout",
                log_type = "ops",
                trace_id,
                plan_kind,
                timeout_ms,
                phase = "next_execution_attempt",
                "gateway timed out while planning the next local execution candidate"
            );
            Err(GatewayError::LocalExecutionPlanningTimeout {
                trace_id: trace_id.to_string(),
                phase: "next_execution_attempt",
                timeout_ms,
            })
        }
    }
}

struct StreamAttemptLoopPort<'a> {
    state: &'a AppState,
    trace_id: &'a str,
    decision: &'a GatewayControlDecision,
    plan_kind: &'a str,
    execution_context: &'a CandidateExecutionContext,
}

#[async_trait]
impl<T> AiAttemptLoopPort<T> for StreamAttemptLoopPort<'_>
where
    T: AiExecutionAttempt + Send + Sync + 'static,
{
    type Response = Response<Body>;
    type Exhaustion = crate::executor::LocalExecutionExhaustion;
    type Error = GatewayError;

    async fn execute_attempt(
        &self,
        attempt: &T,
    ) -> Result<AiAttemptExecutionOutcome<Self::Response>, Self::Error> {
        let plan = attempt.execution_plan();
        let report_context = attempt.report_context();
        let daily_usage_outcome = self
            .execution_context
            .daily_usage_outcome
            .get_or_init(|| async {
                self.state
                    .frontdoor_daily_usage()
                    .check(self.state, self.decision)
                    .await
            })
            .await;
        let candidate_index = parse_request_candidate_report_context(report_context.as_ref())
            .and_then(|context| context.candidate_index)
            .map(|value| value.to_string())
            .unwrap_or_else(|| "-".to_string());
        debug!(
            event_name = "candidate_loop_attempt_started",
            log_type = "debug",
            trace_id = %self.trace_id,
            plan_kind = self.plan_kind,
            request_id = %short_request_id(plan.request_id.as_str()),
            candidate_id = ?plan.candidate_id,
            provider_name = plan.provider_name.as_deref().unwrap_or("-"),
            endpoint_id = %plan.endpoint_id,
            key_id = %plan.key_id,
            model_name = plan.model_name.as_deref().unwrap_or("-"),
            candidate_index = candidate_index.as_str(),
            "candidate loop attempting stream execution candidate"
        );
        if let Some(response) = execution_plan_balance_capacity_response(
            self.state,
            self.trace_id,
            self.decision,
            daily_usage_outcome,
            plan,
            report_context.as_ref(),
        )
        .await?
        {
            return Ok(AiAttemptExecutionOutcome::Responded(response));
        }
        prewarm_direct_reqwest_candidate_client(plan);
        // The attempt owns the canonical report context. Borrow it for the
        // watchdog; only third-party/synthesized attempts using the default
        // trait implementation need an owned fallback clone.
        let watchdog_report_context_owned = if attempt.report_context_ref().is_none() {
            report_context.clone()
        } else {
            None
        };
        let watchdog_report_context = attempt
            .report_context_ref()
            .or(watchdog_report_context_owned.as_ref());
        let execution_state = self.state.clone();
        let execution_trace_id = self.trace_id.to_string();
        let execution_plan_kind = self.plan_kind.to_string();
        let execution_decision = self.decision.clone();
        let execution_report_kind = attempt.report_kind();
        let execution_plan = plan.clone();
        let stop_on_transport_errors = matches!(
            resolve_local_transport_failover_analysis_for_attempt(
                self.state,
                plan,
                watchdog_report_context,
            )
            .await
            .decision,
            LocalFailoverDecision::StopLocalFailover
        );
        let watchdog_started_at = std::time::Instant::now();
        let execution = execute_stream_candidate_with_watchdog(
            self.state,
            self.trace_id,
            self.plan_kind,
            plan,
            watchdog_report_context,
            stop_on_transport_errors,
            move || async move {
                execute_execution_runtime_stream_with_retry_scope(
                    &execution_state,
                    execution_plan,
                    execution_trace_id.as_str(),
                    &execution_decision,
                    execution_plan_kind.as_str(),
                    execution_report_kind,
                    report_context,
                )
                .await
            },
        )
        .await?;
        let mut execution = match execution {
            StreamCandidateWatchdogOutcome::TransportTimeout => {
                AiAttemptExecutionOutcome::Responded(
                    build_transport_error_stop_response(
                        self.state,
                        plan,
                        watchdog_report_context,
                        self.trace_id,
                        self.decision,
                        http::StatusCode::GATEWAY_TIMEOUT.as_u16(),
                        "local_stream_candidate_watchdog_timeout",
                        stream_candidate_watchdog_timeout_message(),
                        watchdog_started_at.elapsed().as_millis() as u64,
                    )
                    .await?,
                )
            }
            StreamCandidateWatchdogOutcome::Executed(execution) => execution,
        };
        match &mut execution {
            AiAttemptExecutionOutcome::Responded(response)
            | AiAttemptExecutionOutcome::Retry {
                fallback_response: Some(response),
                ..
            } => attach_redaction_execution_candidate(response, plan.candidate_id.as_deref()),
            AiAttemptExecutionOutcome::Retry {
                fallback_response: None,
                ..
            } => {}
        }
        Ok(execution)
    }

    async fn mark_unused_attempts(&self, attempts: Vec<T>) -> Result<(), Self::Error> {
        mark_unused_local_candidates(self.state, attempts).await;
        Ok(())
    }

    async fn build_exhaustion(
        &self,
        last_plan: aether_contracts::ExecutionPlan,
        last_report_context: Option<serde_json::Value>,
    ) -> Result<Self::Exhaustion, Self::Error> {
        warn!(
            event_name = "candidate_loop_exhausted",
            log_type = "ops",
            trace_id = %self.trace_id,
            plan_kind = self.plan_kind,
            request_id = %short_request_id(last_plan.request_id.as_str()),
            candidate_id = ?last_plan.candidate_id,
            provider_name = last_plan.provider_name.as_deref().unwrap_or("-"),
            endpoint_id = %last_plan.endpoint_id,
            key_id = %last_plan.key_id,
            model_name = last_plan.model_name.as_deref().unwrap_or("-"),
            "candidate loop exhausted local stream candidates"
        );
        Ok(
            build_local_execution_exhaustion(self.state, &last_plan, last_report_context.as_ref())
                .await,
        )
    }
}

fn prewarm_direct_reqwest_candidate_client(plan: &aether_contracts::ExecutionPlan) {
    let started_at = std::time::Instant::now();
    crate::execution_runtime::transport::prewarm_direct_reqwest_client_cache_for_plan(plan);
    observe_gateway_stage_ms(
        "direct_reqwest_client_prewarm",
        started_at.elapsed().as_millis() as u64,
    );
}

async fn execution_plan_balance_capacity_response(
    state: &AppState,
    trace_id: &str,
    decision: &GatewayControlDecision,
    daily_usage_outcome: &crate::daily_usage_limit::FrontdoorDailyUsageOutcome,
    plan: &aether_contracts::ExecutionPlan,
    report_context: Option<&serde_json::Value>,
) -> Result<Option<Response<Body>>, GatewayError> {
    if let crate::daily_usage_limit::FrontdoorDailyUsageOutcome::Rejected(rejection) =
        daily_usage_outcome
    {
        if !crate::control::execution_plan_cost_is_proven_zero(state, plan, report_context).await {
            let auth = decision.auth_context.as_ref();
            info!(
                event_name = "frontdoor_daily_usage_rejected",
                log_type = "event",
                trace_id,
                user_id = auth.map(|auth| auth.user_id.as_str()).unwrap_or("-"),
                api_key_id = auth.map(|auth| auth.api_key_id.as_str()).unwrap_or("-"),
                scope = rejection.scope,
                limit_usd = rejection.limit_usd,
                used_usd = rejection.used_usd,
                retry_after = rejection.retry_after,
                "gateway rejected candidate at daily usage limit"
            );
            mark_unused_local_candidate(state, plan, report_context).await;
            let mut response = crate::api::response::build_local_daily_usage_limited_response(
                trace_id,
                Some(decision),
                rejection,
            )?;
            attach_redaction_execution_candidate(&mut response, plan.candidate_id.as_deref());
            return Ok(Some(response));
        }
    }

    let rejection = match crate::control::execution_plan_balance_capacity_rejection(
        state,
        decision,
        plan,
        report_context,
    )
    .await
    {
        Ok(rejection) => rejection,
        Err(err) => {
            mark_unused_local_candidate(state, plan, report_context).await;
            return Err(err);
        }
    };
    let Some(rejection) = rejection else {
        return Ok(None);
    };
    mark_unused_local_candidate(state, plan, report_context).await;
    let mut response = crate::api::response::build_local_auth_rejection_response(
        trace_id,
        Some(decision),
        &rejection,
    )?;
    attach_redaction_execution_candidate(&mut response, plan.candidate_id.as_deref());
    Ok(Some(response))
}

pub(crate) async fn mark_unused_local_candidates<T>(state: &AppState, remaining: Vec<T>)
where
    T: AiExecutionAttempt,
{
    for plan_and_report in remaining {
        let report_context = plan_and_report.report_context();
        mark_unused_local_candidate(
            state,
            plan_and_report.execution_plan(),
            report_context.as_ref(),
        )
        .await;
    }
}

async fn mark_unused_local_candidate(
    state: &AppState,
    plan: &aether_contracts::ExecutionPlan,
    report_context: Option<&serde_json::Value>,
) {
    record_local_request_candidate_status(
        state,
        plan,
        report_context,
        SchedulerRequestCandidateStatusUpdate {
            status: RequestCandidateStatus::Unused,
            status_code: None,
            error_type: None,
            error_message: None,
            latency_ms: None,
            started_at_unix_ms: None,
            finished_at_unix_ms: None,
        },
    )
    .await;
}

fn resolve_stream_candidate_watchdog_timeout(
    plan: &aether_contracts::ExecutionPlan,
    _report_context: Option<&serde_json::Value>,
) -> Duration {
    let timeout_ms = plan
        .timeouts
        .as_ref()
        .and_then(|timeouts| timeouts.first_byte_ms)
        .unwrap_or(DEFAULT_STREAM_FIRST_BYTE_WATCHDOG_TIMEOUT_MS)
        .max(1);
    Duration::from_millis(timeout_ms)
}

fn stream_candidate_watchdog_timeout_message() -> &'static str {
    "Stream first byte timeout"
}

fn admission_timeout_gate(error: &GatewayError) -> Option<&'static str> {
    match error {
        GatewayError::AdmissionTimeout { gate, .. } => Some(*gate),
        _ => None,
    }
}

fn admission_timeout_message(error: &GatewayError) -> String {
    match error {
        GatewayError::AdmissionTimeout {
            gate,
            queue_budget_ms,
            ..
        } => {
            format!("gateway admission gate {gate} timed out after {queue_budget_ms}ms")
        }
        other => format!("{other:?}"),
    }
}

fn is_candidate_level_admission_timeout(error: &GatewayError) -> bool {
    matches!(
        admission_timeout_gate(error),
        Some(UPSTREAM_EXECUTION_GATE_NAME | UPSTREAM_TARGET_GATE_NAME)
    )
}

fn should_record_candidate_admission_timeout(error: &GatewayError) -> bool {
    matches!(
        admission_timeout_gate(error),
        Some(UPSTREAM_EXECUTION_GATE_NAME)
    )
}

async fn record_stream_candidate_admission_timeout(
    state: &(impl RequestCandidateRuntimeWriter + ?Sized),
    plan: &aether_contracts::ExecutionPlan,
    report_context: Option<&serde_json::Value>,
    candidate_started_unix_ms: u64,
    error: &GatewayError,
) {
    let terminal_unix_ms = current_unix_ms();
    record_local_request_candidate_status(
        state,
        plan,
        report_context,
        SchedulerRequestCandidateStatusUpdate {
            status: RequestCandidateStatus::Failed,
            status_code: Some(http::StatusCode::TOO_MANY_REQUESTS.as_u16()),
            error_type: Some("gateway_admission_timeout".to_string()),
            error_message: Some(admission_timeout_message(error)),
            latency_ms: Some(terminal_unix_ms.saturating_sub(candidate_started_unix_ms)),
            started_at_unix_ms: Some(candidate_started_unix_ms),
            finished_at_unix_ms: Some(terminal_unix_ms),
        },
    )
    .await;
}

fn log_stream_candidate_admission_timeout(
    trace_id: &str,
    plan_kind: &str,
    plan: &aether_contracts::ExecutionPlan,
    report_context: Option<&serde_json::Value>,
    error: &GatewayError,
) {
    let provider_name = plan.provider_name.as_deref().unwrap_or("-");
    let model_name = plan.model_name.as_deref().unwrap_or("-");
    let candidate_index = parse_request_candidate_report_context(report_context)
        .and_then(|context| context.candidate_index)
        .map(|value| value.to_string())
        .unwrap_or_else(|| "-".to_string());
    let (gate, queue_budget_ms) = match error {
        GatewayError::AdmissionTimeout {
            gate,
            queue_budget_ms,
            ..
        } => (*gate, *queue_budget_ms),
        _ => ("-", 0),
    };
    warn!(
        event_name = "local_stream_candidate_admission_timeout",
        log_type = "event",
        trace_id = %trace_id,
        plan_kind,
        request_id = %short_request_id(plan.request_id.as_str()),
        candidate_id = ?plan.candidate_id,
        provider_name,
        endpoint_id = %plan.endpoint_id,
        key_id = %plan.key_id,
        model_name,
        candidate_index = candidate_index.as_str(),
        gate,
        queue_budget_ms,
        "gateway local stream candidate admission timed out; retrying next candidate"
    );
}

#[derive(Debug)]
enum StreamCandidateWatchdogOutcome {
    Executed(AiAttemptExecutionOutcome<Response<Body>>),
    TransportTimeout,
}

async fn execute_stream_candidate_with_watchdog<Fut>(
    state: &(impl RequestCandidateRuntimeWriter + UpstreamExecutionGateProvider + ?Sized),
    trace_id: &str,
    plan_kind: &str,
    plan: &aether_contracts::ExecutionPlan,
    report_context: Option<&serde_json::Value>,
    stop_on_transport_errors: bool,
    execute: impl FnOnce() -> Fut,
) -> Result<StreamCandidateWatchdogOutcome, GatewayError>
where
    Fut: std::future::Future<
            Output = Result<AiAttemptExecutionOutcome<Response<Body>>, GatewayError>,
        > + Send,
{
    let timeout_duration = resolve_stream_candidate_watchdog_timeout(plan, report_context);
    let candidate_started_at = std::time::Instant::now();
    let candidate_started_unix_ms = current_unix_ms();
    let permit = match acquire_upstream_execution_gate(state, trace_id).await {
        Ok(permit) => permit,
        Err(err) if is_candidate_level_admission_timeout(&err) => {
            record_stream_candidate_admission_timeout(
                state,
                plan,
                report_context,
                candidate_started_unix_ms,
                &err,
            )
            .await;
            log_stream_candidate_admission_timeout(trace_id, plan_kind, plan, report_context, &err);
            return Ok(StreamCandidateWatchdogOutcome::Executed(
                AiAttemptExecutionOutcome::retry(AiAttemptRetryScope::Candidate),
            ));
        }
        Err(err) => return Err(err),
    };
    let permit_hold = permit.map(UpstreamExecutionPermitHold::new);
    let watchdog_started_at = std::time::Instant::now();
    let watchdog_progress = StreamCandidateWatchdogProgress::shared();
    let execution = watchdog_progress.clone().scope(execute());
    tokio::pin!(execution);
    let deadline = tokio::time::sleep(timeout_duration);
    tokio::pin!(deadline);
    let execution_result = tokio::select! {
        biased;
        result = &mut execution => Some(result),
        () = &mut deadline => {
            if watchdog_progress.terminal_started() {
                Some(execution.await)
            } else {
                None
            }
        }
    };
    let outcome = match execution_result {
        Some(result) => result.map(StreamCandidateWatchdogOutcome::Executed),
        None => {
            let finished_at_unix_ms = current_unix_ms();
            let request_id = short_request_id(plan.request_id.as_str());
            let provider_name = plan.provider_name.as_deref().unwrap_or("-");
            let model_name = plan.model_name.as_deref().unwrap_or("-");
            let candidate_index = parse_request_candidate_report_context(report_context)
                .and_then(|context| context.candidate_index)
                .map(|value| value.to_string())
                .unwrap_or_else(|| "-".to_string());
            let timeout_ms = u64::try_from(timeout_duration.as_millis()).unwrap_or(u64::MAX);
            record_local_request_candidate_status(
                state,
                plan,
                report_context,
                SchedulerRequestCandidateStatusUpdate {
                    status: RequestCandidateStatus::Failed,
                    status_code: None,
                    error_type: Some("local_stream_candidate_watchdog_timeout".to_string()),
                    error_message: Some(stream_candidate_watchdog_timeout_message().to_string()),
                    latency_ms: Some(candidate_started_at.elapsed().as_millis() as u64),
                    started_at_unix_ms: Some(candidate_started_unix_ms),
                    finished_at_unix_ms: Some(finished_at_unix_ms),
                },
            )
            .await;
            warn!(
                event_name = "local_stream_candidate_watchdog_timed_out",
                log_type = "event",
                trace_id = %trace_id,
                plan_kind,
                request_id = %request_id,
                candidate_id = ?plan.candidate_id,
                provider_name,
                endpoint_id = %plan.endpoint_id,
                key_id = %plan.key_id,
                model_name,
                candidate_index = candidate_index.as_str(),
                timeout_ms,
                "gateway local stream candidate watchdog timed out"
            );
            if stop_on_transport_errors {
                Ok(StreamCandidateWatchdogOutcome::TransportTimeout)
            } else {
                Ok(StreamCandidateWatchdogOutcome::Executed(
                    AiAttemptExecutionOutcome::retry(AiAttemptRetryScope::Candidate),
                ))
            }
        }
    };
    observe_gateway_stage_ms(
        "stream_candidate_watchdog_inline",
        watchdog_started_at.elapsed().as_millis() as u64,
    );
    match outcome {
        Ok(StreamCandidateWatchdogOutcome::Executed(AiAttemptExecutionOutcome::Responded(
            response,
        ))) => {
            let response = maybe_hold_upstream_execution_permit(Some(response), permit_hold)
                .expect("responded stream attempt must retain its response");
            Ok(StreamCandidateWatchdogOutcome::Executed(
                AiAttemptExecutionOutcome::Responded(response),
            ))
        }
        Ok(StreamCandidateWatchdogOutcome::Executed(AiAttemptExecutionOutcome::Retry {
            scope,
            fallback_response,
        })) => {
            drop(permit_hold);
            Ok(StreamCandidateWatchdogOutcome::Executed(
                AiAttemptExecutionOutcome::Retry {
                    scope,
                    fallback_response,
                },
            ))
        }
        Ok(StreamCandidateWatchdogOutcome::TransportTimeout) => {
            drop(permit_hold);
            Ok(StreamCandidateWatchdogOutcome::TransportTimeout)
        }
        Err(err) if is_candidate_level_admission_timeout(&err) => {
            drop(permit_hold);
            if should_record_candidate_admission_timeout(&err) {
                record_stream_candidate_admission_timeout(
                    state,
                    plan,
                    report_context,
                    candidate_started_unix_ms,
                    &err,
                )
                .await;
            }
            log_stream_candidate_admission_timeout(trace_id, plan_kind, plan, report_context, &err);
            Ok(StreamCandidateWatchdogOutcome::Executed(
                AiAttemptExecutionOutcome::retry(AiAttemptRetryScope::Candidate),
            ))
        }
        Err(err) => {
            drop(permit_hold);
            Err(err)
        }
    }
}

struct UpstreamExecutionPermitHold {
    _permit: ConcurrencyPermit,
    started_at: std::time::Instant,
}

impl UpstreamExecutionPermitHold {
    fn new(permit: ConcurrencyPermit) -> Self {
        Self {
            _permit: permit,
            started_at: std::time::Instant::now(),
        }
    }
}

impl Drop for UpstreamExecutionPermitHold {
    fn drop(&mut self) {
        observe_gateway_stage_ms(
            "upstream_execution_gate_held",
            self.started_at.elapsed().as_millis() as u64,
        );
    }
}

fn maybe_hold_upstream_execution_permit(
    response: Option<Response<Body>>,
    permit_hold: Option<UpstreamExecutionPermitHold>,
) -> Option<Response<Body>> {
    match upstream_execution_gate_stream_hold_mode() {
        UpstreamExecutionStreamHoldMode::Headers => {
            drop(permit_hold);
            response
        }
        UpstreamExecutionStreamHoldMode::FirstBody => match (response, permit_hold) {
            (Some(response), Some(permit_hold)) => Some(
                hold_response_upstream_execution_permit_until_first_body(response, permit_hold),
            ),
            (response, _permit_hold) => response,
        },
        UpstreamExecutionStreamHoldMode::Response => match (response, permit_hold) {
            (Some(response), Some(permit_hold)) => Some(hold_response_upstream_execution_permit(
                response,
                permit_hold,
            )),
            (response, _permit_hold) => response,
        },
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UpstreamExecutionStreamHoldMode {
    Headers,
    FirstBody,
    Response,
}

fn upstream_execution_gate_stream_hold_mode() -> UpstreamExecutionStreamHoldMode {
    if std::env::var(UPSTREAM_EXECUTION_GATE_HOLD_STREAM_RESPONSE_ENV)
        .ok()
        .is_some_and(|value| parse_env_bool(value.as_str()))
    {
        return UpstreamExecutionStreamHoldMode::Response;
    }
    std::env::var(UPSTREAM_EXECUTION_GATE_STREAM_HOLD_MODE_ENV)
        .ok()
        .as_deref()
        .map(parse_upstream_execution_stream_hold_mode)
        .unwrap_or(UpstreamExecutionStreamHoldMode::FirstBody)
}

fn parse_upstream_execution_stream_hold_mode(value: &str) -> UpstreamExecutionStreamHoldMode {
    match value.trim().to_ascii_lowercase().as_str() {
        "headers" | "header" | "off" | "none" | "disabled" | "disable" | "0" => {
            UpstreamExecutionStreamHoldMode::Headers
        }
        "response" | "full" | "body" | "stream" | "1" => UpstreamExecutionStreamHoldMode::Response,
        _ => UpstreamExecutionStreamHoldMode::FirstBody,
    }
}

fn parse_env_bool(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

fn hold_response_upstream_execution_permit_until_first_body(
    response: Response<Body>,
    permit_hold: UpstreamExecutionPermitHold,
) -> Response<Body> {
    let (parts, body) = response.into_parts();
    let stream = async_stream::stream! {
        let mut permit_hold = Some(permit_hold);
        let mut body_stream = body.into_data_stream();
        while let Some(item) = body_stream.next().await {
            drop(permit_hold.take());
            yield item;
        }
    };
    Response::from_parts(parts, Body::from_stream(stream))
}

fn hold_response_upstream_execution_permit(
    response: Response<Body>,
    permit_hold: UpstreamExecutionPermitHold,
) -> Response<Body> {
    let (parts, body) = response.into_parts();
    let stream = async_stream::stream! {
        let _permit_hold = permit_hold;
        let mut body_stream = body.into_data_stream();
        while let Some(item) = body_stream.next().await {
            yield item;
        }
    };
    Response::from_parts(parts, Body::from_stream(stream))
}

trait UpstreamExecutionGateProvider {
    fn upstream_execution_gate(&self) -> Option<&aether_runtime::ConcurrencyGate>;
    fn upstream_execution_gate_queue_budget(&self) -> Duration;
}

impl UpstreamExecutionGateProvider for AppState {
    fn upstream_execution_gate(&self) -> Option<&aether_runtime::ConcurrencyGate> {
        self.upstream_execution_gate.as_deref()
    }

    fn upstream_execution_gate_queue_budget(&self) -> Duration {
        self.frontdoor_runtime_guards.internal_gate_queue_budget
    }
}

async fn acquire_upstream_execution_gate(
    state: &(impl UpstreamExecutionGateProvider + ?Sized),
    trace_id: &str,
) -> Result<Option<ConcurrencyPermit>, GatewayError> {
    let Some(gate) = state.upstream_execution_gate() else {
        return Ok(None);
    };
    let budget = state.upstream_execution_gate_queue_budget();
    let gate_wait_started_at = std::time::Instant::now();
    match timeout(budget, gate.acquire()).await {
        Ok(Ok(permit)) => {
            observe_gateway_stage_ms(
                "upstream_execution_gate_wait",
                gate_wait_started_at.elapsed().as_millis() as u64,
            );
            Ok(Some(permit))
        }
        Ok(Err(err)) => Err(GatewayError::Internal(err.to_string())),
        Err(_) => Err(GatewayError::AdmissionTimeout {
            trace_id: trace_id.to_string(),
            gate: UPSTREAM_EXECUTION_GATE_NAME,
            queue_budget_ms: budget.as_millis() as u64,
        }),
    }
}

pub(crate) async fn mark_unused_local_candidate_items<T, FPlan, FContext>(
    state: &AppState,
    remaining: Vec<T>,
    plan: FPlan,
    report_context: FContext,
) where
    FPlan: Fn(&T) -> &aether_contracts::ExecutionPlan,
    FContext: Fn(&T) -> Option<&serde_json::Value>,
{
    for item in remaining {
        let report_context = report_context(&item);
        record_local_request_candidate_status(
            state,
            plan(&item),
            report_context,
            SchedulerRequestCandidateStatusUpdate {
                status: RequestCandidateStatus::Unused,
                status_code: None,
                error_type: None,
                error_message: None,
                latency_ms: None,
                started_at_unix_ms: None,
                finished_at_unix_ms: None,
            },
        )
        .await;
    }
}
