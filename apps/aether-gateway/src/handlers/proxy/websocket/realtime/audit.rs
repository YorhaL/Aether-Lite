//! One terminal usage row per OpenAI Realtime WebSocket connection.

use std::time::Duration;

use aether_contracts::ExecutionPlan;
use aether_usage_runtime::build_usage_event_data_seed;
use serde_json::{json, Map, Value};
use tracing::warn;

use crate::usage::{UsageEvent, UsageEventType};
use crate::AppState;

use super::protocol::RealtimeUsageTotals;

const REALTIME_AUDIT_WRITE_WAIT: Duration = Duration::from_secs(5);
const REALTIME_AUDIT_LOG_TARGET: &str = "aether_gateway::handlers::proxy::realtime_ws";

pub(super) struct RealtimeSessionAudit {
    plan: ExecutionPlan,
    report_context: Option<Value>,
}

impl RealtimeSessionAudit {
    pub(super) fn new(plan: &ExecutionPlan, report_context: Option<&Value>) -> Self {
        Self {
            plan: plan.clone(),
            report_context: report_context.cloned(),
        }
    }

    pub(super) async fn finish(self, state: &AppState, terminal: RealtimeSessionTerminal) {
        let request_id = self.plan.request_id.clone();
        let event = self.build_terminal_event(terminal);
        let usage_runtime = std::sync::Arc::clone(&state.usage_runtime);
        let usage_data = std::sync::Arc::clone(state.usage_lifecycle_data_state());
        let task = tokio::spawn(async move {
            usage_runtime
                .record_terminal_event_direct(usage_data.as_ref(), event)
                .await;
        });
        match tokio::time::timeout(REALTIME_AUDIT_WRITE_WAIT, task).await {
            Ok(Ok(())) => {}
            Ok(Err(error)) => warn!(
                target: REALTIME_AUDIT_LOG_TARGET,
                event_name = "openai_realtime_session_audit_task_failed",
                log_type = "ops",
                request_id,
                error = %error,
                "OpenAI Realtime session audit task failed"
            ),
            Err(_) => warn!(
                target: REALTIME_AUDIT_LOG_TARGET,
                event_name = "openai_realtime_session_audit_write_slow",
                log_type = "ops",
                request_id,
                wait_ms = REALTIME_AUDIT_WRITE_WAIT.as_millis() as u64,
                write_detached = true,
                "OpenAI Realtime stopped waiting for a slow session audit write"
            ),
        }
    }

