use aether_contracts::{ExecutionStreamTerminalSummary, StandardizedUsage};
use serde_json::Value;

#[derive(Default)]
pub struct StreamingStandardTerminalObserver {
    latest_summary: Option<ExecutionStreamTerminalSummary>,
    pending_event: Option<String>,
}

impl StreamingStandardTerminalObserver {
    pub fn push_line(&mut self, _report_context: &Value, line: Vec<u8>) {
        let Ok(text) = std::str::from_utf8(&line) else {
            return;
        };
        let trimmed = text.trim();
        if let Some(event) = trimmed.strip_prefix("event:").map(str::trim) {
            self.pending_event = (!event.is_empty()).then(|| event.to_string());
            return;
        }
        if trimmed.is_empty() || trimmed.starts_with(':') {
            return;
        }

        let data = trimmed
            .strip_prefix("data:")
            .map(str::trim)
            .unwrap_or(trimmed);
        if data == "[DONE]" {
            self.summary_mut().observed_finish = true;
            return;
        }
        let Ok(mut payload) = serde_json::from_str::<Value>(data) else {
            return;
        };
        if payload.get("type").is_none() {
            if let Some(event) = self.pending_event.take() {
                if let Some(object) = payload.as_object_mut() {
                    object.insert("type".to_string(), Value::String(event));
                }
            }
        }
        self.observe_payload(&payload);
    }

    pub fn finish(&mut self, _report_context: &Value) -> Option<ExecutionStreamTerminalSummary> {
        self.latest_summary.clone()
    }

    pub fn disable_with_error(&mut self, parser_error: impl Into<String>) {
        let summary = self.summary_mut();
        if summary.parser_error.is_none() {
            summary.parser_error = Some(parser_error.into());
        }
    }

    pub fn latest_summary(&self) -> Option<&ExecutionStreamTerminalSummary> {
        self.latest_summary.as_ref()
    }

    fn observe_payload(&mut self, payload: &Value) {
        let response = payload.get("response").unwrap_or(payload);
        let summary = self.summary_mut();

        if summary.response_id.is_none() {
            summary.response_id = string_field(response, &["id", "response_id"])
                .or_else(|| string_field(payload, &["id", "response_id"]));
        }
        if summary.model.is_none() {
            summary.model =
                string_field(response, &["model"]).or_else(|| string_field(payload, &["model"]));
        }
        if summary.provider_actual_service_tier.is_none() {
            summary.provider_actual_service_tier =
                string_field(response, &["service_tier", "serviceTier"])
                    .or_else(|| string_field(payload, &["service_tier", "serviceTier"]));
        }

        if let Some(usage) = extract_usage(payload) {
            summary.standardized_usage = StandardizedUsage::choose_more_complete(
                summary.standardized_usage.take(),
                Some(usage),
            );
        }
        if let Some(reason) = extract_finish_reason(payload) {
            summary.finish_reason = Some(reason);
            summary.observed_finish = true;
        }

        let event_type = payload
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if matches!(
            event_type,
            "response.completed"
                | "response.failed"
                | "response.incomplete"
                | "message_stop"
                | "error"
        ) {
            summary.observed_finish = true;
        }
        if payload.get("error").is_some_and(|value| !value.is_null())
            || response.get("error").is_some_and(|value| !value.is_null())
            || matches!(event_type, "response.failed" | "error")
        {
            summary.finish_reason = Some("error".to_string());
            summary.parser_error = extract_error_message(payload);
        }
    }

    fn summary_mut(&mut self) -> &mut ExecutionStreamTerminalSummary {
        self.latest_summary
            .get_or_insert_with(ExecutionStreamTerminalSummary::default)
    }
}

fn string_field(value: &Value, fields: &[&str]) -> Option<String> {
    fields.iter().find_map(|field| {
        value
            .get(*field)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
    })
}

fn extract_usage(payload: &Value) -> Option<StandardizedUsage> {
    let usage = payload
        .get("usage")
        .or_else(|| payload.pointer("/response/usage"))
        .or_else(|| payload.pointer("/message/usage"))
        .or_else(|| payload.get("usageMetadata"))?
        .as_object()?;

    let mut standardized = StandardizedUsage::new();
    standardized.input_tokens = integer_field(
        usage,
        &[
            "input_tokens",
            "prompt_tokens",
            "promptTokenCount",
            "inputTokens",
        ],
    );
    standardized.output_tokens = integer_field(
        usage,
        &[
            "output_tokens",
            "completion_tokens",
            "candidatesTokenCount",
            "outputTokens",
        ],
    );
    standardized.cache_creation_tokens = integer_field(
        usage,
        &["cache_creation_input_tokens", "cache_creation_tokens"],
    );
    standardized.cache_read_tokens =
        integer_field(usage, &["cache_read_input_tokens", "cached_tokens"]);
    standardized.reasoning_tokens =
        integer_field(usage, &["reasoning_tokens", "thoughtsTokenCount"]);
    let total_tokens = integer_field(usage, &["total_tokens", "totalTokenCount", "totalTokens"]);
    if total_tokens > 0 {
        standardized
            .dimensions
            .insert("total_tokens".to_string(), Value::from(total_tokens));
    }
    standardized.has_token_signal().then_some(standardized)
}

fn integer_field(object: &serde_json::Map<String, Value>, fields: &[&str]) -> i64 {
    fields
        .iter()
        .find_map(|field| {
            object.get(*field).and_then(|value| {
                value
                    .as_i64()
                    .or_else(|| value.as_u64().and_then(|v| i64::try_from(v).ok()))
            })
        })
        .unwrap_or(0)
}

fn extract_finish_reason(payload: &Value) -> Option<String> {
    string_field(payload, &["finish_reason", "stop_reason"])
        .or_else(|| string_field(payload.get("delta")?, &["stop_reason"]))
        .or_else(|| {
            payload
                .pointer("/choices/0/finish_reason")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
        })
        .or_else(|| {
            payload
                .pointer("/candidates/0/finishReason")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
        })
        .or_else(|| {
            payload
                .pointer("/response/status")
                .and_then(Value::as_str)
                .filter(|status| matches!(*status, "completed" | "failed" | "incomplete"))
                .map(ToOwned::to_owned)
        })
}

fn extract_error_message(payload: &Value) -> Option<String> {
    payload
        .pointer("/error/message")
        .or_else(|| payload.pointer("/response/error/message"))
        .or_else(|| payload.get("message"))
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
}
