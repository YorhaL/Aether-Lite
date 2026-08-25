//! Bounded validation and observation for OpenAI Realtime events.

use std::collections::BTreeSet;

use serde_json::{json, Value};

const MAX_MODEL_BYTES: usize = 256;
const MAX_OBSERVED_RESPONSE_IDS: usize = 1_024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(super) enum RealtimeProtocolError {
    #[error("invalid Realtime model query")]
    InvalidModelQuery,
    #[error("invalid Realtime model")]
    InvalidModel,
}

impl RealtimeProtocolError {
    pub(super) const fn client_message(self) -> &'static str {
        match self {
            Self::InvalidModelQuery => {
                "Realtime WebSocket requires exactly one model query parameter"
            }
            Self::InvalidModel => {
                "Realtime model must be a non-empty identifier no longer than 256 bytes"
            }
        }
    }
}

pub(super) fn model_from_query(query: Option<&str>) -> Result<String, RealtimeProtocolError> {
    let mut model = None;
    for (name, value) in url::form_urlencoded::parse(query.unwrap_or_default().as_bytes()) {
        if name.eq_ignore_ascii_case("model") {
            if model.is_some() {
                return Err(RealtimeProtocolError::InvalidModelQuery);
            }
            validate_model(value.as_ref())?;
            model = Some(value.into_owned());
        } else if query_parameter_is_sensitive(name.as_ref()) {
            return Err(RealtimeProtocolError::InvalidModelQuery);
        }
    }
    model.ok_or(RealtimeProtocolError::InvalidModelQuery)
}

fn validate_model(model: &str) -> Result<(), RealtimeProtocolError> {
    if model.is_empty()
        || model.len() > MAX_MODEL_BYTES
        || model.trim() != model
        || model.chars().any(char::is_control)
    {
        return Err(RealtimeProtocolError::InvalidModel);
    }
    Ok(())
}

fn query_parameter_is_sensitive(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "key"
            | "api_key"
            | "api-key"
            | "x-api-key"
            | "access_token"
            | "authorization"
            | "token"
            | "client_secret"
            | "secret_key"
            | "signature"
            | "sig"
    )
}

pub(super) fn error_event(code: &str, message: &str) -> Value {
    json!({
        "type": "error",
        "error": {
            "type": "server_error",
            "code": code,
            "message": message,
        }
    })
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(super) struct RealtimeUsageTotals {
    pub(super) responses: u64,
    pub(super) input_tokens: u64,
    pub(super) output_tokens: u64,
    pub(super) total_tokens: u64,
    pub(super) cached_input_tokens: u64,
    pub(super) input_audio_tokens: u64,
    pub(super) output_audio_tokens: u64,
}

#[derive(Debug, Default)]
pub(super) struct RealtimeUsageObserver {
    totals: RealtimeUsageTotals,
    response_ids: BTreeSet<String>,
}

impl RealtimeUsageObserver {
    pub(super) fn observe(&mut self, raw: &str) {
        let Ok(event) = serde_json::from_str::<Value>(raw) else {
            return;
        };
        if event.get("type").and_then(Value::as_str) != Some("response.done") {
            return;
        }
        let Some(response) = event.get("response").and_then(Value::as_object) else {
            return;
        };
        let Some(usage) = response.get("usage").and_then(Value::as_object) else {
            return;
        };
        if let Some(response_id) = response
            .get("id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            if self.response_ids.contains(response_id)
                || self.response_ids.len() >= MAX_OBSERVED_RESPONSE_IDS
            {
                return;
            }
            self.response_ids.insert(response_id.to_string());
        }
        let input_tokens = json_u64(usage.get("input_tokens"));
        let output_tokens = json_u64(usage.get("output_tokens"));
        let total_tokens = usage
            .get("total_tokens")
            .and_then(Value::as_u64)
            .unwrap_or_else(|| input_tokens.saturating_add(output_tokens));
        self.totals.responses = self.totals.responses.saturating_add(1);
        self.totals.input_tokens = self.totals.input_tokens.saturating_add(input_tokens);
        self.totals.output_tokens = self.totals.output_tokens.saturating_add(output_tokens);
        self.totals.total_tokens = self.totals.total_tokens.saturating_add(total_tokens);
        if let Some(details) = usage.get("input_token_details").and_then(Value::as_object) {
            self.totals.cached_input_tokens = self
                .totals
                .cached_input_tokens
                .saturating_add(json_u64(details.get("cached_tokens")));
            self.totals.input_audio_tokens = self
                .totals
                .input_audio_tokens
                .saturating_add(json_u64(details.get("audio_tokens")));
        }
        if let Some(details) = usage.get("output_token_details").and_then(Value::as_object) {
            self.totals.output_audio_tokens = self
                .totals
                .output_audio_tokens
                .saturating_add(json_u64(details.get("audio_tokens")));
        }
    }

    pub(super) const fn totals(&self) -> RealtimeUsageTotals {
        self.totals
    }
}

fn json_u64(value: Option<&Value>) -> u64 {
    value.and_then(Value::as_u64).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::{model_from_query, RealtimeUsageObserver};

    #[test]
    fn model_query_requires_one_bounded_model() {
        assert_eq!(
            model_from_query(Some("trace=1&model=gpt-realtime-client")),
            Ok("gpt-realtime-client".to_string())
        );
        assert!(model_from_query(None).is_err());
        assert!(model_from_query(Some("model=a&MODEL=b")).is_err());
        assert!(model_from_query(Some("model=a&key=secret")).is_err());
        assert!(model_from_query(Some(format!("model={}", "x".repeat(257)).as_str())).is_err());
    }

    #[test]
    fn response_done_usage_is_observed_once() {
        let event = serde_json::json!({
            "type": "response.done",
            "response": {
                "id": "resp_1",
                "usage": {
                    "input_tokens": 12,
                    "output_tokens": 7,
                    "total_tokens": 19,
                    "input_token_details": {"cached_tokens": 4, "audio_tokens": 3},
                    "output_token_details": {"audio_tokens": 2}
                }
            }
        })
        .to_string();
        let mut observer = RealtimeUsageObserver::default();
        observer.observe(event.as_str());
        observer.observe(event.as_str());

        let totals = observer.totals();
        assert_eq!(totals.responses, 1);
        assert_eq!(totals.total_tokens, 19);
        assert_eq!(totals.cached_input_tokens, 4);
        assert_eq!(totals.input_audio_tokens, 3);
        assert_eq!(totals.output_audio_tokens, 2);
    }
}