    fn build_terminal_event(self, terminal: RealtimeSessionTerminal) -> UsageEvent {
        let usage_available = terminal.usage.responses > 0;
        let pricing_available = usage_available
            && terminal.usage.input_audio_tokens == 0
            && terminal.usage.output_audio_tokens == 0;
        let mut data = build_usage_event_data_seed(&self.plan, self.report_context.as_ref());
        data.request_type = Some("realtime".to_string());
        data.is_stream = Some(true);
        data.status_code = Some(terminal.status_code);
        data.response_time_ms = Some(terminal.elapsed_ms);
        data.first_byte_time_ms = terminal.first_upstream_frame_ms;
        // The Lite billing pipeline understands text token pricing but has no
        // audio-token price dimensions. Keep audio usage in metadata and omit
        // priceable top-level counters rather than charging it as text.
        if pricing_available {
            data.input_tokens = Some(terminal.usage.input_tokens);
            data.output_tokens = Some(terminal.usage.output_tokens);
            data.total_tokens = Some(terminal.usage.total_tokens);
            data.cache_read_input_tokens = Some(terminal.usage.cached_input_tokens);
        } else {
            clear_usage_and_cost(&mut data);
        }
        data.cache_creation_cost_usd = None;
        data.cache_read_cost_usd = None;
        data.total_cost_usd = None;
        data.actual_total_cost_usd = None;
        if terminal.disposition != RealtimeSessionDisposition::Completed {
            data.error_message = Some(terminal.termination.to_string());
            data.error_category = Some(terminal.disposition.error_category().to_string());
        }
        data.request_metadata = attach_terminal_metadata(
            data.request_metadata,
            usage_available,
            pricing_available,
            &terminal,
        );
        UsageEvent::new(
            terminal.disposition.event_type(),
            self.plan.request_id,
            data,
        )
    }
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

fn attach_terminal_metadata(
    metadata: Option<Value>,
    usage_available: bool,
    pricing_available: bool,
    terminal: &RealtimeSessionTerminal,
) -> Option<Value> {
    let mut object = match metadata {
        Some(Value::Object(object)) => object,
        _ => Map::new(),
    };
    object.insert("usage_available".to_string(), Value::Bool(usage_available));
    object.insert(
        "usage_pricing_available".to_string(),
        Value::Bool(pricing_available),
    );
    object.insert("websocket_mode".to_string(), Value::Bool(true));
    object.insert(
        "websocket_transport".to_string(),
        Value::String("openai_realtime".to_string()),
    );
    object.insert(
        "realtime_session".to_string(),
        json!({
            "schema_version": "1",
            "transport": "websocket",
            "state": terminal.disposition.state(),
            "termination": terminal.termination,
            "elapsed_ms": terminal.elapsed_ms,
            "client_frames": terminal.client_frames,
            "client_bytes": terminal.client_bytes,
            "upstream_frames": terminal.upstream_frames,
            "upstream_bytes": terminal.upstream_bytes,
            "first_upstream_frame_ms": terminal.first_upstream_frame_ms,
            "usage_state": if usage_available { "authoritative" } else { "unavailable" },
            "pricing_state": if !usage_available {
                "usage_unavailable"
            } else if pricing_available {
                "compatible_text_usage"
            } else {
                "unsupported_audio_breakdown"
            },
            "usage_scope": "response_done",
            "usage_response_count": terminal.usage.responses,
            "input_tokens": terminal.usage.input_tokens,
            "output_tokens": terminal.usage.output_tokens,
            "total_tokens": terminal.usage.total_tokens,
            "cached_input_tokens": terminal.usage.cached_input_tokens,
            "input_audio_tokens": terminal.usage.input_audio_tokens,
            "output_audio_tokens": terminal.usage.output_audio_tokens,
        }),
    );
    Some(Value::Object(object))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RealtimeSessionDisposition {
    Completed,
    Failed,
    Cancelled,
}

impl RealtimeSessionDisposition {
    const fn event_type(self) -> UsageEventType {
        match self {
            Self::Completed => UsageEventType::Completed,
            Self::Failed => UsageEventType::Failed,
            Self::Cancelled => UsageEventType::Cancelled,
        }
    }

    const fn state(self) -> &'static str {
        match self {
            Self::Completed => "closed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    const fn error_category(self) -> &'static str {
        match self {
            Self::Completed => "none",
            Self::Failed => "transport_error",
            Self::Cancelled => "client_cancelled",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct RealtimeSessionTerminal {
    pub(super) disposition: RealtimeSessionDisposition,
    pub(super) status_code: u16,
    pub(super) termination: &'static str,
    pub(super) elapsed_ms: u64,
    pub(super) first_upstream_frame_ms: Option<u64>,
    pub(super) client_frames: u64,
    pub(super) client_bytes: u64,
    pub(super) upstream_frames: u64,
    pub(super) upstream_bytes: u64,
    pub(super) usage: RealtimeUsageTotals,
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use aether_contracts::{ExecutionPlan, RequestBody};

    use super::*;

    fn sample_plan() -> ExecutionPlan {
        ExecutionPlan {
            request_id: "realtime-request".to_string(),
            candidate_id: Some("candidate-realtime".to_string()),
            provider_name: Some("OpenAI".to_string()),
            provider_id: "provider-realtime".to_string(),
            endpoint_id: "endpoint-realtime".to_string(),
            key_id: "key-realtime".to_string(),
            method: "GET".to_string(),
            url: "wss://example.test/v1/realtime?model=gpt-realtime".to_string(),
            headers: BTreeMap::new(),
            content_type: None,
            content_encoding: None,
            body: RequestBody {
                json_body: None,
                body_bytes_b64: None,
                body_ref: None,
            },
            stream: true,
            client_api_format: "openai:realtime".to_string(),
            provider_api_format: "openai:realtime".to_string(),
            model_name: Some("gpt-realtime".to_string()),
            transport_profile: None,
            timeouts: None,
        }
    }

    fn terminal(usage: RealtimeUsageTotals) -> RealtimeSessionTerminal {
        RealtimeSessionTerminal {
            disposition: RealtimeSessionDisposition::Completed,
            status_code: 200,
            termination: "client_close_frame",
            elapsed_ms: 1500,
            first_upstream_frame_ms: Some(30),
            client_frames: 4,
            client_bytes: 600,
            upstream_frames: 8,
            upstream_bytes: 1200,
            usage,
        }
    }

    #[test]
    fn text_usage_is_priceable_but_audio_usage_is_metadata_only() {
        let text = RealtimeSessionAudit::new(&sample_plan(), None).build_terminal_event(terminal(
            RealtimeUsageTotals {
                responses: 1,
                input_tokens: 12,
                output_tokens: 4,
                total_tokens: 16,
                cached_input_tokens: 2,
                ..RealtimeUsageTotals::default()
            },
        ));
        assert_eq!(text.data.total_tokens, Some(16));
        assert_eq!(
            text.data.request_metadata.as_ref().unwrap()["usage_pricing_available"],
            true
        );

        let audio = RealtimeSessionAudit::new(&sample_plan(), None).build_terminal_event(terminal(
            RealtimeUsageTotals {
                responses: 1,
                input_tokens: 12,
                output_tokens: 4,
                total_tokens: 16,
                input_audio_tokens: 5,
                ..RealtimeUsageTotals::default()
            },
        ));
        assert_eq!(audio.data.total_tokens, None);
        assert_eq!(
            audio.data.request_metadata.as_ref().unwrap()["realtime_session"]["total_tokens"],
            16
        );
        assert_eq!(
            audio.data.request_metadata.as_ref().unwrap()["usage_pricing_available"],
            false
        );
    }
}
