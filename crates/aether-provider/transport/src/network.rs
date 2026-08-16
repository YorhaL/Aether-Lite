use aether_contracts::{
    ExecutionTimeouts, ResolvedTransportProfile, TRANSPORT_BACKEND_REQWEST_RUSTLS,
    TRANSPORT_HTTP_MODE_AUTO, TRANSPORT_POOL_SCOPE_KEY,
};
use serde_json::{Map, Value};

use super::snapshot::GatewayProviderTransportSnapshot;

const DEFAULT_PROVIDER_STREAM_FIRST_BYTE_TIMEOUT_SECS: f64 = 30.0;

pub fn resolve_transport_execution_timeouts(
    transport: &GatewayProviderTransportSnapshot,
) -> Option<ExecutionTimeouts> {
    Some(ExecutionTimeouts {
        total_ms: transport
            .provider
            .request_timeout_secs
            .filter(|value| value.is_finite() && *value > 0.0)
            .map(timeout_secs_to_ms),
        first_byte_ms: Some(timeout_secs_to_ms(
            transport
                .provider
                .stream_first_byte_timeout_secs
                .filter(|value| value.is_finite() && *value > 0.0)
                .unwrap_or(DEFAULT_PROVIDER_STREAM_FIRST_BYTE_TIMEOUT_SECS),
        )),
        ..ExecutionTimeouts::default()
    })
}

fn timeout_secs_to_ms(secs: f64) -> u64 {
    ((secs * 1000.0).round() as u64).max(1)
}

pub fn resolve_transport_profile(
    transport: &GatewayProviderTransportSnapshot,
) -> Option<ResolvedTransportProfile> {
    let configured = resolve_transport_profile_from_fingerprint(transport.key.fingerprint.as_ref())
        .or_else(|| {
            resolve_transport_profile_from_provider_config(transport.provider.config.as_ref())
        });
    configured
}

fn resolve_transport_profile_from_provider_config(
    config: Option<&Value>,
) -> Option<ResolvedTransportProfile> {
    let fingerprint = config?.get("fingerprint");
    resolve_transport_profile_from_fingerprint(fingerprint)
}

fn resolve_transport_profile_from_fingerprint(
    fingerprint: Option<&Value>,
) -> Option<ResolvedTransportProfile> {
    let fingerprint = fingerprint?;
    fingerprint
        .get("transport_profile")
        .and_then(parse_transport_profile_value)
}

fn parse_transport_profile_value(value: &Value) -> Option<ResolvedTransportProfile> {
    if let Some(profile_id) = value
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return Some(ResolvedTransportProfile {
            profile_id: profile_id.to_string(),
            backend: TRANSPORT_BACKEND_REQWEST_RUSTLS.to_string(),
            http_mode: TRANSPORT_HTTP_MODE_AUTO.to_string(),
            pool_scope: TRANSPORT_POOL_SCOPE_KEY.to_string(),
            header_fingerprint: None,
            extra: None,
        });
    }

    let object = value.as_object()?;
    let profile_id = json_string_field(object, "profile_id")
        .or_else(|| json_string_field(object, "id"))
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())?;
    let backend = json_string_field(object, "backend")
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| TRANSPORT_BACKEND_REQWEST_RUSTLS.to_string());
    let http_mode = json_string_field(object, "http_mode")
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| TRANSPORT_HTTP_MODE_AUTO.to_string());
    let pool_scope = json_string_field(object, "pool_scope")
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| TRANSPORT_POOL_SCOPE_KEY.to_string());
    let header_fingerprint = object.get("header_fingerprint").cloned();
    let extra = object.get("extra").cloned();

    Some(ResolvedTransportProfile {
        profile_id,
        backend,
        http_mode,
        pool_scope,
        header_fingerprint,
        extra,
    })
}

fn json_string_field(object: &Map<String, Value>, key: &str) -> Option<String> {
    object
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}
