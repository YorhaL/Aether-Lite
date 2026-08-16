use http::Uri;

use super::{classify_control_route, headers};

#[test]
fn classifies_admin_monitoring_trace_request_as_admin_proxy_route() {
    let headers = headers(&[]);
    let uri: Uri = "/api/admin/monitoring/trace/request-1"
        .parse()
        .expect("uri should parse");
    let decision =
        classify_control_route(&http::Method::GET, &uri, &headers).expect("route should classify");

    assert_eq!(decision.route_class.as_deref(), Some("admin_proxy"));
    assert_eq!(decision.route_family.as_deref(), Some("monitoring"));
    assert_eq!(decision.route_kind.as_deref(), Some("trace_request"));
    assert_eq!(
        decision.auth_endpoint_signature.as_deref(),
        Some("admin:monitoring")
    );
    assert!(!decision.is_execution_runtime_candidate());
}

#[test]
fn classifies_admin_monitoring_cache_stats_as_admin_proxy_route() {
    let headers = headers(&[]);
    let uri: Uri = "/api/admin/monitoring/cache/stats"
        .parse()
        .expect("uri should parse");
    let decision =
        classify_control_route(&http::Method::GET, &uri, &headers).expect("route should classify");

    assert_eq!(decision.route_class.as_deref(), Some("admin_proxy"));
    assert_eq!(decision.route_family.as_deref(), Some("monitoring"));
    assert_eq!(decision.route_kind.as_deref(), Some("monitoring_cache"));
    assert_eq!(
        decision.auth_endpoint_signature.as_deref(),
        Some("admin:monitoring")
    );
    assert!(!decision.is_execution_runtime_candidate());
}

#[test]
fn classifies_admin_monitoring_cache_affinities_as_admin_proxy_route() {
    let headers = headers(&[]);
    let uri: Uri = "/api/admin/monitoring/cache/affinities"
        .parse()
        .expect("uri should parse");
    let decision =
        classify_control_route(&http::Method::GET, &uri, &headers).expect("route should classify");

    assert_eq!(decision.route_class.as_deref(), Some("admin_proxy"));
    assert_eq!(decision.route_family.as_deref(), Some("monitoring"));
    assert_eq!(decision.route_kind.as_deref(), Some("monitoring_cache"));
    assert_eq!(
        decision.auth_endpoint_signature.as_deref(),
        Some("admin:monitoring")
    );
    assert!(!decision.is_execution_runtime_candidate());
}

#[test]
fn classifies_admin_monitoring_cache_affinity_as_admin_proxy_route() {
    let headers = headers(&[]);
    let uri: Uri = "/api/admin/monitoring/cache/affinity/user-1"
        .parse()
        .expect("uri should parse");
    let decision =
        classify_control_route(&http::Method::GET, &uri, &headers).expect("route should classify");

    assert_eq!(decision.route_class.as_deref(), Some("admin_proxy"));
    assert_eq!(decision.route_family.as_deref(), Some("monitoring"));
    assert_eq!(decision.route_kind.as_deref(), Some("monitoring_cache"));
    assert_eq!(
        decision.auth_endpoint_signature.as_deref(),
        Some("admin:monitoring")
    );
    assert!(!decision.is_execution_runtime_candidate());
}

#[test]
fn classifies_admin_monitoring_cache_users_delete_as_admin_proxy_route() {
    let headers = headers(&[]);
    let uri: Uri = "/api/admin/monitoring/cache/users/user-1"
        .parse()
        .expect("uri should parse");
    let decision = classify_control_route(&http::Method::DELETE, &uri, &headers)
        .expect("route should classify");

    assert_eq!(decision.route_class.as_deref(), Some("admin_proxy"));
    assert_eq!(decision.route_family.as_deref(), Some("monitoring"));
    assert_eq!(decision.route_kind.as_deref(), Some("monitoring_cache"));
    assert_eq!(
        decision.auth_endpoint_signature.as_deref(),
        Some("admin:monitoring")
    );
    assert!(!decision.is_execution_runtime_candidate());
}

#[test]
fn classifies_admin_monitoring_cache_affinity_delete_as_admin_proxy_route() {
    let headers = headers(&[]);
    let uri: Uri = "/api/admin/monitoring/cache/affinity/user-key-1/endpoint-1/model-alpha/openai"
        .parse()
        .expect("uri should parse");
    let decision = classify_control_route(&http::Method::DELETE, &uri, &headers)
        .expect("route should classify");

    assert_eq!(decision.route_class.as_deref(), Some("admin_proxy"));
    assert_eq!(decision.route_family.as_deref(), Some("monitoring"));
    assert_eq!(decision.route_kind.as_deref(), Some("monitoring_cache"));
    assert_eq!(
        decision.auth_endpoint_signature.as_deref(),
        Some("admin:monitoring")
    );
    assert!(!decision.is_execution_runtime_candidate());
}

