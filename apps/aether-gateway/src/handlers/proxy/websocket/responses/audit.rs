//! Per-turn usage and candidate lifecycle for Responses WebSocket mode.

use std::time::{Duration, Instant};

use aether_data_contracts::repository::candidates::RequestCandidateStatus;
use aether_scheduler_core::SchedulerRequestCandidateStatusUpdate;
use aether_usage_runtime::{build_lifecycle_usage_seed, build_usage_event_data_seed};
use serde_json::{json, Map, Value};
use tracing::warn;

use crate::request_candidate_runtime::{
    ensure_execution_request_candidate_slot, record_local_request_candidate_status,
};
use crate::usage::{UsageEvent, UsageEventType};
use crate::AppState;

use super::protocol::{ProviderTerminal, ProviderTerminalKind};

const AUDIT_WRITE_WAIT: Duration = Duration::from_secs(5);

pub(super) struct ResponsesTurnAudit {
    plan: aether_contracts::ExecutionPlan,
    report_context: Option<Value>,
    lifecycle_seed: aether_usage_runtime::LifecycleUsageSeed,
    started_at: Instant,
    started_at_unix_ms: u64,
    turn_index: u64,
    first_event_elapsed_ms: Option<u64>,
    client_delivery_failed: bool,
}

impl ResponsesTurnAudit {
    pub(super) async fn begin(
        state: &AppState,
        mut plan: aether_contracts::ExecutionPlan,
        mut report_context: Option<Value>,
        turn_index: u64,
    ) -> Self {
        ensure_execution_request_candidate_slot(state, &mut plan, &mut report_context).await;
        let started_at_unix_ms = crate::clock::current_unix_ms();
        record_local_request_candidate_status(
            state,
            &plan,
            report_context.as_ref(),
            candidate_update(
                RequestCandidateStatus::Pending,
                None,
                None,
                None,
                Some(started_at_unix_ms),
                None,
            ),
        )
        .await;
        let lifecycle_seed = build_lifecycle_usage_seed(&plan, report_context.as_ref());
        state
            .usage_runtime
            .record_pending_direct(
                state.usage_lifecycle_data_state().as_ref(),
                lifecycle_seed.clone(),
            )
            .await;
        Self {
            plan,
            report_context,
            lifecycle_seed,
            started_at: Instant::now(),
            started_at_unix_ms,
            turn_index,
            first_event_elapsed_ms: None,
            client_delivery_failed: false,
        }
    }

    pub(super) fn plan(&self) -> &aether_contracts::ExecutionPlan {
        &self.plan
    }

    pub(super) fn report_context(&self) -> Option<&Value> {
        self.report_context.as_ref()
    }

    pub(super) async fn observe_first_event(&mut self, state: &AppState) {
        if self.first_event_elapsed_ms.is_some() {
            return;
        }
        let elapsed_ms = elapsed_ms(self.started_at);
        self.first_event_elapsed_ms = Some(elapsed_ms);
        state.usage_runtime.record_stream_started(
            state.usage_lifecycle_data_state().as_ref(),
            &self.lifecycle_seed,
            200,
            None,
        );
        record_local_request_candidate_status(
            state,
            &self.plan,
            self.report_context.as_ref(),
            candidate_update(
                RequestCandidateStatus::Streaming,
                Some(200),
                None,
                Some(elapsed_ms),
                Some(self.started_at_unix_ms),
                None,
            ),
        )
        .await;
    }

    pub(super) fn observe_client_delivery_failed(&mut self) {
        self.client_delivery_failed = true;
    }

