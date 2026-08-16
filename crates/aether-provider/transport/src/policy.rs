use super::snapshot::GatewayProviderTransportSnapshot;
use super::{body_rules_are_locally_supported, header_rules_are_locally_supported};

pub fn supports_local_standard_transport(
    transport: &GatewayProviderTransportSnapshot,
    api_format: &str,
) -> bool {
    local_standard_transport_unsupported_reason(transport, api_format).is_none()
}

pub fn supports_local_gemini_transport(
    transport: &GatewayProviderTransportSnapshot,
    api_format: &str,
) -> bool {
    local_gemini_transport_unsupported_reason(transport, api_format).is_none()
}

pub fn local_standard_transport_unsupported_reason(
    transport: &GatewayProviderTransportSnapshot,
    api_format: &str,
) -> Option<&'static str> {
    local_transport_unsupported_reason(transport, api_format)
}

pub fn local_gemini_transport_unsupported_reason(
    transport: &GatewayProviderTransportSnapshot,
    api_format: &str,
) -> Option<&'static str> {
    local_transport_unsupported_reason(transport, api_format)
}

pub fn local_standard_transport_unsupported_reason_with_network(
    transport: &GatewayProviderTransportSnapshot,
    api_format: &str,
) -> Option<&'static str> {
    local_transport_unsupported_reason(transport, api_format)
}

pub fn local_gemini_transport_unsupported_reason_with_network(
    transport: &GatewayProviderTransportSnapshot,
    api_format: &str,
) -> Option<&'static str> {
    local_transport_unsupported_reason(transport, api_format)
}

fn local_transport_unsupported_reason(
    transport: &GatewayProviderTransportSnapshot,
    api_format: &str,
) -> Option<&'static str> {
    if !transport.provider.is_active {
        return Some("provider_inactive");
    }
    if !transport.endpoint.is_active {
        return Some("endpoint_inactive");
    }
    if !transport.key.is_active {
        return Some("key_inactive");
    }
    if !aether_ai_formats::api_format_alias_matches(&transport.endpoint.api_format, api_format) {
        return Some("transport_api_format_mismatch");
    }
    if !header_rules_are_locally_supported(transport.endpoint.header_rules.as_ref()) {
        return Some("transport_header_rules_unsupported");
    }
    if !body_rules_are_locally_supported(transport.endpoint.body_rules.as_ref()) {
        return Some("transport_body_rules_unsupported");
    }
    None
}