#[test]
fn classifies_admin_monitoring_cache_flush_as_admin_proxy_route() {
    let headers = headers(&[]);
    let uri: Uri = "/api/admin/monitoring/cache"
        .parse()
        .expect("uri should parse");
    let decision = classify_control_route(&http::Method::DELETE, &uri, &headers)
        .expect("route should classify");

    assert_eq!(decision.route_class.as_deref(), Some("admin_proxy"));
    assert_eq!(decision.route_family.as_deref(), Some("monitoring"));
    assert_eq!(decision.route_kind.as_deref(), Some("monitoring_cache"));
    assert_eq!(
        decision.auth_endpoint_signature.as_deref(),
        Some("admin:monitoring")
    );
    assert!(!decision.is_execution_runtime_candidate());
}

#[test]
fn classifies_admin_monitoring_cache_provider_delete_as_admin_proxy_route() {
    let headers = headers(&[]);
    let uri: Uri = "/api/admin/monitoring/cache/providers/provider-1"
        .parse()
        .expect("uri should parse");
    let decision = classify_control_route(&http::Method::DELETE, &uri, &headers)
        .expect("route should classify");

    assert_eq!(decision.route_class.as_deref(), Some("admin_proxy"));
    assert_eq!(decision.route_family.as_deref(), Some("monitoring"));
    assert_eq!(decision.route_kind.as_deref(), Some("monitoring_cache"));
    assert_eq!(
        decision.auth_endpoint_signature.as_deref(),
        Some("admin:monitoring")
    );
    assert!(!decision.is_execution_runtime_candidate());
}

#[test]
fn classifies_admin_monitoring_cache_metrics_as_admin_proxy_route() {
    let headers = headers(&[]);
    let uri: Uri = "/api/admin/monitoring/cache/metrics"
        .parse()
        .expect("uri should parse");
    let decision =
        classify_control_route(&http::Method::GET, &uri, &headers).expect("route should classify");

    assert_eq!(decision.route_class.as_deref(), Some("admin_proxy"));
    assert_eq!(decision.route_family.as_deref(), Some("monitoring"));
    assert_eq!(decision.route_kind.as_deref(), Some("monitoring_cache"));
    assert_eq!(
        decision.auth_endpoint_signature.as_deref(),
        Some("admin:monitoring")
    );
    assert!(!decision.is_execution_runtime_candidate());
}

#[test]
fn classifies_admin_monitoring_cache_config_as_admin_proxy_route() {
    let headers = headers(&[]);
    let uri: Uri = "/api/admin/monitoring/cache/config"
        .parse()
        .expect("uri should parse");
    let decision =
        classify_control_route(&http::Method::GET, &uri, &headers).expect("route should classify");

    assert_eq!(decision.route_class.as_deref(), Some("admin_proxy"));
    assert_eq!(decision.route_family.as_deref(), Some("monitoring"));
    assert_eq!(decision.route_kind.as_deref(), Some("monitoring_cache"));
    assert_eq!(
        decision.auth_endpoint_signature.as_deref(),
        Some("admin:monitoring")
    );
    assert!(!decision.is_execution_runtime_candidate());
}

#[test]
fn classifies_admin_monitoring_model_mapping_stats_as_admin_proxy_route() {
    let headers = headers(&[]);
    let uri: Uri = "/api/admin/monitoring/cache/model-mapping/stats"
        .parse()
        .expect("uri should parse");
    let decision =
        classify_control_route(&http::Method::GET, &uri, &headers).expect("route should classify");

    assert_eq!(decision.route_class.as_deref(), Some("admin_proxy"));
    assert_eq!(decision.route_family.as_deref(), Some("monitoring"));
    assert_eq!(decision.route_kind.as_deref(), Some("monitoring_cache"));
    assert_eq!(
        decision.auth_endpoint_signature.as_deref(),
        Some("admin:monitoring")
    );
    assert!(!decision.is_execution_runtime_candidate());
}

#[test]
fn classifies_admin_monitoring_model_mapping_delete_as_admin_proxy_route() {
    let headers = headers(&[]);
    let uri: Uri = "/api/admin/monitoring/cache/model-mapping"
        .parse()
        .expect("uri should parse");
    let decision = classify_control_route(&http::Method::DELETE, &uri, &headers)
        .expect("route should classify");

    assert_eq!(decision.route_class.as_deref(), Some("admin_proxy"));
    assert_eq!(decision.route_family.as_deref(), Some("monitoring"));
    assert_eq!(decision.route_kind.as_deref(), Some("monitoring_cache"));
    assert_eq!(
        decision.auth_endpoint_signature.as_deref(),
        Some("admin:monitoring")
    );
    assert!(!decision.is_execution_runtime_candidate());
}

