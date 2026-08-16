use http::Uri;

use super::{classify_control_route, headers};

#[test]
fn classifies_internal_gateway_resolve_as_internal_proxy_route() {
    let headers = headers(&[]);
    let uri: Uri = "/api/internal/gateway/resolve"
        .parse()
        .expect("uri should parse");
    let decision =
        classify_control_route(&http::Method::POST, &uri, &headers).expect("route should classify");

    assert_eq!(decision.route_class.as_deref(), Some("internal_proxy"));
    assert_eq!(decision.route_family.as_deref(), Some("internal_gateway"));
    assert_eq!(decision.route_kind.as_deref(), Some("resolve"));
    assert_eq!(decision.auth_endpoint_signature.as_deref(), Some(""));
    assert!(!decision.is_execution_runtime_candidate());
}
