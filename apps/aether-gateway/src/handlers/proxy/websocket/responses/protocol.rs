//! Validation and terminal observation for the standard Responses WebSocket protocol.

use std::collections::BTreeSet;

use serde_json::{json, Value};

const MAX_MODEL_BYTES: usize = 256;
const MAX_RESPONSE_ID_BYTES: usize = 256;
const MAX_OWNED_RESPONSE_IDS: usize = 1_024;

#[derive(Debug, Clone)]
pub(super) struct ResponseCreateEvent {
    pub(super) value: Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(super) enum ResponsesProtocolError {
    #[error("client message must be a JSON response.create event")]
    InvalidEvent,
    #[error("response.create.model is invalid")]
    InvalidModel,
    #[error("response.create.previous_response_id is invalid")]
    InvalidPreviousResponseId,
    #[error("response.create.stream_id is invalid")]
    InvalidStreamId,
    #[error("named response streams are not supported")]
    NamedStreamUnsupported,
    #[error("previous_response_id is not owned by this connection")]
    PreviousResponseNotOwned,
}

impl ResponsesProtocolError {
    pub(super) const fn code(self) -> &'static str {
        match self {
            Self::InvalidEvent => "invalid_response_create",
            Self::InvalidModel => "invalid_response_create_model",
            Self::InvalidPreviousResponseId => "invalid_response_create_previous_response_id",
            Self::InvalidStreamId => "invalid_response_create_stream_id",
            Self::NamedStreamUnsupported => "responses_websocket_named_stream_unsupported",
            Self::PreviousResponseNotOwned => "responses_websocket_previous_response_not_owned",
        }
    }

    pub(super) const fn message(self) -> &'static str {
        match self {
            Self::InvalidEvent => "Expected a JSON response.create event",
            Self::InvalidModel => {
                "response.create.model must be a non-empty identifier no longer than 256 bytes"
            }
            Self::InvalidPreviousResponseId => {
                "response.create.previous_response_id must be null or a non-empty identifier no longer than 256 bytes"
            }
            Self::InvalidStreamId => {
                "response.create.stream_id must be 1-256 ASCII letters, numbers, underscores, hyphens, or periods"
            }
            Self::NamedStreamUnsupported => {
                "Lite currently supports only the implicit default Responses WebSocket lane; omit response.create.stream_id"
            }
            Self::PreviousResponseNotOwned => {
                "previous_response_id must refer to a completed or incomplete response observed on this authenticated WebSocket connection"
            }
        }
    }

    pub(super) const fn param(self) -> Option<&'static str> {
        match self {
            Self::InvalidModel => Some("model"),
            Self::InvalidPreviousResponseId | Self::PreviousResponseNotOwned => {
                Some("previous_response_id")
            }
            Self::InvalidStreamId | Self::NamedStreamUnsupported => Some("stream_id"),
            Self::InvalidEvent => None,
        }
    }
}

pub(super) fn parse_response_create_event(
    raw: &str,
    owned_response_ids: &BTreeSet<String>,
) -> Result<ResponseCreateEvent, ResponsesProtocolError> {
    let value: Value =
        serde_json::from_str(raw).map_err(|_| ResponsesProtocolError::InvalidEvent)?;
    let object = value
        .as_object()
        .ok_or(ResponsesProtocolError::InvalidEvent)?;
    if object.get("type").and_then(Value::as_str) != Some("response.create") {
        return Err(ResponsesProtocolError::InvalidEvent);
    }
    validate_stream_id(&value)?;
    object
        .get("model")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|model| !model.is_empty() && model.len() <= MAX_MODEL_BYTES)
        .filter(|model| !model.chars().any(char::is_control))
        .ok_or(ResponsesProtocolError::InvalidModel)?;
    let previous_response_id = match object.get("previous_response_id") {
        None | Some(Value::Null) => None,
        Some(Value::String(value))
            if !value.trim().is_empty() && value.len() <= MAX_RESPONSE_ID_BYTES =>
        {
            Some(value.clone())
        }
        Some(_) => return Err(ResponsesProtocolError::InvalidPreviousResponseId),
    };
    if previous_response_id
        .as_ref()
        .is_some_and(|response_id| !owned_response_ids.contains(response_id))
    {
        return Err(ResponsesProtocolError::PreviousResponseNotOwned);
    }
    Ok(ResponseCreateEvent { value })
}

fn validate_stream_id(event: &Value) -> Result<(), ResponsesProtocolError> {
    let Some(value) = event.get("stream_id") else {
        return Ok(());
    };
    let Some(stream_id) = value.as_str() else {
        return Err(ResponsesProtocolError::InvalidStreamId);
    };
    if stream_id.is_empty()
        || stream_id.len() > 256
        || !stream_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
    {
        return Err(ResponsesProtocolError::InvalidStreamId);
    }
    Err(ResponsesProtocolError::NamedStreamUnsupported)
}

