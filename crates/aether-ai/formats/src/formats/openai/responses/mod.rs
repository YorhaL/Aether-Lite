use serde_json::Value;

pub mod spec;

pub const OPENAI_RESPONSES_OPERATION_COMPACT: &str = "compact";

pub fn openai_responses_request_operation(api_format: &str, body: &Value) -> Option<&'static str> {
    if aether_ai_formats::is_openai_responses_compact_format(api_format) {
        return Some(OPENAI_RESPONSES_OPERATION_COMPACT);
    }
    if !aether_ai_formats::is_openai_responses_format(api_format) {
        return None;
    }

    body.get("input")
        .and_then(Value::as_array)
        .is_some_and(|items| {
            items
                .iter()
                .any(|item| item.get("type").and_then(Value::as_str) == Some("compaction_trigger"))
        })
        .then_some(OPENAI_RESPONSES_OPERATION_COMPACT)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{openai_responses_request_operation, OPENAI_RESPONSES_OPERATION_COMPACT};

    #[test]
    fn resolves_compaction_trigger_as_compact_operation_on_responses_transport() {
        assert_eq!(
            openai_responses_request_operation(
                "openai:responses",
                &json!({"input": [{"type": "compaction_trigger"}]})
            ),
            Some(OPENAI_RESPONSES_OPERATION_COMPACT)
        );
        assert_eq!(
            openai_responses_request_operation(
                "openai:responses",
                &json!({"input": [{"role": "user", "content": "keep working"}]})
            ),
            None
        );
    }
}
