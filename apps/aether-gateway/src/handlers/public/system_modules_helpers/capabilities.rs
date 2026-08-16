use serde_json::json;

#[derive(Clone, Copy)]
pub(crate) struct PublicCapabilityDefinition {
    pub(crate) name: &'static str,
    pub(crate) display_name: &'static str,
    pub(crate) short_name: &'static str,
    pub(crate) description: &'static str,
    pub(crate) match_mode: &'static str,
    pub(crate) config_mode: &'static str,
}

pub(crate) const PUBLIC_CAPABILITY_DEFINITIONS: &[PublicCapabilityDefinition] = &[
    PublicCapabilityDefinition {
        name: "cache_1h",
        display_name: "1 小时缓存",
        short_name: "1h缓存",
        description: "使用 1 小时缓存 TTL（价格更高，适合长对话）",
        match_mode: "compatible",
        config_mode: "request_param",
    },
    PublicCapabilityDefinition {
        name: "context_1m",
        display_name: "CLI 1M 上下文",
        short_name: "CLI 1M",
        description: "支持 1M tokens 上下文窗口",
        match_mode: "compatible",
        config_mode: "request_param",
    },
];

pub(crate) fn serialize_public_capability(
    capability: PublicCapabilityDefinition,
) -> serde_json::Value {
    json!({
        "name": capability.name,
        "display_name": capability.display_name,
        "short_name": capability.short_name,
        "description": capability.description,
        "match_mode": capability.match_mode,
        "config_mode": capability.config_mode,
    })
}

pub(crate) fn capability_detail_by_name(name: &str) -> Option<serde_json::Value> {
    PUBLIC_CAPABILITY_DEFINITIONS
        .iter()
        .copied()
        .find(|capability| capability.name == name)
        .map(|capability| {
            json!({
                "name": capability.name,
                "display_name": capability.display_name,
                "description": capability.description,
                "match_mode": capability.match_mode,
                "config_mode": capability.config_mode,
            })
        })
}

fn capability_short_name_by_name(name: &str) -> Option<&'static str> {
    PUBLIC_CAPABILITY_DEFINITIONS
        .iter()
        .find(|capability| capability.name == name)
        .map(|capability| capability.short_name)
}

pub(crate) fn supported_capability_names(
    supported_capabilities: Option<&serde_json::Value>,
) -> Vec<String> {
    supported_capabilities
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(serde_json::Value::as_str)
        .map(ToOwned::to_owned)
        .collect()
}

pub(crate) fn enabled_key_capability_short_names(
    capabilities: Option<&serde_json::Value>,
) -> Vec<String> {
    capabilities
        .and_then(serde_json::Value::as_object)
        .into_iter()
        .flatten()
        .filter_map(|(name, enabled)| {
            enabled.as_bool().filter(|value| *value).map(|_| {
                capability_short_name_by_name(name)
                    .unwrap_or(name.as_str())
                    .to_string()
            })
        })
        .collect()
}
