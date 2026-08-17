use std::net::SocketAddr;

use aether_gateway::build_router;
use axum::body::Body;
use axum::extract::ConnectInfo;
use http::Request;
use http_body_util::BodyExt;
use tower::ServiceExt;

const GATEWAY_HEADER: &str = "x-aether-gateway";
const ADMIN_USER_ID_HEADER: &str = "x-aether-admin-user-id";
const ADMIN_USER_ROLE_HEADER: &str = "x-aether-admin-user-role";
const ADMIN_SESSION_ID_HEADER: &str = "x-aether-admin-session-id";
const ADMIN_MANAGEMENT_TOKEN_ID_HEADER: &str = "x-aether-admin-management-token-id";

#[tokio::test]
async fn public_admin_routes_reject_unsigned_identity_headers() {
    let router = build_router().expect("gateway should build");

    for identity_header in [ADMIN_SESSION_ID_HEADER, ADMIN_MANAGEMENT_TOKEN_ID_HEADER] {
        for (method, path) in [
            (http::Method::GET, "/api/admin/providers"),
            (
                http::Method::GET,
                "/api/admin/endpoints/providers/provider-1/keys",
            ),
            (http::Method::GET, "/api/admin/endpoints/keys/key-1/reveal"),
            (http::Method::POST, "/api/announcements"),
            (http::Method::PUT, "/api/announcements/announcement-1"),
            (http::Method::DELETE, "/api/announcements/announcement-1"),
        ] {
            let mut request = Request::builder()
                .method(method.clone())
                .uri(path)
                .header(GATEWAY_HEADER, "rust-phase3b")
                .header(ADMIN_USER_ID_HEADER, "1")
                .header(ADMIN_USER_ROLE_HEADER, "admin")
                .header(identity_header, "x")
                .body(Body::empty())
                .expect("request should build");
            request
                .extensions_mut()
                .insert(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 40_000))));

            let response = router
                .clone()
                .oneshot(request)
                .await
                .expect("request should complete");
            assert_eq!(
                response.status(),
                http::StatusCode::UNAUTHORIZED,
                "header: {identity_header}, method: {method}, path: {path}"
            );

            let body = response
                .into_body()
                .collect()
                .await
                .expect("body should collect")
                .to_bytes();
            let payload: serde_json::Value =
                serde_json::from_slice(&body).expect("body should be json");
            assert_eq!(
                payload["detail"], "admin authentication required",
                "header: {identity_header}, method: {method}, path: {path}"
            );
        }
    }
}
