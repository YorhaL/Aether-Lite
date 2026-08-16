use axum::{body::Body, http, response::Response};

pub(super) use super::{
    build_auth_error_response, build_public_support_route_not_found_response,
    resolve_authenticated_local_user, AppState, GatewayPublicRequestContext,
};

#[path = "monitoring/rate_limit_status.rs"]
mod user_monitoring_rate_limit_status;

use self::user_monitoring_rate_limit_status::handle_user_rate_limit_status;

pub(super) async fn maybe_build_local_user_monitoring_response(
    state: &AppState,
    request_context: &GatewayPublicRequestContext,
    headers: &http::HeaderMap,
) -> Option<Response<Body>> {
    let decision = request_context.control_decision.as_ref()?;
    if decision.route_family.as_deref() != Some("monitoring_user") {
        return None;
    }

    match decision.route_kind.as_deref() {
        Some("rate_limit_status")
            if request_context.request_method == http::Method::GET
                && request_context.request_path == "/api/monitoring/rate-limit-status" =>
        {
            Some(handle_user_rate_limit_status(state, request_context, headers).await)
        }
        _ => Some(build_public_support_route_not_found_response(
            request_context,
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        maybe_build_local_user_monitoring_response, AppState, GatewayPublicRequestContext,
    };
    use crate::control::GatewayControlDecision;
    use axum::body::to_bytes;
    use axum::http::{HeaderMap, Method, StatusCode, Uri};

    fn request_context(method: Method, uri: &str, route_kind: &str) -> GatewayPublicRequestContext {
        GatewayPublicRequestContext::from_request_parts(
            "trace-monitoring-unhandled",
            &method,
            &uri.parse::<Uri>().expect("uri should parse"),
            &HeaderMap::new(),
            Some(GatewayControlDecision::synthetic(
                uri,
                Some("public_support".to_string()),
                Some("monitoring_user".to_string()),
                Some(route_kind.to_string()),
                Some("user:monitoring".to_string()),
            )),
        )
    }

    #[tokio::test]
    async fn monitoring_unhandled_route_returns_not_found_response() {
        let state = AppState::new().expect("gateway should build");
        let request_context = request_context(
            Method::GET,
            "/api/monitoring/rate-limit-status/history",
            "rate_limit_status",
        );
        let response =
            maybe_build_local_user_monitoring_response(&state, &request_context, &HeaderMap::new())
                .await
                .expect("monitoring handler should return response");

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body should read");
        let payload: serde_json::Value =
            serde_json::from_slice(&body).expect("json body should parse");
        assert_eq!(payload["detail"], "Route not found");
        assert_eq!(payload["route_family"], "monitoring_user");
        assert_eq!(payload["route_kind"], "rate_limit_status");
        assert_eq!(
            payload["request_path"],
            "/api/monitoring/rate-limit-status/history"
        );
    }
}