pub(super) fn gateway_error_event(
    status: u16,
    code: &str,
    message: &str,
    param: Option<&str>,
) -> Value {
    json!({
        "type": "error",
        "status": status,
        "error": {
            "type": "invalid_request_error",
            "code": code,
            "message": message,
            "param": param,
        }
    })
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(super) struct ResponsesUsage {
    pub(super) input_tokens: u64,
    pub(super) output_tokens: u64,
    pub(super) total_tokens: u64,
    pub(super) cached_input_tokens: u64,
    pub(super) reasoning_output_tokens: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ProviderTerminalKind {
    Completed,
    Incomplete,
    Failed,
    Error,
}

#[derive(Debug, Clone)]
pub(super) struct ProviderTerminal {
    pub(super) kind: ProviderTerminalKind,
    pub(super) response_id: Option<String>,
    pub(super) usage: Option<ResponsesUsage>,
    pub(super) event: Value,
}

pub(super) fn observe_provider_event(event: &Value) -> Option<ProviderTerminal> {
    let event_type = event.get("type").and_then(Value::as_str)?;
    let kind = match event_type {
        "response.completed" => ProviderTerminalKind::Completed,
        "response.incomplete" => ProviderTerminalKind::Incomplete,
        "response.failed" => ProviderTerminalKind::Failed,
        "error" => ProviderTerminalKind::Error,
        _ => return None,
    };
    let response = event.get("response");
    let response_id = response
        .and_then(|response| response.get("id"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty() && value.len() <= MAX_RESPONSE_ID_BYTES)
        .map(ToOwned::to_owned);
    let usage = response
        .and_then(|response| response.get("usage"))
        .and_then(parse_usage);
    Some(ProviderTerminal {
        kind,
        response_id,
        usage,
        event: event.clone(),
    })
}

fn parse_usage(value: &Value) -> Option<ResponsesUsage> {
    let object = value.as_object()?;
    let input_tokens = json_u64(object.get("input_tokens"));
    let output_tokens = json_u64(object.get("output_tokens"));
    let total_tokens = object
        .get("total_tokens")
        .and_then(Value::as_u64)
        .unwrap_or_else(|| input_tokens.saturating_add(output_tokens));
    let cached_input_tokens = object
        .get("input_tokens_details")
        .and_then(Value::as_object)
        .map(|details| json_u64(details.get("cached_tokens")))
        .unwrap_or(0);
    let reasoning_output_tokens = object
        .get("output_tokens_details")
        .and_then(Value::as_object)
        .map(|details| json_u64(details.get("reasoning_tokens")))
        .unwrap_or(0);
    Some(ResponsesUsage {
        input_tokens,
        output_tokens,
        total_tokens,
        cached_input_tokens,
        reasoning_output_tokens,
    })
}

fn json_u64(value: Option<&Value>) -> u64 {
    value.and_then(Value::as_u64).unwrap_or(0)
}

pub(super) fn retain_owned_response_id(ids: &mut BTreeSet<String>, response_id: String) {
    if ids.len() < MAX_OWNED_RESPONSE_IDS {
        ids.insert(response_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_default_lane_and_connection_owned_continuations() {
        let mut ids = BTreeSet::new();
        ids.insert("resp_owned".to_string());
        assert!(parse_response_create_event(
            r#"{"type":"response.create","model":"gpt-client","previous_response_id":"resp_owned"}"#,
            &ids
        )
        .is_ok());
        assert_eq!(
            parse_response_create_event(
                r#"{"type":"response.create","model":"gpt-client","previous_response_id":"resp_other"}"#,
                &ids
            )
            .unwrap_err(),
            ResponsesProtocolError::PreviousResponseNotOwned
        );
    }

    #[test]
    fn rejects_named_lanes_without_silently_merging_them() {
        assert_eq!(
            parse_response_create_event(
                r#"{"type":"response.create","stream_id":"main","model":"gpt-client"}"#,
                &BTreeSet::new()
            )
            .unwrap_err(),
            ResponsesProtocolError::NamedStreamUnsupported
        );
        assert_eq!(
            parse_response_create_event(
                r#"{"type":"response.create","stream_id":"bad/lane","model":"gpt-client"}"#,
                &BTreeSet::new()
            )
            .unwrap_err(),
            ResponsesProtocolError::InvalidStreamId
        );
    }

    #[test]
    fn observes_authoritative_terminal_usage() {
        let event = json!({
            "type": "response.completed",
            "response": {
                "id": "resp_1",
                "usage": {
                    "input_tokens": 12,
                    "output_tokens": 8,
                    "total_tokens": 20,
                    "input_tokens_details": {"cached_tokens": 4},
                    "output_tokens_details": {"reasoning_tokens": 3}
                }
            }
        });
        let terminal = observe_provider_event(&event).expect("terminal event");
        assert_eq!(terminal.response_id.as_deref(), Some("resp_1"));
        assert_eq!(terminal.usage.expect("usage").total_tokens, 20);
        assert_eq!(terminal.usage.expect("usage").reasoning_output_tokens, 3);
    }

    #[test]
    fn gateway_errors_keep_the_responses_websocket_status_shape() {
        let event = gateway_error_event(
            400,
            "invalid_response_create",
            "Expected response.create",
            None,
        );
        assert_eq!(event["type"], "error");
        assert_eq!(event["status"], 400);
        assert_eq!(event["error"]["code"], "invalid_response_create");
    }
}