    pub(super) async fn finish(
        self,
        state: &AppState,
        outcome: ResponsesTurnOutcome,
        terminal: Option<&ProviderTerminal>,
    ) {
        let elapsed = elapsed_ms(self.started_at);
        let finished_at = crate::clock::current_unix_ms();
        let (event_type, candidate_status, status_code) = outcome.lifecycle();
        let error = (event_type != UsageEventType::Completed).then(|| outcome.reason().to_string());
        record_local_request_candidate_status(
            state,
            &self.plan,
            self.report_context.as_ref(),
            candidate_update(
                candidate_status,
                Some(status_code),
                error.clone(),
                Some(elapsed),
                Some(self.started_at_unix_ms),
                Some(finished_at),
            ),
        )
        .await;

        let mut data = build_usage_event_data_seed(&self.plan, self.report_context.as_ref());
        data.request_type = Some("responses".to_string());
        data.is_stream = Some(true);
        data.status_code = Some(status_code);
        data.response_time_ms = Some(elapsed);
        data.first_byte_time_ms = self.first_event_elapsed_ms;
        let usage = terminal.and_then(|terminal| terminal.usage);
        if let Some(usage) = usage {
            data.input_tokens = Some(usage.input_tokens);
            data.output_tokens = Some(usage.output_tokens);
            data.total_tokens = Some(usage.total_tokens);
            data.cache_read_input_tokens = Some(usage.cached_input_tokens);
        } else {
            clear_usage_and_cost(&mut data);
        }
        if let Some(error) = error {
            data.error_message = Some(error);
            data.error_category = Some(outcome.error_category().to_string());
        }
        if let Some(terminal) = terminal {
            data.response_body = terminal.event.get("response").cloned();
            data.client_response_body = data.response_body.clone();
        }
        data.request_metadata = attach_metadata(
            data.request_metadata,
            self.turn_index,
            outcome,
            usage,
            terminal,
            self.client_delivery_failed,
        );
        let event = UsageEvent::new(event_type, self.plan.request_id.clone(), data);
        let runtime = std::sync::Arc::clone(&state.usage_runtime);
        let usage_data = std::sync::Arc::clone(state.usage_lifecycle_data_state());
        let request_id = self.plan.request_id;
        let task = tokio::spawn(async move {
            runtime
                .record_terminal_event_direct(usage_data.as_ref(), event)
                .await;
        });
        match tokio::time::timeout(AUDIT_WRITE_WAIT, task).await {
            Ok(Ok(())) => {}
            Ok(Err(error)) => warn!(
                event_name = "responses_websocket_turn_audit_task_failed",
                log_type = "ops",
                request_id,
                error = %error,
                "Responses WebSocket turn audit task failed"
            ),
            Err(_) => warn!(
                event_name = "responses_websocket_turn_audit_write_slow",
                log_type = "ops",
                request_id,
                write_detached = true,
                "Responses WebSocket stopped waiting for a slow turn audit write"
            ),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ResponsesTurnOutcome {
    Completed,
    Incomplete,
    ProviderFailed,
    ProviderError,
    ClientDisconnected,
    ConnectionLimit,
    ConnectionAdmissionLost,
    UpstreamClosed,
    UpstreamReadFailed,
    UpstreamWriteFailed,
    UpstreamConnectFailed,
    FirstEventTimeout,
    TurnTimeout,
}

impl ResponsesTurnOutcome {
    pub(super) const fn from_provider_terminal(terminal: &ProviderTerminal) -> Self {
        match terminal.kind {
            ProviderTerminalKind::Completed => Self::Completed,
            ProviderTerminalKind::Incomplete => Self::Incomplete,
            ProviderTerminalKind::Failed => Self::ProviderFailed,
            ProviderTerminalKind::Error => Self::ProviderError,
        }
    }

    const fn lifecycle(self) -> (UsageEventType, RequestCandidateStatus, u16) {
        match self {
            Self::Completed | Self::Incomplete => (
                UsageEventType::Completed,
                RequestCandidateStatus::Success,
                200,
            ),
            Self::ClientDisconnected | Self::ConnectionLimit => (
                UsageEventType::Cancelled,
                RequestCandidateStatus::Cancelled,
                499,
            ),
            Self::ConnectionAdmissionLost => {
                (UsageEventType::Failed, RequestCandidateStatus::Failed, 503)
            }
            Self::FirstEventTimeout | Self::TurnTimeout => {
                (UsageEventType::Failed, RequestCandidateStatus::Failed, 504)
            }
            Self::ProviderFailed
            | Self::ProviderError
            | Self::UpstreamClosed
            | Self::UpstreamReadFailed
            | Self::UpstreamWriteFailed
            | Self::UpstreamConnectFailed => {
                (UsageEventType::Failed, RequestCandidateStatus::Failed, 502)
            }
        }
    }

    pub(super) const fn reason(self) -> &'static str {
        match self {
            Self::Completed => "response_completed",
            Self::Incomplete => "response_incomplete",
            Self::ProviderFailed => "response_failed",
            Self::ProviderError => "provider_error",
            Self::ClientDisconnected => "client_disconnected",
            Self::ConnectionLimit => "connection_duration_limit",
            Self::ConnectionAdmissionLost => "connection_admission_lost",
            Self::UpstreamClosed => "upstream_closed",
            Self::UpstreamReadFailed => "upstream_read_failed",
            Self::UpstreamWriteFailed => "upstream_write_failed",
            Self::UpstreamConnectFailed => "upstream_connect_failed",
            Self::FirstEventTimeout => "first_event_timeout",
            Self::TurnTimeout => "turn_timeout",
        }
    }

    const fn error_category(self) -> &'static str {
        match self {
            Self::ClientDisconnected | Self::ConnectionLimit => "client_cancelled",
            Self::ConnectionAdmissionLost => "admission_lost",
            Self::FirstEventTimeout | Self::TurnTimeout => "timeout",
            Self::Completed | Self::Incomplete => "none",
            _ => "transport_error",
        }
    }
}

fn candidate_update(
    status: RequestCandidateStatus,
    status_code: Option<u16>,
    error_message: Option<String>,
    latency_ms: Option<u64>,
    started_at_unix_ms: Option<u64>,
    finished_at_unix_ms: Option<u64>,
) -> SchedulerRequestCandidateStatusUpdate {
    SchedulerRequestCandidateStatusUpdate {
        status,
        status_code,
        error_type: error_message
            .as_ref()
            .map(|_| "responses_websocket_turn".to_string()),
        error_message,
        latency_ms,
        started_at_unix_ms,
        finished_at_unix_ms,
    }
}

fn attach_metadata(
    metadata: Option<Value>,
    turn_index: u64,
    outcome: ResponsesTurnOutcome,
    usage: Option<super::protocol::ResponsesUsage>,
    terminal: Option<&ProviderTerminal>,
    client_delivery_failed: bool,
) -> Option<Value> {
    let mut object = match metadata {
        Some(Value::Object(object)) => object,
        _ => Map::new(),
    };
    object.insert("websocket_mode".to_string(), Value::Bool(true));
    object.insert(
        "websocket_transport".to_string(),
        Value::String("responses".to_string()),
    );
    object.insert("usage_available".to_string(), Value::Bool(usage.is_some()));
    object.insert(
        "responses_websocket_turn".to_string(),
        json!({
            "schema_version": "1",
            "turn_index": turn_index,
            "termination": outcome.reason(),
            "client_delivery_failed": client_delivery_failed,
            "response_id": terminal.and_then(|terminal| terminal.response_id.as_deref()),
            "reasoning_output_tokens": usage.map(|usage| usage.reasoning_output_tokens),
        }),
    );
    Some(Value::Object(object))
}

fn clear_usage_and_cost(data: &mut crate::usage::UsageEventData) {
    data.input_tokens = None;
    data.output_tokens = None;
    data.total_tokens = None;
    data.cache_creation_input_tokens = None;
    data.cache_creation_ephemeral_5m_input_tokens = None;
    data.cache_creation_ephemeral_1h_input_tokens = None;
    data.cache_read_input_tokens = None;
    data.cache_creation_cost_usd = None;
    data.cache_read_cost_usd = None;
    data.total_cost_usd = None;
    data.actual_total_cost_usd = None;
}

fn elapsed_ms(started_at: Instant) -> u64 {
    started_at.elapsed().as_millis().min(u128::from(u64::MAX)) as u64
}

#[cfg(test)]
mod tests {
    use super::{attach_metadata, ResponsesTurnOutcome};
    use crate::usage::UsageEventType;

    #[test]
    fn completed_provider_outcome_stays_billable_when_client_delivery_failed() {
        let (event_type, _, _) = ResponsesTurnOutcome::Completed.lifecycle();
        assert_eq!(event_type, UsageEventType::Completed);
        let metadata = attach_metadata(None, 1, ResponsesTurnOutcome::Completed, None, None, true)
            .expect("Responses metadata should be present");
        assert_eq!(
            metadata["responses_websocket_turn"]["client_delivery_failed"],
            true
        );
        assert_eq!(
            metadata["responses_websocket_turn"]["termination"],
            "response_completed"
        );
    }
}