#[test]
fn classifies_admin_monitoring_model_mapping_delete_model_as_admin_proxy_route() {
    let headers = headers(&[]);
    let uri: Uri = "/api/admin/monitoring/cache/model-mapping/model-alpha"
        .parse()
        .expect("uri should parse");
    let decision = classify_control_route(&http::Method::DELETE, &uri, &headers)
        .expect("route should classify");

    assert_eq!(decision.route_class.as_deref(), Some("admin_proxy"));
    assert_eq!(decision.route_family.as_deref(), Some("monitoring"));
    assert_eq!(decision.route_kind.as_deref(), Some("monitoring_cache"));
    assert_eq!(
        decision.auth_endpoint_signature.as_deref(),
        Some("admin:monitoring")
    );
    assert!(!decision.is_execution_runtime_candidate());
}

#[test]
fn classifies_admin_monitoring_model_mapping_delete_provider_as_admin_proxy_route() {
    let headers = headers(&[]);
    let uri: Uri = "/api/admin/monitoring/cache/model-mapping/provider/provider-1/model-alpha"
        .parse()
        .expect("uri should parse");
    let decision = classify_control_route(&http::Method::DELETE, &uri, &headers)
        .expect("route should classify");

    assert_eq!(decision.route_class.as_deref(), Some("admin_proxy"));
    assert_eq!(decision.route_family.as_deref(), Some("monitoring"));
    assert_eq!(decision.route_kind.as_deref(), Some("monitoring_cache"));
    assert_eq!(
        decision.auth_endpoint_signature.as_deref(),
        Some("admin:monitoring")
    );
    assert!(!decision.is_execution_runtime_candidate());
}

#[test]
fn classifies_admin_monitoring_redis_keys_as_admin_proxy_route() {
    let headers = headers(&[]);
    let uri: Uri = "/api/admin/monitoring/cache/redis-keys"
        .parse()
        .expect("uri should parse");
    let decision =
        classify_control_route(&http::Method::GET, &uri, &headers).expect("route should classify");

    assert_eq!(decision.route_class.as_deref(), Some("admin_proxy"));
    assert_eq!(decision.route_family.as_deref(), Some("monitoring"));
    assert_eq!(decision.route_kind.as_deref(), Some("monitoring_cache"));
    assert_eq!(
        decision.auth_endpoint_signature.as_deref(),
        Some("admin:monitoring")
    );
    assert!(!decision.is_execution_runtime_candidate());
}

#[test]
fn classifies_admin_monitoring_redis_keys_delete_as_admin_proxy_route() {
    let headers = headers(&[]);
    let uri: Uri = "/api/admin/monitoring/cache/redis-keys/dashboard"
        .parse()
        .expect("uri should parse");
    let decision = classify_control_route(&http::Method::DELETE, &uri, &headers)
        .expect("route should classify");

    assert_eq!(decision.route_class.as_deref(), Some("admin_proxy"));
    assert_eq!(decision.route_family.as_deref(), Some("monitoring"));
    assert_eq!(decision.route_kind.as_deref(), Some("monitoring_cache"));
    assert_eq!(
        decision.auth_endpoint_signature.as_deref(),
        Some("admin:monitoring")
    );
    assert!(!decision.is_execution_runtime_candidate());
}

#[test]
fn classifies_admin_monitoring_resilience_status_as_admin_proxy_route() {
    let headers = headers(&[]);
    let uri: Uri = "/api/admin/monitoring/resilience-status"
        .parse()
        .expect("uri should parse");
    let decision =
        classify_control_route(&http::Method::GET, &uri, &headers).expect("route should classify");

    assert_eq!(decision.route_class.as_deref(), Some("admin_proxy"));
    assert_eq!(decision.route_family.as_deref(), Some("monitoring"));
    assert_eq!(
        decision.route_kind.as_deref(),
        Some("monitoring_resilience")
    );
    assert_eq!(
        decision.auth_endpoint_signature.as_deref(),
        Some("admin:monitoring")
    );
    assert!(!decision.is_execution_runtime_candidate());
}

#[test]
fn classifies_admin_monitoring_resilience_error_stats_as_admin_proxy_route() {
    let headers = headers(&[]);
    let uri: Uri = "/api/admin/monitoring/resilience/error-stats"
        .parse()
        .expect("uri should parse");
    let decision = classify_control_route(&http::Method::DELETE, &uri, &headers)
        .expect("route should classify");

    assert_eq!(decision.route_class.as_deref(), Some("admin_proxy"));
    assert_eq!(decision.route_family.as_deref(), Some("monitoring"));
    assert_eq!(
        decision.route_kind.as_deref(),
        Some("monitoring_resilience")
    );
    assert_eq!(
        decision.auth_endpoint_signature.as_deref(),
        Some("admin:monitoring")
    );
    assert!(!decision.is_execution_runtime_candidate());
}
