pub mod auth;
mod cache;
mod candidate_policy;
mod diagnostics;
mod headers;
mod network;
pub mod policy;
mod request_url;
pub mod rules;
pub mod snapshot;
pub mod url;

pub use auth::{build_passthrough_headers, ensure_upstream_auth_header};
pub use cache::{provider_transport_snapshot_looks_refreshed, ProviderTransportSnapshotCacheKey};
pub use candidate_policy::{
    candidate_common_transport_skip_reason, candidate_transport_pair_skip_reason,
    CandidateTransportPolicyFacts,
};
pub use diagnostics::{append_transport_diagnostics_to_value, build_transport_diagnostics};
pub use headers::{should_skip_request_header, should_skip_upstream_passthrough_header};
pub use network::{resolve_transport_execution_timeouts, resolve_transport_profile};
pub use policy::{
    local_gemini_transport_unsupported_reason,
    local_gemini_transport_unsupported_reason_with_network,
    local_standard_transport_unsupported_reason,
    local_standard_transport_unsupported_reason_with_network, supports_local_gemini_transport,
    supports_local_standard_transport,
};
pub use request_url::{
    build_transport_request_url, build_transport_request_url_for_request_body,
    TransportRequestUrlParams,
};
pub use rules::{
    apply_local_body_rules, apply_local_body_rules_with_request_headers, apply_local_header_rules,
    apply_local_header_rules_with_request_headers, body_rules_are_locally_supported,
    body_rules_handle_path, body_rules_have_enabled_rules, header_rules_are_locally_supported,
    header_rules_have_enabled_rules,
};
pub use snapshot::{
    read_provider_transport_snapshot, GatewayProviderTransportSnapshot,
    ProviderTransportSnapshotSource,
};
