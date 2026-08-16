use crate::GatewayProviderTransportSnapshot;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CandidateTransportPolicyFacts<'a> {
    pub endpoint_api_format: &'a str,
    pub global_model_name: &'a str,
    pub selected_provider_model_name: &'a str,
    pub mapping_matched_model: Option<&'a str>,
}

pub fn candidate_common_transport_skip_reason(
    transport: &GatewayProviderTransportSnapshot,
    candidate: CandidateTransportPolicyFacts<'_>,
    requested_model: Option<&str>,
) -> Option<&'static str> {
    if !matches!(
        transport.key.auth_type.trim().to_ascii_lowercase().as_str(),
        "api_key" | "bearer"
    ) {
        return Some("key_auth_type_unsupported");
    }
    if !transport.provider.is_active {
        return Some("provider_inactive");
    }
    if !transport.endpoint.is_active {
        return Some("endpoint_inactive");
    }
    if !transport.key.is_active {
        return Some("key_inactive");
    }

    let endpoint_api_format = transport.endpoint.api_format.trim();
    if !aether_ai_formats::api_format_alias_matches(
        candidate.endpoint_api_format,
        endpoint_api_format,
    ) {
        return Some("endpoint_api_format_changed");
    }
    if !transport_key_supports_api_format(transport, endpoint_api_format) {
        return Some("key_api_format_disabled");
    }
    if !transport_key_allows_candidate_model(
        transport,
        requested_model.unwrap_or_default(),
        candidate,
    ) {
        return Some("key_model_disabled");
    }
    None
}

pub fn candidate_transport_pair_skip_reason(
    transport: &GatewayProviderTransportSnapshot,
    normalized_client_api_format: &str,
) -> Option<&'static str> {
    (!aether_ai_formats::api_format_alias_matches(
        transport.endpoint.api_format.trim(),
        normalized_client_api_format,
    ))
    .then_some("api_format_mismatch")
}

fn transport_key_supports_api_format(
    transport: &GatewayProviderTransportSnapshot,
    endpoint_api_format: &str,
) -> bool {
    match transport.key.api_formats.as_deref() {
        None => true,
        Some(formats) => formats.iter().any(|value| {
            aether_ai_formats::api_format_permission_covers(value, endpoint_api_format)
        }),
    }
}

fn transport_key_allows_candidate_model(
    transport: &GatewayProviderTransportSnapshot,
    requested_model: &str,
    candidate: CandidateTransportPolicyFacts<'_>,
) -> bool {
    let Some(allowed_models) = transport.key.allowed_models.as_deref() else {
        return true;
    };
    let requested_model = requested_model.trim();
    let requested_base_model = aether_ai_formats::model_directive_base_model(requested_model);
    let candidate_models = [
        candidate.global_model_name.trim(),
        candidate.selected_provider_model_name.trim(),
        candidate.mapping_matched_model.unwrap_or_default().trim(),
    ];

    allowed_models.iter().any(|allowed_model| {
        let allowed_model = allowed_model.trim();
        !allowed_model.is_empty()
            && (allowed_model == requested_model
                || requested_base_model
                    .as_deref()
                    .is_some_and(|base_model| allowed_model == base_model)
                || candidate_models.contains(&allowed_model))
    })
}
