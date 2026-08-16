use aether_ai_serving::AiRequestGzipPolicy;
use serde_json::Value;

use super::state::GatewayProviderTransportSnapshot;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct TransportRequestEncodingPolicy {
    pub content_encoding: Option<String>,
    pub request_gzip: Option<AiRequestGzipPolicy>,
}

pub(crate) fn resolve_transport_request_encoding_policy(
    transport: &GatewayProviderTransportSnapshot,
) -> TransportRequestEncodingPolicy {
    let request_gzip = transport_request_gzip_policy_from_config(
        transport.endpoint.config.as_ref(),
    )
    .or_else(|| transport_request_gzip_policy_from_config(transport.provider.config.as_ref()));
    if request_gzip.is_some() {
        return TransportRequestEncodingPolicy {
            content_encoding: None,
            request_gzip,
        };
    }

    TransportRequestEncodingPolicy::default()
}

fn transport_request_gzip_policy_from_config(
    config: Option<&Value>,
) -> Option<AiRequestGzipPolicy> {
    let object = config?.as_object()?;

    for key in ["request_gzip", "request_body_gzip"] {
        if let Some(policy) = object
            .get(key)
            .and_then(transport_request_gzip_policy_from_value)
        {
            return Some(policy);
        }
    }

    let enabled = first_config_bool(
        object,
        &["request_gzip_enabled", "request_body_gzip_enabled"],
    );
    let min_bytes = first_config_usize(
        object,
        &["request_gzip_min_bytes", "request_body_gzip_min_bytes"],
    );

    match (enabled, min_bytes) {
        (Some(false), _) => Some(AiRequestGzipPolicy {
            enabled: Some(false),
            min_bytes: None,
        }),
        (Some(true), min_bytes) => Some(AiRequestGzipPolicy {
            enabled: Some(true),
            min_bytes,
        }),
        (None, Some(min_bytes)) => Some(AiRequestGzipPolicy {
            enabled: Some(true),
            min_bytes: Some(min_bytes),
        }),
        (None, None) => None,
    }
}

fn transport_request_gzip_policy_from_value(value: &Value) -> Option<AiRequestGzipPolicy> {
    if let Some(enabled) = value.as_bool() {
        return Some(AiRequestGzipPolicy {
            enabled: Some(enabled),
            min_bytes: None,
        });
    }

    let object = value.as_object()?;
    let enabled = first_config_bool(object, &["enabled"]);
    let min_bytes = first_config_usize(object, &["min_bytes"]);

    match (enabled, min_bytes) {
        (Some(false), _) => Some(AiRequestGzipPolicy {
            enabled: Some(false),
            min_bytes: None,
        }),
        (Some(true), min_bytes) => Some(AiRequestGzipPolicy {
            enabled: Some(true),
            min_bytes,
        }),
        (None, Some(min_bytes)) => Some(AiRequestGzipPolicy {
            enabled: Some(true),
            min_bytes: Some(min_bytes),
        }),
        (None, None) => None,
    }
}

fn first_config_bool(object: &serde_json::Map<String, Value>, keys: &[&str]) -> Option<bool> {
    keys.iter()
        .find_map(|key| object.get(*key).and_then(config_bool))
}

fn config_bool(value: &Value) -> Option<bool> {
    value.as_bool().or_else(|| {
        value.as_str().and_then(|text| {
            let normalized = text.trim();
            if normalized.eq_ignore_ascii_case("true") {
                Some(true)
            } else if normalized.eq_ignore_ascii_case("false") {
                Some(false)
            } else {
                None
            }
        })
    })
}

fn first_config_usize(object: &serde_json::Map<String, Value>, keys: &[&str]) -> Option<usize> {
    keys.iter()
        .find_map(|key| object.get(*key).and_then(config_usize))
}

fn config_usize(value: &Value) -> Option<usize> {
    value
        .as_u64()
        .and_then(|number| usize::try_from(number).ok())
        .or_else(|| {
            value
                .as_str()
                .and_then(|text| text.trim().parse::<usize>().ok())
        })
}
