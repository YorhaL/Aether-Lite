use serde_json::{json, Value};

use crate::rules::{body_rules_are_locally_supported, header_rules_are_locally_supported};
use crate::snapshot::GatewayProviderTransportSnapshot;

pub fn append_transport_diagnostics_to_value(
    value: Value,
    transport: Option<&GatewayProviderTransportSnapshot>,
    client_api_format: &str,
    provider_api_format: &str,
) -> Value {
    let Value::Object(mut object) = value else {
        return value;
    };
    object.insert(
        "transport_diagnostics".to_string(),
        transport
            .map(|transport| {
                build_transport_diagnostics(transport, client_api_format, provider_api_format)
            })
            .unwrap_or_else(|| json!({ "transport_snapshot_available": false })),
    );
    Value::Object(object)
}

pub fn build_transport_diagnostics(
    transport: &GatewayProviderTransportSnapshot,
    client_api_format: &str,
    provider_api_format: &str,
) -> Value {
    json!({
        "transport_snapshot_available": true,
        "provider_is_active": transport.provider.is_active,
        "endpoint_is_active": transport.endpoint.is_active,
        "key_is_active": transport.key.is_active,
        "endpoint_custom_path": transport.endpoint.custom_path,
        "header_rules": transport.endpoint.header_rules,
        "header_rules_supported": header_rules_are_locally_supported(
            transport.endpoint.header_rules.as_ref(),
        ),
        "body_rules": transport.endpoint.body_rules,
        "body_rules_supported": body_rules_are_locally_supported(
            transport.endpoint.body_rules.as_ref(),
        ),
        "key_auth_type": transport.key.auth_type,
        "client_api_format": client_api_format,
        "provider_api_format": provider_api_format,
        "api_format_matches": aether_ai_formats::api_format_alias_matches(
            client_api_format,
            provider_api_format,
        ),
    })
}
