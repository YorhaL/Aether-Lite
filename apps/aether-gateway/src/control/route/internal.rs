use super::{classified, ClassifiedRoute};

pub(super) fn classify_internal_route(
    method: &http::Method,
    normalized_path: &str,
) -> Option<ClassifiedRoute> {
    if method == http::Method::POST && normalized_path.starts_with("/api/internal/gateway/") {
        let route_kind = match normalized_path {
            "/api/internal/gateway/resolve" => "resolve",
            "/api/internal/gateway/auth-context" => "auth_context",
            "/api/internal/gateway/decision-sync" => "decision_sync",
            "/api/internal/gateway/decision-stream" => "decision_stream",
            "/api/internal/gateway/plan-sync" => "plan_sync",
            "/api/internal/gateway/plan-stream" => "plan_stream",
            "/api/internal/gateway/report-sync" => "report_sync",
            "/api/internal/gateway/report-stream" => "report_stream",
            "/api/internal/gateway/execute-sync" => "execute_sync",
            "/api/internal/gateway/execute-stream" => "execute_stream",
            _ => "unhandled",
        };
        Some(classified(
            "internal_proxy",
            "internal_gateway",
            route_kind,
            "",
            false,
        ))
    } else {
        None
    }
}
