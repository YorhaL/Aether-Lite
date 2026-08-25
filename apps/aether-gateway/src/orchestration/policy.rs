use std::collections::BTreeSet;

use aether_contracts::ExecutionPlan;
use serde_json::{json, Value};
use tracing::debug;

use crate::provider_transport::GatewayProviderTransportSnapshot;
use crate::AppState;

pub(crate) const CYBER_CONTINUE_FAILOVER_CONFIG_KEY: &str = "cyber_continue_failover";
pub(crate) const RESPONSES_WEBSOCKET_CONFIG_KEY: &str = "responses_websocket";

/// Responses WebSocket support is opt-in per provider because an ordinary
/// HTTP-compatible `/v1/responses` endpoint does not necessarily implement
/// the WebSocket upgrade protocol.
pub(crate) fn responses_websocket_enabled(provider_config: Option<&Value>) -> bool {
    provider_config
        .and_then(|config| config.get(RESPONSES_WEBSOCKET_CONFIG_KEY))
        .and_then(Value::as_object)
        .and_then(|responses| responses.get("enabled"))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LocalFailoverPolicy {
    pub(crate) max_retries: Option<u64>,
    pub(crate) stop_status_codes: BTreeSet<u16>,
    pub(crate) continue_status_codes: BTreeSet<u16>,
    pub(crate) stop_on_transport_errors: bool,
    pub(crate) success_failover_patterns: Vec<LocalFailoverRegexRule>,
    pub(crate) error_stop_patterns: Vec<LocalFailoverRegexRule>,
    pub(crate) stop_cyber_policy_errors: bool,
    pub(crate) retry_client_errors_by_default: bool,
}

impl Default for LocalFailoverPolicy {
    fn default() -> Self {
        Self {
            max_retries: None,
            stop_status_codes: BTreeSet::new(),
            continue_status_codes: BTreeSet::new(),
            stop_on_transport_errors: false,
            success_failover_patterns: Vec::new(),
            error_stop_patterns: Vec::new(),
            stop_cyber_policy_errors: true,
            retry_client_errors_by_default: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LocalFailoverRegexRule {
    pub(crate) pattern: String,
    pub(crate) status_codes: BTreeSet<u16>,
}

pub(crate) async fn resolve_local_failover_policy(
    state: &AppState,
    plan: &ExecutionPlan,
    _report_context: Option<&serde_json::Value>,
) -> LocalFailoverPolicy {
    let mut policy = match state
        .read_provider_transport_snapshot(&plan.provider_id, &plan.endpoint_id, &plan.key_id)
        .await
    {
        Ok(Some(transport)) => local_failover_policy_from_transport(&transport),
        Ok(None) | Err(_) => LocalFailoverPolicy::default(),
    };
    let cyber_continue_failover = cyber_continue_failover_enabled(state).await;
    policy.stop_cyber_policy_errors = !cyber_continue_failover;
    debug!(
        event_name = "local_failover_policy_loaded",
        log_type = "debug",
        request_id = %plan.request_id,
        provider_id = %plan.provider_id,
        endpoint_id = %plan.endpoint_id,
        key_id = %plan.key_id,
        source = "transport_snapshot",
        max_retries = ?policy.max_retries,
        stop_status_code_count = policy.stop_status_codes.len(),
        continue_status_code_count = policy.continue_status_codes.len(),
        stop_on_transport_errors = policy.stop_on_transport_errors,
        success_failover_pattern_count = policy.success_failover_patterns.len(),
        error_stop_pattern_count = policy.error_stop_patterns.len(),
        cyber_continue_failover,
        "gateway loaded local failover policy from transport snapshot"
    );
    policy
}

pub(crate) async fn cyber_continue_failover_enabled(state: &AppState) -> bool {
    state
        .read_system_config_json_value(CYBER_CONTINUE_FAILOVER_CONFIG_KEY)
        .await
        .ok()
        .flatten()
        .as_ref()
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

pub(crate) fn local_failover_policy_from_transport(
    transport: &GatewayProviderTransportSnapshot,
) -> LocalFailoverPolicy {
    let rules = transport
        .provider
        .config
        .as_ref()
        .and_then(|config| config.get("failover_rules"))
        .and_then(Value::as_object);
    let max_retries = rules
        .and_then(|value| value.get("max_retries"))
        .and_then(parse_u64_value)
        .or_else(|| {
            transport
                .endpoint
                .max_retries
                .and_then(|value| u64::try_from(value).ok())
        })
        .or_else(|| {
            transport
                .provider
                .max_retries
                .and_then(|value| u64::try_from(value).ok())
        });

    LocalFailoverPolicy {
        max_retries,
        retry_client_errors_by_default:
            crate::ai_serving::api_format_defaults_to_client_error_failover(
                &transport.endpoint.api_format,
            ),
        stop_cyber_policy_errors: true,
        stop_status_codes: rules
            .map(|value| {
                parse_status_code_set(
                    value,
                    &[
                        "stop_on_status_codes",
                        "early_stop_status_codes",
                        "non_retryable_status_codes",
                        "stop_status_codes",
                    ],
                )
            })
            .unwrap_or_default(),
        continue_status_codes: rules
            .map(|value| {
                parse_status_code_set(
                    value,
                    &[
                        "continue_on_status_codes",
                        "retryable_status_codes",
                        "retry_on_status_codes",
                        "continue_status_codes",
                    ],
                )
            })
            .unwrap_or_default(),
        stop_on_transport_errors: rules
            .and_then(|value| value.get("stop_on_transport_errors"))
            .and_then(Value::as_bool)
            .unwrap_or(false),
        success_failover_patterns: rules
            .map(|value| parse_regex_rules(value, "success_failover_patterns"))
            .unwrap_or_default(),
        error_stop_patterns: rules
            .map(|value| parse_regex_rules(value, "error_stop_patterns"))
            .unwrap_or_default(),
    }
}

pub(crate) fn local_failover_policy_from_report_context(
    report_context: Option<&Value>,
) -> Option<LocalFailoverPolicy> {
    let object = report_context
        .and_then(Value::as_object)?
        .get("local_failover_policy")?
        .as_object()?;

    Some(LocalFailoverPolicy {
        max_retries: object.get("max_retries").and_then(parse_u64_value),
        stop_status_codes: object
            .get("stop_status_codes")
            .map(parse_status_code_list)
            .unwrap_or_default(),
        continue_status_codes: object
            .get("continue_status_codes")
            .map(parse_status_code_list)
            .unwrap_or_default(),
        stop_on_transport_errors: object
            .get("stop_on_transport_errors")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        success_failover_patterns: parse_regex_rules(object, "success_failover_patterns"),
        error_stop_patterns: parse_regex_rules(object, "error_stop_patterns"),
        stop_cyber_policy_errors: object
            .get("stop_cyber_policy_errors")
            .and_then(Value::as_bool)
            .unwrap_or(true),
        retry_client_errors_by_default: object
            .get("retry_client_errors_by_default")
            .and_then(Value::as_bool)
            .unwrap_or(true),
    })
}

pub(crate) fn append_local_failover_policy_to_value(
    value: Value,
    transport: &GatewayProviderTransportSnapshot,
) -> Value {
    let Value::Object(mut object) = value else {
        return value;
    };
    object.insert(
        "local_failover_policy".to_string(),
        local_failover_policy_to_value(&local_failover_policy_from_transport(transport)),
    );
    Value::Object(object)
}

fn parse_status_code_list(value: &Value) -> BTreeSet<u16> {
    value
        .as_array()
        .into_iter()
        .flat_map(|values| values.iter())
        .filter_map(|value| parse_u64_value(value).and_then(|value| u16::try_from(value).ok()))
        .collect()
}

fn local_failover_policy_to_value(policy: &LocalFailoverPolicy) -> Value {
    json!({
        "max_retries": policy.max_retries,
        "stop_status_codes": policy.stop_status_codes.iter().copied().collect::<Vec<_>>(),
        "continue_status_codes": policy.continue_status_codes.iter().copied().collect::<Vec<_>>(),
        "stop_on_transport_errors": policy.stop_on_transport_errors,
        "success_failover_patterns": policy.success_failover_patterns.iter().map(local_failover_regex_rule_to_value).collect::<Vec<_>>(),
        "error_stop_patterns": policy.error_stop_patterns.iter().map(local_failover_regex_rule_to_value).collect::<Vec<_>>(),
        "stop_cyber_policy_errors": policy.stop_cyber_policy_errors,
        "retry_client_errors_by_default": policy.retry_client_errors_by_default,
    })
}

fn local_failover_regex_rule_to_value(rule: &LocalFailoverRegexRule) -> Value {
    json!({
        "pattern": rule.pattern,
        "status_codes": rule.status_codes.iter().copied().collect::<Vec<_>>(),
    })
}

fn parse_regex_rules(
    rules: &serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> Vec<LocalFailoverRegexRule> {
    let allow_status_only = key == "error_stop_patterns";
    rules
        .get(key)
        .and_then(Value::as_array)
        .into_iter()
        .flat_map(|items| items.iter())
        .filter_map(|value| parse_regex_rule(value, allow_status_only))
        .collect()
}

fn parse_regex_rule(
    value: &serde_json::Value,
    allow_status_only: bool,
) -> Option<LocalFailoverRegexRule> {
    let object = value.as_object()?;
    let pattern = object
        .get("pattern")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default();
    let status_codes: BTreeSet<u16> = object
        .get("status_codes")
        .and_then(Value::as_array)
        .into_iter()
        .flat_map(|values| values.iter())
        .filter_map(|value| parse_u64_value(value).and_then(|value| u16::try_from(value).ok()))
        .collect();
    if pattern.is_empty() && (!allow_status_only || status_codes.is_empty()) {
        return None;
    }
    Some(LocalFailoverRegexRule {
        pattern: pattern.to_string(),
        status_codes,
    })
}

fn parse_status_code_set(
    rules: &serde_json::Map<String, serde_json::Value>,
    keys: &[&str],
) -> BTreeSet<u16> {
    keys.iter()
        .filter_map(|key| rules.get(*key))
        .filter_map(Value::as_array)
        .flat_map(|values| values.iter())
        .filter_map(|value| parse_u64_value(value).and_then(|value| u16::try_from(value).ok()))
        .collect()
}

fn parse_u64_value(value: &serde_json::Value) -> Option<u64> {
    value
        .as_u64()
        .or_else(|| value.as_i64().and_then(|value| u64::try_from(value).ok()))
}

#[cfg(test)]
mod responses_websocket_tests {
    use super::responses_websocket_enabled;

    #[test]
    fn responses_websocket_is_provider_opt_in() {
        assert!(!responses_websocket_enabled(None));
        assert!(!responses_websocket_enabled(Some(&serde_json::json!({
            "responses_websocket": {"enabled": false}
        }))));
        assert!(responses_websocket_enabled(Some(&serde_json::json!({
            "responses_websocket": {"enabled": true}
        }))));
    }
}
